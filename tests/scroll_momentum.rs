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
        let root = self.root;
        let event = Event::Scroll {
            x: 100.0,
            y: 100.0,
            delta_x: 0.0,
            delta_y: delta,
            source: ScrollSource::Finger,
        };
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event));
    }

    /// Run `n` animation frames, as the loop does while anything is animating.
    fn frames(&mut self, n: usize) {
        let root = self.root;
        for _ in 0..n {
            self.tree
                .with_widget_mut(root, |w, id, t| w.advance_animations(t, id));
        }
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
