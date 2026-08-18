//! Material's ripple: a disc that grows from the point of contact and fades.
//!
//! Two properties define the effect, and both are load-bearing.
//!
//! **The radius never goes backwards.** The exit is a fade of the opacity, not
//! a contraction of the disc. An ink drop that retreats into the finger says
//! the opposite of what the gesture did, and it is what every Material
//! implementation avoids — Flutter fades out, Compose fades out, Android fades
//! out.
//!
//! **A short press does not truncate the growth.** A mouse click lasts 60-150
//! milliseconds against a growth measured in hundreds, so the release lands
//! mid-expansion essentially always. Releasing therefore *finishes* the
//! expansion — the remainder is compressed into the exit — rather than
//! abandoning it wherever it got to. This is Flutter's `confirm()`, and it is
//! the difference between a click that reads as accepted and one that reads as
//! interrupted.
//!
//! Leaving without releasing is the other case, and it is not the same one:
//! nothing was activated, so there is nothing to complete. It just fades, fast.
//! That is Flutter's `cancel()`.
//!
//! # Several at once
//!
//! Each press is its own ripple and they overlap, because that is what the
//! gesture was: two clicks are two events, and collapsing them into one disc
//! that restarts loses the second. [`MAX_LIVE`] bounds the work; past it the
//! oldest is dropped, which is the one nearest to invisible anyway.

use std::time::Instant;

use smallvec::SmallVec;

use crate::widgets::state_layer::RippleConfig;

/// Opacity rise, in seconds. Short: the disc should be there on contact, not
/// arrive after it.
const FADE_IN: f32 = 0.075;

/// Growth of a ripple nobody has released yet, in seconds. Deliberately slow —
/// a held button keeps spreading, and the release is what finishes it.
const HELD_GROWTH: f32 = 1.0;

/// How long the *remaining* growth takes once the press is confirmed.
const CONFIRM_GROWTH: f32 = 0.225;

/// Opacity fall after a confirmed press. Longer than the growth it overlaps,
/// so the disc is still visible as it completes.
const FADE_OUT: f32 = 0.375;

/// Opacity fall after the pointer leaves without releasing. Nothing was
/// activated, so the feedback should get out of the way.
const CANCEL_FADE: f32 = 0.075;

/// Where the disc starts, as a fraction of its final radius. Material's ripple
/// appears as a disc rather than growing from a point.
pub const RIPPLE_START_RADIUS: f32 = 0.3;

/// How many ripples may be alive at once.
pub const MAX_LIVE: usize = 4;

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn since(start: Instant, now: Instant) -> f32 {
    now.saturating_duration_since(start).as_secs_f32()
}

/// Why a ripple stopped growing on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitKind {
    /// Released inside: the press happened, so the expansion completes.
    Confirmed,
    /// The pointer left without releasing: nothing to complete.
    Cancelled,
}

/// The moment a ripple began to leave, and the state it left from.
#[derive(Debug, Clone, Copy)]
struct Exit {
    kind: ExitKind,
    at: Instant,
    /// Growth when the exit began, so the remainder can be compressed into it
    /// instead of restarting from zero.
    growth: f32,
    opacity: f32,
}

/// One press.
#[derive(Debug, Clone)]
pub struct Ripple {
    /// Where the pointer went down, in local container coordinates.
    origin: (f32, f32),
    born: Instant,
    exit: Option<Exit>,
    /// 0 at the start radius, 1 fully expanded.
    growth: f32,
    opacity: f32,
}

impl Ripple {
    fn new(origin: (f32, f32), now: Instant) -> Self {
        Self {
            origin,
            born: now,
            exit: None,
            growth: 0.0,
            opacity: 0.0,
        }
    }

    /// Where the pointer went down, in local container coordinates.
    pub fn origin(&self) -> (f32, f32) {
        self.origin
    }

    /// How far along the expansion is: 0 at [`RIPPLE_START_RADIUS`], 1 covering the
    /// container. Also drives the drift of the centre — see
    /// [`RippleState::iter`].
    pub fn growth(&self) -> f32 {
        self.growth
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    fn is_leaving(&self) -> bool {
        self.exit.is_some()
    }

    fn begin_exit(&mut self, kind: ExitKind, now: Instant) {
        if self.exit.is_some() {
            return;
        }
        self.exit = Some(Exit {
            kind,
            at: now,
            growth: self.growth,
            opacity: self.opacity,
        });
    }

    /// The growth a ripple nobody has released yet has reached.
    fn held_growth(&self, config: &RippleConfig, now: Instant) -> f32 {
        let duration = HELD_GROWTH / config.expand_speed;
        ease_out((since(self.born, now) / duration).min(1.0))
    }

    /// Advance, and report whether this ripple is still worth drawing.
    fn advance(&mut self, config: &RippleConfig, now: Instant) -> bool {
        let Some(exit) = self.exit else {
            self.growth = self.held_growth(config, now);
            self.opacity = (since(self.born, now) / FADE_IN).min(1.0);
            // A ripple held past its growth is not animating: it just sits
            // there until the release, and the frame loop must be allowed to
            // go quiet.
            return self.growth < 1.0 || self.opacity < 1.0;
        };

        let elapsed = since(exit.at, now);
        match exit.kind {
            ExitKind::Confirmed => {
                // The remainder of the expansion, compressed. Never backwards:
                // it starts from where the release found it.
                let t = (elapsed / (CONFIRM_GROWTH / config.expand_speed)).min(1.0);
                self.growth = exit.growth + (1.0 - exit.growth) * ease_out(t);

                let f = (elapsed / (FADE_OUT / config.fade_speed)).min(1.0);
                self.opacity = exit.opacity * (1.0 - ease_out(f));
                f < 1.0
            }
            ExitKind::Cancelled => {
                // Nothing was activated, so the growth is not finished for it —
                // it simply carries on at its own pace while the disc leaves.
                self.growth = self.held_growth(config, now);

                let f = (elapsed / (CANCEL_FADE / config.fade_speed)).min(1.0);
                self.opacity = exit.opacity * (1.0 - f);
                f < 1.0
            }
        }
    }
}

/// Every ripple currently alive on one container.
#[derive(Debug, Clone, Default)]
pub struct RippleState {
    live: SmallVec<[Ripple; 2]>,
}

impl RippleState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a ripple at the given local coordinates.
    ///
    /// The coordinates are relative to the container's origin (0,0 = top-left).
    pub fn start(&mut self, local_x: f32, local_y: f32, now: Instant) {
        // At capacity the oldest goes. It is the one furthest through its own
        // fade, so it is the least of them to lose.
        while self.live.len() >= MAX_LIVE {
            self.live.remove(0);
        }
        self.live.push(Ripple::new((local_x, local_y), now));
    }

    /// The press was released inside: finish its expansion and fade it out.
    ///
    /// Only the ripple still being held is confirmed. The others already have
    /// an exit of their own and keep it.
    pub fn release(&mut self, now: Instant) {
        if let Some(held) = self.live.iter_mut().rev().find(|r| !r.is_leaving()) {
            held.begin_exit(ExitKind::Confirmed, now);
        }
    }

    /// The pointer left without releasing: nothing was activated, so every
    /// ripple still being held just goes.
    pub fn cancel(&mut self, now: Instant) {
        for ripple in self.live.iter_mut().filter(|r| !r.is_leaving()) {
            ripple.begin_exit(ExitKind::Cancelled, now);
        }
    }

    /// Whether anything is on screen.
    pub fn is_active(&self) -> bool {
        !self.live.is_empty()
    }

    /// Whether anything is still moving.
    pub fn is_animating(&self) -> bool {
        self.live
            .iter()
            .any(|r| r.is_leaving() || r.growth < 1.0 || r.opacity < 1.0)
    }

    /// The ripples to draw, oldest first.
    ///
    /// The caller owns the geometry: the radius is
    /// `max_radius * (RIPPLE_START_RADIUS + (1 - RIPPLE_START_RADIUS) * growth)`, and the
    /// centre drifts from [`Ripple::origin`] toward the container's own centre
    /// by the same `growth` — which is what makes a ripple settle onto the
    /// button instead of staying lopsided.
    pub fn iter(&self) -> impl Iterator<Item = &Ripple> {
        self.live.iter()
    }

    /// Advance every ripple, dropping the ones that have faded out.
    ///
    /// Returns whether any is still animating. Dropping a ripple at zero
    /// opacity needs no deferred frame: it draws nothing at that point, so
    /// there is no last frame to preserve.
    pub fn advance(&mut self, config: &RippleConfig, now: Instant) -> bool {
        let mut animating = false;
        self.live.retain_mut(|ripple| {
            let alive = ripple.advance(config, now);
            animating |= alive;
            alive || ripple.opacity > 0.0
        });
        animating
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn cfg() -> RippleConfig {
        RippleConfig::default()
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// The property the old model broke: a click grows the disc and never
    /// pulls it back.
    #[test]
    fn a_click_never_moves_the_radius_backwards() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);

        // 80ms of press, the middle of a real click.
        state.advance(&cfg(), at(t0, 80));
        let at_release = state.iter().next().unwrap().growth();
        state.release(at(t0, 80));

        let mut previous = at_release;
        for ms in [100, 140, 200, 260, 320, 400, 460] {
            state.advance(&cfg(), at(t0, ms));
            let Some(ripple) = state.iter().next() else {
                break;
            };
            assert!(
                ripple.growth() >= previous - f32::EPSILON,
                "growth went backwards at {ms}ms: {previous} -> {}",
                ripple.growth()
            );
            previous = ripple.growth();
        }
    }

    /// And it gets all the way there: the release finishes the expansion
    /// instead of abandoning it.
    #[test]
    fn a_click_completes_the_expansion_it_started() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.advance(&cfg(), at(t0, 80));
        state.release(at(t0, 80));

        // The compressed remainder lands well inside the fade it overlaps.
        state.advance(&cfg(), at(t0, 80 + 225));
        let ripple = state.iter().next().expect("still visible");
        assert!(
            ripple.growth() > 0.99,
            "the expansion has to finish, got {}",
            ripple.growth()
        );
        assert!(ripple.opacity() > 0.0, "and still be visible while it does");
    }

    #[test]
    fn a_confirmed_ripple_leaves_when_it_has_faded() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(at(t0, 80));

        assert!(state.advance(&cfg(), at(t0, 200)));
        assert!(state.is_active());

        state.advance(&cfg(), at(t0, 80 + 400));
        assert!(!state.is_active(), "a faded ripple is not kept around");
    }

    /// Leaving without releasing is a different event and gets a different
    /// exit: no completion, and a fade short enough to get out of the way.
    #[test]
    fn leaving_without_releasing_does_not_complete_the_expansion() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.advance(&cfg(), at(t0, 80));
        let at_exit = state.iter().next().unwrap().growth();

        state.cancel(at(t0, 80));
        state.advance(&cfg(), at(t0, 80 + 40));
        let ripple = state.iter().next().expect("still fading");
        assert!(
            ripple.growth() < at_exit + 0.1,
            "a cancelled ripple is not rushed to completion"
        );

        state.advance(&cfg(), at(t0, 80 + 80));
        assert!(!state.is_active(), "and it leaves quickly");
    }

    /// Two clicks are two events, and they overlap rather than replacing each
    /// other.
    #[test]
    fn a_second_press_does_not_erase_the_first() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(at(t0, 80));
        state.advance(&cfg(), at(t0, 120));

        state.start(90.0, 40.0, at(t0, 120));
        state.advance(&cfg(), at(t0, 160));

        let origins: Vec<_> = state.iter().map(|r| r.origin()).collect();
        assert_eq!(origins, vec![(10.0, 10.0), (90.0, 40.0)]);
    }

    /// A release confirms the press being held, not the ones already leaving.
    #[test]
    fn a_release_confirms_only_the_ripple_still_held() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(at(t0, 50));
        state.start(90.0, 40.0, at(t0, 60));

        state.release(at(t0, 100));

        let leaving: Vec<_> = state.iter().map(|r| r.is_leaving()).collect();
        assert_eq!(leaving, vec![true, true]);
        // The first one's exit was not restarted by the second release.
        let first = state.iter().next().unwrap();
        assert_eq!(first.exit.map(|e| e.at), Some(at(t0, 50)));
    }

    #[test]
    fn the_oldest_goes_when_the_bound_is_reached() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        for i in 0..(MAX_LIVE + 2) {
            state.start(i as f32, 0.0, at(t0, i as u64 * 10));
        }

        assert_eq!(state.iter().count(), MAX_LIVE);
        let origins: Vec<_> = state.iter().map(|r| r.origin().0).collect();
        assert_eq!(origins, vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn a_held_ripple_stops_asking_for_frames_once_it_is_fully_grown() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);

        assert!(state.advance(&cfg(), at(t0, 100)));
        assert!(
            !state.advance(&cfg(), at(t0, 2000)),
            "a held, fully grown ripple must let the loop go quiet"
        );
        assert!(state.is_active(), "but it is still on screen");
    }
}
