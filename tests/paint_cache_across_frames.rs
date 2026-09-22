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
//!
//! Three more for the flatten cache under a clip, which is the same question
//! one phase later. A command names its clip rather than carrying a copy of it
//! (#441), so what a replay has to get right is which clip the commands find
//! when they are put back: the one they were cached under if it has not moved,
//! the one they *now* sit under if it has, and the scroller's viewport
//! staying where it is while the rows go past it.

use guido::layout::Flex;

mod common;
use common::Harness;
use guido::prelude::*;
use guido::renderer::{
    CommandLayer, CornerRadii, DrawCommand, FlattenScratch, FlattenedCommand, NodeId, RenderNode,
    ScratchCapacity, flatten_root_into,
};
use guido::shape::PlacedShape;
use guido::tree::WidgetId;
use guido::widgets::Corners;
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

// The clipped scenarios. Both put a subtree under a clip and ask what flatten
// does with it on the next frame; what separates them is whether the clip and
// the content moved together.
/// The box under the static clip, which is how its command is picked out.
const CLIPPED_FILL: Color = Color::rgb(0.2, 0.7, 0.5);
/// The scrolling row the second one follows, for the same reason.
const SCROLLED_FILL: Color = Color::rgb(0.7, 0.2, 0.5);
/// Tall enough that three of them overflow the viewport — and that the third
/// is still partly inside it, so the visible-range search culls nothing and
/// every paint stays complete. A culled paint is partial, a partial paint is
/// never cached, and a list that repaints is never asked whether its flatten
/// could be reused.
const TALL_ROW: f32 = 80.0;
const TALL_ROWS: usize = 3;
/// Short enough that every row is still partly in view afterwards.
const SHORT_SCROLL: f32 = 40.0;
/// The two boxes of the clip that appears. They are the same height and the
/// inner one is nudged down out of the outer one, so the outer cuts the bottom
/// `NUDGE` pixels off the inner — which makes "was the outer taken into
/// account" a number rather than a matter of opinion.
const OUTER_CLIP: f32 = 100.0;
const NUDGE: f32 = 40.0;
/// The box next to the static clip, whose whole job is to be marked dirty.
const POKER: f32 = 50.0;
/// The clip's corners, before and after they are rounded. A shape change that
/// no layout notices, so the box under it stays clean across the two frames.
const SQUARE_CORNERS: Corners = Corners {
    radii: CornerRadii {
        top_left: 0.0,
        top_right: 0.0,
        bottom_right: 0.0,
        bottom_left: 0.0,
    },
    curvature: 1.0,
};
const ROUNDED_CORNERS: Corners = Corners {
    radii: CornerRadii {
        top_left: 24.0,
        top_right: 24.0,
        bottom_right: 24.0,
        bottom_left: 24.0,
    },
    curvature: 1.0,
};

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
                .scroll(Scroll::vertical())
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

/// A static `Overflow::Hidden` box over one child, above a box with nothing to
/// do but ask for the next frame.
///
/// The corners are the caller's because they are the one thing that changes
/// the clip's *shape* without changing anything a layout would notice — so
/// what is underneath stays clean, which is the only state in which flatten is
/// asked whether it may replay it.
///
/// Nothing here scrolls and nothing moves. The clip and what it cuts are in
/// the same place frame after frame, which is the case the flatten guard used
/// to refuse along with the one it exists for.
fn hidden_box_over_a_poker<M>(corners: impl IntoAnimated<Corners, M>) -> Container {
    container()
        .layout(Flex::column().spacing(GAP))
        .padding(PAD)
        .child(
            container()
                .width(VIEWPORT)
                .height(VIEWPORT)
                .corners(corners)
                .overflow(Overflow::Hidden)
                .child(
                    container()
                        .width(VIEWPORT)
                        .height(TALL_ROW)
                        .background(CLIPPED_FILL),
                ),
        )
        .child(container().width(POKER).height(POKER))
}

/// The same, but each row hides its own overflow: a clip inside the subtree
/// that scrolls, under the clip that does not.
///
/// The inner box is twice the row's height, so the row's clip has something to
/// cut, and the first row's is filled distinctly so its command can be picked
/// out of the frame.
fn scroller_of_clipping_rows() -> Container {
    container().padding(PAD).child(
        container()
            .width(VIEWPORT)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column())
                    .children((0..TALL_ROWS).map(|row| {
                        container()
                            .width(VIEWPORT)
                            .height(TALL_ROW)
                            .overflow(Overflow::Hidden)
                            .child(
                                container()
                                    .width(VIEWPORT)
                                    .height(TALL_ROW * 2.0)
                                    .background(if row == 0 {
                                        CLIPPED_FILL
                                    } else {
                                        Color::rgb(0.25, 0.25, 0.35)
                                    }),
                            )
                    })),
            ),
    )
}

/// A box that may or may not hide its overflow, over one that always does,
/// over something taller than either.
///
/// The inner box is the subject: it declares a clip of its own and never
/// changes, so the paint cache hands it back clean and flatten is asked whether
/// it may be replayed. The outer one is a clip that *appears* — the case a
/// cached subtree cannot know about, because when its entry was made there was
/// nothing above it at all.
fn a_clip_that_appears_over_one_that_does_not(outer: RwSignal<Overflow>) -> Container {
    container()
        .layout(Flex::column().spacing(GAP))
        .child(
            container()
                .width(VIEWPORT)
                .height(OUTER_CLIP)
                .overflow(outer)
                .child(
                    container()
                        .width(VIEWPORT)
                        .height(OUTER_CLIP)
                        .translate((0.0, NUDGE))
                        .overflow(Overflow::Hidden)
                        .child(
                            container()
                                .width(VIEWPORT)
                                .height(TALL_ROW * 2.0)
                                .background(CLIPPED_FILL),
                        ),
                ),
        )
        .child(container().width(POKER).height(POKER))
}

/// A vertical scroller over rows tall enough that every one of them is at
/// least partly in view.
///
/// The list itself repaints when the offset moves — it is the scrollable's
/// own child and carries the offset — so the rows are what stays clean, and
/// the first of them is filled distinctly so its command can be picked out of
/// the frame.
fn scroller_of_tall_rows() -> Container {
    container().padding(PAD).child(
        container()
            .width(VIEWPORT)
            .height(VIEWPORT)
            .scroll(Scroll::vertical())
            .child(
                container()
                    .layout(Flex::column())
                    .children((0..TALL_ROWS).map(|row| {
                        container()
                            .width(VIEWPORT)
                            .height(TALL_ROW)
                            .background(if row == 0 {
                                SCROLLED_FILL
                            } else {
                                Color::rgb(0.25, 0.25, 0.35)
                            })
                    })),
            ),
    )
}

/// A surface that renders frame after frame into one retained root node.
struct Surface {
    surface: Harness,
    /// Retained across frames and `clear()`ed per frame — `ManagedSurface::root_node`.
    root_node: RenderNode,
    commands: Vec<FlattenedCommand>,
    layers: Vec<CommandLayer>,
    /// Retained across frames and `clear()`ed per frame, like the two above —
    /// `ManagedSurface::flatten_scratch`.
    scratch: FlattenScratch,
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
            scratch: FlattenScratch::default(),
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
            scratch,
            ..
        } = self;
        tree.paint_widget(*root, root_node);
        let _ = flatten_root_into(root_node, commands, layers, scratch);
        for child in &root_node.children {
            guido::cache_paint_results(tree, child);
        }
        tree.clear_needs_paint(*root);
    }

    /// `count` frames, with `poke` marked dirty before each so that none of
    /// them is skipped for having nothing to do.
    fn frames(&mut self, poke: WidgetId, count: usize) {
        for _ in 0..count {
            self.surface.tree.mark_needs_paint(poke);
            self.frame();
        }
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
        let cmd = self.command_of(fill);
        let DrawCommand::RoundedRect { rect, .. } = &*cmd.command else {
            unreachable!("`command_of` matched on this variant")
        };
        (
            Rc::clone(&cmd.command),
            (
                cmd.world_transform.tx() + rect.x,
                cmd.world_transform.ty() + rect.y,
            ),
        )
    }

    /// The command the box filled with `fill` contributed to the last frame.
    ///
    /// One lookup, and it insists the fill names exactly one box: a second
    /// scheme could drift onto a second box, and then what is returned and what
    /// is asserted of it are about different boxes.
    fn command_of(&self, fill: Color) -> &FlattenedCommand {
        let mut matches = self.commands.iter().filter(
            |cmd| matches!(&*cmd.command, DrawCommand::RoundedRect { color, .. } if *color == fill),
        );
        let found = matches.next().expect("the box contributed a command");
        assert!(
            matches.next().is_none(),
            "the fill names more than one box, so neither what this returns \
             nor what is asserted of it is about the box the test means"
        );
        found
    }

    /// Scroll the nth column's list.
    fn scroll(&mut self, column: usize, delta: f32) {
        let x = if column == 0 {
            COLUMN_A_X + VIEWPORT / 2.0
        } else {
            COLUMN_B_X + VIEWPORT / 2.0
        };
        let scroller = self.scroller(column);
        self.scroll_at(x, SCROLLER_TOP + VIEWPORT / 2.0, delta, scroller);
    }

    /// Scroll whatever sits under `(x, y)`, the way the loop does it: the event
    /// moves the offset, and the `JobRequest::Paint` it asks for arrives as
    /// `mark_needs_paint` when the job queue is drained (`jobs.rs`).
    fn scroll_at(&mut self, x: f32, y: f32, delta: f32, scroller: WidgetId) {
        let root = self.surface.root;
        self.surface.tree.with_widget_mut(root, |w, id, t| {
            w.event(t, id, &Event::scroll(x, y, 0.0, delta, ScrollSource::Wheel))
        });
        self.surface.tree.mark_needs_paint(scroller);
    }

    /// The node a widget contributed to this frame's render tree, found by the
    /// id the two share.
    ///
    /// Indices would not do it: a scroller culls what left the viewport, so the
    /// nth child of a render node is not the nth child of the widget.
    fn node_of(&self, widget: WidgetId) -> &RenderNode {
        fn find(node: &RenderNode, id: NodeId) -> Option<&RenderNode> {
            if node.id == id {
                return Some(node);
            }
            node.children.iter().find_map(|child| find(child, id))
        }
        find(&self.root_node, widget.as_u64()).expect("the widget contributed a node to this frame")
    }

    /// The clip the box filled with `fill` was drawn under.
    ///
    /// This is the effective clip the GPU is handed — already intersected with
    /// every ancestor's — so it says where the cut actually falls, and with
    /// what corners, whether the command was flattened this frame or replayed
    /// from the cache.
    fn clip_shape(&self, fill: Color) -> PlacedShape {
        self.command_of(fill)
            .clip()
            .expect("the box was drawn under a clip")
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
                cmd.clip()
                    .is_some_and(|clip| (clip.world_aabb().x - column_x).abs() < 0.01)
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

/// A static clip and what it cuts are both replayed, and the cut still falls
/// on the viewport.
///
/// Nothing here moves, which used to be the only way into the flatten cache
/// under a clip and is now the dullest of three. Both halves are watched. The
/// `Overflow::Hidden` box is cached like any other node — the clip it sets is
/// one of the clips it placed, so a replay places it again — and the box
/// underneath keeps the entry it was flattened into, because a replay writes
/// nothing back.
///
/// The poker is there because a frame with nothing dirty is skipped, and a
/// skipped frame asks nothing.
#[test]
fn a_static_clip_and_what_it_cuts_are_both_replayed() {
    let mut s = Surface::new(
        hidden_box_over_a_poker(SQUARE_CORNERS),
        PAD + VIEWPORT + PAD,
        PAD + VIEWPORT + GAP + POKER + PAD,
    );

    s.frame();
    let children = s.surface.tree.get_children(s.surface.root);
    let (hidden, poker) = (children[0], children[1]);
    let clipped = s.surface.tree.get_children(hidden)[0];
    let viewport = Rect::new(PAD, PAD, VIEWPORT, VIEWPORT);
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).world_aabb(),
        viewport,
        "the box is drawn under the box that hides its overflow"
    );

    let entries = |s: &Surface| {
        (
            s.node_of(hidden)
                .cached_flatten
                .borrow()
                .clone()
                .expect("a node that sets a clip is cached like any other: the clip is one of the ones it placed"),
            s.node_of(clipped)
                .cached_flatten
                .borrow()
                .clone()
                .expect("and so is what it cuts"),
        )
    };
    let first = entries(&s);

    // The pointer moves onto the box next door: that one repaints, the box
    // that hides its overflow is untouched and clean.
    s.frames(poker, 1);

    let second = entries(&s);
    assert!(
        Rc::ptr_eq(&first.0, &second.0) && Rc::ptr_eq(&first.1, &second.1),
        "the clipped box was flattened again although neither it nor the clip \
         above it had moved: replay writes nothing back, so a fresh entry here \
         means the cache refused"
    );
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).world_aabb(),
        viewport,
        "the replayed commands lost the clip they name"
    );
}

/// A clip that appears above a replayed subtree cuts it.
///
/// The subtree declares a clip of its own, which travels with it — and when its
/// entry was made nothing was above that clip at all. "Nothing above it" is a
/// fact about the frame the entry was made on, not about the subtree: an
/// ancestor turning `Overflow::Hidden` on is an ordinary signal write, and the
/// subtree below it is clean through the whole of it. Replay it as it was
/// cached and the new clip cuts nothing, which is the one failure this area
/// exists to prevent, arrived at from the other side.
#[test]
fn a_clip_that_appears_above_a_replayed_subtree_cuts_it() {
    let outer = create_signal(Overflow::Visible);
    let mut s = Surface::new(
        a_clip_that_appears_over_one_that_does_not(outer),
        VIEWPORT,
        OUTER_CLIP + GAP + POKER,
    );

    s.frame();
    let children = s.surface.tree.get_children(s.surface.root);
    let (outer_box, poker) = (children[0], children[1]);
    let inner = s.surface.tree.get_children(outer_box)[0];
    let (painted, _) = s.box_drawn(CLIPPED_FILL);
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).world_aabb(),
        Rect::new(0.0, NUDGE, VIEWPORT, OUTER_CLIP),
        "the box starts cut by the inner clip alone, which is the only one \
         switched on"
    );

    s.frames(poker, 1);
    let first = s
        .node_of(inner)
        .cached_flatten
        .borrow()
        .clone()
        .expect("a subtree that only translates is cached, clip of its own or not");

    outer.set(Overflow::Hidden);
    s.surface.tree.mark_needs_paint(outer_box);
    s.frame();

    let (reused, _) = s.box_drawn(CLIPPED_FILL);
    assert!(
        Rc::ptr_eq(&painted, &reused),
        "the inner box repainted when the clip above it appeared, so flatten \
         was never asked whether its commands could be replayed — this test is \
         about the answer, not about the question never being put"
    );
    let second = s
        .node_of(inner)
        .cached_flatten
        .borrow()
        .clone()
        .expect("cached again after the next frame");
    assert!(
        Rc::ptr_eq(&first, &second),
        "the inner box was flattened again over a clip it does not carry"
    );
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).world_aabb(),
        Rect::new(0.0, NUDGE, VIEWPORT, OUTER_CLIP - NUDGE),
        "the clip that appeared above the replayed subtree cuts nothing: its \
         own clip was put back answering to no ancestor, because on the frame \
         it was cached there was no ancestor to answer to"
    );
}

/// A list that culled its rows is not written down, and the caption beside it
/// is.
///
/// The other half of the question flatten asks about an entry: not whether it
/// could be replayed, but whether anything will ever ask for it. A subtree with
/// something culled under it is dropped from the paint cache by
/// `cache_paint_results`, so the widget repaints next frame and the replay path
/// is never offered what flatten kept. Collecting it copies everything the
/// subtree drew — the whole viewport, in a scrolling list, every frame — for a
/// frame that cannot use it.
///
/// The caption is in the same tree and is cut short by nothing, which is what
/// says this refuses the culled subtree rather than everything.
#[test]
fn a_list_that_culled_its_rows_is_not_written_down() {
    let mut s = Surface::new(two_columns(), 2.0 * (PAD + VIEWPORT) + GAP, 400.0);

    s.frame();
    let scroller = s.scroller(0);
    let list = s.surface.tree.get_children(scroller)[0];
    s.scroll(0, SCROLL);
    s.frame();

    assert!(
        s.surface.tree.get_children(list).len() > s.node_of(list).children.len(),
        "nothing was culled, so this test is about a subtree that does not \
         exist and would pass however the entry is decided"
    );
    assert!(
        s.node_of(list).cached_flatten.borrow().is_none(),
        "the list culled the rows that left the viewport, so its paint is \
         partial, so it repaints next frame and no frame can ever replay what \
         flatten copied down for it"
    );
    assert!(
        s.caption_node(0).cached_flatten.borrow().is_some(),
        "the caption had nothing culled under it and is where it was, so it is \
         exactly what an entry is for — refusing it too would be refusing the \
         cache itself"
    );
}

/// A scrolled row is replayed where it now is, and its viewport stays put.
///
/// This is the case the whole of #441 is about. The row is clean — the paint
/// cache hands back the very node it painted — and it has moved by a pure
/// translation, so there is nothing to re-derive: the commands it drew are the
/// commands it draws now, one offset along. What used to stop that was the
/// clip. Each command carried its ancestors' clip resolved into it, so
/// replaying one shifted the viewport along with the row and the list painted
/// straight through its scroller. The commands name their clip now and the
/// renderer asks where it is, so the row moves and the viewport does not.
///
/// The rows are tall enough that none is ever culled, which is what keeps their
/// paints complete and so cacheable. The list above them is not the subject: it
/// is the scrollable's own child and carries the offset, so it repaints and is
/// never asked the question at all.
#[test]
fn a_scrolled_row_is_replayed_and_its_viewport_stays_put() {
    let mut s = Surface::new(
        scroller_of_tall_rows(),
        PAD + VIEWPORT + PAD,
        PAD + VIEWPORT + PAD,
    );

    s.frame();
    let scroller = s.surface.tree.get_children(s.surface.root)[0];
    let list = s.surface.tree.get_children(scroller)[0];
    let row = s.surface.tree.get_children(list)[0];
    let viewport = Rect::new(PAD, PAD, VIEWPORT, VIEWPORT);
    let (painted, at_rest) = s.box_drawn(SCROLLED_FILL);
    assert_eq!(
        at_rest,
        (PAD, PAD),
        "the row starts at the top of the viewport"
    );
    assert_eq!(
        s.clip_shape(SCROLLED_FILL).world_aabb(),
        viewport,
        "the row is drawn under the scroller's viewport"
    );

    let first = s
        .node_of(row)
        .cached_flatten
        .borrow()
        .clone()
        .expect("a row that only translates is worth keeping, and this test is about what happens to that entry when it moves");

    s.scroll_at(
        PAD + VIEWPORT / 2.0,
        PAD + VIEWPORT / 2.0,
        SHORT_SCROLL,
        scroller,
    );
    s.frame();

    let (reused, moved_to) = s.box_drawn(SCROLLED_FILL);
    assert!(
        Rc::ptr_eq(&painted, &reused),
        "the row repainted instead of coming out of the paint cache, so flatten \
         was never asked whether its commands could be replayed — this test is \
         about the answer, not about the question never being put"
    );
    assert_eq!(
        moved_to,
        (PAD, PAD - SHORT_SCROLL),
        "the row did not scroll"
    );
    assert_eq!(
        s.clip_shape(SCROLLED_FILL).world_aabb(),
        viewport,
        "the clip travelled with the scrolled row instead of staying on the \
         viewport, so it now cuts nothing"
    );
    let second = s
        .node_of(row)
        .cached_flatten
        .borrow()
        .clone()
        .expect("still cached after the scrolled frame");
    assert!(
        Rc::ptr_eq(&first, &second),
        "the row was flattened again although it only moved, and its clip is \
         no longer something it carries — replay writes nothing back, so a \
         fresh entry here means the cache refused"
    );
}

/// A scrolled row carries the clip it set itself and not the one it is under.
///
/// The two halves of a replayed clip, in one frame, and the only scenario that
/// tells them apart. The row hides its own overflow, so it places a clip that
/// belongs to it and must travel with it; it sits in a scroller, so it is
/// *under* a clip that must not travel with it. Shift both and the list paints
/// through its scroller; shift neither and the row's own cut stays behind
/// where the row used to be.
///
/// The arithmetic is what makes the assertion exact. The row starts at the top
/// of the viewport and is `TALL_ROW` high, so its own clip cuts there and
/// nowhere else. Scrolled up by `SHORT_SCROLL` the row's clip has half left the
/// viewport, and the intersection of the two is what is left: the viewport's
/// top edge and the row's bottom one. Only a frame that moved one clip and not
/// the other can produce it.
#[test]
fn a_scrolled_row_carries_its_own_clip_and_not_its_scroller_s() {
    let mut s = Surface::new(
        scroller_of_clipping_rows(),
        PAD + VIEWPORT + PAD,
        PAD + VIEWPORT + PAD,
    );

    s.frame();
    let scroller = s.surface.tree.get_children(s.surface.root)[0];
    let list = s.surface.tree.get_children(scroller)[0];
    let row = s.surface.tree.get_children(list)[0];
    // The width is read off the frame rather than declared: the scrollbar
    // takes a gutter out of the row, and nothing here is about that.
    let at_rest = s.clip_shape(CLIPPED_FILL).world_aabb();
    assert_eq!(
        (at_rest.x, at_rest.y, at_rest.height),
        (PAD, PAD, TALL_ROW),
        "the box is cut by the row that hides its overflow, which is the \
         tighter of the two clips over it"
    );
    let first = s
        .node_of(row)
        .cached_flatten
        .borrow()
        .clone()
        .expect("a row that only translates is worth keeping, clip of its own or not");

    s.scroll_at(
        PAD + VIEWPORT / 2.0,
        PAD + VIEWPORT / 2.0,
        SHORT_SCROLL,
        scroller,
    );
    s.frame();

    let second = s
        .node_of(row)
        .cached_flatten
        .borrow()
        .clone()
        .expect("still cached after the scrolled frame");
    assert!(
        Rc::ptr_eq(&first, &second),
        "the row was flattened again although it only moved — replay writes \
         nothing back, so a fresh entry means the cache refused, and this test \
         is about what the replay does with two clips"
    );
    assert_eq!(
        s.box_drawn(CLIPPED_FILL).1,
        (PAD, PAD - SHORT_SCROLL),
        "the row did not scroll"
    );
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).world_aabb(),
        Rect::new(at_rest.x, PAD, at_rest.width, TALL_ROW - SHORT_SCROLL),
        "the row's own clip and the scroller's viewport did not end up in the \
         two different places they belong: the row's travels with it, the \
         viewport's stays"
    );
}

/// A subtree whose clip changed shape under it is replayed, and cut by the
/// shape the clip is now.
///
/// The row of #442's table that used to be a refusal. The clip does not move
/// at all here — `dx = dy = 0` — and its corners change, which is a cut in a
/// different place with everything else equal. A command that carried a copy
/// of its clip had to be thrown away for that, and could not have been kept
/// whatever the entry was compared against. A command that names its clip is
/// simply cut by what the name now points at.
///
/// The corners are what changes because nothing about them reaches layout: the
/// box under the clip keeps its constraints, its position and its paint, which
/// is the one state in which flatten is asked the question at all.
#[test]
fn a_subtree_whose_clip_changed_shape_is_cut_by_the_new_shape() {
    let corners = create_signal(SQUARE_CORNERS);
    let mut s = Surface::new(
        hidden_box_over_a_poker(corners),
        PAD + VIEWPORT + PAD,
        PAD + VIEWPORT + GAP + POKER + PAD,
    );

    s.frame();
    let children = s.surface.tree.get_children(s.surface.root);
    let hidden = children[0];
    let clipped = s.surface.tree.get_children(hidden)[0];
    let (painted, _) = s.box_drawn(CLIPPED_FILL);
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).radii,
        SQUARE_CORNERS.radii,
        "the box starts under a clip with square corners"
    );
    let first = s
        .node_of(clipped)
        .cached_flatten
        .borrow()
        .clone()
        .expect("a subtree under a static clip is cached");

    corners.set(ROUNDED_CORNERS);
    s.surface.tree.mark_needs_paint(hidden);
    s.frame();

    let (reused, _) = s.box_drawn(CLIPPED_FILL);
    assert!(
        Rc::ptr_eq(&painted, &reused),
        "the box repainted when the clip around it changed, so flatten was \
         never asked whether its commands could be replayed — this test is \
         about the answer, not about the question never being put"
    );
    let second = s
        .node_of(clipped)
        .cached_flatten
        .borrow()
        .clone()
        .expect("cached again after the next frame");
    assert!(
        Rc::ptr_eq(&first, &second),
        "the box was flattened again over a change to a shape it does not \
         carry — replay writes nothing back, so a fresh entry means the cache \
         refused what it no longer has any reason to refuse"
    );
    assert_eq!(
        s.clip_shape(CLIPPED_FILL).radii,
        ROUNDED_CORNERS.radii,
        "the box was cut by the corners its clip used to have, so naming the \
         clip bought nothing"
    );
}

/// A subtree whose ancestor turned is flattened again, not shifted.
///
/// `replay_offset` refuses unless **both** transforms are pure translations,
/// and the second of those two is the one with no obvious way to fail: the
/// entry was made when the subtree was upright, and what turned it is an
/// ancestor, which repaints itself and leaves the subtree below perfectly
/// clean. Replay would then hand the renderer the commands it recorded upright,
/// nudged sideways by the difference between two translations that no longer
/// describe where anything is.
///
/// Nothing is clipped here, which is deliberate: above a clip the entry is made
/// whatever the subtree did, so this is the one place the translation-only test
/// is all that stands between a turned subtree and a replay of its old self.
#[test]
fn a_subtree_whose_ancestor_turned_is_not_reused() {
    let angle = create_signal(0.0f32);
    let mut s = Surface::new(
        container()
            .layout(Flex::column().spacing(GAP))
            .padding(PAD)
            .child(
                container().rotate(angle).child(
                    container()
                        .width(PUSHED_WIDTH)
                        .height(BOX_HEIGHT)
                        .background(CLIPPED_FILL),
                ),
            )
            .child(container().width(POKER).height(POKER)),
        PAD + VIEWPORT + PAD,
        PAD + VIEWPORT + PAD,
    );

    s.frame();
    let children = s.surface.tree.get_children(s.surface.root);
    let (turner, poker) = (children[0], children[1]);
    let turned = s.surface.tree.get_children(turner)[0];
    s.frames(poker, 1);
    let first = s.node_of(turned).cached_flatten.borrow().clone();
    assert!(
        first.is_some(),
        "nothing is clipped here, so the box is cached from its first flatten \
         — without an entry there is nothing for the next frame to refuse"
    );
    assert!(
        s.command_of(CLIPPED_FILL)
            .world_transform
            .is_translation_only(),
        "the box starts upright, which is what makes the next frame a test of \
         anything"
    );

    angle.set(std::f32::consts::FRAC_PI_4);
    s.surface.tree.mark_needs_paint(turner);
    s.frame();

    assert!(
        s.node_of(turned).cached_flatten.borrow().is_none(),
        "the box still holds the entry it was cached with. Replay leaves it \
         untouched and a full flatten under a turn clears it, because a turned \
         subtree can be replayed by no offset at all — so the entry surviving \
         says its old self was handed back"
    );
    drop(first);
    assert!(
        !s.command_of(CLIPPED_FILL)
            .world_transform
            .is_translation_only(),
        "the box was drawn by a transform that only translates, so it was \
         replayed as the upright thing it used to be rather than flattened \
         under the turn"
    );
}

/// A frame flattens into the buffers the frame before it left behind.
///
/// The output buffers have always been the surface's, cleared and refilled;
/// the intermediate the commands are grouped in was built and freed inside
/// every painted frame — a vector of groups, five command buffers and five
/// bounds per group, and a clip tree.
///
/// Reading the capacity after two frames of the same tree would ask nothing.
/// The scratch is full of *this* frame's buffers when it is read, and two
/// identical frames grow to identical capacities, so a `flatten_root_into`
/// that threw the scratch away and default-constructed a new one on the way
/// in would report exactly what a retained one reports.
///
/// What tells them apart is a frame that needs *less* than the one before it.
/// The third here is one box, flattened through the surface's own scratch:
/// retained, the scratch still holds what the two scrolling columns needed;
/// rebuilt, it would hold what one box needs, which is almost nothing.
///
/// Two scrolling columns for the frames that fill it, because the scratch has
/// four buffers and this fills all of them: the rows still in view are twenty
/// commands, which is past the sixteen rects a `LayerBounds` holds inline, and
/// the two scrollers put four clips in the clip tree.
#[test]
fn a_frame_flattens_into_the_buffers_the_last_one_left() {
    let mut s = Surface::new(
        two_columns(),
        PAD + VIEWPORT + GAP + VIEWPORT + PAD,
        PAD + CAPTION + CAPTION_GAP + VIEWPORT + PAD,
    );

    // The pointer resting on the second column's scrollbar: something is dirty
    // every frame, so neither of the two is skipped for having nothing to do.
    s.frame();
    let poke = s.scroller(1);

    s.frames(poke, 1);
    let wide = s.scratch.capacity();
    assert!(
        !s.commands.is_empty(),
        "the frame drew nothing, so there is nothing for the scratch to have held"
    );
    // `groups` is left out: a scratch that has never been used holds one
    // group already, so it is the one number that says nothing here. It is
    // covered by the equality below like the rest.
    let ScratchCapacity {
        commands,
        rects,
        clips,
        ..
    } = wide;
    assert!(
        commands > 0 && rects > 0 && clips > 0,
        "the scratch the surface owns came out of the frame empty-handed \
         ({wide:?}) — flatten never touched the one it was handed"
    );

    // One box, through the same scratch: no clip, no bounds to spill, one
    // command. Everything it does not need it has to find already there.
    let mut one_box = RenderNode::new(0);
    one_box.bounds = Rect::new(0.0, 0.0, 10.0, 10.0);
    one_box.commands.push(Rc::new(DrawCommand::rounded_rect(
        one_box.bounds,
        CLIPPED_FILL,
        0.0,
    )));
    let _ = flatten_root_into(&one_box, &mut s.commands, &mut s.layers, &mut s.scratch);

    assert_eq!(
        s.commands.len(),
        1,
        "the narrow frame is meant to need next to nothing — a frame that \
         needs as much as the last one cannot tell a kept buffer from a new one"
    );
    assert_eq!(
        s.scratch.capacity(),
        wide,
        "the narrow frame left the scratch its own size, so the buffers the \
         wide frames filled were given back and built again"
    );
}
