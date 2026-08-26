//! The scrollbar handle is drawn where the offset says it is.
//!
//! Scrolling is paint-only, and the loop runs `advance_animations` for a
//! `JobType::Animation` job alone. A wheel scroll and a click on the track both
//! request a plain `Paint`, so every case here dispatches an event and paints
//! with no animation pass in between — which is what the loop actually does for
//! them. Running one here would mask the defect: it repositions the handle, and
//! the test would pass without the fix.
//!
//! Both axes, because both are drawn from the same derivation and either can
//! lose it on its own.

use guido::layout::{Constraints, Flex};
use guido::prelude::*;
use guido::renderer::{PaintContext, RenderNode};
use guido::tree::{Tree, WidgetId};

/// The scrollbar sits 2px in from the edges of the container along its axis,
/// and is `width` across — 6 by default. So a 200-long viewport carries a
/// 196-long track starting at 2.
const VIEWPORT: f32 = 200.0;
const TRACK_START: f32 = 2.0;
const TRACK_LENGTH: f32 = 196.0;

/// A vertical scroller: 200x200 over 648px of content — 20 rows of 24, spaced
/// 8, padded 8. The handle is `viewport / content * track` long.
const V_CONTENT: f32 = 648.0;
const V_HANDLE: f32 = 60.4938;

/// A horizontal scroller: 200x80 over 888px of content — 10 columns of 80,
/// spaced 8, padded 8.
const H_CONTENT: f32 = 888.0;
const H_HANDLE: f32 = 44.1441;

/// Where the handle belongs for a given offset: the same proportion
/// `ScrollState::scrollbar_handle_rect` computes, written out so the test says
/// what it expects rather than deferring to the code under test.
fn expected(offset: f32, content: f32, handle: f32) -> f32 {
    let max_scroll = content - VIEWPORT;
    TRACK_START + (offset / max_scroll) * (TRACK_LENGTH - handle)
}

struct H {
    tree: Tree,
    root: WidgetId,
    /// Whether the handle travels in x or in y — the same question as which
    /// axis the container scrolls on.
    horizontal: bool,
}

impl H {
    fn vertical() -> Self {
        Self::new(
            container()
                .width(VIEWPORT)
                .height(VIEWPORT)
                .scrollable(ScrollAxis::Vertical)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .padding(8.0)
                        .children((0..20).map(|_| container().width(120.0).height(24.0))),
                ),
            VIEWPORT,
            false,
        )
    }

    fn horizontal() -> Self {
        Self::new(
            container()
                .width(VIEWPORT)
                .height(80.0)
                .scrollable(ScrollAxis::Horizontal)
                .child(
                    container()
                        .layout(Flex::row().spacing(8.0))
                        .padding(8.0)
                        .children((0..10).map(|_| container().width(80.0).height(24.0))),
                ),
            80.0,
            true,
        )
    }

    fn new(view: impl Widget + 'static, height: f32, horizontal: bool) -> Self {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(view));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        // Once, deliberately: layout does not run again for a paint-only scroll,
        // so neither does this test.
        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, VIEWPORT, height))
        });
        Self {
            tree,
            root,
            horizontal,
        }
    }

    fn dispatch(&mut self, event: Event) {
        let root = self.root;
        self.tree
            .with_widget_mut(root, |w, id, t| w.event(t, id, &event));
    }

    fn paint(&mut self) -> RenderNode {
        let root = self.root;
        let mut node = RenderNode::new(root.as_u64());
        self.tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
        node
    }

    /// The painted position of the scrollbar handle, along the axis it travels.
    ///
    /// `paint_scrollbar_containers` draws the track and then the handle, after
    /// the content, so the scroller's children are content, track, handle. The
    /// handle's extent is asserted so that a change to that order fails here
    /// loudly rather than by silently measuring the wrong node.
    fn handle_pos(&mut self) -> f32 {
        let horizontal = self.horizontal;
        let expect_extent = if horizontal { H_HANDLE } else { V_HANDLE };

        let node = self.paint();
        assert_eq!(
            node.children.len(),
            3,
            "expected content, track and handle under the scroller"
        );
        let handle = &node.children[2];
        let extent = if horizontal {
            handle.bounds.width
        } else {
            handle.bounds.height
        };
        assert!(
            (extent - expect_extent).abs() < 0.01,
            "third child is {}x{}, not the handle",
            handle.bounds.width,
            handle.bounds.height
        );

        if horizontal {
            handle.local_transform.tx()
        } else {
            handle.local_transform.ty()
        }
    }

    /// How far the content has been scrolled, read off its painted origin.
    fn scrolled_by(&mut self) -> f32 {
        let content = &self.paint().children[0];
        if self.horizontal {
            -content.local_transform.tx()
        } else {
            -content.local_transform.ty()
        }
    }

    fn wheel(&mut self, delta: f32) {
        let (delta_x, delta_y) = if self.horizontal {
            (delta, 0.0)
        } else {
            (0.0, delta)
        };
        self.dispatch(Event::Scroll {
            x: 100.0,
            y: 40.0,
            delta_x,
            delta_y,
            source: ScrollSource::Wheel,
        });
    }
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 0.01
}

/// A mouse wheel asks for a plain `Paint`: it is not a gesture, so there is no
/// momentum to advance and no animation frame to carry the handle along. It is
/// the ordinary way to scroll, and the handle has to follow it.
#[test]
fn a_wheel_scroll_moves_the_handle() {
    let mut h = H::vertical();
    assert!(
        near(h.handle_pos(), TRACK_START),
        "handle starts at the top"
    );

    h.wheel(120.0);

    assert!(near(h.scrolled_by(), 120.0), "the content scrolled");
    let want = expected(120.0, V_CONTENT, V_HANDLE);
    let got = h.handle_pos();
    assert!(near(got, want), "handle at {got}, expected {want}");
}

/// Clicking the foot of the track jumps the content to the end, so the handle
/// belongs hard against the bottom of the track.
#[test]
fn a_track_click_moves_the_handle() {
    let mut h = H::vertical();

    h.dispatch(Event::MouseDown {
        x: 195.0,
        y: 190.0,
        button: MouseButton::Left,
    });

    let max_scroll = V_CONTENT - VIEWPORT;
    assert!(near(h.scrolled_by(), max_scroll), "the content jumped");
    let want = expected(max_scroll, V_CONTENT, V_HANDLE);
    let got = h.handle_pos();
    assert!(near(got, want), "handle at {got}, expected {want}");
}

/// The same, sideways. A horizontal scroller is drawn from its own branch of
/// `paint_scrollbar_containers`, so it can lose the derivation on its own —
/// and `examples/scroll_example.rs` has one.
#[test]
fn a_wheel_scroll_moves_the_horizontal_handle() {
    let mut h = H::horizontal();
    assert!(
        near(h.handle_pos(), TRACK_START),
        "handle starts at the left"
    );

    h.wheel(200.0);

    assert!(near(h.scrolled_by(), 200.0), "the content scrolled");
    let want = expected(200.0, H_CONTENT, H_HANDLE);
    let got = h.handle_pos();
    assert!(near(got, want), "handle at {got}, expected {want}");
}

/// Clicking the right end of a horizontal track puts the handle hard against
/// it, the mirror of the vertical case.
#[test]
fn a_track_click_moves_the_horizontal_handle() {
    let mut h = H::horizontal();

    h.dispatch(Event::MouseDown {
        x: 190.0,
        y: 75.0,
        button: MouseButton::Left,
    });

    let max_scroll = H_CONTENT - VIEWPORT;
    assert!(near(h.scrolled_by(), max_scroll), "the content jumped");
    let want = expected(max_scroll, H_CONTENT, H_HANDLE);
    let got = h.handle_pos();
    assert!(near(got, want), "handle at {got}, expected {want}");
}

/// The handle travels the track in proportion to the offset, at every offset
/// rather than at the two the cases above happen to reach.
#[test]
fn the_handle_follows_the_offset_across_the_track() {
    let max_scroll = V_CONTENT - VIEWPORT;
    for step in 0..=8 {
        let offset = max_scroll * step as f32 / 8.0;
        let mut h = H::vertical();
        if offset > 0.0 {
            h.wheel(offset);
        }
        let want = expected(offset, V_CONTENT, V_HANDLE);
        let got = h.handle_pos();
        assert!(
            near(got, want),
            "at offset {offset}: handle at {got}, expected {want}"
        );
    }
}

/// The invariant behind the others: what is painted is what the hit test
/// accepts a drag on. Pressing the middle of the *painted* handle must begin a
/// drag — which leaves the offset alone — and not land on the track, which
/// would jump the content somewhere else.
#[test]
fn pressing_the_painted_handle_starts_a_drag_rather_than_jumping_the_track() {
    let mut h = H::vertical();
    h.wheel(120.0);

    let painted_centre = h.handle_pos() + V_HANDLE / 2.0;
    h.dispatch(Event::MouseDown {
        x: 195.0,
        y: painted_centre,
        button: MouseButton::Left,
    });

    let after = h.scrolled_by();
    assert!(
        near(after, 120.0),
        "pressing the painted handle at y={painted_centre} moved the content to {after}: \
         the paint and the hit test disagree about where the handle is"
    );
}
