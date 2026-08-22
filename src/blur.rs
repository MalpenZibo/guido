//! Compositor-side background blur (`ext-background-effect-v1`).
//!
//! Containers opt in via [`backdrop_blur`](crate::widgets::Container::backdrop_blur),
//! which emits one `DrawCommand::BackdropBlur` carrying the sources it asked
//! for. The renderer filters the surface's own backdrop from that command; this
//! module reads the compositor's region off the same one, after the frame has
//! been flattened, and tessellates its rounded corners into axis-aligned rects
//! — `wl_region` has no notion of curves.
//!
//! Nothing is remembered between frames: see [`regions_from_commands`] for why
//! that is the whole point. No-ops when the compositor does not support the
//! protocol or its blur capability.

use crate::backdrop::BackdropSources;
use crate::renderer::{CornerRadii, DrawCommand, EllipticalRadii, FlattenedCommand};
use crate::widgets::Rect;

/// An axis-aligned rectangle of a blur region, in logical surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlurRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The region to hand `ext-background-effect-v1`, read off the frame that was
/// just drawn.
///
/// **Derived, never remembered.** An earlier version kept a registry that
/// containers wrote to during paint, and every way a container can *stop*
/// painting — hidden, culled, outside a scroller's window, satisfied from the
/// paint cache — was a way for that registry to disagree with the screen. Each
/// one had to be found and told, and one of them was always missing.
///
/// The flattened command list is the frame. A container that did not paint has
/// no command in it; one served from the paint cache carries its command along
/// unchanged; a culled one is absent. Ordering stops mattering too, since the
/// list only exists after the paint that built it.
pub(crate) fn regions_from_commands(commands: &[FlattenedCommand]) -> Vec<BlurRect> {
    let mut out = Vec::new();
    for cmd in commands {
        let DrawCommand::BackdropBlur {
            rect,
            sources,
            corner_radii,
            ..
        } = &*cmd.command
        else {
            continue;
        };
        if !sources.contains(BackdropSources::COMPOSITOR) {
            continue;
        }

        // World coordinates *and* the clip, by the same computation the renderer
        // uses for the surface half of this very command — so the two cannot
        // describe different shapes. A card half out of a viewport is filtered
        // only where it is on show, and a region published for the whole card
        // blurs the desktop beside a panel that is not there.
        let Some((world, world_radii)) = cmd.clipped_world_rounded_rect(*rect, *corner_radii)
        else {
            continue;
        };
        out.extend(rounded_rect_to_blur_rects(world, world_radii));
    }
    out.sort_unstable_by_key(|r| (r.y, r.x, r.width, r.height));
    out
}

/// Approximate a rounded rectangle as a union of axis-aligned rects that
/// **inscribe** the shape. Zero radii or a degenerate rectangle yield the
/// bounding box.
///
/// Every corner separately, and every corner an ellipse. One radius for the
/// whole shape was the seam this used to be reached through: the caller
/// computed four and collapsed them with `.max()` on the way in, so a card cut
/// square on one side by a clip was still tessellated as though that side were
/// round, and the region lost a wedge the size of the radius along the cut —
/// which is the artefact the clipping exists to prevent.
///
/// `bounds` is in logical surface pixels, matching what `wl_region` expects.
fn rounded_rect_to_blur_rects(bounds: Rect, radii: EllipticalRadii) -> Vec<BlurRect> {
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Vec::new();
    }

    let (w, h) = (bounds.width, bounds.height);
    let (rx, ry) = clamp_radii(radii, w, h);

    // How far each side is eaten into at height `y`, by whichever corner on that
    // side reaches it. Zero between the bands, and correct where a shape short
    // enough for its corners to meet has both reaching the same height.
    let left = |y: f32| {
        corner_inset(y, rx.top_left, ry.top_left).max(corner_inset(
            h - y,
            rx.bottom_left,
            ry.bottom_left,
        ))
    };
    let right = |y: f32| {
        corner_inset(y, rx.top_right, ry.top_right).max(corner_inset(
            h - y,
            rx.bottom_right,
            ry.bottom_right,
        ))
    };

    let band_top = ry.top_left.max(ry.top_right);
    let band_bottom = ry.bottom_left.max(ry.bottom_right);
    if band_top <= 0.5 && band_bottom <= 0.5 {
        return Vec::from_iter(to_blur_rect(bounds));
    }

    let mut out = Vec::new();

    // The straight middle is one rect rather than a stack of slabs.
    if h - band_bottom > band_top {
        out.extend(to_blur_rect(Rect::new(
            bounds.x,
            bounds.y + band_top,
            w,
            h - band_top - band_bottom,
        )));
    }

    let emit_band = |y_start: f32, y_end: f32, out: &mut Vec<BlurRect>| {
        let band_h = y_end - y_start;
        if band_h <= 0.0 {
            return;
        }
        let steps = (band_h.ceil() as u32).clamp(4, 32);
        let slab_h = band_h / steps as f32;
        for i in 0..steps {
            let y0 = y_start + i as f32 * slab_h;
            let y1 = y0 + slab_h;
            // Widest inset within the slab keeps it inside the curve.
            let (l, r) = (left(y0).max(left(y1)), right(y0).max(right(y1)));
            let slab_w = w - l - r;
            if slab_w <= 0.0 {
                continue;
            }
            out.extend(to_blur_rect(Rect::new(
                bounds.x + l,
                bounds.y + y0,
                slab_w,
                slab_h,
            )));
        }
    };

    emit_band(0.0, band_top, &mut out);
    // Starting where the top band ended, so corners tall enough to meet in a
    // short shape are tessellated once rather than twice.
    emit_band((h - band_bottom).max(band_top), h, &mut out);

    out
}

/// Shrink radii until no two sharing an edge can overlap, all by one factor so
/// the shape keeps its proportions — the rule CSS `border-radius` uses.
fn clamp_radii(radii: EllipticalRadii, w: f32, h: f32) -> (CornerRadii, CornerRadii) {
    let non_negative = |r: CornerRadii| CornerRadii {
        top_left: r.top_left.max(0.0),
        top_right: r.top_right.max(0.0),
        bottom_right: r.bottom_right.max(0.0),
        bottom_left: r.bottom_left.max(0.0),
    };
    let (rx, ry) = (non_negative(radii.x), non_negative(radii.y));

    let ratio = |extent: f32, sum: f32| if sum > extent { extent / sum } else { 1.0 };
    let f = ratio(w, rx.top_left + rx.top_right)
        .min(ratio(w, rx.bottom_left + rx.bottom_right))
        .min(ratio(h, ry.top_left + ry.bottom_left))
        .min(ratio(h, ry.top_right + ry.bottom_right));

    (rx.scaled(f), ry.scaled(f))
}

/// How far a corner eats into its side at distance `d` from the edge it sits
/// on, for an ellipse `rx` wide and `ry` tall. Zero once past the corner.
fn corner_inset(d: f32, rx: f32, ry: f32) -> f32 {
    if rx <= 0.0 || ry <= 0.0 || d >= ry {
        return 0.0;
    }
    let dy = (ry - d) / ry;
    rx - rx * (1.0 - dy * dy).max(0.0).sqrt()
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

    /// The same radius on every corner and both axes.
    fn round(radius: f32) -> EllipticalRadii {
        EllipticalRadii::circular(CornerRadii::uniform(radius))
    }

    #[test]
    fn zero_radius_is_bounding_box() {
        let rects = rounded_rect_to_blur_rects(Rect::new(10.0, 20.0, 100.0, 50.0), round(0.0));
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
        assert!(rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 0.0, 40.0), round(8.0)).is_empty());
        assert!(rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 40.0, -1.0), round(8.0)).is_empty());
    }

    #[test]
    fn rounded_corners_are_cut() {
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 100.0, 60.0), round(20.0));
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
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 80.0, 80.0), round(r));
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
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 40.0, 40.0), round(100.0));
        assert!(!rects.is_empty());
        assert!(covers(&rects, 20, 20));
        assert!(!covers(&rects, 0, 0));
    }

    /// A square corner is square. This is the seam the region used to be lost
    /// at: a clip squared off two corners, the caller collapsed the four radii
    /// with `.max()`, and the shape arrived here as though all four were round —
    /// so the region gave back a wedge the size of the radius along the cut,
    /// which is the artefact the clipping exists to prevent.
    #[test]
    fn a_square_corner_is_not_rounded_because_another_one_is() {
        let square_right = EllipticalRadii::circular(CornerRadii {
            top_left: 16.0,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: 16.0,
        });
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 40.0, 100.0), square_right);

        assert!(
            covers(&rects, 39, 0) && covers(&rects, 39, 99),
            "the cut edge is straight, so its corners reach the edge"
        );
        assert!(
            !covers(&rects, 0, 0) && !covers(&rects, 0, 99),
            "and the round ones are still cut"
        );
    }

    /// Only the corner that has a radius is cut. A list row rounding its top
    /// corners and butting against the next one keeps its full bottom edge.
    #[test]
    fn corners_are_cut_one_by_one() {
        let rects = rounded_rect_to_blur_rects(
            Rect::new(0.0, 0.0, 100.0, 60.0),
            EllipticalRadii::circular(CornerRadii::top(20.0)),
        );

        assert!(!covers(&rects, 0, 0) && !covers(&rects, 99, 0), "top cut");
        assert!(
            covers(&rects, 0, 59) && covers(&rects, 99, 59),
            "bottom square"
        );
    }

    /// An unevenly scaled corner is an ellipse: 32 wide and 16 tall reaches the
    /// left edge 16 down, where a circle of either radius would not.
    #[test]
    fn an_elliptical_corner_is_cut_on_both_of_its_axes() {
        let radii = EllipticalRadii {
            x: CornerRadii::uniform(32.0),
            y: CornerRadii::uniform(16.0),
        };
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, 200.0, 100.0), radii);

        assert!(!covers(&rects, 0, 0), "the corner itself is outside");
        assert!(
            covers(&rects, 0, 20),
            "the curve is 16 tall, so the left edge is reached by 20 down"
        );
        assert!(
            !covers(&rects, 5, 1),
            "and 32 wide, so it is still outside 5 in at the top"
        );
    }

    /// Every emitted rect stays inside the ellipse it approximates, for radii
    /// that differ per corner and per axis — the property the whole tessellation
    /// exists to have, checked where the shape is least uniform.
    #[test]
    fn slabs_stay_inside_a_shape_with_four_different_corners() {
        let radii = EllipticalRadii {
            x: CornerRadii {
                top_left: 30.0,
                top_right: 10.0,
                bottom_right: 0.0,
                bottom_left: 20.0,
            },
            y: CornerRadii {
                top_left: 15.0,
                top_right: 10.0,
                bottom_right: 0.0,
                bottom_left: 40.0,
            },
        };
        let (w, h) = (120.0f32, 100.0f32);
        let rects = rounded_rect_to_blur_rects(Rect::new(0.0, 0.0, w, h), radii);
        assert!(!rects.is_empty());

        // The centre of each corner's ellipse, and its two semi-axes.
        let corners = [
            (30.0f32, 15.0f32, 30.0f32, 15.0f32, true, true),
            (w - 10.0, 10.0, 10.0, 10.0, false, true),
            (20.0, h - 40.0, 20.0, 40.0, true, false),
        ];
        for rect in &rects {
            for (px, py) in [
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x, rect.y + rect.height),
                (rect.x + rect.width, rect.y + rect.height),
            ] {
                let (px, py) = (px as f32, py as f32);
                for (cx, cy, rx, ry, left, top) in corners {
                    // Only points in this corner's quadrant are governed by it.
                    let in_x = if left { px < cx } else { px > cx };
                    let in_y = if top { py < cy } else { py > cy };
                    if !in_x || !in_y {
                        continue;
                    }
                    let d = ((px - cx) / rx).powi(2) + ((py - cy) / ry).powi(2);
                    assert!(
                        d <= 1.0 + 0.1,
                        "({px}, {py}) is outside the {rx}x{ry} corner at ({cx}, {cy}): {d}"
                    );
                }
            }
        }
    }
}
