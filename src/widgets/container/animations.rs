use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition, TransitionConfig};

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

        self.start = self.current;
        self.target = new_target;
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
    /// A spring integrates in a space normalised over its own segment, so a
    /// new target means a new space: the speed is converted back into the
    /// property's own units, then into the new segment's. The sign says
    /// whether the motion is already heading towards the new target — a
    /// negative one is a spring that has to be turned around, which is the
    /// whole point of keeping it.
    ///
    /// Zero unless a spring is actually in flight. Starting from rest is right
    /// for a value that was standing still, and is what this did for every
    /// target change before: a hover reversed mid-flight restarted from a stop
    /// instead of turning around, and a spring that never keeps its momentum
    /// is an easing curve wearing a spring's name.
    fn carried_velocity(&self, new_target: &T) -> f32 {
        let Some(spring) = &self.spring_state else {
            return 0.0;
        };
        let old_span = T::distance(&self.start, &self.target);
        let new_span = T::distance(&self.current, new_target);
        if old_span <= f32::EPSILON || new_span <= f32::EPSILON {
            return 0.0;
        }

        // Normalised velocity is per unit of the old segment; the property
        // moves `speed` of its own units per second.
        let speed = spring.velocity * old_span;

        // Which way that is going, against the new target: a step further
        // along the direction of travel either closes the distance or opens
        // it. `lerp` and `distance` are all this needs, so it holds for every
        // animatable type rather than just the scalar ones.
        let ahead = T::lerp(&self.start, &self.target, spring.position + 0.01);
        let closing = T::distance(&ahead, new_target) < T::distance(&self.current, new_target);

        let magnitude = speed.abs() / new_span;
        if closing { magnitude } else { -magnitude }
    }

    /// Advance the animation and return whether the value changed
    pub fn advance(&mut self) -> AdvanceResult<T> {
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

        // Update progress
        if let Some(ref state) = self.spring_state {
            // For spring animations, only mark complete when spring has settled
            if state.is_settled(0.01) {
                self.progress = 1.0;
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

    /// Check if animation is still running
    pub fn is_animating(&self) -> bool {
        self.progress < 1.0 || (self.spring_state.is_some() && self.progress < 0.99)
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
    /// `Container::animate_transform_from`).
    pub(crate) fn with_enter_from(mut self, from: T) -> Self {
        self.enter_from = Some(from);
        self
    }

    /// Begin animating from an explicit value (enter transitions): the
    /// widget appears mid-animation instead of snapping to its target.
    pub(crate) fn begin_from(&mut self, from: T, target: T) {
        self.current = from;
        self.start = from;
        self.initialized = true;
        // Reuse animate_to for direction selection and spring setup; it
        // starts from `current`, which is now the enter value
        let t = target;
        self.target = from; // force the retarget below to fire
        self.animate_to(t);
    }

    /// Set value immediately without animation (for initialization)
    pub fn set_immediate(&mut self, value: T) {
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

    /// Step a spring for `ms`, in frames, returning the extreme value reached.
    fn run(anim: &mut AnimationState<f32>, ms: u64) -> (f32, f32) {
        let (mut low, mut high) = (*anim.current(), *anim.current());
        for _ in 0..(ms / 8) {
            std::thread::sleep(std::time::Duration::from_millis(8));
            anim.advance();
            low = low.min(*anim.current());
            high = high.max(*anim.current());
        }
        (low, high)
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
        let velocity = anim.spring_state.as_ref().expect("a spring").velocity;
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
        let velocity = anim.spring_state.as_ref().expect("a spring").velocity;
        assert!(velocity > 0.0, "still heading there, got {velocity}");
    }

    /// A value that was standing still starts from rest, as it always did.
    #[test]
    fn a_settled_spring_starts_from_rest() {
        use crate::animation::SpringConfig;

        let mut anim = AnimationState::new(0.0_f32, spring(SpringConfig::DEFAULT));
        anim.set_immediate(0.0);

        anim.animate_to(1.0);
        assert_eq!(anim.spring_state.as_ref().expect("a spring").velocity, 0.0);
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
