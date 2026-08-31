//! Momentum belongs to the gesture that ended.
//!
//! Both cases here dispatch real `Event::Scroll`s through a real container and
//! then run animation frames, which is what the loop does. What they assert is
//! that a frame in the middle of a gesture, or a frame long after one, does not
//! move the content: only the deltas the user actually produced do.

use std::time::{Duration, Instant};

use guido::layout::{Constraints, Flex};
use guido::prelude::*;
use guido::renderer::{PaintContext, RenderNode};
use guido::tree::{Tree, WidgetId};

struct H {
    tree: Tree,
    root: WidgetId,
}

impl H {
    fn new() -> Self {
        let view = container()
            .width(200.0)
            .height(200.0)
            .scrollable(ScrollAxis::Vertical)
            .child(
                container()
                    .layout(Flex::column().spacing(8.0))
                    .padding(8.0)
                    .children((0..20).map(|_| container().width(120.0).height(24.0))),
            );

        let mut tree = Tree::new();
        let root = tree.register(Box::new(view));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 200.0, 200.0))
        });
        Self { tree, root }
    }

    /// One scroll sample from any source, at a named moment.
    fn scroll_at(&mut self, delta: f32, source: ScrollSource, at: std::time::Instant) {
        self.tree.set_event_instant(Some(at));
        self.scroll(delta, source);
        self.tree.set_event_instant(None);
    }

    /// The finger lifted at a named moment.
    fn scroll_end_at(&mut self, at: std::time::Instant) {
        self.tree.set_event_instant(Some(at));
        self.scroll_end();
        self.tree.set_event_instant(None);
    }

    /// One scroll sample from any source.
    fn scroll(&mut self, delta: f32, source: ScrollSource) {
        let root = self.root;
        let event = Event::Scroll {
            at: Some(Point::new(100.0, 100.0)),
            delta_x: 0.0,
            delta_y: delta,
            source,
        };
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event));
    }

    /// Play a flick of six samples eight milliseconds apart, and lift.
    ///
    /// The instants are named rather than slept through: a flick's velocity is
    /// the distance over the gap between samples, so a test that sleeps is
    /// measuring the scheduler as much as the gesture — and on a loaded machine
    /// it measures something else again.
    ///
    /// Returns the moment of the lift, which is where whatever comes next
    /// starts counting from.
    fn flick_from(&mut self, source: ScrollSource, start: Instant) -> Instant {
        let mut at = start;
        for _ in 0..6 {
            self.scroll_at(10.0, source, at);
            at += Duration::from_millis(8);
        }
        self.scroll_end_at(at);
        at
    }

    /// Run `n` animation frames at sixty a second from `start`, as the loop does
    /// while anything is animating, and return the moment of the last one.
    fn frames_from(&mut self, n: usize, start: Instant) -> Instant {
        let root = self.root;
        let mut at = start;
        for _ in 0..n {
            at += Duration::from_millis(16);
            self.tree.set_frame_instant(Some(at));
            self.tree
                .with_widget_mut(root, |w, id, t| w.advance_animations(t, id));
        }
        self.tree.set_frame_instant(None);
        at
    }

    /// The finger lifted, as the compositor's `axis_stop` reaches the tree.
    fn scroll_end(&mut self) {
        let root = self.root;
        let event = Event::ScrollEnd {
            at: Some(Point::new(100.0, 100.0)),
        };
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event));
    }

    fn offset(&mut self) -> f32 {
        let root = self.root;
        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
        -node.children[0].local_transform.ty()
    }
}

/// A gap between two samples is what "scrolling slowly" is made of, not a
/// signal that the finger left. Frames taken during the gap must not fling the
/// list: after two 30px samples the content has moved 60px and no more.
#[test]
fn a_gap_inside_a_gesture_does_not_start_the_momentum() {
    let mut h = H::new();

    let t0 = Instant::now();
    h.scroll_at(30.0, ScrollSource::Finger, t0);
    let after = h.frames_from(10, t0 + Duration::from_millis(60));

    h.scroll_at(30.0, ScrollSource::Finger, after);
    h.frames_from(10, after + Duration::from_millis(60));

    let offset = h.offset();
    assert!(
        (offset - 60.0).abs() < 0.01,
        "content moved {offset}px for 60px of gesture: a gap between samples \
         was read as the end of the gesture"
    );
}

/// A gesture that never ended leaves nothing behind to be resumed. The finger
/// is still down as far as the library knows, so however long the surface sits
/// idle, the frame that eventually runs must not move the content.
#[test]
fn a_gesture_that_never_ended_is_not_resumed_by_a_later_frame() {
    let mut h = H::new();

    let t0 = Instant::now();
    h.scroll_at(30.0, ScrollSource::Finger, t0);
    let after_gesture = h.offset();

    // The surface goes idle: nothing advances for a while. Named rather than
    // slept, so "a while" is a number and the test costs nothing.
    //
    // Something unrelated then asks for an animation frame — re-entering the
    // container starts the scrollbar's hover expansion, which does exactly this.
    h.frames_from(10, t0 + Duration::from_millis(120));

    let offset = h.offset();
    assert!(
        (offset - after_gesture).abs() < 0.01,
        "content drifted from {after_gesture} to {offset} on a later frame: \
         a momentum nobody advanced was waiting instead of being over"
    );
}

/// And the other half: a gesture that does end carries on. The whole path,
/// from the samples through the end-of-gesture event to the frames that run
/// the momentum — which is what `wl_pointer.axis_stop` reaches.
#[test]
fn a_gesture_that_ended_carries_on_past_the_last_sample() {
    let mut h = H::new();

    let t0 = Instant::now();
    let mut at = t0;
    for _ in 0..6 {
        h.scroll_at(10.0, ScrollSource::Finger, at);
        at += Duration::from_millis(8);
    }
    let at_lift = h.offset();
    assert!(
        (at_lift - 60.0).abs() < 0.01,
        "the gesture itself moved {at_lift}px, not the 60px dispatched"
    );

    h.scroll_end_at(at);
    h.frames_from(60, at);

    let coasted = h.offset() - at_lift;
    assert!(
        coasted > 10.0,
        "the finger lifted after a 60px flick and the content coasted {coasted}px"
    );
}

/// A continuous source is a gesture: it is measured for a speed and it coasts.
/// A wheel is not — its steps have no duration to divide by, and a stop, if one
/// ever arrives, has nothing to release.
///
/// `is_continuous` is the line between them, and without this both sides of it
/// are unwatched: spelling it `matches!(self, Finger)` leaves the suite green.
#[test]
fn a_continuous_gesture_coasts_and_a_wheel_does_not() {
    let mut wheel = H::new();
    let lifted = wheel.flick_from(ScrollSource::Wheel, Instant::now());
    let at_lift = wheel.offset();
    wheel.frames_from(60, lifted);
    let wheel_coast = wheel.offset() - at_lift;
    assert!(
        wheel_coast.abs() < 0.01,
        "a wheel coasted {wheel_coast}px: it has no gesture to have a speed"
    );

    let mut continuous = H::new();
    let lifted = continuous.flick_from(ScrollSource::Continuous, Instant::now());
    let at_lift = continuous.offset();
    continuous.frames_from(60, lifted);
    let continuous_coast = continuous.offset() - at_lift;
    assert!(
        continuous_coast > 10.0,
        "a continuous gesture coasted {continuous_coast}px after a 60px flick"
    );
}

/// A momentum belongs to a moment as well as to a gesture. If the loop goes
/// idle with velocity left — the pointer wandered off, nothing asked for a
/// frame — that motion is over. The frame that eventually arrives, from
/// whatever asked for it, must not carry on where it left off.
///
/// This is the same failure as #246 one gate further along: the gesture *did*
/// end, so the flag that replaced the timeout is set, and stays set.
#[test]
fn a_momentum_left_half_run_is_not_carried_on_by_a_later_frame() {
    let mut h = H::new();
    let lifted = h.flick_from(ScrollSource::Finger, Instant::now());

    let stopped = h.frames_from(5, lifted);
    let interrupted_at = h.offset();
    assert!(
        interrupted_at > 60.0,
        "the flick was coasting when it stopped"
    );

    // The loop goes idle mid-flight, and then something unrelated asks for an
    // animation frame — re-entering the container starts the scrollbar's hover
    // expansion, which does exactly this.
    h.frames_from(30, stopped + Duration::from_millis(400));

    let after = h.offset();
    assert!(
        (after - interrupted_at).abs() < 0.01,
        "content carried on from {interrupted_at} to {after}: a momentum \
         abandoned mid-flight was resumed instead of being over"
    );
}

/// The same gesture, twice as fast, coasts further — and by how much is a
/// number, on any machine, with nothing asleep.
///
/// This could not be written before. A velocity is distance over time, and the
/// only time available was whatever had elapsed between two `sleep`s, so the
/// suite could ask that the content moved and never how fast. `Event::Scroll`
/// now arrives with the moment the compositor saw it, and a test can say what
/// that moment is.
#[test]
fn the_speed_of_a_flick_is_the_distance_over_the_time_it_took() {
    // Sixty pixels of finger, in six samples. The only difference between the
    // two gestures is how long they took.
    fn coast(sample_gap_ms: u64) -> f32 {
        let mut h = H::new();
        let t0 = Instant::now();
        for i in 1..=6u64 {
            h.scroll_at(
                10.0,
                ScrollSource::Finger,
                t0 + Duration::from_millis(i * sample_gap_ms),
            );
        }
        let lifted = h.offset();
        let at = t0 + Duration::from_millis(6 * sample_gap_ms);
        h.scroll_end_at(at);
        h.frames_from(60, at);
        h.offset() - lifted
    }

    let slow = coast(16);
    let fast = coast(8);

    assert!(
        slow > 0.0,
        "a flick has to coast at all before its speed can be compared, got {slow}"
    );
    assert!(
        fast > slow,
        "the same sixty pixels in half the time is twice the speed and has to \
         coast further: 8ms samples gave {fast}, 16ms samples {slow}"
    );
}
