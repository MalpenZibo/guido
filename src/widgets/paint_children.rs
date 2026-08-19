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
//! - A culled child makes the parent's paint **partial**, and a partial paint
//!   is never cached — otherwise reusing it later would permanently hide the
//!   children this pass skipped.
//! - A cached node may be re-parented but never re-sized: its commands were
//!   built for the size it was painted at. A child that changed size ran its
//!   own layout and marked itself dirty, so it should not reach the reuse
//!   path; falling through to a full paint if it ever does keeps a missed
//!   invalidation from showing stale content stretched to the wrong box.

use crate::renderer::PaintContext;
use crate::transform::Transform;
use crate::tree::{Tree, WidgetId};
use crate::widgets::Rect;

/// How a parent wants its children painted.
pub(crate) struct ChildPaintOptions {
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
}

impl Default for ChildPaintOptions {
    fn default() -> Self {
        Self {
            scroll_offset: (0.0, 0.0),
            cull_rect: None,
            cache_requires_full_visibility: false,
        }
    }
}

/// Paint `children` into `ctx`, culling and reusing cached paint where possible.
pub(crate) fn paint_children(
    tree: &Tree,
    ctx: &mut PaintContext,
    children: &[WidgetId],
    opts: &ChildPaintOptions,
) {
    let (offset_x, offset_y) = opts.scroll_offset;
    // A culled child never paints, so it cannot withdraw a compositor blur region
    // of its own. Collected here and swept once at the end. The check is hoisted:
    // the answer cannot change while the loop runs, and a long culled list would
    // otherwise pay a thread-local borrow per row.
    let mut culled: Vec<WidgetId> = Vec::new();
    let sweeping_blur = !crate::blur::is_empty();

    for &child_id in children {
        // Child bounds come from the tree in the parent's local coordinates
        let child_bounds = tree.get_bounds(child_id).unwrap_or_default();
        let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);
        let (child_x, child_y) = (child_bounds.x, child_bounds.y);
        let child_position = Transform::translate(child_x - offset_x, child_y - offset_y);

        // Cull clean children that fall outside the visible rect
        if let Some(ref cull) = opts.cull_rect
            && !tree.needs_paint(child_id)
        {
            let child_rect = Rect::new(child_x, child_y, child_bounds.width, child_bounds.height);
            if !cull.intersects(&child_rect) {
                crate::render_stats::record_paint_child_culled();
                ctx.mark_partial();
                if sweeping_blur {
                    culled.push(child_id);
                }
                continue;
            }
        }

        if reuse_cached(
            tree,
            ctx,
            child_id,
            (child_x, child_y),
            child_local,
            child_position,
            opts,
        ) {
            continue;
        }

        // Full paint: the child is dirty, has no usable cache, or is only
        // partially visible inside a scrolling parent.
        let mut child_ctx = ctx.add_child(child_id.as_u64(), child_local);
        child_ctx.set_transform(child_position);

        // The cull rect travels into the child's own coordinate space
        if let Some(ref cull) = opts.cull_rect {
            child_ctx.set_cull_rect(Rect::new(
                cull.x - child_x,
                cull.y - child_y,
                cull.width,
                cull.height,
            ));
        }

        tree.with_widget(child_id, |child| {
            child.paint(tree, child_id, &mut child_ctx)
        });
        crate::render_stats::record_paint_child_painted();
    }

    crate::blur::unregister_unpainted(tree, &culled);
}

/// Try to satisfy a child from its paint cache. Returns whether it worked.
fn reuse_cached(
    tree: &Tree,
    ctx: &mut PaintContext,
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

    if tree.needs_paint(child_id) {
        return false;
    }
    let Some(cached) = tree.cached_paint(child_id) else {
        return false;
    };
    // Never re-size a cached node — see the module docs.
    if cached.bounds.width != child_local.width || cached.bounds.height != child_local.height {
        return false;
    }

    if cached.parent_position == child_position {
        // Unchanged position: the render tree and the cache share the node.
        ctx.add_child_rc(std::rc::Rc::clone(cached));
    } else {
        // Moved: shallow header clone (children and commands stay Rc-shared),
        // with the user transform extracted and recomposed at the new position.
        let mut reused = (**cached).clone();
        let user_part = cached
            .parent_position
            .inverse()
            .then(&cached.local_transform);
        reused.local_transform = child_position.then(&user_part);
        reused.parent_position = child_position;
        reused.bounds = child_local;
        reused.repainted.set(false);
        ctx.add_child_rc(std::rc::Rc::new(reused));
    }
    crate::render_stats::record_paint_child_cached();
    true
}
