//! The scrollbar handle is drawn where the offset says it is, and answers a
//! press wherever it is drawn.
//!
//! Scrolling is paint-only, and the loop runs `advance_animations` for a
//! `JobType::Animation` job alone. A wheel scroll and a click on the track both
//! request a plain `Paint`, so the cases that ask *where the handle is* dispatch
//! an event and paint with no animation pass in between — which is what the loop
//! actually does for them. Running one there would mask the defect: it
//! repositions the handle, and the test would pass without the fix.
//!
//! The cases that ask *what the handle answers* run one, because they have to:
//! a ripple is drawn only once it has grown, and it grows in an animation pass.
//! They read what the paint says rather than assuming the frame they are on.
//!
//! Both axes, because both are drawn from the same derivation and either can
//! lose it on its own.

use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::layout::{Constraints, Flex};
use guido::prelude::*;
use guido::renderer::{DrawCommand, PaintContext, RenderNode};
use guido::tree::{Tree, WidgetId};

/// The scrollbar sits 2px in from the edges of the container along its axis,
/// and is `width` across — 6 by default. So a 200-long viewport carries a
/// 196-long track starting at 2.
const VIEWPORT: f32 = 200.0;
const TRACK_START: f32 = 2.0;
const TRACK_LENGTH: f32 = 196.0;
const BAR_WIDTH: f32 = 6.0;

/// A vertical scroller: 200x200 over 648px of content — 20 rows of 24, spaced
/// 8, padded 8. The handle is `viewport / content * track` long.
const V_CONTENT: f32 = 648.0;
const V_HANDLE: f32 = 60.4938;

/// A horizontal scroller: 200x80 over 888px of content — 10 columns of 80,
/// spaced 8, padded 8.
const H_VIEWPORT: f32 = 80.0;
const H_CONTENT: f32 = 888.0;
const H_HANDLE: f32 = 44.1441;

/// How far `H::vertical_inset` holds the scroller off its parent's origin.
///
/// The cases above lay the scroller out at (0, 0), where its own coordinate
/// space and its parent's are the same one — and the scrollbars are laid out
/// from the scroller's *local* bounds while the events reaching it are in its
/// parent's. At the origin those two agree by accident, so an offset is what
/// makes the difference between them visible.
const INSET: f32 = 20.0;

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

    /// The same scroller, moved `INSET` off its parent's origin — the ordinary
    /// case, and the one where the scroller's own coordinate space is not the
    /// space the events reaching it are in.
    ///
    /// Moved rather than wrapped: the `Tree` holds a widget's origin apart from
    /// its cached size, so setting it after layout is the whole of what a
    /// parent would have done to it, without a second widget standing between
    /// the scroller and every reader below.
    fn vertical_inset() -> Self {
        let mut h = Self::vertical();
        h.tree.set_origin(h.root, INSET, INSET);
        h
    }

    /// The horizontal scroller, moved off its parent's origin the same way
    /// `vertical_inset` moves the vertical one.
    fn horizontal_inset() -> Self {
        let mut h = Self::horizontal();
        h.tree.set_origin(h.root, INSET, INSET);
        h
    }

    fn horizontal() -> Self {
        Self::new(
            container()
                .width(VIEWPORT)
                .height(H_VIEWPORT)
                .scrollable(ScrollAxis::Horizontal)
                .child(
                    container()
                        .layout(Flex::row().spacing(8.0))
                        .padding(8.0)
                        .children((0..10).map(|_| container().width(80.0).height(24.0))),
                ),
            H_VIEWPORT,
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

    /// The same, at a named instant, so that the frame after it can be placed a
    /// known distance later.
    fn dispatch_at(&mut self, event: Event, at: Instant) {
        self.tree.set_event_instant(Some(at));
        self.dispatch(event);
        self.tree.set_event_instant(None);
    }

    /// One animation frame, at `at`.
    ///
    /// The loop gives every widget that asked for one its own `Animation` job,
    /// so a ripple living on the scrollbar handle advances even though nothing
    /// above it is animating. Advancing the whole subtree is that queue's
    /// stand-in: no test here can name the handle, which the scroller owns
    /// privately.
    fn frame(&mut self, at: Instant) {
        self.tree.set_frame_instant(Some(at));
        for id in self.tree.collect_subtree_post_order(self.root) {
            self.tree
                .with_widget_mut(id, |w, id, t| w.advance_animations(t, id));
        }
        self.tree.set_frame_instant(None);
    }

    /// The opacity the scrollbar handle is filled with this frame.
    ///
    /// The three states the handle declares differ only in the alpha of a white
    /// fill — resting, hovered, pressed — so that is what says which one it is
    /// in.
    fn handle_fill_alpha(&mut self) -> f32 {
        self.handle_node()
            .commands
            .iter()
            .find_map(|cmd| match &**cmd {
                DrawCommand::RoundedRect { color, .. } => Some(color.a),
                _ => None,
            })
            .expect("the handle fills itself")
    }

    /// The ripple this frame draws on the scrollbar handle, in the handle's own
    /// coordinates, if it draws one.
    fn handle_ripple(&mut self) -> Option<(f32, f32)> {
        self.handle_ripple_disc().map(|(center, _)| center)
    }

    /// The same, with how opaque the disc is — which is what says whether it is
    /// held, completing or abandoned.
    fn handle_ripple_disc(&mut self) -> Option<((f32, f32), f32)> {
        self.handle_node()
            .overlay_commands
            .iter()
            .find_map(|cmd| match &**cmd {
                DrawCommand::Circle { center, color, .. } => Some((*center, color.a)),
                _ => None,
            })
    }

    /// The scrollbar handle's painted node.
    ///
    /// `paint_scrollbar_containers` draws the track and then the handle, after
    /// the content, so the scroller's children are content, track, handle. The
    /// handle's extent is asserted so that a change to that order fails here
    /// loudly rather than by silently measuring the wrong node.
    fn handle_node(&mut self) -> Rc<RenderNode> {
        let horizontal = self.horizontal;
        let expect_extent = if horizontal { H_HANDLE } else { V_HANDLE };

        let node = self.paint();
        assert_eq!(
            node.children.len(),
            3,
            "expected content, track and handle under the scroller"
        );
        let handle = Rc::clone(&node.children[2]);
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
        handle
    }

    /// The painted position of the scrollbar handle, along the axis it travels.
    fn handle_pos(&mut self) -> f32 {
        let handle = self.handle_node();
        if self.horizontal {
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

/// A press has to reach the whole handle, and the disc it starts has to grow
/// from where it landed — wherever the scroller itself sits.
///
/// The handle is a `Container` of its own and the scroller forwards the press
/// to it. Forwarding it unchanged hands a point in the scroller's parent space
/// to a widget laid out in the scroller's own, and the two differ by exactly
/// the scroller's origin: off the surface origin the handle answers for a strip
/// beside itself, so part of it — or all of it — takes no press at all, and
/// where one does land the ripple starts that far from the finger.
///
/// Four quadrants, because a shift shows up as a fraction of the handle
/// answering rather than none of it, which is how #261 was seen by hand.
#[test]
fn a_press_anywhere_on_the_handle_ripples_from_where_it_landed() {
    let handle_x = INSET + VIEWPORT - BAR_WIDTH - TRACK_START;
    let handle_y = INSET + TRACK_START;

    for (qx, qy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
        let (local_x, local_y) = (BAR_WIDTH * qx, V_HANDLE * qy);
        let mut h = H::vertical_inset();

        let pressed = Instant::now();
        h.dispatch_at(
            Event::MouseDown {
                x: handle_x + local_x,
                y: handle_y + local_y,
                button: MouseButton::Left,
            },
            pressed,
        );
        h.frame(pressed + Duration::from_millis(16));

        let (cx, cy) = h
            .handle_ripple()
            .unwrap_or_else(|| panic!("the press at quadrant ({qx}, {qy}) started no ripple"));

        // The disc settles from the press point onto the handle's centre as it
        // grows, so one frame's worth of that drift is expected and a whole
        // scroller origin's worth is the defect.
        assert!(
            (cx - local_x).abs() < 2.0 && (cy - local_y).abs() < 2.0,
            "quadrant ({qx}, {qy}): ripple at ({cx}, {cy}), pressed at ({local_x}, {local_y})"
        );
    }
}

/// The hover colour reaches the handle wherever the scroller sits.
///
/// The scroller decides hover itself, against the widened hit area, and then
/// tells the handle with a synthetic `MouseEnter` at the handle's centre. That
/// point is in the scroller's space like everything else the scroller
/// computes, and the handle answers in its own — so the enter has the same
/// origin to shed as the press does.
#[test]
fn hovering_the_handle_colours_it_wherever_the_scroller_sits() {
    let mut h = H::vertical_inset();
    let resting = h.handle_fill_alpha();

    h.dispatch(Event::MouseMove {
        x: INSET + VIEWPORT - BAR_WIDTH / 2.0 - TRACK_START,
        y: INSET + TRACK_START + V_HANDLE / 2.0,
    });

    let hovered = h.handle_fill_alpha();
    assert!(
        hovered > resting,
        "the handle is filled at {hovered} hovered and {resting} at rest: the enter never landed"
    );
}

/// A hovered handle is drawn wider than it is laid out, and the part that only
/// the hover adds has to answer a press like the rest of it.
///
/// The widening is a scale the *scroller* applies at paint time — about the
/// handle's outer edge, and never stored on the handle — so the handle's own
/// bounds stay its resting strip however wide it is drawn. A press on the half
/// that only exists while hovered therefore reached a widget that had no idea
/// it was there.
///
/// Where the pixels are is read off the paint rather than assumed, so the case
/// does not depend on the spring having settled by any particular frame.
#[test]
fn the_width_a_hover_adds_to_the_handle_answers_a_press_too() {
    let mut h = H::vertical_inset();
    let hovered = Instant::now();
    h.dispatch(Event::MouseMove {
        x: INSET + VIEWPORT - BAR_WIDTH / 2.0 - TRACK_START,
        y: INSET + TRACK_START + V_HANDLE / 2.0,
    });
    for step in 1..=8 {
        h.frame(hovered + Duration::from_millis(16 * step));
    }

    // The handle's painted edges, in the scroller's space: its own transform
    // carries the hover scale, so the left edge is where the widening reaches.
    let painted = h.handle_node().local_transform;
    let (left, _) = painted.transform_point(0.0, V_HANDLE / 2.0);
    let (right, _) = painted.transform_point(BAR_WIDTH, V_HANDLE / 2.0);
    assert!(
        right - left > BAR_WIDTH + 0.5,
        "the hover widened the handle to {} from {BAR_WIDTH}: nothing was added to press on",
        right - left
    );

    let pressed = hovered + Duration::from_millis(200);
    h.dispatch_at(
        Event::MouseDown {
            x: INSET + left + 1.0,
            y: INSET + TRACK_START + V_HANDLE / 2.0,
            button: MouseButton::Left,
        },
        pressed,
    );
    h.frame(pressed + Duration::from_millis(16));

    let (cx, _) = h
        .handle_ripple()
        .expect("a press on the widened edge started no ripple");
    assert!(
        cx < BAR_WIDTH / 2.0,
        "pressed the handle's left edge and the ripple started at {cx}, \
         past the middle of a {BAR_WIDTH}-wide handle"
    );
}

/// The horizontal handle answers a press off-origin too.
///
/// The forwarding is shared between the axes, but the geometry either side of
/// it is not: the rects come from a different branch of
/// `scrollbar_handle_rect`, and the widening turns about the bottom edge rather
/// than the right one. The file's own opening argument applies — either axis
/// can lose it on its own.
#[test]
fn the_horizontal_handle_ripples_from_where_it_landed_too() {
    let handle_x = INSET + TRACK_START;
    let handle_y = INSET + H_VIEWPORT - BAR_WIDTH - TRACK_START;

    for (qx, qy) in [(0.25, 0.25), (0.75, 0.75)] {
        let (local_x, local_y) = (H_HANDLE * qx, BAR_WIDTH * qy);
        let mut h = H::horizontal_inset();

        let pressed = Instant::now();
        h.dispatch_at(
            Event::MouseDown {
                x: handle_x + local_x,
                y: handle_y + local_y,
                button: MouseButton::Left,
            },
            pressed,
        );
        h.frame(pressed + Duration::from_millis(16));

        let (cx, cy) = h
            .handle_ripple()
            .unwrap_or_else(|| panic!("the press at quadrant ({qx}, {qy}) started no ripple"));
        assert!(
            (cx - local_x).abs() < 2.0 && (cy - local_y).abs() < 2.0,
            "quadrant ({qx}, {qy}): ripple at ({cx}, {cy}), pressed at ({local_x}, {local_y})"
        );
    }
}

/// A release on the handle finishes the ripple rather than abandoning it —
/// including on the width the hover added.
///
/// The `MouseUp` that ends a drag is forwarded like the `MouseDown` that began
/// it, and the handle decides from its coordinates which of three things
/// happened. Each leaves the disc in a different state a frame later, which is
/// why the assertion is on the opacity and not on the disc being there:
///
/// - released inside — the expansion completes, fading over `FADE_OUT`, so the
///   disc is still drawn and dimmer than it was.
/// - released elsewhere — abandoned, fading over `CANCEL_FADE`, five times
///   quicker, and gone by the time this looks.
/// - never arrived — no exit begins at all, and the disc sits at full strength
///   for ever. Only "dimmer, and still there" excludes both of the others.
///
/// Hovered, and pressed on the widening, because that is the only state in
/// which the release has anything to undo: at rest the scale is 1 and dropping
/// it changes nothing.
#[test]
fn releasing_on_the_widened_handle_finishes_the_ripple_rather_than_abandoning_it() {
    let mut h = H::vertical_inset();
    let y = INSET + TRACK_START + V_HANDLE / 2.0;
    let hovered = Instant::now();
    h.dispatch(Event::MouseMove {
        x: INSET + VIEWPORT - BAR_WIDTH / 2.0 - TRACK_START,
        y,
    });
    for step in 1..=8 {
        h.frame(hovered + Duration::from_millis(16 * step));
    }

    let (left, _) = h
        .handle_node()
        .local_transform
        .transform_point(0.0, V_HANDLE / 2.0);
    let x = INSET + left + 1.0;

    let pressed = hovered + Duration::from_millis(200);
    h.dispatch_at(
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
        },
        pressed,
    );
    // Past FADE_IN, so the disc is at its full strength and the fade below is
    // the only thing that can have dimmed it.
    let released = pressed + Duration::from_millis(100);
    h.frame(released);
    let (_, held) = h.handle_ripple_disc().expect("the press started no ripple");

    h.dispatch_at(
        Event::MouseUp {
            x,
            y,
            button: MouseButton::Left,
        },
        released,
    );
    h.frame(released + Duration::from_millis(150));

    let (_, leaving) = h
        .handle_ripple_disc()
        .expect("the disc was gone 150ms after the release: it was abandoned, not completed");
    assert!(
        leaving < held,
        "the disc is still at {leaving} against {held} held: the release never reached the handle"
    );
}
