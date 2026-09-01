//! The paint cache, across frames.
//!
//! Every other test in `tests/` builds a fresh `RenderNode` per frame, which
//! never reaches `reuse_cached`: with nothing in `Tree`'s paint cache a widget
//! is always painted in full, so a stale cache entry is structurally invisible.
//! This one keeps one root node for the life of the surface and clears it per
//! frame, exactly as `ManagedSurface::root_node` does, and runs the three phases
//! of a frame in the order `render_surface` runs them — paint, flatten, then
//! `cache_paint_results`. Widgets come back out of the paint cache as the
//! frames go by — a scrolling column does not, which is the whole subject
//! here, but the captions and rows do — so the render tree persists and
//! `repainted`, `cached_flatten` and `cached_paint` all mean what they mean in
//! the running loop. The last assertion pins that, because a harness that
//! quietly stopped reaching `reuse_cached` would go green for the wrong
//! reason and take the only test of this path with it.
//!
//! Two scenarios, one per branch of `reuse_cached`. Two sibling scrollable
//! columns, because a child served at an unchanged position needs a second
//! widget to ask for the frame the first one is then served stale into; and a
//! row whose first box grows, because a child served at a *changed* one needs
//! something to move it while leaving it clean.

use guido::layout::Flex;

mod common;
use common::Harness;
use guido::prelude::*;
use guido::renderer::{
    CommandLayer, DrawCommand, FlattenedCommand, PaintContext, RenderNode, flatten_root_into,
};
use guido::tree::WidgetId;
use guido::widgets::Rect;
use std::rc::Rc;

const VIEWPORT: f32 = 200.0;
/// The row's padding, and the column's own padding around its rows.
const PAD: f32 = 8.0;
/// Between the two columns.
const GAP: f32 = 16.0;
/// Stands in for the "Vertical Scroll" caption in `examples/scroll_example.rs`.
/// A fixed box rather than text: text metrics come from the fonts installed on
/// the machine, and this test asserts on coordinates.
const CAPTION: f32 = 17.0;
/// Between the caption and the scroller.
const CAPTION_GAP: f32 = 4.0;

const ROW_HEIGHT: f32 = 24.0;
const ROW_GAP: f32 = 8.0;
const ROWS: usize = 20;
/// Far enough that rows leave the viewport, which is what makes the paint
/// partial — and not a whole number of rows, so a stale offset cannot happen
/// to land on the same picture.
const SCROLL: f32 = 120.0;

/// Where each scroller's viewport starts, in surface coordinates.
const SCROLLER_TOP: f32 = PAD + CAPTION + CAPTION_GAP;
const COLUMN_A_X: f32 = PAD;
const COLUMN_B_X: f32 = PAD + VIEWPORT + GAP;

// The second scenario's two boxes. One takes its width from a signal; the
// other sits behind it in the row and never changes, so growing the first is
// the whole of what moves the second.
const BOX_HEIGHT: f32 = 30.0;
/// What the grower is declared at, and what it grows to.
const GROWER_WIDTH: f32 = 40.0;
const GROWN_WIDTH: f32 = 90.0;
const PUSHED_WIDTH: f32 = 50.0;
/// The pushed box's fill, which is how its command is picked out of the frame.
const PUSHED_FILL: Color = Color::rgb(0.9, 0.4, 0.1);
/// A transform of the box's own, so that the branch under test has a user part
/// to recompose and not only a position to strip. Without one `user_part` works
/// out to the identity, and the recomposition could be dropped entirely with
/// this test still green.
const PUSHED_NUDGE: (f32, f32) = (5.0, 3.0);

/// A caption and a vertical scroller over `ROWS` rows — one column of
/// `examples/scroll_example.rs`.
fn column() -> Container {
    container()
        .layout(Flex::column().spacing(CAPTION_GAP))
        .child(container().width(VIEWPORT).height(CAPTION))
        .child(
            container()
                .width(VIEWPORT)
                .height(VIEWPORT)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .scrollable(ScrollAxis::Vertical)
                .child(
                    // The list itself is drawn, not just its rows: it spans
                    // the whole content, so it is never culled and never
                    // narrowed away by the visible-range search, and its
                    // command carries the scroll offset exactly. The rows do
                    // not — the topmost row painted is always the one at the
                    // top of the viewport, whatever the offset.
                    container()
                        .layout(Flex::column().spacing(ROW_GAP))
                        .padding(PAD)
                        .background(Color::rgb(0.1, 0.1, 0.12))
                        .children((0..ROWS).map(|_| {
                            container()
                                .width(120.0)
                                .height(ROW_HEIGHT)
                                .background(Color::rgb(0.25, 0.25, 0.35))
                        })),
                ),
        )
}

/// The two scrolling columns, side by side.
fn two_columns() -> Container {
    container()
        .layout(Flex::row().spacing(GAP))
        .padding(PAD)
        .child(column())
        .child(column())
}

/// A row of two boxes: one whose width `grower` decides, and one behind it.
///
/// The grower's height comes from a child rather than from `.height()`, and
/// that is load-bearing. A container with a fixed width *and* a fixed height is
/// a relayout boundary (`is_relayout_boundary_for`), and marking a boundary
/// stops there — so a grower declared both ways would resize inside itself and
/// never move the box behind it, which is the whole scenario.
fn pushed_row(grower: RwSignal<f32>) -> Container {
    container()
        .layout(Flex::row().spacing(GAP))
        .padding(PAD)
        .child(
            container()
                .width(grower)
                .child(container().height(BOX_HEIGHT)),
        )
        .child(
            container()
                .width(PUSHED_WIDTH)
                .height(BOX_HEIGHT)
                .translate(PUSHED_NUDGE)
                .background(PUSHED_FILL),
        )
}

/// A surface that renders frame after frame into one retained root node.
struct Surface {
    surface: Harness,
    /// Retained across frames and `clear()`ed per frame — `SurfaceState::root_node`.
    root_node: RenderNode,
    commands: Vec<FlattenedCommand>,
    layers: Vec<CommandLayer>,
    width: f32,
    height: f32,
}

impl Surface {
    fn new(view: impl Widget + 'static, width: f32, height: f32) -> Self {
        let harness = Harness::laid_out(view, width, height);
        let root = harness.root;

        Self {
            surface: harness,
            root_node: RenderNode::new(root.as_u64()),
            commands: Vec::new(),
            layers: Vec::new(),
            width,
            height,
        }
    }

    /// The scroller inside the nth column: root → column → [caption, scroller].
    fn scroller(&self, column: usize) -> WidgetId {
        let col = self.surface.tree.get_children(self.surface.root)[column];
        self.surface.tree.get_children(col)[1]
    }

    /// The node the nth column's caption contributed to this frame's render
    /// tree. `reuse_cached` shares the cached `Rc` itself when the position
    /// has not moved, so a caption served from the cache is the *same
    /// allocation* frame after frame, where one that repaints is a new node
    /// each time. Comparing the tree against the cache would not tell those
    /// apart: `cache_paint_results` stores whatever was painted.
    fn caption_node(&self, column: usize) -> Rc<RenderNode> {
        Rc::clone(&self.root_node.children[column].children[0])
    }

    /// One frame: the skip gate, paint into the retained root, flatten, then
    /// cache — per child of the root and never on the root itself, which is
    /// what `render_surface` does.
    fn frame(&mut self) {
        if !self.surface.tree.needs_paint(self.surface.root) {
            return;
        }

        self.root_node.clear();
        self.root_node.bounds = Rect::new(0.0, 0.0, self.width, self.height);

        let Self {
            surface: Harness { tree, root },
            root_node,
            commands,
            layers,
            ..
        } = self;
        tree.with_widget_mut(*root, |w, id, t| {
            let mut ctx = PaintContext::new(root_node);
            w.paint(t, id, &mut ctx);
        });
        let _ = flatten_root_into(root_node, commands, layers);
        for child in &root_node.children {
            guido::cache_paint_results(tree, child);
        }
        tree.clear_needs_paint(*root);
    }

    /// The command the box filled with `fill` contributed to the last frame:
    /// the `Rc` itself, and where it landed in surface coordinates.
    ///
    /// One lookup for both, because they are assertions about the same box and
    /// two schemes could drift onto two boxes. The `Rc` is `caption_node`'s
    /// identity argument one level down: the moved branch of `reuse_cached`
    /// rebuilds the node header, so the node cannot answer here — but the
    /// commands stay shared through it and through flatten, so they can.
    fn box_drawn(&self, fill: Color) -> (Rc<DrawCommand>, (f32, f32)) {
        let mut matches = self.commands.iter().filter_map(|cmd| match &*cmd.command {
            DrawCommand::RoundedRect { rect, color, .. } if *color == fill => Some((
                Rc::clone(&cmd.command),
                (
                    cmd.world_transform.tx() + rect.x,
                    cmd.world_transform.ty() + rect.y,
                ),
            )),
            _ => None,
        });
        let found = matches.next().expect("the box contributed a command");
        assert!(
            matches.next().is_none(),
            "the fill names more than one box, so neither what this returns \
             nor what is asserted of it is about the box the test means"
        );
        found
    }

    /// Scroll the nth column's list, the way the loop does it: the event moves
    /// the offset, and the `JobRequest::Paint` it asks for arrives as
    /// `mark_needs_paint` when the job queue is drained (`jobs.rs`).
    fn scroll(&mut self, column: usize, delta: f32) {
        let root = self.surface.root;
        let x = if column == 0 {
            COLUMN_A_X + VIEWPORT / 2.0
        } else {
            COLUMN_B_X + VIEWPORT / 2.0
        };
        self.surface.tree.with_widget_mut(root, |w, id, t| {
            w.event(
                t,
                id,
                &Event::scroll(
                    x,
                    SCROLLER_TOP + VIEWPORT / 2.0,
                    0.0,
                    delta,
                    ScrollSource::Wheel,
                ),
            )
        });
        let scroller = self.scroller(column);
        self.surface.tree.mark_needs_paint(scroller);
    }

    /// The top of what the nth column's scroller draws, in surface
    /// coordinates: the lowest world y among the commands its clip applies to.
    ///
    /// Only a scroller sets a clip, and the two are at different x, so this
    /// picks out one scroller's subtree and nothing else. The lowest command in
    /// it is the list, translated up by the scroll offset — so this number is
    /// `SCROLLER_TOP - offset`, read off the commands handed to the GPU.
    fn drawn_top(&self, column: usize) -> f32 {
        let column_x = if column == 0 { COLUMN_A_X } else { COLUMN_B_X };
        self.commands
            .iter()
            .filter(|cmd| {
                cmd.clip
                    .as_ref()
                    .is_some_and(|clip| (clip.rect.x - column_x).abs() < 0.01)
            })
            .map(|cmd| cmd.world_transform.ty())
            .fold(f32::INFINITY, f32::min)
    }
}

/// Scrolling one list and then repainting for the *other* one must not put the
/// first list back where it was: the commands the second frame produces have to
/// carry the offset the first frame scrolled to.
///
/// The stale copy comes from the paint cache. A scrolling column culls the rows
/// that left the viewport, which makes its paint partial and so uncacheable —
/// and the entry kept from the last complete paint is the list at rest, from
/// before any of the scrolling happened. Nothing is dirty by the third frame,
/// so `reuse_cached` serves that entry, content and scrollbar handle together.
#[test]
fn a_scrolled_list_survives_a_frame_painted_for_its_sibling() {
    let mut s = Surface::new(
        two_columns(),
        PAD + VIEWPORT + GAP + VIEWPORT + PAD,
        PAD + CAPTION + CAPTION_GAP + VIEWPORT + PAD,
    );

    s.frame();
    let caption = s.caption_node(0);
    let at_rest = s.drawn_top(0);
    assert_eq!(
        at_rest, SCROLLER_TOP,
        "at rest the top of what is drawn is the top of the viewport"
    );

    s.scroll(0, SCROLL);
    s.frame();
    assert_eq!(
        s.drawn_top(0),
        SCROLLER_TOP - SCROLL,
        "the list should be drawn {SCROLL}px above the viewport"
    );

    // The pointer moves onto the other list's scrollbar: that column repaints,
    // this one is untouched and clean.
    let other = s.scroller(1);
    s.surface.tree.mark_needs_paint(other);
    s.frame();

    assert_eq!(
        s.drawn_top(0),
        SCROLLER_TOP - SCROLL,
        "the first list snapped back when the second one asked for a frame"
    );
    assert_eq!(
        s.drawn_top(1),
        SCROLLER_TOP,
        "the second list was never scrolled"
    );
    assert!(
        Rc::ptr_eq(&caption, &s.caption_node(0)),
        "the caption was repainted rather than served from the paint cache — \
         a harness that stops reaching `reuse_cached` goes green however \
         broken the cache is"
    );
}

/// The other branch of `reuse_cached`: a child served from the paint cache
/// whose position changed since it was painted.
///
/// The box behind the grower never becomes dirty — its constraints do not
/// change when the grower widens, so its own layout early-returns and it keeps
/// its cache entry — but the row hands it a new origin. `set_origin` says so in
/// as many words: a widget that moves is not repainted, the parent re-emits it
/// at the new offset, and re-parenting the cached node is how. That
/// re-parenting strips the old position out of the cached transform and
/// recomposes the box's own transform at the new one. Both halves are
/// unwatched without this: the two negations that strip the position can each
/// be dropped with the rest of the suite still green, and so can the
/// recomposition, which is why the box carries a transform of its own.
#[test]
fn a_cached_child_is_drawn_where_it_moved_to() {
    let grower = create_signal(GROWER_WIDTH);
    let mut s = Surface::new(
        pushed_row(grower),
        PAD + GROWN_WIDTH + GAP + PUSHED_WIDTH + PAD,
        PAD + BOX_HEIGHT + PAD,
    );

    s.frame();
    let (painted, at_rest) = s.box_drawn(PUSHED_FILL);
    assert_eq!(
        at_rest,
        (
            PAD + GROWER_WIDTH + GAP + PUSHED_NUDGE.0,
            PAD + PUSHED_NUDGE.1
        ),
        "the box starts behind the grower at its declared width, nudged by its \
         own transform"
    );

    grower.set(GROWN_WIDTH);
    // The relayout a Layout job produces: `process_jobs` marks the widget the
    // signal woke and lays out from the relayout boundary that marking names.
    let grower_id = s.surface.tree.get_children(s.surface.root)[0];
    let boundary = s
        .surface
        .tree
        .mark_needs_layout(grower_id)
        .expect("the grower is registered and was not already dirty");
    s.surface.lay_out(s.width, s.height);
    assert_eq!(
        boundary, s.surface.root,
        "the grower is its own relayout boundary, so nothing outside it would \
         have moved in the running loop either"
    );
    s.frame();

    let (reused, moved_to) = s.box_drawn(PUSHED_FILL);
    assert_eq!(
        moved_to,
        (
            PAD + GROWN_WIDTH + GAP + PUSHED_NUDGE.0,
            PAD + PUSHED_NUDGE.1
        ),
        "the box did not land at its new position plus its own transform — \
         the cached transform either kept a position that should have been \
         stripped out or lost the user part that should have been kept"
    );
    assert!(
        Rc::ptr_eq(&painted, &reused),
        "the box was repainted rather than re-parented out of the paint \
         cache — the moved branch of `reuse_cached` was never reached, so \
         this test would go green however broken it is"
    );
}
