use std::time::Instant;

use crate::animation::{
    Animatable, Keyframes, SpringState, Transition, TransitionConfig, carry_velocity,
};
use crate::reactive::Signal;

/// A sequence a property can be told to play, and the signal that tells it.
///
/// Unlike everything else in an `AnimationState` this has no target: while it
/// runs it *replaces* the declared value, and when it ends the property is
/// handed back — the rule CSS gives an animation over a normal declaration.
struct Timeline<T> {
    keyframes: Keyframes<T>,
    /// When the current run started, if one is running.
    playing: Option<Instant>,
    /// A signal whose every change plays the sequence once, and the count last
    /// acted on.
    ///
    /// A count rather than a flag, because two refusals in a row are two
    /// events and a signal that stays equal notifies nobody. Reading it and
    /// committing to it live in one place, so the pass that subscribes and the
    /// pass that plays cannot disagree about what has been seen.
    trigger: Option<Signal<u32>>,
    last_play: u32,
}

/// A transition of no duration: a timeline speaks for its property while it
/// plays, and outside it the declared value applies at once, exactly as it
/// would with no animation at all.
pub(super) fn instant_transition() -> Transition {
    Transition::new(0.0, crate::animation::TimingFunction::Linear)
}

/// Result of advancing an animation, indicating whether the value changed
#[derive(Debug, Clone, PartialEq)]
pub enum AdvanceResult<T> {
    /// Value did not change (animation not running or same value)
    NoChange,
    /// Value changed to a new value
    Changed(T),
}

impl<T> AdvanceResult<T> {
    /// Returns true if the value changed
    pub fn is_changed(&self) -> bool {
        matches!(self, AdvanceResult::Changed(_))
    }
}

/// Animation state for animatable properties
pub struct AnimationState<T: Animatable> {
    /// Current interpolated value
    current: T,
    /// Target value from Signal
    target: T,
    /// Value when animation started
    start: T,
    /// Progress from 0.0 to 1.0 (or beyond for overshoot)
    progress: f32,
    /// Time when animation started
    start_time: Instant,
    /// Forward transition (used when value increases or no reverse is set)
    transition: Transition,
    /// Optional reverse transition (used when value decreases)
    reverse_transition: Option<Transition>,
    /// Whether the current animation is using the reverse transition
    using_reverse: bool,
    /// Spring state (for spring timing functions)
    spring_state: Option<SpringState>,
    /// Whether the animation has been initialized with its first real value
    initialized: bool,
    /// Previous value for change detection
    prev_value: Option<T>,
    /// Pending enter value: consumed at first layout to start an enter
    /// animation instead of snapping to the target
    enter_from: Option<T>,
    /// A sequence to play on demand, and what plays it. Boxed and absent by
    /// default: `ContainerAnims` holds eleven of these states and only three
    /// can carry a sequence — `keyframes_translate`, `keyframes_rotate` and
    /// `keyframes_scale` — so the rest pay a pointer rather than the struct.
    timeline: Option<Box<Timeline<T>>>,
}

impl<T: Animatable> AnimationState<T> {
    pub fn new(initial_value: T, config: impl Into<TransitionConfig>) -> Self {
        let config = config.into();
        let spring_state = if matches!(
            config.forward.timing,
            crate::animation::TimingFunction::Spring(_)
        ) {
            Some(SpringState::new())
        } else {
            None
        };
        Self {
            current: initial_value,
            target: initial_value,
            start: initial_value,
            progress: 1.0, // Start completed
            start_time: Instant::now(),
            transition: config.forward,
            reverse_transition: config.reverse,
            using_reverse: false,
            spring_state,
            initialized: false, // Not yet initialized with real content-based value
            prev_value: None,
            enter_from: None,
            timeline: None,
        }
    }

    /// Get the currently active transition (forward or reverse).
    fn active_transition(&self) -> &Transition {
        if self.using_reverse {
            self.reverse_transition.as_ref().unwrap_or(&self.transition)
        } else {
            &self.transition
        }
    }

    /// Start animating to a new target value
    pub fn animate_to(&mut self, new_target: T) {
        // Don't restart if we're already animating to this target
        if new_target == self.target {
            return;
        }

        // A target that is not a number is not one this can be aimed at, and
        // the test above cannot refuse it: NaN is not equal to itself, so every
        // pass would read as a new target, begin a fresh segment, and ask for
        // another Animation job — a surface that never goes idle for a value
        // that was never going to arrive.
        //
        // Refusing it here is only half: `resync_animation_targets` compares
        // the stored target against the one the signals resolve to, and no
        // stored value compares equal to a NaN — so the drift check has to
        // refuse it as well, or it asks for an Animation job on every paint
        // and wakes the loop just as forever by the other route.
        if !new_target.is_reachable() {
            crate::transform::warn_unusable(format_args!(
                "animation: a target is not a usable number; the property stays \
                 where it is and stops following its signal"
            ));
            return;
        }

        // Detect direction and select transition
        self.using_reverse =
            self.reverse_transition.is_some() && T::is_reverse(&self.current, &new_target);

        // Check if active transition is spring before mutating other fields
        let is_spring = matches!(
            self.active_transition().timing,
            crate::animation::TimingFunction::Spring(_)
        );

        let carried = if is_spring {
            self.carried_velocity(&new_target)
        } else {
            0.0
        };

        self.target = new_target;
        self.begin_segment(self.current, carried);
    }

    /// Start a fresh run toward the current target, from rest.
    fn begin_segment_from(&mut self, from: T) {
        self.begin_segment(from, 0.0);
    }

    /// The bookkeeping every new segment shares.
    fn begin_segment(&mut self, from: T, carried: f32) {
        let is_spring = matches!(
            self.active_transition().timing,
            crate::animation::TimingFunction::Spring(_)
        );
        self.start = from;
        self.current = from;
        self.progress = 0.0;
        self.start_time = Instant::now();
        self.spring_state = if is_spring {
            Some(SpringState::moving_at(carried))
        } else {
            None
        };
    }

    /// The velocity the running spring should carry into the new segment.
    ///
    /// Zero unless a spring is actually in flight. Starting from rest is right
    /// for a value that was standing still, and is what this did for every
    /// target change before: a hover reversed mid-flight restarted from a stop
    /// instead of turning around, and a spring that never keeps its momentum
    /// is an easing curve wearing a spring's name.
    ///
    /// The projection is in [`carry_velocity`]; what belongs here is the one
    /// case where the answer is "none of it": a transition that has not
    /// started moving yet. Its spring is created now and stepped only after
    /// the delay, so whatever the value is doing at this instant is stale by
    /// the time it would be released — at full strength, and against a value
    /// that has not moved in the meantime.
    fn carried_velocity(&self, new_target: &T) -> f32 {
        let Some(spring) = &self.spring_state else {
            return 0.0;
        };
        if self.active_transition().delay_ms > 0.0 {
            return 0.0;
        }
        carry_velocity(
            spring.velocity,
            &self.start,
            &self.target,
            &self.current,
            new_target,
        )
    }

    /// Advance the animation and return whether the value changed
    pub fn advance(&mut self) -> AdvanceResult<T> {
        // A sequence speaks for the property while it runs, and nothing else
        // does — the same rule the cascade gives a CSS animation over a normal
        // declaration.
        if let Some(result) = self.advance_timeline() {
            return result;
        }
        if self.progress >= 1.0 && self.spring_state.is_none() {
            return AdvanceResult::NoChange;
        }
        let was_running = self.is_animating();

        // Extract scalar transition values upfront to avoid borrow conflicts
        // with self.spring_state. Copy SpringConfig (which is Copy) instead of
        // cloning the entire TimingFunction (which may contain an Arc).
        let active = self.active_transition();
        let delay_ms = active.delay_ms;
        let duration_ms = active.duration_ms;
        let spring_config = match active.timing {
            crate::animation::TimingFunction::Spring(config) => Some(config),
            _ => None,
        };

        let elapsed = self.start_time.elapsed().as_secs_f32() * 1000.0; // Convert to ms
        let adjusted_elapsed = (elapsed - delay_ms).max(0.0);

        if adjusted_elapsed <= 0.0 {
            // Still in delay period
            return AdvanceResult::NoChange;
        }

        // Calculate eased value based on timing function type
        let eased_t = if let Some(ref mut spring_state) = self.spring_state {
            // For spring animations: use real elapsed time in seconds (not normalized)
            // This allows the spring to continue oscillating until it naturally settles
            let elapsed_secs = adjusted_elapsed / 1000.0;
            if let Some(ref config) = spring_config {
                spring_state.step(elapsed_secs, config)
            } else {
                // Fallback: shouldn't happen, but use normalized time
                adjusted_elapsed / duration_ms
            }
        } else {
            // For non-spring animations: use normalized time 0..1
            let t = (adjusted_elapsed / duration_ms).min(1.0);
            // Safe to borrow self again — spring_state mutable borrow ended above
            self.active_transition().timing.evaluate(t)
        };

        // Interpolate
        let mut new_value = T::lerp(&self.start, &self.target, eased_t);
        let mut settled = false;

        // Update progress
        if let Some(ref state) = self.spring_state {
            // For spring animations, only mark complete when spring has settled
            if state.is_settled(0.01) {
                self.progress = 1.0;
                // Settled means at rest. Keeping the state would let the next
                // retarget inherit whatever velocity was still under the
                // threshold, and rescale it by however short the new segment
                // is — a spring that had visibly stopped starting the next one
                // with a kick.
                settled = true;
                // Snap to exact target to avoid floating-point drift.
                // The spring settles within 0.01 of the target, but downstream
                // checks (e.g. Transform::is_translation_only) use much tighter
                // tolerances (1e-6), so the lerped value must be exact.
                new_value = self.target;
            } else {
                // Keep progress < 1.0 to continue animating
                self.progress = 0.5;
            }
        } else {
            // For non-spring animations, use time-based progress
            let t = (adjusted_elapsed / duration_ms).min(1.0);
            self.progress = t;
        }

        if settled {
            self.spring_state = None;
        }

        // Check if value actually changed
        let changed = self.prev_value.as_ref() != Some(&new_value);
        self.current = new_value;
        self.prev_value = Some(new_value);

        // Completion edge: fire the transition's callback once per run.
        // The callback may write signals or push surface commands; it must
        // not touch the widget tree synchronously (it runs inside
        // advance_animations).
        if was_running
            && !self.is_animating()
            && let Some(cb) = self.active_transition().on_complete.clone()
        {
            // A completion callback reads for the current value, like an
            // event handler — it is not a place to subscribe from.
            crate::reactive::diagnostics::snapshot_zone(|| cb());
        }

        if changed {
            AdvanceResult::Changed(new_value)
        } else {
            AdvanceResult::NoChange
        }
    }

    /// How far past its target this animation can travel, as a fraction of the
    /// distance — the worse of the forward and reverse curves.
    pub fn peak_overshoot(&self) -> f32 {
        let forward = self.transition.timing.peak_overshoot();
        match &self.reverse_transition {
            Some(reverse) => forward.max(reverse.timing.peak_overshoot()),
            None => forward,
        }
    }

    /// Check if animation is still running
    pub fn is_animating(&self) -> bool {
        self.timeline.as_ref().is_some_and(|t| t.playing.is_some())
            || self.progress < 1.0
            || (self.spring_state.is_some() && self.progress < 0.99)
    }

    /// Give this property a sequence to play, keeping whatever already plays
    /// it.
    pub(crate) fn set_timeline(&mut self, keyframes: Keyframes<T>) {
        match &mut self.timeline {
            Some(timeline) => timeline.keyframes = keyframes,
            None => {
                self.timeline = Some(Box::new(Timeline {
                    keyframes,
                    playing: None,
                    trigger: None,
                    last_play: 0,
                }))
            }
        }
    }

    /// Give it the signal that plays it. Ignored without a sequence, which
    /// cannot happen through the builders.
    pub(crate) fn set_play_trigger(&mut self, trigger: Signal<u32>) {
        if let Some(timeline) = &mut self.timeline {
            timeline.last_play = trigger.get_untracked();
            timeline.trigger = Some(trigger);
        }
    }

    /// Carry what a caller declared over from the state this one replaces.
    ///
    /// The builders each construct a fresh `AnimationState`, so without this
    /// the order `keyframes_*`, `animate_*` and `animate_*_from` were written
    /// in would decide what survived — silently, since a trigger would go on
    /// firing against a state that had nothing to play and an enter would
    /// simply never run.
    ///
    /// Both fields, not just the sequence. Carrying one and dropping the other
    /// is the same defect this exists to fix, and it was open on `enter_from`
    /// for as long as this took only the timeline: writing
    /// `animate_scale_from(0.9, ..)` before `animate_scale(..)` lost the enter.
    ///
    /// An `enter_from` already on `self` wins — it came from the builder being
    /// written now, and the later declaration is the one a caller means.
    pub(crate) fn adopt_declarations_of(&mut self, previous: Option<Self>) {
        let Some(previous) = previous else { return };
        if let Some(timeline) = previous.timeline {
            self.timeline = Some(timeline);
        }
        if self.enter_from.is_none() {
            self.enter_from = previous.enter_from;
        }
    }

    /// Whether the trigger has moved since the sequence last played.
    ///
    /// Reading the signal is the subscription, so the pass that asks this is
    /// the pass that gets woken.
    pub(crate) fn wants_play(&self) -> bool {
        self.timeline
            .as_ref()
            .and_then(|t| t.trigger.map(|signal| signal.get() != t.last_play))
            .unwrap_or(false)
    }

    /// The same question, answered once: `true` hands over the play and marks
    /// it taken, so nothing can ask twice for the same change.
    pub(crate) fn take_play(&mut self) -> bool {
        let Some(timeline) = &mut self.timeline else {
            return false;
        };
        let Some(trigger) = timeline.trigger else {
            return false;
        };
        let now = trigger.get();
        if now == timeline.last_play {
            return false;
        }
        timeline.last_play = now;
        true
    }

    /// Start the sequence, from the top. Playing it again while it runs
    /// restarts it: the second refusal is not half a shake.
    pub(crate) fn play(&mut self) {
        if let Some(timeline) = &mut self.timeline
            && !timeline.keyframes.is_empty()
        {
            timeline.playing = Some(Instant::now());
        }
    }

    /// Advance the running timeline. `None` when there is none, or when the
    /// one that was running has just handed the property back.
    fn advance_timeline(&mut self) -> Option<AdvanceResult<T>> {
        // Both halves together, so a `playing` without a sequence to play
        // cannot survive the question. On its own it would keep
        // `is_animating` true for good: a surface asking for a frame every
        // vsync with nothing to draw.
        let Some(timeline) = &mut self.timeline else {
            return None;
        };
        let started = timeline.playing?;

        let elapsed = started.elapsed().as_secs_f32() * 1000.0;
        let Some(value) = timeline.keyframes.value_at(elapsed) else {
            // Over. The property goes back to whatever declares it — by
            // *animating* there from where the sequence left it, not by
            // snapping to it. The declared transition was suspended for the
            // duration, not cancelled, and ending it at the last frame is
            // what made a card jump the moment a hover arrived mid-shake.
            timeline.playing = None;
            let landed = self.current;
            self.begin_segment_from(landed);
            // Returning `None` lets this frame run the ordinary path, so the
            // hand-back is animated by the declared transition and reaches
            // its completion edge — which is what fires `on_complete`.
            return None;
        };

        let changed = self.prev_value.as_ref() != Some(&value);
        self.current = value;
        self.prev_value = Some(value);
        Some(if changed {
            AdvanceResult::Changed(value)
        } else {
            AdvanceResult::NoChange
        })
    }

    /// Get current value
    pub fn current(&self) -> &T {
        &self.current
    }

    /// Get target value
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Take the pending enter value, if any (consumed at first layout).
    pub(crate) fn take_enter_from(&mut self) -> Option<T> {
        self.enter_from.take()
    }

    /// Store an enter value: the first layout starts an animation from it
    /// to the effective target instead of snapping (see
    /// `Container::animate_scale_from`).
    pub(crate) fn with_enter_from(mut self, from: T) -> Self {
        self.enter_from = Some(from);
        self
    }

    /// Begin animating from an explicit value (enter transitions): the
    /// widget appears mid-animation instead of snapping to its target.
    /// Returns whether an animation actually began: `animate_to` refuses a
    /// target that is not a number, and a caller that asked for a paint on the
    /// strength of an enter it did not get would waste the frame.
    pub(crate) fn begin_from(&mut self, from: T, target: T) -> bool {
        if !from.is_reachable() {
            return false;
        }
        self.current = from;
        self.start = from;
        self.initialized = true;
        // Reuse animate_to for direction selection and spring setup; it
        // starts from `current`, which is now the enter value
        let t = target;
        self.target = from; // force the retarget below to fire
        self.animate_to(t);
        self.is_animating()
    }

    /// Set value immediately without animation (for initialization)
    pub fn set_immediate(&mut self, value: T) {
        // The seeding store, and the one that matters most: a value that is
        // not a number lands in `current` and `start` as well as `target`, and
        // from there it never leaves. `advance` computes `lerp(start, target,
        // t)`, which is NaN for every `t` once `start` is, and only the spring
        // branch assigns `target` outright at the end — so a later, perfectly
        // finite target animates from NaN to NaN forever and the widget stays
        // untransformed for the life of the process.
        //
        // Refusing it here leaves the animation where it was, which is a value
        // that can still be animated away from.
        if !value.is_reachable() {
            // Seeded all the same, with whatever it already holds. This is the
            // only thing that sets `initialized`, and a property that never
            // gets it stays `is_initial()` for the life of the process — which
            // also switches off its drift detection, so nothing would ever
            // notice it had stopped following its signal.
            self.initialized = true;
            crate::transform::warn_unusable(format_args!(
                "animation: a seeded value is not a usable number; the property \
                 keeps what it had and stops following its signal"
            ));
            return;
        }
        self.current = value;
        self.target = value;
        self.start = value;
        self.progress = 1.0;
        self.initialized = true;
    }

    /// Check if animation has never been initialized (first layout)
    pub fn is_initial(&self) -> bool {
        !self.initialized
    }
}

/// Macro to advance an animation field, optionally updating its target first.
/// Uses AdvanceResult to determine when to mark dirty flags.
/// Pushes Animation job with appropriate RequiredJob for continuation.
#[macro_export]
macro_rules! advance_anim {
    // Layout animation: marks needs_layout when value changes
    ($self:expr, $anim:ident, $id:expr, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Layout
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Layout animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $id:expr, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Layout
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Paint animation: push paint job when value changes
    ($self:expr, $anim:ident, $id:expr, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Paint
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Paint animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $id:expr, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Paint
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
}

thread_local! {
    /// While set, layout reads animation TARGETS instead of current
    /// values. Content-sized surfaces measure under this flag so their
    /// natural size is animation-invariant: an animated growth resizes
    /// the surface once, to the final size, and the animation plays
    /// inside it — never one compositor configure per frame.
    static MEASURE_FINAL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f` with layout reading animation targets instead of current values.
pub(crate) fn with_measure_final<R>(f: impl FnOnce() -> R) -> R {
    MEASURE_FINAL.with(|m| m.set(true));
    let out = f();
    MEASURE_FINAL.with(|m| m.set(false));
    out
}

pub(crate) fn measuring_final() -> bool {
    MEASURE_FINAL.with(|m| m.get())
}

impl<T: Animatable + Copy> AnimationState<T> {
    /// The value this animation contributes right now.
    ///
    /// Normally the in-flight value — that is the animation. While a natural
    /// size is being measured it is the destination instead, so a
    /// content-sized surface resizes once to where the animation is going and
    /// the animation plays inside it, rather than asking the compositor for a
    /// new size on every frame.
    ///
    /// Every read of an animated value goes through here, so that rule is
    /// stated once.
    #[inline]
    pub fn displayed(&self) -> T {
        if measuring_final() {
            *self.target()
        } else {
            *self.current()
        }
    }
}

/// The animated value if an animation exists, the fallback otherwise.
#[inline]
pub fn get_animated_value<T: Animatable + Copy>(
    anim: Option<&AnimationState<T>>,
    fallback: impl FnOnce() -> T,
) -> T {
    match anim {
        Some(a) => a.displayed(),
        None => fallback(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// on_complete fires exactly once per completed run; a retarget that
    /// completes again fires again.
    #[test]
    fn on_complete_fires_once_per_run() {
        use crate::animation::TimingFunction;
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(0));
        let counter = fired.clone();
        let mut anim = AnimationState::new(
            0.0_f32,
            crate::animation::Transition::new(1, TimingFunction::Linear)
                .on_complete(move || counter.set(counter.get() + 1)),
        );
        anim.set_immediate(0.0);

        anim.animate_to(1.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        anim.advance();
        assert_eq!(fired.get(), 1, "settle must fire the callback once");
        anim.advance();
        assert_eq!(fired.get(), 1, "no refire after completion");

        anim.animate_to(2.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        anim.advance();
        assert_eq!(fired.get(), 2, "a new completed run fires again");
    }
    use crate::animation::TimingFunction;

    /// Step a running sequence to `ms` into its run.
    fn play_at<T: Animatable>(anim: &mut AnimationState<T>, ms: u64) {
        if let Some(timeline) = anim.timeline.as_mut() {
            timeline.playing = Some(Instant::now() - std::time::Duration::from_millis(ms));
        }
        anim.advance();
    }

    /// While a sequence runs it speaks for the property, and when it ends the
    /// property goes back to whatever is declared — including a value that
    /// changed while it was playing.
    #[test]
    fn a_timeline_plays_and_then_hands_the_property_back() {
        use crate::animation::Keyframes;

        let mut anim = AnimationState::new(0.0_f32, Transition::new(0.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.set_timeline(Keyframes::new(60.0).at(0.0, 0.0).at(0.5, 10.0).at(1.0, 0.0));

        anim.play();
        assert!(anim.is_animating(), "a playing timeline is an animation");

        play_at(&mut anim, 30);
        assert!(
            *anim.current() > 4.0,
            "halfway through it should be near the peak, got {}",
            anim.current()
        );

        // The declared value moves while the sequence is running.
        anim.animate_to(3.0);
        play_at(&mut anim, 70);
        at(&mut anim, 10);
        assert_eq!(*anim.current(), 3.0, "over, and back to what is declared");
        assert!(!anim.is_animating());
    }

    /// Played again mid-run it restarts: the second refusal is not half a
    /// shake.
    #[test]
    fn playing_again_starts_the_sequence_over() {
        use crate::animation::Keyframes;

        let mut anim = AnimationState::new(0.0_f32, Transition::new(0.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.set_timeline(Keyframes::new(80.0).at(0.0, 0.0).at(1.0, 8.0));

        anim.play();
        play_at(&mut anim, 60);
        let far = *anim.current();

        anim.play();
        anim.advance();
        assert!(
            *anim.current() < far,
            "back near the top of the run, got {} after {far}",
            anim.current()
        );
    }

    /// The end of a sequence hands the property back to its declared
    /// transition — it does not cancel it.
    ///
    /// Forcing the value to the target on the last frame made the case the
    /// whole feature is sold on jump: a card shaking, a pointer arriving
    /// mid-shake, and the hover landing instantly instead of on its spring.
    #[test]
    fn the_end_of_a_sequence_animates_back_rather_than_snapping() {
        use crate::animation::Keyframes;

        let mut anim = AnimationState::new(0.0_f32, Transition::new(200.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.set_timeline(Keyframes::new(60.0).at(0.0, 0.0).at(1.0, 10.0));

        anim.play();
        play_at(&mut anim, 30);

        // Something declares a new value while the sequence is running.
        anim.animate_to(100.0);

        // The sequence runs out.
        play_at(&mut anim, 70);
        let handed_back = *anim.current();
        assert!(
            handed_back < 100.0,
            "the declared transition has to run, not be skipped: got \
             {handed_back}"
        );
        assert!(anim.is_animating(), "and it is still going");

        at(&mut anim, 400);
        assert_eq!(*anim.current(), 100.0, "arriving under its own transition");
    }

    /// And the transition it hands back to reaches its completion edge, so a
    /// callback gated on the property arriving still fires.
    #[test]
    fn a_sequence_does_not_swallow_the_completion_callback() {
        use crate::animation::Keyframes;
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(0));
        let seen = fired.clone();
        let transition = Transition::new(50.0, TimingFunction::Linear)
            .on_complete(move || seen.set(seen.get() + 1));

        let mut anim = AnimationState::new(0.0_f32, transition);
        anim.set_immediate(0.0);
        anim.set_timeline(Keyframes::new(60.0).at(0.0, 0.0).at(1.0, 5.0));

        anim.animate_to(1.0);
        anim.play();
        play_at(&mut anim, 30);
        assert_eq!(fired.get(), 0, "nothing has arrived yet");

        play_at(&mut anim, 70);
        at(&mut anim, 100);
        assert_eq!(fired.get(), 1, "the hand-back completes, and says so");
    }

    /// A `playing` with nothing to play cannot outlive the sequence: on its
    /// own it would keep `is_animating` true for good, and the surface asking
    /// for a frame every vsync with nothing to draw.
    #[test]
    fn a_play_without_a_sequence_does_not_pin_the_frame_loop() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(10.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.play();
        anim.advance();
        assert!(!anim.is_animating(), "nothing to play, nothing to animate");
    }

    /// The trigger is asked and committed in one place, so the pass that
    /// subscribes and the pass that plays cannot disagree about what they
    /// have seen.
    #[test]
    fn a_trigger_is_taken_once_per_change() {
        use crate::animation::Keyframes;
        use crate::reactive::create_signal;

        let plays = create_signal(0_u32);
        let mut anim = AnimationState::new(0.0_f32, Transition::new(0.0, TimingFunction::Linear));
        anim.set_timeline(Keyframes::new(40.0).at(0.0, 0.0).at(1.0, 1.0));
        anim.set_play_trigger(plays.into());

        assert!(!anim.wants_play(), "nothing has happened yet");

        plays.set(1);
        assert!(anim.wants_play());
        assert!(anim.take_play(), "the first ask takes it");
        assert!(!anim.take_play(), "the second finds nothing left");
        assert!(!anim.wants_play());
    }

    /// Step an animation to `ms` after its segment began.
    ///
    /// `advance` reads the clock through `start_time`, so moving that back is
    /// the whole simulation. Sleeping instead would make the interruption
    /// point depend on how loaded the machine is — and the interesting half of
    /// a spring's phase space is on the far side of its overshoot, which a
    /// stretched sleep wanders into by accident.
    fn at<T: Animatable>(anim: &mut AnimationState<T>, ms: u64) {
        anim.start_time = Instant::now() - std::time::Duration::from_millis(ms);
        anim.advance();
    }

    /// Step through `ms` in 8ms frames, returning the extremes reached.
    fn run(anim: &mut AnimationState<f32>, ms: u64) -> (f32, f32) {
        let (mut low, mut high) = (*anim.current(), *anim.current());
        for frame in 1..=(ms / 8) {
            at(anim, frame * 8);
            low = low.min(*anim.current());
            high = high.max(*anim.current());
        }
        (low, high)
    }

    /// The spring's own state, for the tests that assert on the momentum
    /// rather than on what it produced.
    fn velocity_of<T: Animatable>(anim: &AnimationState<T>) -> f32 {
        anim.spring_state.as_ref().expect("a spring").velocity
    }

    fn spring(config: crate::animation::SpringConfig) -> Transition {
        Transition::new(0.0, TimingFunction::Spring(config))
    }

    /// A spring interrupted mid-flight turns around; it does not stop and
    /// start again. Every target change used to hand the integrator a fresh
    /// `SpringState`, velocity zero, so an animation reversed halfway through
    /// snapped to a halt first — a spring in name and an easing curve in
    /// behaviour.
    #[test]
    fn a_spring_reversed_in_flight_carries_its_momentum() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::BOUNCY));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 40);
        assert!(
            *anim.current() > 0.0,
            "the spring has to be moving before it can be interrupted"
        );

        anim.animate_to(0.0);
        let velocity = velocity_of(&anim);
        assert!(
            velocity < 0.0,
            "still travelling away from the new target, so the new segment \
             starts with the spring having to turn around, got {velocity}"
        );
    }

    /// And that momentum is what a wobble is made of: sent back to where it
    /// came from, the spring overshoots past it rather than easing into it.
    #[test]
    fn the_carried_momentum_overshoots_the_way_back() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::BOUNCY));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 40);
        anim.animate_to(0.0);
        let (low, _) = run(&mut anim, 400);

        assert!(
            low < -0.02,
            "the return leg should cross zero and come back, got {low} at its \
             furthest"
        );
    }

    /// Retargeted the way it was already going, the spring keeps closing.
    #[test]
    fn a_spring_sent_further_the_same_way_keeps_closing() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::DEFAULT));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 40);

        anim.animate_to(2.0);
        assert!(
            velocity_of(&anim) > 0.0,
            "still heading there, got {}",
            velocity_of(&anim)
        );
    }

    /// A value that was standing still starts from rest.
    ///
    /// The obvious version of this test — `set_immediate` then `animate_to` —
    /// asserts nothing: `start == target` makes the segment degenerate, so it
    /// returns before ever reading the velocity. This one lets a spring
    /// genuinely settle first, which is where the residue actually lives.
    #[test]
    fn a_spring_that_has_settled_starts_the_next_one_from_rest() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::DEFAULT));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 1200);
        assert!(!anim.is_animating(), "it has to have settled first");

        anim.animate_to(1.0001);
        assert_eq!(
            velocity_of(&anim),
            0.0,
            "a settled spring is at rest, whatever was left under the threshold"
        );
    }

    /// Interrupted *after* its overshoot — the half of the phase space every
    /// preset spends time in — the spring is travelling backwards along its own
    /// segment, and the momentum it carries has to reflect that.
    ///
    /// Reading the direction off the segment instead of off the integrator
    /// inverts it here: the value is already falling toward the new target and
    /// would be kicked back up, away from it.
    #[test]
    fn a_spring_interrupted_past_its_overshoot_carries_the_way_it_is_moving() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::BOUNCY));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 184);

        assert!(
            *anim.current() > 1.0 && velocity_of(&anim) < 0.0,
            "the setup needs a spring past its target and coming back, got \
             value {} velocity {}",
            anim.current(),
            velocity_of(&anim)
        );

        // Falling toward 1.0 from above is falling toward 0.0 as well.
        anim.animate_to(0.0);
        assert!(
            velocity_of(&anim) > 0.0,
            "it was already heading that way, so the new segment closes on its \
             target, got {}",
            velocity_of(&anim)
        );
    }

    /// Momentum does not leak between channels that have nothing to do with
    /// each other: a slide to the right, interrupted by a slide straight down,
    /// starts the new segment from rest because the two directions are
    /// orthogonal.
    ///
    /// Translation against scale no longer needs saying — they are separate
    /// properties with separate states, so nothing could cross between them
    /// even by mistake.
    #[test]
    fn momentum_does_not_cross_from_one_channel_to_another() {
        use crate::animation::SpringConfig;
        use crate::transform::Translate;

        let mut anim = AnimationState::new(Translate::NONE, spring(SpringConfig::DEFAULT));
        anim.set_immediate(Translate::NONE);
        anim.animate_to(Translate::new(200.0, 0.0));
        at(&mut anim, 40);
        assert!(velocity_of(&anim) > 0.0, "it has to be moving first");

        // Straight down from wherever the rightward slide got to: the y
        // channel moves, the x channel does not.
        let here = *anim.current();
        anim.animate_to(Translate::new(here.x, here.y + 200.0));

        assert!(
            velocity_of(&anim).abs() < 0.5,
            "a rightward slide has no business driving a downward one, got {}",
            velocity_of(&anim)
        );
    }

    /// #212: a half turn used to annihilate the widget. Interpolating the six
    /// matrix coefficients took the diagonal from 1 to -1, so at the midpoint
    /// the matrix was all zeros and the widget shrank to a point and came
    /// back. An angle has no midpoint that is not an angle.
    #[test]
    fn a_half_turn_does_not_collapse_the_widget() {
        use crate::transform::{Scale, Transform, Translate};

        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.animate_to(180.0);

        let mut smallest = f32::INFINITY;
        for frame in 0..=25 {
            at(&mut anim, frame * 4);
            let composed = Transform::compose(Translate::NONE, *anim.current(), Scale::NONE);
            smallest = smallest.min(composed.extract_scale());
        }

        assert!(
            (smallest - 1.0).abs() < 1e-4,
            "a rotation must not change the size it turns; smallest scale was {smallest}"
        );
    }

    /// #212, the other half: 0 -> 360 is the same matrix at both ends, so a
    /// matrix lerp was constant and the widget sat still. The angle is a
    /// number, and 360 is not 0.
    #[test]
    fn a_full_turn_is_a_turn_and_not_a_no_op() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.animate_to(360.0);

        at(&mut anim, 50);
        let halfway = *anim.current();
        assert!(
            (halfway - 180.0).abs() < 5.0,
            "halfway through a full turn is half a turn, got {halfway}"
        );

        at(&mut anim, 100);
        assert!(
            (*anim.current() - 360.0).abs() < 1e-3,
            "and it ends where it was sent, got {}",
            *anim.current()
        );
    }

    /// Nothing takes a shorter way round on the caller's behalf: 350 -> 370 is
    /// twenty degrees forward, not three hundred and forty back. Only the
    /// caller knows whether an angle that crossed the wrap meant to.
    #[test]
    fn an_angle_past_the_wrap_keeps_going_forward() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(350.0);
        anim.animate_to(370.0);

        at(&mut anim, 50);
        let halfway = *anim.current();
        assert!(
            (halfway - 360.0).abs() < 1.0,
            "halfway from 350 to 370 is 360, got {halfway}"
        );
    }

    /// And the angle moves at the rate the easing asks for. The matrix lerp
    /// got the endpoints right and everything between them wrong: a quarter of
    /// the way into a 0 -> 90 run it passed through 8.13 degrees, not 22.5.
    #[test]
    fn the_angle_moves_at_the_rate_the_easing_asks_for() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.animate_to(90.0);

        at(&mut anim, 25);
        assert!(
            (*anim.current() - 22.5).abs() < 1.0,
            "a quarter of the way is a quarter of the angle, got {}",
            *anim.current()
        );
    }

    /// A target that is not a number cannot be reached, and must not be aimed
    /// at: `animate_to`'s early-out is an equality test, NaN fails it against
    /// itself, and the animation would begin a new segment on every pass and
    /// hold the frame callback armed forever.
    #[test]
    fn an_unreachable_target_does_not_pin_the_frame_loop() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(0.0);

        anim.animate_to(f32::NAN);
        assert!(!anim.is_animating(), "nothing to animate towards");
        assert!(anim.current().is_finite(), "and nothing poisoned");

        // And a real target after it still works.
        anim.animate_to(10.0);
        assert!(anim.is_animating());
    }

    /// A refused target leaves the property where it was, rather than half
    /// aimed at something it will never reach.
    #[test]
    fn a_refused_target_leaves_the_property_where_it_was() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(3.0);
        anim.animate_to(f32::NAN);

        assert_eq!(*anim.current(), 3.0, "still where it settled");
        assert!(!anim.is_animating(), "and not chasing anything");
    }

    /// And the seeding store is guarded too — it is the one that poisons for
    /// good, because `advance` interpolates from `start` and only a spring
    /// assigns the target outright at the end.
    #[test]
    fn seeding_an_unreachable_value_does_not_poison_the_property() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(f32::NAN);
        assert!(anim.current().is_finite(), "nothing to interpolate from");

        // The signal comes back to its senses, and so does the property.
        anim.set_immediate(1.0);
        anim.animate_to(2.0);
        at(&mut anim, 100);
        assert!(
            (*anim.current() - 2.0).abs() < 1e-3,
            "got {}",
            *anim.current()
        );
    }

    /// A target that moves every frame — a field growing as it is typed into —
    /// converges instead of building speed without bound.
    #[test]
    fn a_spring_following_a_moving_target_stays_bounded() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::DEFAULT));
        anim.set_immediate(0.0);

        // 40 frames of the target creeping upward, then it stops.
        for frame in 1..=40u64 {
            anim.animate_to(frame as f32);
            at(&mut anim, 8);
        }
        let (_, high) = run(&mut anim, 800);

        assert!(
            high < 60.0,
            "following a target that stopped at 40 must not fly past it, \
             reached {high}"
        );
        assert!(
            (*anim.current() - 40.0).abs() < 0.5,
            "and it has to arrive, got {}",
            anim.current()
        );
    }

    #[test]
    fn a_property_with_no_timeline_cannot_be_played() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(10.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.play();
        assert!(!anim.is_animating(), "nothing to play");
    }

    /// A transition that has not started moving yet has nothing to carry: its
    /// spring is stepped only after the delay, so a velocity stored now would
    /// be released at full strength against a value that had not moved since.
    #[test]
    fn a_delayed_transition_starts_from_rest() {
        use crate::animation::SpringConfig;

        let delayed = Transition::new(0.0, TimingFunction::Spring(SpringConfig::BOUNCY)).delay(200);
        let mut anim = AnimationState::new(0.0_f32, delayed);
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 400);
        assert!(*anim.current() > 0.0, "past the delay and moving");

        anim.animate_to(0.0);
        assert_eq!(velocity_of(&anim), 0.0);
    }

    /// Nothing of this touches the timed transitions, which have no momentum
    /// to keep.
    #[test]
    fn a_timed_transition_has_no_spring_to_carry() {
        let mut anim = AnimationState::new(0.0_f32, Transition::new(100.0, TimingFunction::Linear));
        anim.set_immediate(0.0);
        anim.animate_to(1.0);
        run(&mut anim, 24);
        anim.animate_to(0.0);
        assert!(anim.spring_state.is_none());
    }

    #[test]
    fn test_animation_state_new() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let state = AnimationState::new(0.0f32, transition);

        assert_eq!(*state.current(), 0.0);
        assert_eq!(*state.target(), 0.0);
        assert!(!state.is_animating()); // Starts completed
        assert!(state.is_initial()); // Not yet initialized
    }

    #[test]
    fn test_animation_state_animate_to() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.animate_to(100.0);

        assert_eq!(*state.target(), 100.0);
        assert!(state.is_animating());
    }

    #[test]
    fn test_animation_state_animate_to_same_target() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.animate_to(100.0);
        let first_start_time = state.start_time;

        // Animate to same target should not restart
        state.animate_to(100.0);
        assert_eq!(state.start_time, first_start_time);
    }

    #[test]
    fn test_animation_state_set_immediate() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.set_immediate(50.0);

        assert_eq!(*state.current(), 50.0);
        assert_eq!(*state.target(), 50.0);
        assert!(!state.is_animating());
        assert!(!state.is_initial()); // Now initialized
    }

    #[test]
    fn test_animation_state_is_initial() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        assert!(state.is_initial());

        state.set_immediate(10.0);
        assert!(!state.is_initial());
    }

    #[test]
    fn test_get_animated_value_with_some() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(42.0f32, transition);
        state.set_immediate(42.0);

        let value = get_animated_value(Some(&state), || 0.0);
        assert_eq!(value, 42.0);
    }

    #[test]
    fn test_get_animated_value_with_none() {
        let value = get_animated_value::<f32>(None, || 99.0);
        assert_eq!(value, 99.0);
    }
}
