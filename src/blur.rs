//! Compositor-side background blur (`ext-background-effect-v1`).
//!
//! Containers opt in via `.background_blur()`. During paint they register
//! their id and corner radius here; each frame the surface sync reads the
//! current bounds from the tree, tessellates rounded corners into
//! axis-aligned rects (`wl_region` has no notion of curves), and hands them
//! to the platform layer. No-ops when the compositor doesn't support the
//! protocol or its blur capability.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::tree::{Tree, WidgetId};
use crate::widgets::Rect;

/// An axis-aligned rectangle of a blur region, in logical surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlurRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

thread_local! {
    /// Widgets currently requesting background blur: id → corner radius.
    /// Bounds are read fresh from the tree at collect time; entries whose
    /// widget left the tree are pruned then too.
    static BLUR_WIDGETS: RefCell<HashMap<WidgetId, f32>> = RefCell::new(HashMap::new());
}

/// Record a widget's blur request (called from `Container::paint`).
pub(crate) fn register_blur(id: WidgetId, corner_radius: f32) {
    BLUR_WIDGETS.with(|reg| {
        reg.borrow_mut().insert(id, corner_radius);
    });
}

/// Withdraw a widget's blur request.
///
/// The registry cannot discover this on its own: `collect_for_surface` prunes
/// widgets that left the *tree*, and a container that merely stopped asking for
/// blur is still in it. So the paint that stops asking has to say so — which is
/// exactly the frame on which a radius driven by a signal reaches zero.
pub(crate) fn unregister_blur(id: WidgetId) {
    BLUR_WIDGETS.with(|reg| {
        reg.borrow_mut().remove(&id);
    });
}

/// Collect tessellated blur rects for every blur widget under `root`,
/// sorted for deterministic change detection. Prunes stale entries.
pub(crate) fn collect_for_surface(tree: &Tree, root: WidgetId) -> Vec<BlurRect> {
    BLUR_WIDGETS.with(|reg| {
        let mut reg = reg.borrow_mut();
        let mut out = Vec::new();
        reg.retain(|&id, &mut radius| {
            if !tree.contains(id) {
                return false;
            }
            // Only widgets belonging to this surface's subtree
            if is_under(tree, id, root)
                && let Some(bounds) = tree.get_surface_relative_bounds(id)
            {
                out.extend(rounded_rect_to_blur_rects(bounds, radius));
            }
            true
        });
        out.sort_unstable_by_key(|r| (r.y, r.x, r.width, r.height));
        out
    })
}

fn is_under(tree: &Tree, mut id: WidgetId, root: WidgetId) -> bool {
    loop {
        if id == root {
            return true;
        }
        match tree.get_parent(id) {
            Some(parent) => id = parent,
            None => return false,
        }
    }
}

/// Reset blur state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_blur() {
    BLUR_WIDGETS.with(|reg| reg.borrow_mut().clear());
}

/// Approximate a uniformly rounded rectangle as a union of axis-aligned
/// rects that **inscribe** the shape. A zero radius or degenerate rectangle
/// yields the bounding box.
///
/// `bounds` is in logical surface pixels, matching what `wl_region` expects.
fn rounded_rect_to_blur_rects(bounds: Rect, radius: f32) -> Vec<BlurRect> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Vec::new();
    }

    let w = bounds.width;
    let h = bounds.height;

    // Clamp so two corners sharing an edge can never overlap.
    let r = radius.clamp(0.0, w.min(h) / 2.0);

    if r <= 0.5 {
        return Vec::from_iter(to_blur_rect(bounds));
    }

    let mut out = Vec::new();

    // Middle band spans the full width between the corner bands.
    if h - r > r {
        out.extend(to_blur_rect(Rect::new(
            bounds.x,
            bounds.y + r,
            w,
            h - 2.0 * r,
        )));
    }

    let mut emit_band = |y_start: f32, y_end: f32| {
        let band_h = y_end - y_start;
        let steps = (band_h.ceil() as u32).clamp(4, 32);
        let slab_h = band_h / steps as f32;
        for i in 0..steps {
            let y0 = y_start + i as f32 * slab_h;
            let y1 = y0 + slab_h;
            // Widest inset within the slab keeps it inside the curve.
            let inset = corner_inset(y0, r, h).max(corner_inset(y1, r, h));
            let slab_w = w - 2.0 * inset;
            if slab_w <= 0.0 {
                continue;
            }
            out.extend(to_blur_rect(Rect::new(
                bounds.x + inset,
                bounds.y + y0,
                slab_w,
                slab_h,
            )));
        }
    };

    emit_band(0.0, r);
    emit_band(h - r, h);

    out
}

/// Horizontal inset of a uniformly rounded edge at height `y`.
fn corner_inset(y: f32, r: f32, h: f32) -> f32 {
    if y < r {
        let dy = r - y;
        r - (r * r - dy * dy).max(0.0).sqrt()
    } else if y > h - r {
        let dy = y - (h - r);
        r - (r * r - dy * dy).max(0.0).sqrt()
    } else {
        0.0
    }
}

/// Round-to-nearest on all four edges so adjacent slabs tile without gaps
/// (inward rounding would drop sub-pixel slabs). `None` when the rounding
/// collapses the rectangle to nothing.
fn to_blur_rect(b: Rect) -> Option<BlurRect> {
    let x0 = b.x.round() as i32;
    let y0 = b.y.round() as i32;
    let x1 = (b.x + b.width).round() as i32;
    let y1 = (b.y + b.height).round() as i32;
    (x1 > x0 && y1 > y0).then_some(BlurRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(rects: &[BlurRect], x: i32, y: i32) -> bool {
        rects
            .iter()
            .any(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height)
    }

    #[test]
    fn zero_radius_is_bounding_box() {
        let rects = rounded_rect_to_blur_rects(Rect::new(10.0, 20.0, 100.0, 50.0), 0.0);
        assert_eq!(
            rects,
            vec![BlurRect {
                x: 10,
                y: 20,
                width: 100,
                height: 50
            }]
        );
    }

    #[test]
    fn degenerate_rect_is_empty() {
        assert!(rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 0.0, 40.0), 8.0).is_empty());
        assert!(rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 40.0, -1.0), 8.0).is_empty());
    }

    #[test]
    fn rounded_corners_are_cut() {
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 100.0, 60.0), 20.0);
        // Corner pixels outside the curve are not covered…
        assert!(!covers(&rects, 0, 0));
        assert!(!covers(&rects, 99, 0));
        assert!(!covers(&rects, 0, 59));
        assert!(!covers(&rects, 99, 59));
        // …while the center and edge midpoints are.
        assert!(covers(&rects, 50, 30));
        assert!(covers(&rects, 0, 30));
        assert!(covers(&rects, 99, 30));
        assert!(covers(&rects, 50, 0));
        assert!(covers(&rects, 50, 59));
    }

    #[test]
    fn slabs_stay_inside_the_curve() {
        let r = 16.0f32;
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 80.0, 80.0), r);
        for rect in &rects {
            // Every rect corner must be inside (or on) the rounded shape,
            // with half-pixel tolerance for edge rounding.
            for (px, py) in [
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x, rect.y + rect.height),
                (rect.x + rect.width, rect.y + rect.height),
            ] {
                let (px, py) = (px as f32, py as f32);
                let cx = px.clamp(r, 80.0 - r);
                let cy = py.clamp(r, 80.0 - r);
                let dist = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                assert!(
                    dist <= r + 0.75,
                    "corner ({px}, {py}) is {dist} from the curve center, radius {r}"
                );
            }
        }
    }

    #[test]
    fn radius_clamped_to_half_size() {
        // Radius larger than half the shape: a circle-ish region, still non-empty
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 40.0, 40.0), 100.0);
        assert!(!rects.is_empty());
        assert!(covers(&rects, 20, 20));
        assert!(!covers(&rects, 0, 0));
    }
}
