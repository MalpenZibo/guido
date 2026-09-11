//! Keyframes: a value written down at points along a run, rather than aimed at.
//!
//! Every other animation in guido moves *towards* something: a property gets a
//! new value and a transition carries it there. That covers the whole of what a
//! state change looks like, and none of what a sequence looks like — a shake, a
//! flash, a bounce, anything that has to pass through somewhere on its way and
//! end where it started.
//!
//! A timeline is the other shape. It has no target; it plays — as soon as the
//! widget exists, or on a trigger if it was given one — and while it plays it
//! *replaces* the declared value — the same rule CSS
//! settled on, where an animation outranks a normal declaration for as long as
//! it runs and hands the property back afterwards.
//!
//! It is declared where the value is, with
//! [`timeline`](crate::animation::Animate::timeline), on whatever the property
//! rests at between plays:
//!
//! ```no_run
//! # use guido::prelude::*;
//! # let rejections = create_signal(0u32);
//! container().rotate(0.0.timeline(
//!     Keyframes::new(320.0)
//!         .at(0.0, 0.0)
//!         .at(0.2, 1.5)
//!         .at(0.5, -1.0)
//!         .at(0.8, 0.4)
//!         .at(1.0, 0.0)
//!         .played_by(rejections),
//! ));
//! ```
//!
//! A timeline belongs to the one property it moves — a rotation for a shake, a
//! translation for a nod, a background for a flash — so a hover that scales is
//! untouched by a sequence that rotates.
//!
//! # What the offsets mean
//!
//! An offset is a fraction of one run, `0.0` to `1.0`, and the easing declared
//! at a stop governs the segment that *starts* there — CSS's rule, where
//! `animation-timing-function` inside a keyframe applies from that keyframe
//! onwards. Before the first stop and after the last one the nearest one holds.

use crate::animation::{Animatable, TimingFunction};
use crate::layout::IntoF32;
use crate::reactive::{IntoSignal, Signal};

/// One point on a timeline: where it sits, what the value is there, and how the
/// segment leaving it is eased.
#[derive(Clone)]
struct Stop<T> {
    offset: f32,
    value: T,
    easing: TimingFunction,
}

/// How many times a sequence runs.
///
/// One field with two kinds of value, so "three times *and* endlessly" cannot
/// be written. `animation-iteration-count` is the same shape: a number, or
/// `infinite`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Repeat {
    /// Run this many times and then rest on the declared value. At least
    /// once: a count of zero is stored as one by
    /// [`repeat`](Keyframes::repeat).
    Times(u32),
    /// Run until the widget carrying it is gone. There is no other way to stop
    /// it, which is the point: the widget being on screen *is* the animation
    /// running, so showing and hiding it is the only switch to keep in step.
    Forever,
}

/// A sequence of values over a fixed duration.
///
/// Plays as soon as the widget carrying it exists, unless it is
/// [`played_by`](Self::played_by) a trigger — a shake waits to be asked, a
/// spinner does not.
#[derive(Clone)]
pub struct Keyframes<T> {
    stops: Vec<Stop<T>>,
    duration_ms: f32,
    repeat: Repeat,
    /// What replays it, if anything. `None` plays once from the moment the
    /// widget appears, which is what a wait wants and what a gesture does not.
    trigger: Option<Signal<u32>>,
}

impl<T: Animatable> Keyframes<T> {
    /// A timeline that takes `duration_ms` to play once.
    pub fn new(duration_ms: impl IntoF32) -> Self {
        Self {
            stops: Vec::new(),
            duration_ms: duration_ms.into_f32().max(1.0),
            repeat: Repeat::Times(1),
            trigger: None,
        }
    }

    /// A stop, at a fraction of the run. The segment leaving it is linear.
    pub fn at(self, offset: f32, value: T) -> Self {
        self.at_with(offset, value, TimingFunction::Linear)
    }

    /// A stop whose outgoing segment is eased.
    pub fn at_with(mut self, offset: f32, value: T, easing: TimingFunction) -> Self {
        // `f32::clamp` passes NaN straight through, and an offset that
        // compares false against everything makes the search below find no
        // stop at all. An offset is usually computed — `i as f32 / count` with
        // an empty count is all it takes — so this is absorbed at the door
        // rather than left to surface as an index one place before the first.
        let offset = if offset.is_nan() {
            0.0
        } else {
            offset.clamp(0.0, 1.0)
        };

        // A spring has no duration, and a keyframe segment is nothing but one.
        // `TimingFunction::evaluate` returns `t` unchanged for a spring, so
        // taking it at face value would play the segment dead linear and say
        // nothing about it.
        let easing = match easing {
            TimingFunction::Spring(_) => {
                #[cfg(debug_assertions)]
                log::warn!(
                    "keyframes: a spring cannot ease a segment of fixed \
                     duration; using EaseInOut for the stop at {offset}"
                );
                TimingFunction::EaseInOut
            }
            other => other,
        };
        let stop = Stop {
            offset,
            value,
            easing,
        };
        // Kept sorted on the way in: written out of order is a mistake worth
        // absorbing rather than a shape worth honouring.
        let at = self
            .stops
            .iter()
            .position(|existing| existing.offset > offset)
            .unwrap_or(self.stops.len());
        self.stops.insert(at, stop);
        self
    }

    /// How many times it runs. Once by default, and never fewer —
    /// [`Repeat::Times`] is at least once.
    pub fn repeat(mut self, repeat: Repeat) -> Self {
        // A count is usually computed, and `items.len()` on an empty list is
        // all it takes to compute zero. Absorbed here, as the duration and a
        // NaN offset are, so that what is stored means what it says rather
        // than being corrected by whoever reads it.
        self.repeat = match repeat {
            Repeat::Times(times) => Repeat::Times(times.max(1)),
            Repeat::Forever => Repeat::Forever,
        };
        self
    }

    /// Replay it on every change of `trigger`, instead of playing when the
    /// widget appears.
    ///
    /// A count and not a flag, because two refusals in a row are two events
    /// and a signal that stays equal notifies nobody.
    pub fn played_by<M>(mut self, trigger: impl IntoSignal<u32, M>) -> Self {
        self.trigger = Some(trigger.into_signal());
        self
    }

    pub(crate) fn trigger(&self) -> Option<Signal<u32>> {
        self.trigger
    }

    /// How long the whole thing lasts, repeats included, or `None` where it
    /// does not end.
    pub fn total_ms(&self) -> Option<f32> {
        match self.repeat {
            Repeat::Times(times) => Some(self.duration_ms * times as f32),
            Repeat::Forever => None,
        }
    }

    /// How long one run takes.
    pub(crate) fn duration_ms(&self) -> f32 {
        self.duration_ms
    }

    /// Whether there is anything to play.
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// The value `elapsed_ms` into the run, or `None` once it is over.
    pub fn value_at(&self, elapsed_ms: f32) -> Option<T> {
        if self.stops.is_empty() {
            return None;
        }
        if self.total_ms().is_some_and(|total| elapsed_ms >= total) {
            return None;
        }
        let within = (elapsed_ms.max(0.0) % self.duration_ms) / self.duration_ms;
        Some(self.sample(within))
    }

    /// The value at `t` in `0.0..=1.0` of a single run.
    fn sample(&self, t: f32) -> T {
        let first = &self.stops[0];
        if t <= first.offset {
            return first.value;
        }
        let last = &self.stops[self.stops.len() - 1];
        if t >= last.offset {
            return last.value;
        }

        // The pair this sits between. Timelines are short by nature — a
        // handful of stops — so a scan is the whole search.
        //
        // Never zero: `stops[next - 1]` is the stop before this one, and the
        // two early returns above have already covered every `t` that could
        // land before the first. The floor makes that structural rather than
        // an argument, since the only way past them was a comparison that
        // answered false to everything.
        let next = self
            .stops
            .iter()
            .position(|stop| stop.offset > t)
            .unwrap_or(self.stops.len() - 1)
            .max(1);
        let from = &self.stops[next - 1];
        let to = &self.stops[next];

        let span = to.offset - from.offset;
        if span <= f32::EPSILON {
            return to.value;
        }
        let local = (t - from.offset) / span;
        T::lerp(&from.value, &to.value, from.easing.evaluate(local))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shake() -> Keyframes<f32> {
        Keyframes::new(300.0)
            .at(0.0, 0.0)
            .at(0.5, 10.0)
            .at(1.0, 0.0)
    }

    #[test]
    fn a_timeline_passes_through_its_stops() {
        let kf = shake();
        assert_eq!(kf.value_at(0.0), Some(0.0));
        assert_eq!(kf.value_at(150.0), Some(10.0));
        assert_eq!(kf.value_at(75.0), Some(5.0), "halfway up the first segment");
        assert_eq!(kf.value_at(225.0), Some(5.0), "and halfway back down");
    }

    /// The end is what hands the property back to whatever declared it.
    #[test]
    fn a_finished_timeline_has_no_value_of_its_own() {
        assert_eq!(shake().value_at(300.0), None);
        assert_eq!(shake().value_at(1000.0), None);
    }

    #[test]
    fn stops_written_out_of_order_are_put_in_order() {
        let kf = Keyframes::new(100.0)
            .at(1.0, 0.0)
            .at(0.0, 0.0)
            .at(0.5, 10.0);
        assert_eq!(kf.value_at(50.0), Some(10.0));
    }

    /// Before the first stop and after the last, the nearest one holds — a
    /// timeline that starts at 0.25 is not a timeline that starts undefined.
    #[test]
    fn the_ends_hold_rather_than_extrapolate() {
        let kf = Keyframes::new(100.0).at(0.25, 4.0).at(0.75, 8.0);
        assert_eq!(kf.value_at(0.0), Some(4.0));
        assert_eq!(kf.value_at(90.0), Some(8.0));
    }

    #[test]
    fn a_repeat_plays_the_same_run_again() {
        let kf = shake().repeat(Repeat::Times(2));
        assert_eq!(kf.total_ms(), Some(600.0));
        assert_eq!(kf.value_at(150.0), Some(10.0));
        assert_eq!(kf.value_at(450.0), Some(10.0), "the peak of the second run");
        assert_eq!(kf.value_at(600.0), None);
    }

    /// The stored count is the discriminating one: with the clamp at the
    /// reader, `total_ms` answered 300 either way and the field still said
    /// zero.
    #[test]
    fn a_count_of_zero_is_kept_as_the_one_run_it_means() {
        assert_eq!(shake().repeat(Repeat::Times(0)).repeat, Repeat::Times(1));
    }

    #[test]
    fn easing_declared_at_a_stop_governs_the_segment_leaving_it() {
        // EaseIn starts slow, so a quarter of the way through the first
        // segment is less than a quarter of the way up.
        let eased = Keyframes::new(100.0)
            .at_with(0.0, 0.0, TimingFunction::EaseIn)
            .at(1.0, 10.0);
        let linear = Keyframes::new(100.0).at(0.0, 0.0).at(1.0, 10.0);
        assert!(eased.value_at(25.0).unwrap() < linear.value_at(25.0).unwrap());
    }

    #[test]
    fn an_empty_timeline_plays_nothing() {
        let kf: Keyframes<f32> = Keyframes::new(100.0);
        assert!(kf.is_empty());
        assert_eq!(kf.value_at(0.0), None);
    }

    /// An offset that compares false against everything used to send the
    /// search one place before the first stop.
    #[test]
    fn a_nan_offset_does_not_walk_off_the_front() {
        let kf = Keyframes::new(100.0).at(f32::NAN, 7.0_f32);
        assert_eq!(kf.value_at(0.0), Some(7.0));
        assert_eq!(kf.value_at(50.0), Some(7.0));
    }

    #[test]
    fn a_nan_offset_lands_at_the_start() {
        let kf = Keyframes::new(100.0).at(f32::NAN, 0.0_f32).at(1.0, 10.0);
        assert_eq!(kf.value_at(0.0), Some(0.0));
        assert_eq!(kf.value_at(99.0).map(|v| v > 9.0), Some(true));
    }

    /// A spring has no duration and a segment is nothing but one, so it cannot
    /// be the easing — and it must not quietly become a straight line either.
    #[test]
    fn a_spring_easing_is_replaced_rather_than_played_flat() {
        use crate::animation::SpringConfig;

        let sprung = Keyframes::new(100.0)
            .at_with(0.0, 0.0_f32, TimingFunction::Spring(SpringConfig::BOUNCY))
            .at(1.0, 10.0);
        let eased = Keyframes::new(100.0)
            .at_with(0.0, 0.0_f32, TimingFunction::EaseInOut)
            .at(1.0, 10.0);

        assert_eq!(sprung.value_at(25.0), eased.value_at(25.0));
        assert_ne!(
            sprung.value_at(25.0),
            Keyframes::new(100.0)
                .at(0.0, 0.0_f32)
                .at(1.0, 10.0)
                .value_at(25.0),
            "and it is not the linear one either"
        );
    }
}
