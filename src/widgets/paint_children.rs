//! Painting a widget's children, with culling and paint-cache reuse.
//!
//! This is the parent half of the paint-cache protocol, and it belongs to no
//! widget in particular: any widget that owns children has to cull what falls
//! outside the visible rect, reuse a clean child's cached `RenderNode` instead
//! of repainting it, and re-parent that cached node when the child only moved.
//! It lives here rather than inside `Container` so that a composite widget
//! written outside the library gets the same behaviour by calling it.
//!
//! The invariants it upholds:
//!
//! - The children are narrowed to the cull rect twice over, and the two are
//!   not alternatives. A binary search takes the slice down to the ones the
//!   rect can reach, which is the only bound that holds on a first paint;
//!   the per-child test below then drops what the search let through and the
//!   cache says is clean. The search needs the children ordered along an
//!   axis and says nothing when they are not, which is why the cheaper test
//!   still runs underneath it.
//! - A culled child makes the parent's paint **partial**, and a partial paint
//!   is never cached — otherwise reusing it later would permanently hide the
//!   children this pass skipped. It also invalidates whatever the last
//!   complete paint cached, so `reuse_cached` below cannot serve a picture
//!   older than the widget's last paint; see `cache_paint_results`.
//! - A cached node may be re-parented but never re-sized: its commands were
//!   built for the size it was painted at. A child that changed size ran its
//!   own layout and marked itself dirty, so it should not reach the reuse
//!   path; falling through to a full paint if it ever does keeps a missed
//!   invalidation from showing stale content stretched to the wrong box.

use crate::layout::Axis;
use crate::renderer::PaintContext;
use crate::transform::Transform;
use crate::tree::WidgetId;
use crate::widgets::Rect;

/// How a parent wants its children painted.
pub struct ChildPaintOptions {
    /// Subtracted from each child's laid-out position — the scroll offset for
    /// a scrolling parent, zero for everyone else.
    pub scroll_offset: (f32, f32),
    /// Children lying entirely outside this rect (in the parent's coordinate
    /// space) are skipped. `None` means everything is painted.
    pub cull_rect: Option<Rect>,
    /// When set, only a child lying entirely inside `cull_rect` may reuse its
    /// paint cache: a partially visible one must repaint so its own children
    /// see the current cull rect. A scrolling parent needs this because the
    /// rect moves under the content; a parent that merely inherits its own
    /// parent's cull rect does not.
    pub cache_requires_full_visibility: bool,
    /// The axis the parent's last layout left these children ordered along, if
    /// it left them ordered along one. Without it the children are painted
    /// whole, however small the cull rect: a binary search over a slice that
    /// is not partitioned answers about whichever child it probed.
    pub children_sorted_along: Option<Axis>,
    /// The furthest any of these children paints outside its own bounds — a
    /// shadow's reach, or the distance its own transform can move it. Both
    /// narrowings grow by this, so a child that draws somewhere other than
    /// where it was laid out is still reached.
    pub children_reach: f32,
}

impl Default for ChildPaintOptions {
    fn default() -> Self {
        Self {
            scroll_offset: (0.0, 0.0),
            cull_rect: None,
            cache_requires_full_visibility: false,
            children_sorted_along: None,
            children_reach: 0.0,
        }
    }
}

/// The children the cull rect can reach, found by binary search.
///
/// This is what bounds the work unconditionally. The per-child test in
/// `paint_children` only drops children the paint cache calls clean, so on a
/// first paint — and after anything that dirties the subtree — it drops
/// nothing and every child is painted in full. A list of ten thousand rows
/// costs ten thousand paints unless the slice is narrowed first.
///
/// Marks the paint partial when it does narrow, for the same reason the
/// per-child cull does: a node that did not paint all of itself must not be
/// cached as though it had, or the rect moves on and the cache serves the
/// children it skipped.
fn visible_window<'a>(
    children: &'a [WidgetId],
    ctx: &mut PaintContext,
    opts: &ChildPaintOptions,
) -> &'a [WidgetId] {
    let tree = ctx.tree();
    // One child cannot be narrowed to fewer than one, and none cannot be
    // narrowed at all. Before the counting, not after: a scroller's subtree is
    // mostly single-child wrappers and childless leaves, and counting those as
    // windows that could not narrow would bury the one container that matters
    // under every box below it.
    if children.len() < 2 {
        return children;
    }
    let (Some(cull), Some(axis)) = (opts.cull_rect, opts.children_sorted_along) else {
        if opts.cull_rect.is_some() {
            // A rect to narrow to and nothing to narrow with. This is the case
            // that silently costs a list its virtualization, and it used to
            // report a window of one child out of one.
            crate::render_stats::record_paint_window_declined(children.len() as u64);
        }
        return children;
    };

    // Grown by the widest reach among the children, because the bounds being
    // searched are where they were laid out and not where they draw.
    let (near, far) = cull.span(axis);
    let (near, far) = (near - opts.children_reach, far + opts.children_reach);
    let span = |cid: WidgetId| tree.get_bounds(cid).map(|b| b.span(axis));
    let first = children.partition_point(|&cid| span(cid).is_some_and(|(_, f)| f <= near));
    // From `first`, so `last` cannot come out below it however degenerate the
    // rect is, and the slice below is well formed by construction.
    let last =
        first + children[first..].partition_point(|&cid| span(cid).is_some_and(|(n, _)| n < far));

    // One either side, for the boundary rather than for transforms: the
    // predicates compare edges with `<=` and `<`, so a child whose edge lands
    // exactly on the rect can fall either way under float rounding. What a
    // child's own transform does to where it draws is `children_reach` above,
    // measured rather than guessed at one child's width.
    let window = &children[first.saturating_sub(1)..(last + 1).min(children.len())];
    crate::render_stats::record_paint_window(children.len() as u64, window.len() as u64);
    if window.len() < children.len() {
        ctx.mark_partial();
    }
    window
}

/// Paint `children` into `ctx`, culling and reusing cached paint where possible.
pub fn paint_children(ctx: &mut PaintContext, children: &[WidgetId], opts: &ChildPaintOptions) {
    // The only thing left here that is about a *sequence* of children: which
    // slice of an ordered row can be seen at all. Everything about painting
    // one of them belongs to `paint_child`, so a widget with a single child
    // can have it too.
    for &child_id in visible_window(children, ctx, opts) {
        ctx.paint_child(child_id, opts);
    }
}

impl PaintContext<'_> {
    /// Paint one child of the widget this context is painting.
    ///
    /// The one way a child is painted below the root — `Tree::paint_widget` is
    /// the root's — and what makes the guarantees the same either way: the
    /// child is culled if it cannot be seen, taken from its cache if it has
    /// one, given its place in this node, and given its own `JobType::Paint`
    /// scope, so what it reads while drawing belongs to *it*.
    ///
    /// A widget that forgot the scope used to subscribe to nothing: it drew
    /// once and then stayed as it was, because no scope was open to hear the
    /// read. There is nothing to forget now.
    pub fn paint_child(&mut self, child_id: WidgetId, opts: &ChildPaintOptions) {
        let (offset_x, offset_y) = opts.scroll_offset;
        // Child bounds come from the tree in the parent's local coordinates
        let child_bounds = self.tree().get_bounds(child_id).unwrap_or_default();
        let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);
        let (child_x, child_y) = (child_bounds.x, child_bounds.y);
        let child_position = Transform::translate(child_x - offset_x, child_y - offset_y);

        // Cull clean children that fall outside the visible rect. By the rect
        // the child can paint into rather than the one it was laid out in: a
        // shadow falls outside the box that cast it, and a transform moves the
        // box. Its own reach is what the two have in common.
        if let Some(ref cull) = opts.cull_rect
            && !self.tree().needs_paint(child_id)
        {
            let drawn = child_bounds.outset(self.tree().paint_overflow(child_id));
            if !cull.intersects(&drawn) {
                crate::render_stats::record_paint_child_culled();
                self.mark_partial();
                return;
            }
        }

        if self.reuse_cached(
            child_id,
            (child_x, child_y),
            child_local,
            child_position,
            opts,
        ) {
            return;
        }

        // Full paint: the child is dirty, has no usable cache, or is only
        // partially visible inside a scrolling parent.
        let cull_in_child_space = opts.cull_rect.map(|cull| {
            // The cull rect travels into the child's own coordinate space
            Rect::new(cull.x - child_x, cull.y - child_y, cull.width, cull.height)
        });
        self.paint_child_placed(child_id, child_local, child_position, cull_in_child_space);
    }

    /// Paint a child at a placement the caller decides, rather than at the one
    /// its bounds give it.
    ///
    /// For a child whose placement is not a translation: the scrollbar parts,
    /// which are scaled by the animation that shows and hides them. They take
    /// no part in the paint cache for that reason — reusing a moved node
    /// undoes its old placement by negating it, which is only undoing if the
    /// placement was a translation — and none in culling, because a scrollbar
    /// is inside the box it belongs to by construction.
    ///
    /// What it does share is the part that must not be optional: the child's
    /// own `JobType::Paint` scope.
    pub fn paint_child_placed(
        &mut self,
        child_id: WidgetId,
        child_local: Rect,
        placement: Transform,
        cull_rect: Option<Rect>,
    ) {
        let mut child_ctx = self.add_child(child_id, child_local);
        child_ctx.set_transform(placement);
        if let Some(cull) = cull_rect {
            child_ctx.set_cull_rect(cull);
        }

        child_ctx.tree().with_widget(child_id, |child| {
            crate::reactive::with_signal_tracking(child_id, crate::jobs::JobType::Paint, || {
                child.paint(&mut child_ctx)
            })
        });
        crate::render_stats::record_paint_child_painted();
    }

    /// Try to satisfy a child from its paint cache. Returns whether it worked.
    fn reuse_cached(
        &mut self,
        child_id: WidgetId,
        (child_x, child_y): (f32, f32),
        child_local: Rect,
        child_position: Transform,
        opts: &ChildPaintOptions,
    ) -> bool {
        if opts.cache_requires_full_visibility {
            let Some(ref cull) = opts.cull_rect else {
                return false;
            };
            let fully_visible = child_x >= cull.x
                && child_x + child_local.width <= cull.x + cull.width
                && child_y >= cull.y
                && child_y + child_local.height <= cull.y + cull.height;
            if !fully_visible {
                return false;
            }
        }

        if self.tree().needs_paint(child_id) {
            return false;
        }
        let Some(cached) = self.tree().cached_paint(child_id) else {
            return false;
        };
        // Never re-size a cached node — see the module docs.
        if cached.bounds.width != child_local.width || cached.bounds.height != child_local.height {
            return false;
        }

        // The branch below undoes the old placement by negating it, which is only
        // undoing if the placement was a translation. `paint_child` builds one;
        // anything else reaches a child through `paint_child_placed`, which does
        // not come here. Said out loud, because the doc above is otherwise the
        // only thing holding it.
        debug_assert!(
            cached.parent_position.is_pure_translation(),
            "a cached child was placed with something other than a translation"
        );

        if cached.parent_position == child_position {
            // Unchanged position: the render tree and the cache share the node.
            self.add_child_rc(std::rc::Rc::clone(cached));
        } else {
            // Moved: shallow header clone (children and commands stay Rc-shared),
            // with the user transform extracted and recomposed at the new position.
            let mut reused = (**cached).clone();
            // A position is a pure translation, so undoing it is negating it —
            // no determinant, no inverse, and nothing to unwrap.
            let pos = &cached.parent_position;
            let user_part =
                Transform::translate(-pos.tx(), -pos.ty()).then(&cached.local_transform);
            reused.local_transform = child_position.then(&user_part);
            reused.parent_position = child_position;
            reused.bounds = child_local;
            reused.repainted.set(false);
            self.add_child_rc(std::rc::Rc::new(reused));
        }
        crate::render_stats::record_paint_child_cached();
        true
    }
}
