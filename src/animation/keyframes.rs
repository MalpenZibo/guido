//! Keyframes: a value written down at points along a run, rather than aimed at.
//!
//! Every other animation in guido moves *towards* something: a property gets a
//! new value and a transition carries it there. That covers the whole of what a
//! state change looks like, and none of what a sequence looks like — a shake, a
//! flash, a bounce, anything that has to pass through somewhere on its way and
//! end where it started.
//!
//! A timeline is the other shape. It has no target; it plays, from a trigger,
//! and while it plays it *replaces* the declared value — the same rule CSS
//! settled on, where an animation outranks a normal declaration for as long as
//! it runs and hands the property back afterwards.
//!
//! ```ignore
//! container()
//!     .keyframes_transform(
//!         Keyframes::new(320.0)
//!             .at(0.0, Transform::IDENTITY)
//!             .at(0.2, Transform::rotate_degrees(1.5))
//!             .at(0.5, Transform::rotate_degrees(-1.0))
//!             .at(0.8, Transform::rotate_degrees(0.4))
//!             .at(1.0, Transform::IDENTITY),
//!         rejections,
//!     )
//! ```
//!
//! # What the offsets mean
//!
//! An offset is a fraction of one run, `0.0` to `1.0`, and the easing declared
//! at a stop governs the segment that *starts* there — CSS's rule, where
//! `animation-timing-function` inside a keyframe applies from that keyframe
//! onwards. Before the first stop and after the last one the nearest one holds.

use crate::animation::{Animatable, TimingFunction};
use crate::layout::IntoF32;

/// One point on a timeline: where it sits, what the value is there, and how the
/// segment leaving it is eased.
#[derive(Clone)]
struct Stop<T> {
    offset: f32,
    value: T,
    easing: TimingFunction,
}

/// A sequence of values over a fixed duration, played on a trigger.
#[derive(Clone)]
pub struct Keyframes<T> {
    stops: Vec<Stop<T>>,
    duration_ms: f32,
    repeat: u32,
}

impl<T: Animatable> Keyframes<T> {
    /// A timeline that takes `duration_ms` to play once.
    pub fn new(duration_ms: impl IntoF32) -> Self {
        Self {
            stops: Vec::new(),
            duration_ms: duration_ms.into_f32().max(1.0),
            repeat: 1,
        }
    }

    /// A stop, at a fraction of the run. The segment leaving it is linear.
    pub fn at(self, offset: f32, value: T) -> Self {
        self.at_with(offset, value, TimingFunction::Linear)
    }

    /// A stop whose outgoing segment is eased.
    pub fn at_with(mut self, offset: f32, value: T, easing: TimingFunction) -> Self {
        let offset = offset.clamp(0.0, 1.0);
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

    /// Play it `times` times in a row. Once by default.
    pub fn repeat(mut self, times: u32) -> Self {
        self.repeat = times.max(1);
        self
    }

    /// How long the whole thing lasts, repeats included.
    pub fn total_ms(&self) -> f32 {
        self.duration_ms * self.repeat as f32
    }

    /// Whether there is anything to play.
    pub fn is_empty(&self) -> bool {
        self.stops.is_empty()
    }

    /// The value `elapsed_ms` into the run, or `None` once it is over.
    pub fn value_at(&self, elapsed_ms: f32) -> Option<T> {
        if self.stops.is_empty() || elapsed_ms >= self.total_ms() {
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
        let next = self
            .stops
            .iter()
            .position(|stop| stop.offset > t)
            .unwrap_or(self.stops.len() - 1);
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
        let kf = shake().repeat(2);
        assert_eq!(kf.total_ms(), 600.0);
        assert_eq!(kf.value_at(150.0), Some(10.0));
        assert_eq!(kf.value_at(450.0), Some(10.0), "the peak of the second run");
        assert_eq!(kf.value_at(600.0), None);
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
}
