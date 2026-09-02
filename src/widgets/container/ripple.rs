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
//! # Where the disc goes
//!
//! The radius that counts is the container's, not the contact point's: the disc
//! ends centred on the container, so what it has to reach is the container's
//! own farthest corner from there — half the diagonal. Deriving it from the
//! press instead would make two clicks on the same button different sizes, and
//! an edge press cover the box a third of the way before the growth ended,
//! which is the truncation this whole model exists to remove.
//!
//! # Several at once
//!
//! Each press is its own ripple and they overlap, because that is what the
//! gesture was: two clicks are two events, and collapsing them into one disc
//! that restarts loses the second. [`MAX_LIVE_RIPPLES`] bounds the work; past it the
//! oldest is dropped, which is the one nearest to invisible anyway.

use std::time::{Duration, Instant};

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
const START_RADIUS: f32 = 0.3;

/// How many ripples may be alive at once.
pub const MAX_LIVE_RIPPLES: usize = 4;

/// A speed multiplier the durations can be divided by without the arithmetic
/// running away. Zero would make a phase last forever and pin the frame loop;
/// a negative one would run the eases backwards into a negative radius.
fn sane_speed(speed: f32) -> f32 {
    if speed.is_finite() {
        speed.clamp(0.05, 20.0)
    } else {
        1.0
    }
}

/// How far through a phase we are, always within 0..=1.
fn phase(elapsed: f32, duration: f32) -> f32 {
    if duration <= 0.0 {
        return 1.0;
    }
    (elapsed / duration).clamp(0.0, 1.0)
}

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
    /// The press was abandoned — released elsewhere, or the pointer left
    /// without releasing. Nothing to complete.
    Cancelled,
}

/// The moment a ripple began to leave, and the growth it left from.
#[derive(Debug, Clone, Copy)]
struct Exit {
    kind: ExitKind,
    at: Instant,
    /// Growth when the exit began, so the remainder can be compressed into it
    /// instead of restarting from zero.
    growth: f32,
}

/// One press.
#[derive(Debug, Clone)]
pub struct Ripple {
    /// Where the pointer went down, in local container coordinates.
    origin: (f32, f32),
    born: Instant,
    exit: Option<Exit>,
    /// 0 at [`START_RADIUS`], 1 fully expanded.
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

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// The radius the disc reaches when fully grown: enough to cover the
    /// container from its centre, which is where the disc ends up. Half the
    /// diagonal, and deliberately not a function of where the press landed —
    /// see the module docs.
    fn full_radius(width: f32, height: f32) -> f32 {
        0.5 * (width * width + height * height).sqrt()
    }

    /// What to draw, in the container's local coordinates.
    pub fn radius(&self, width: f32, height: f32) -> f32 {
        Self::full_radius(width, height) * (START_RADIUS + (1.0 - START_RADIUS) * self.growth)
    }

    /// The centre settles from the press point onto the container's own as the
    /// disc grows, which is what stops a press near a corner from looking
    /// lopsided.
    pub fn center(&self, width: f32, height: f32) -> (f32, f32) {
        let (x, y) = self.origin;
        (
            x + (width / 2.0 - x) * self.growth,
            y + (height / 2.0 - y) * self.growth,
        )
    }

    fn is_leaving(&self) -> bool {
        self.exit.is_some()
    }

    /// Begin the exit, from the growth the ripple has actually reached.
    ///
    /// Advancing first is not tidiness: events are dispatched as a batch before
    /// the frame's animation pass runs, so a touch tap — down and up in one
    /// batch — arrives before `advance` has ever run, and the compressed
    /// remainder has to start from the real growth rather than a stale zero.
    fn begin_exit(&mut self, kind: ExitKind, config: &RippleConfig, now: Instant) {
        if self.exit.is_some() {
            return;
        }
        self.advance(config, now);
        self.exit = Some(Exit {
            kind,
            at: now,
            growth: self.growth,
        });
    }

    /// How opaque the disc is.
    ///
    /// The rise is never cut short: the fall does not begin until the disc has
    /// finished appearing, so a press reaches full strength however brief it
    /// was. Cutting the rise off at the release instead — the obvious reading —
    /// makes every click shorter than the rise dimmer than the one before it,
    /// and a tap that arrives in a single event batch invisible outright.
    fn opacity_at(&self, config: &RippleConfig, now: Instant) -> f32 {
        let fade_speed = sane_speed(config.fade_speed);
        let rise_over = FADE_IN / fade_speed;
        let risen = phase(since(self.born, now), rise_over);

        let Some(exit) = self.exit else {
            return risen;
        };
        let falls_over = match exit.kind {
            ExitKind::Confirmed => FADE_OUT,
            ExitKind::Cancelled => CANCEL_FADE,
        } / fade_speed;

        let falls_from = exit.at.max(self.born + Duration::from_secs_f32(rise_over));
        risen * (1.0 - ease_out(phase(since(falls_from, now), falls_over)))
    }

    /// When the disc has finished leaving, so it can be dropped.
    fn gone(&self, config: &RippleConfig, now: Instant) -> bool {
        let Some(exit) = self.exit else {
            return false;
        };
        let fade_speed = sane_speed(config.fade_speed);
        let falls_over = match exit.kind {
            ExitKind::Confirmed => FADE_OUT,
            ExitKind::Cancelled => CANCEL_FADE,
        } / fade_speed;
        let falls_from = exit
            .at
            .max(self.born + Duration::from_secs_f32(FADE_IN / fade_speed));
        phase(since(falls_from, now), falls_over) >= 1.0
    }

    /// The growth a ripple nobody has released yet has reached.
    fn held_growth(&self, config: &RippleConfig, now: Instant) -> f32 {
        let duration = HELD_GROWTH / sane_speed(config.expand_speed);
        ease_out(phase(since(self.born, now), duration))
    }

    /// Advance, and report whether this ripple is still worth drawing.
    fn advance(&mut self, config: &RippleConfig, now: Instant) -> bool {
        self.opacity = self.opacity_at(config, now);

        let Some(exit) = self.exit else {
            self.growth = self.held_growth(config, now);
            // A ripple held past its growth is not animating: it just sits
            // there until the release, and the frame loop must be allowed to
            // go quiet.
            return self.growth < 1.0 || self.opacity < 1.0;
        };

        match exit.kind {
            ExitKind::Confirmed => {
                // The remainder of the expansion, compressed — and never given
                // longer than the fade it overlaps, or the disc would go
                // invisible with the growth unfinished, which is the very
                // truncation the confirm exists to prevent.
                let fade = FADE_OUT / sane_speed(config.fade_speed);
                let over = (CONFIRM_GROWTH / sane_speed(config.expand_speed)).min(fade);
                let t = phase(since(exit.at, now), over);
                self.growth = exit.growth + (1.0 - exit.growth) * ease_out(t);
            }
            ExitKind::Cancelled => {
                // Nothing was activated, so the growth is not finished for it —
                // it simply carries on at its own pace while the disc leaves.
                self.growth = self.held_growth(config, now);
            }
        }
        !self.gone(config, now)
    }
}

/// Every ripple currently alive on one container.
#[derive(Debug, Clone, Default)]
pub struct RippleState {
    /// A plain `Vec`: it allocates on the first press, and the containers that
    /// never ripple — most of the ones carrying any handler at all — pay three
    /// words instead of an inline buffer they will not use.
    live: Vec<Ripple>,
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
        while self.live.len() >= MAX_LIVE_RIPPLES {
            self.live.remove(0);
        }
        self.live.push(Ripple::new((local_x, local_y), now));
    }

    /// The press was released inside: finish its expansion and fade it out.
    ///
    /// Only the ripple still being held is confirmed. The others already have
    /// an exit of their own and keep it.
    pub fn release(&mut self, config: &RippleConfig, now: Instant) {
        if let Some(held) = self.live.iter_mut().rev().find(|r| !r.is_leaving()) {
            held.begin_exit(ExitKind::Confirmed, config, now);
        }
    }

    /// The press was abandoned: every ripple still being held just goes.
    pub fn cancel(&mut self, config: &RippleConfig, now: Instant) {
        for ripple in self.live.iter_mut().filter(|r| !r.is_leaving()) {
            ripple.begin_exit(ExitKind::Cancelled, config, now);
        }
    }

    /// Whether anything is on screen.
    pub fn is_active(&self) -> bool {
        !self.live.is_empty()
    }

    /// The ripples to draw, oldest first.
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
        let at_release = state.iter().next().unwrap().growth;
        state.release(&cfg(), at(t0, 80));

        let mut previous = at_release;
        for ms in [100, 140, 200, 260, 320, 400, 460] {
            state.advance(&cfg(), at(t0, ms));
            let Some(ripple) = state.iter().next() else {
                break;
            };
            assert!(
                ripple.growth >= previous - f32::EPSILON,
                "growth went backwards at {ms}ms: {previous} -> {}",
                ripple.growth
            );
            previous = ripple.growth;
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
        state.release(&cfg(), at(t0, 80));

        // The compressed remainder lands well inside the fade it overlaps.
        state.advance(&cfg(), at(t0, 80 + 225));
        let ripple = state.iter().next().expect("still visible");
        assert!(
            ripple.growth > 0.99,
            "the expansion has to finish, got {}",
            ripple.growth
        );
        assert!(ripple.opacity > 0.0, "and still be visible while it does");
    }

    /// The bug this model shipped with once: events arrive as a batch before
    /// the frame's animation pass, so a touch tap reaches `release` before
    /// `advance` has ever run. Cutting the rise off there left the disc at zero
    /// for its whole life — a ripple that drew nothing, for 375ms of frames.
    #[test]
    fn a_tap_released_before_its_first_frame_is_still_seen() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(&cfg(), t0);

        let mut peak: f32 = 0.0;
        for ms in [8, 16, 32, 60, 75, 100, 160] {
            state.advance(&cfg(), at(t0, ms));
            if let Some(ripple) = state.iter().next() {
                peak = peak.max(ripple.opacity);
            }
        }
        assert!(
            peak > 0.99,
            "a tap has to be fully visible at some point, peaked at {peak}"
        );
    }

    /// And no click is dimmer than another for being quicker: the rise always
    /// finishes before the fall starts.
    #[test]
    fn a_click_shorter_than_the_rise_still_reaches_full_strength() {
        for click_ms in [20, 40, 60, 74] {
            let t0 = Instant::now();
            let mut state = RippleState::new();
            state.start(10.0, 10.0, t0);
            state.advance(&cfg(), at(t0, click_ms));
            state.release(&cfg(), at(t0, click_ms));

            state.advance(&cfg(), at(t0, 75));
            let ripple = state.iter().next().expect("still visible");
            assert!(
                ripple.opacity > 0.99,
                "a {click_ms}ms click has to reach full strength, got {}",
                ripple.opacity
            );
        }
    }

    /// The disc ends centred on the container, so what it has to reach is the
    /// container's own farthest corner — never the contact point's. Deriving it
    /// from the press made two clicks on one button different sizes, and an
    /// edge press cover the box long before the growth ended.
    #[test]
    fn the_size_does_not_depend_on_where_the_press_landed() {
        let t0 = Instant::now();
        let (w, h) = (200.0, 40.0);

        let mut corner = RippleState::new();
        corner.start(0.0, 0.0, t0);
        let mut middle = RippleState::new();
        middle.start(w / 2.0, h / 2.0, t0);

        for state in [&mut corner, &mut middle] {
            state.release(&cfg(), t0);
            state.advance(&cfg(), at(t0, 300));
        }

        let radius = |s: &RippleState| s.iter().next().unwrap().radius(w, h);
        assert_eq!(radius(&corner), radius(&middle));
    }

    /// And at full growth it covers the container exactly: every corner is
    /// inside, and it was not already inside a third of the way back.
    #[test]
    fn a_finished_ripple_covers_the_container_and_not_before() {
        let t0 = Instant::now();
        let (w, h) = (200.0, 40.0);
        let mut state = RippleState::new();
        state.start(0.0, 0.0, t0);
        state.release(&cfg(), t0);
        state.advance(&cfg(), at(t0, 400));

        let ripple = state.iter().next().expect("still visible");
        assert!(ripple.growth > 0.99, "the growth has to have finished");

        let (cx, cy) = ripple.center(w, h);
        let radius = ripple.radius(w, h);
        for (x, y) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
            let reach = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            assert!(
                reach <= radius + 0.01,
                "corner ({x},{y}) is {reach} away, disc reaches {radius}"
            );
        }
    }

    /// A speed of zero would divide a duration to infinity and pin the frame
    /// loop; a negative one would run the ease backwards into a negative
    /// radius. Both are public fields.
    #[test]
    fn an_impossible_speed_does_not_stall_or_invert_the_effect() {
        for (expand, fade) in [(0.0, 0.0), (-2.0, -2.0), (f32::NAN, f32::INFINITY)] {
            let config = RippleConfig {
                expand_speed: expand,
                fade_speed: fade,
                ..Default::default()
            };
            let t0 = Instant::now();
            let mut state = RippleState::new();
            state.start(10.0, 10.0, t0);
            state.release(&config, t0);

            // The slowest a clamped speed can make it: the rise and the fall
            // both stretched by the 0.05 floor.
            for ms in [50, 200, 1000, 5000, 10_000] {
                state.advance(&config, at(t0, ms));
                for ripple in state.iter() {
                    assert!(
                        (0.0..=1.0).contains(&ripple.growth),
                        "growth left its range with speeds ({expand}, {fade}): {}",
                        ripple.growth
                    );
                    assert!((0.0..=1.0).contains(&ripple.opacity));
                }
            }
            assert!(
                !state.is_active(),
                "the ripple has to leave even with speeds ({expand}, {fade})"
            );
        }
    }

    #[test]
    fn a_confirmed_ripple_leaves_when_it_has_faded() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(&cfg(), at(t0, 80));

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
        let at_exit = state.iter().next().unwrap().growth;

        state.cancel(&cfg(), at(t0, 80));
        state.advance(&cfg(), at(t0, 80 + 40));
        let ripple = state.iter().next().expect("still fading");
        assert!(
            ripple.growth < at_exit + 0.1,
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
        state.release(&cfg(), at(t0, 80));
        state.advance(&cfg(), at(t0, 120));

        state.start(90.0, 40.0, at(t0, 120));
        state.advance(&cfg(), at(t0, 160));

        let origins: Vec<_> = state.iter().map(|r| r.origin).collect();
        assert_eq!(origins, vec![(10.0, 10.0), (90.0, 40.0)]);
    }

    /// A release confirms the press being held, not the ones already leaving.
    #[test]
    fn a_release_confirms_only_the_ripple_still_held() {
        let t0 = Instant::now();
        let mut state = RippleState::new();
        state.start(10.0, 10.0, t0);
        state.release(&cfg(), at(t0, 50));
        state.start(90.0, 40.0, at(t0, 60));

        state.release(&cfg(), at(t0, 100));

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
        for i in 0..(MAX_LIVE_RIPPLES + 2) {
            state.start(i as f32, 0.0, at(t0, i as u64 * 10));
        }

        assert_eq!(state.iter().count(), MAX_LIVE_RIPPLES);
        let origins: Vec<_> = state.iter().map(|r| r.origin.0).collect();
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
