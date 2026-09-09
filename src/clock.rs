//! Two clocks, two types.
//!
//! A `Tree` answers two questions about time and they are not the same
//! question: *when is this frame being drawn for* and *when did the event now
//! being dispatched happen*. An event's instant is the older of the two — it
//! was stamped when the compositor sent it and is read when the frame that
//! carries it runs — so differencing one against the other measures the loop's
//! input latency, which is almost never what the differencing code meant.
//!
//! Both were `std::time::Instant`, so a value from either fitted anywhere the
//! other was expected and the mistake compiled. `ScrollState::momentum_since`
//! is what that costs: written from the event clock when a finger lifts,
//! written from the frame clock while the flick runs, and differenced against
//! the frame clock to decide whether the motion had gone stale — so the first
//! frame after a lift measured input latency against a 200ms threshold and
//! could cancel a flick that had only just started (#265).
//!
//! Neither type carries `elapsed()`. Nor is either a wall against a determined
//! caller — `From<Instant>` and `into_inner` are both public, because a job
//! deadline genuinely leaves the pass's sequence. What the types buy is that
//! crossing has to be *written*, and every crossing can be found by grepping
//! for the two names that do it.
//!
//! That there is no `elapsed()` is the other half of the same rule: an instant from a pass is a moment in that pass's own
//! sequence, and the wall clock is not in it. Ask the pass what time it is
//! instead.

use std::time::{Duration, Instant};

macro_rules! pass_clock {
    ($name:ident, $what:literal, $owner:literal) => {
        #[doc = concat!("The moment ", $what, ".")]
        ///
        #[doc = concat!("Declared by ", $owner, " and read from anywhere below it. Comparable")]
        /// and differenceable only against its own kind: the other clock
        /// answers a different question, and arithmetic across the two is the
        /// mistake this type exists to stop.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
        pub struct $name(Instant);

        impl $name {
            /// How much of this clock's own sequence separates two of its
            /// moments. Panics if `earlier` is later, as `Instant` does.
            pub fn duration_since(self, earlier: Self) -> Duration {
                self.0.duration_since(earlier.0)
            }

            /// The same, answering zero rather than panicking when the order
            /// is the other way round.
            pub fn saturating_duration_since(self, earlier: Self) -> Duration {
                self.0.saturating_duration_since(earlier.0)
            }

            /// This moment as a plain `Instant`.
            ///
            /// For the places that genuinely leave the pass's sequence: a job
            /// deadline the loop will compare against the wall clock, a
            /// duration added to reach one. Every other use is the arithmetic
            /// the type exists to prevent, written the long way round.
            pub fn into_inner(self) -> Instant {
                self.0
            }
        }

        impl std::ops::Add<Duration> for $name {
            type Output = Self;

            /// A moment later on the same clock, which is what a deadline
            /// inside a pass's own sequence is.
            fn add(self, later: Duration) -> Self {
                Self(self.0 + later)
            }
        }

        impl From<Instant> for $name {
            fn from(at: Instant) -> Self {
                Self(at)
            }
        }
    };
}

pass_clock!(
    FrameInstant,
    "the frame now running is being drawn for",
    "`render_surface`, around the three passes a frame is made of"
);
pass_clock!(
    EventInstant,
    "the event now being dispatched happened",
    "`dispatch_events`, once per event"
);

impl EventInstant {
    /// The same moment, read as the start of a frame timeline.
    ///
    /// A widget whose animation runs on frames is often *started* by an event:
    /// a ripple by the press under it, a caret's blink by the keystroke that
    /// moved it. The two clocks are separated by the loop's input latency, so
    /// this shifts such a timeline's start by that much.
    ///
    /// Sound where the value is where a timeline *begins* — the moment
    /// progress is measured forward from, including a phase origin like the
    /// caret's blink. Not sound where it is a *deadline* whose crossing
    /// cancels something, because then the latency is the thing being
    /// measured: #265 is a flick cancelled on its first frame because a loop's
    /// lateness was read as the motion having gone idle.
    ///
    /// Deliberate, greppable, and the only way across.
    pub fn as_frame_start(self) -> FrameInstant {
        FrameInstant(self.0)
    }
}
