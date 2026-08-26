//! Momentum belongs to the gesture that ended.
//!
//! Both cases here dispatch real `Event::Scroll`s through a real container and
//! then run animation frames, which is what the loop does. What they assert is
//! that a frame in the middle of a gesture, or a frame long after one, does not
//! move the content: only the deltas the user actually produced do.

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

    /// One touchpad scroll sample, as a finger moving by `delta` pixels.
    fn finger_scroll(&mut self, delta: f32) {
        self.scroll(delta, ScrollSource::Finger);
    }

    /// One scroll sample from any source.
    fn scroll(&mut self, delta: f32, source: ScrollSource) {
        let root = self.root;
        let event = Event::Scroll {
            x: 100.0,
            y: 100.0,
            delta_x: 0.0,
            delta_y: delta,
            source,
        };
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event));
    }

    /// Play a flick of `samples` movements and lift, from `source`.
    fn flick(&mut self, source: ScrollSource) {
        for _ in 0..6 {
            self.scroll(10.0, source);
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        self.scroll_end();
    }

    /// Run `n` animation frames, as the loop does while anything is animating.
    fn frames(&mut self, n: usize) {
        let root = self.root;
        for _ in 0..n {
            self.tree
                .with_widget_mut(root, |w, id, t| w.advance_animations(t, id));
        }
    }

    /// The finger lifted, as the compositor's `axis_stop` reaches the tree.
    fn scroll_end(&mut self) {
        let root = self.root;
        let event = Event::ScrollEnd { x: 100.0, y: 100.0 };
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

    h.finger_scroll(30.0);
    std::thread::sleep(std::time::Duration::from_millis(60));
    h.frames(10);

    h.finger_scroll(30.0);
    std::thread::sleep(std::time::Duration::from_millis(60));
    h.frames(10);

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

    h.finger_scroll(30.0);
    let after_gesture = h.offset();

    // The surface goes idle: nothing advances for a while.
    std::thread::sleep(std::time::Duration::from_millis(120));

    // Something unrelated asks for an animation frame — re-entering the
    // container starts the scrollbar's hover expansion, which does exactly this.
    h.frames(10);

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

    for _ in 0..6 {
        h.finger_scroll(10.0);
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let at_lift = h.offset();
    assert!(
        (at_lift - 60.0).abs() < 0.01,
        "the gesture itself moved {at_lift}px, not the 60px dispatched"
    );

    h.scroll_end();
    h.frames(60);

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
    wheel.flick(ScrollSource::Wheel);
    let at_lift = wheel.offset();
    wheel.frames(60);
    let wheel_coast = wheel.offset() - at_lift;
    assert!(
        wheel_coast.abs() < 0.01,
        "a wheel coasted {wheel_coast}px: it has no gesture to have a speed"
    );

    let mut continuous = H::new();
    continuous.flick(ScrollSource::Continuous);
    let at_lift = continuous.offset();
    continuous.frames(60);
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
    h.flick(ScrollSource::Finger);

    h.frames(5);
    let interrupted_at = h.offset();
    assert!(
        interrupted_at > 60.0,
        "the flick was coasting when it stopped"
    );

    // The loop goes idle mid-flight.
    std::thread::sleep(std::time::Duration::from_millis(400));

    // Re-entering the container starts the scrollbar's hover expansion, which
    // asks for an animation frame.
    h.frames(30);

    let after = h.offset();
    assert!(
        (after - interrupted_at).abs() < 0.01,
        "content carried on from {interrupted_at} to {after}: a momentum \
         abandoned mid-flight was resumed instead of being over"
    );
}
