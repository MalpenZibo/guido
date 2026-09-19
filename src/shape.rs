//! A rounded rect, and the transform that puts it on the screen.
//!
//! **One type, because a clip and a region are the same object.** A clip is a
//! rounded rect a widget declared and a transform that places it; so is the
//! area a backdrop blur filters, and so is the region a surface takes input
//! in. They were two structs with the same four fields under two names, and
//! the seam showed at the one call site that held both: a region tessellated
//! its own shape exactly and then asked the clip beside it for a bounding box,
//! because only one of the two knew how to be a shape.
//!
//! Now the geometry lives here and both are it. That is what lets a region be
//! narrowed by a *shape* rather than by a box — the corners of a rounded
//! scroller cut the card inside it, and a turned clip cuts what it covers.

use crate::renderer::CornerRadii;
use crate::transform::Transform;
use crate::widgets::Rect;

/// A rounded rect as its widget declared it, and the transform that places it.
///
/// **The shape is kept, not its bounding box.** A clip used to be stored as a
/// world-space rect, built by mapping the declared one through the world
/// transform — which is an enclosing box, equal to the shape only for a
/// transform that keeps the axes. Under a rotation it is larger and
/// square-cornered, and past about 40° it is large enough that
/// `Overflow::Hidden` stops hiding: an 80×80 box turned 45° bounds to 113×113,
/// so a child overflowing by 20 is not touched. A `wl_region` had the same
/// problem from the other side: a 150×90 card turned 20° published a 172×136
/// box, 73% more area, and the compositor blurred the desktop in the four
/// triangles where the card is not.
///
/// Keeping the declaration and carrying [`placement`](Self::placement) beside
/// it costs one matrix and answers every consumer. A shader inverts it and
/// tests the fragment in the shape's own space, where it is an ordinary
/// rounded rect again and the radii are circles rather than the ellipses a
/// scaled box would need. The region tessellator cuts bands out of the
/// transformed outline. Whatever still wants a box asks
/// [`world_aabb`](Self::world_aabb) for one — and gets
/// [`world_radii`](Self::world_radii) to cut it with.
///
/// **What it cannot express is two shapes in two different rotated spaces.**
/// One rect and one matrix describe one parallelogram, and the intersection of
/// two that are turned differently is not one. `intersect_clips` in the
/// flattener falls back to the box around both and #397 records it. The
/// tessellator is the exception: it never writes the intersection down, so it
/// can take both shapes and meet them a scanline at a time.
#[derive(Debug, Clone, Copy)]
pub struct PlacedShape {
    /// The rect, in the space of the widget that declared it.
    pub rect: Rect,
    /// Its corner radii, in that same space — circles there, whatever the
    /// transform makes of them out here.
    pub radii: CornerRadii,
    /// Superellipse curvature (K-value): 0 is a bevel, 1 a circle, 2 a
    /// squircle.
    pub curvature: f32,
    /// That space to logical surface pixels.
    pub placement: Transform,
}

impl PlacedShape {
    /// A declared shape, placed — which is to say, not changed at all.
    ///
    /// It used to be mapped into world space by its caller, and that mapping
    /// is what this whole type exists to stop doing: the declaration is exact
    /// and its image under a rotation is not a rect, so the only thing that
    /// could be stored was the box around it. Carrying the transform keeps
    /// both.
    pub fn placed(rect: Rect, radii: CornerRadii, curvature: f32, placement: Transform) -> Self {
        Self {
            rect,
            radii,
            curvature,
            placement,
        }
    }

    /// The axis-aligned world box containing the shape.
    ///
    /// What a clip used to be, for the consumer that can still only take a
    /// box: glyphon clips text to an integer rect, which is over-generous
    /// under rotation and is the remainder of #199.
    #[inline]
    pub fn world_aabb(&self) -> Rect {
        self.placement.map_rect(self.rect)
    }

    /// The corner radii that box is cut with.
    ///
    /// Beside [`world_aabb`](Self::world_aabb) because they are one answer:
    /// the box and the corners of the box.
    ///
    /// One radius per corner, so an unevenly scaled corner — an ellipse — is
    /// approximated by the geometric mean of its two axes. Nothing that has a
    /// choice reads this: the shader tests in the shape's own space where the
    /// corner is a circle again, and the tessellator cuts the placed shape
    /// rather than a box with radii bolted on.
    #[inline]
    pub fn world_radii(&self) -> CornerRadii {
        self.radii.scaled(self.placement.extract_scale())
    }

    /// This shape reduced to its world box — what a clip would have been
    /// before it carried a transform.
    ///
    /// The shape is lost here, deliberately: this is the fallback for the one
    /// case a rect and a matrix cannot describe.
    pub fn flattened(&self) -> Self {
        Self {
            rect: self.world_aabb(),
            radii: self.world_radii(),
            curvature: self.curvature,
            placement: Transform::IDENTITY,
        }
    }

    /// The map into this shape's space, from the *physical* pixels a shader
    /// works in — see [`Transform::physical_inverse`], which is where the
    /// arithmetic lives because the backdrop pass needs the same thing.
    #[inline]
    pub fn to_local(&self, scale: f32) -> Option<Transform> {
        self.placement.physical_inverse(scale)
    }

    /// The corners, clamped so no two sharing an edge overlap.
    pub(crate) fn clamped_radii(&self) -> CornerRadii {
        clamp_radii(self.radii, self.rect.width, self.rect.height)
    }

    /// The outline, in the shape's own coordinates: the straight parts and
    /// the curved ones.
    ///
    /// **A corner flatter than a circle becomes a chord.** The arc solve below
    /// parameterises a circle, and `curvature` says the corner is a
    /// superellipse — `k = 0` a bevel, `1` a circle, `2` a squircle. Above a
    /// circle the shape is *larger* than the circle, so cutting the circle
    /// inscribes it and the tessellation's promise holds. Below one it is
    /// smaller, and cutting the circle would claim ground the shape does not
    /// cover: a bevelled container would blur the desktop past its own cut and
    /// take clicks there. The chord between the arc's two ends is inside every
    /// superellipse with `k >= 0`, and is exactly the shape of the bevel
    /// itself.
    fn outline(&self) -> (Vec<Edge>, Vec<Corner>) {
        let r = self.clamped_radii();
        let (x0, y0) = (self.rect.x, self.rect.y);
        let (x1, y1) = (x0 + self.rect.width, y0 + self.rect.height);

        let corners = [
            Corner {
                centre: (x0 + r.top_left, y0 + r.top_left),
                radius: r.top_left,
                quadrant: (-1.0, -1.0),
            },
            Corner {
                centre: (x1 - r.top_right, y0 + r.top_right),
                radius: r.top_right,
                quadrant: (1.0, -1.0),
            },
            Corner {
                centre: (x1 - r.bottom_right, y1 - r.bottom_right),
                radius: r.bottom_right,
                quadrant: (1.0, 1.0),
            },
            Corner {
                centre: (x0 + r.bottom_left, y1 - r.bottom_left),
                radius: r.bottom_left,
                quadrant: (-1.0, 1.0),
            },
        ];

        // Each side runs between the two corners that are on it and no others.
        // Writing one of those four names twice is a side that overshoots its
        // own corner, and is invisible while the radii agree.
        let mut edges = vec![
            Edge::new((x0 + r.top_left, y0), (x1 - r.top_right, y0)),
            Edge::new((x1, y0 + r.top_right), (x1, y1 - r.bottom_right)),
            Edge::new((x1 - r.bottom_right, y1), (x0 + r.bottom_left, y1)),
            Edge::new((x0, y1 - r.bottom_left), (x0, y0 + r.top_left)),
        ];

        let curved = if self.curvature >= 1.0 {
            corners.into_iter().filter(|c| c.radius > 0.0).collect()
        } else {
            for corner in corners.iter().filter(|c| c.radius > 0.0) {
                edges.push(Edge::new(corner.on_x_side(), corner.on_y_side()));
            }
            Vec::new()
        };
        (edges, curved)
    }

    /// Where the shape starts and stops, vertically, on the surface.
    pub(crate) fn y_bounds(&self) -> (f32, f32) {
        let b = self.placement.map_rect(self.rect);
        (b.y, b.y + b.height)
    }

    /// The exact horizontal extent of the shape at surface scanline `y`.
    ///
    /// **The boundary, piece by piece.** The shape is convex and its outline is
    /// four straight edges and four corner arcs; where a horizontal line meets
    /// the outline is where the shape begins and ends on that line. Each piece
    /// is transformed, so nothing here cares whether the shape is turned — a
    /// rotation is not a case, it is just what the matrix happens to hold. An
    /// axis-aligned shape falls out of the same arithmetic, which is why
    /// `corner_inset` is gone: it was this, solved in advance for the one
    /// transform that keeps the axes.
    pub(crate) fn span_at(&self, y: f32) -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut hit = |x: f32| {
            lo = lo.min(x);
            hi = hi.max(x);
        };

        let (edges, corners) = self.outline();

        for edge in edges {
            let (ax, ay) = self.placement.transform_point(edge.from.0, edge.from.1);
            let (bx, by) = self.placement.transform_point(edge.to.0, edge.to.1);
            // Horizontal after transforming: it either lies on this scanline
            // or misses it, and if it lies on it both ends are hits.
            //
            // Exactly zero rather than nearly: this is a *length*, so any
            // absolute tolerance means something different for a 10 pixel edge
            // than a 1000 pixel one. The value is either 0 — identity,
            // translate, axis scale — or large enough that the sloped branch is
            // the right one.
            if by == ay {
                if (ay - y).abs() < 1e-4 {
                    hit(ax);
                    hit(bx);
                }
                continue;
            }
            let t = (y - ay) / (by - ay);
            if (0.0..=1.0).contains(&t) {
                hit(ax + t * (bx - ax));
            }
        }

        for corner in corners {
            let (cx, cy) = corner.centre;
            let r = corner.radius;
            let (_, tcy) = self.placement.transform_point(cx, cy);
            // The arc is `centre + A·r·(cos t, sin t)`. Its height above the
            // scanline is `P cos t + Q sin t = R`, which is one cosine with a
            // phase — so there are two answers, or none.
            //
            // `c` and `d`, the matrix's *second row*: the layout is
            // `[a, b, tx, c, d, ty]` and `y ↦ c·x + d·y + ty`, so those are the
            // two coefficients that decide a point's height. Reading `b` here —
            // the x-from-y term — is inert for anything axis-aligned, where `b`
            // and `c` are both zero, and mirrors every solution about the
            // corner's centre the moment something turns.
            let (p, q) = (self.placement.c() * r, self.placement.d() * r);
            let r_target = y - tcy;
            let amplitude = (p * p + q * q).sqrt();
            if amplitude < f32::EPSILON || r_target.abs() > amplitude {
                continue;
            }
            let phase = q.atan2(p);
            let base = (r_target / amplitude).clamp(-1.0, 1.0).acos();
            for theta in [phase + base, phase - base] {
                let (ct, st) = (theta.cos(), theta.sin());
                // Only this corner's own quadrant of the circle. Asked in the
                // circle's own frame, which is where the quadrant is defined —
                // the transform is applied after.
                if ct * corner.quadrant.0 < -1e-6 || st * corner.quadrant.1 < -1e-6 {
                    continue;
                }
                let (wx, _) = self.placement.transform_point(cx + r * ct, cy + r * st);
                hit(wx);
            }
        }

        (lo <= hi).then_some((lo, hi))
    }
}

/// One straight part of an outline, in the shape's own coordinates.
#[derive(Debug, Clone, Copy)]
struct Edge {
    from: (f32, f32),
    to: (f32, f32),
}

impl Edge {
    fn new(from: (f32, f32), to: (f32, f32)) -> Self {
        Self { from, to }
    }
}

/// One corner's circle, in the shape's own coordinates.
#[derive(Debug, Clone, Copy)]
struct Corner {
    centre: (f32, f32),
    radius: f32,
    /// Which quadrant of the circle this corner is, as a sign per axis.
    quadrant: (f32, f32),
}

impl Corner {
    /// Where the arc meets the side it shares an x with — the left or right
    /// edge.
    fn on_x_side(&self) -> (f32, f32) {
        (self.centre.0 + self.quadrant.0 * self.radius, self.centre.1)
    }

    /// Where it meets the side it shares a y with — the top or bottom edge.
    fn on_y_side(&self) -> (f32, f32) {
        (self.centre.0, self.centre.1 + self.quadrant.1 * self.radius)
    }
}

/// Shrink radii until no two sharing an edge can overlap, all by one factor so
/// the shape keeps its proportions — the rule CSS `border-radius` uses.
///
/// The shader clamps each corner independently, so the two disagree for
/// per-corner radii and a region could reach slightly outside the drawn shape.
/// Noted at the end of #198 and still true; the difference is bounded by the
/// radius and only appears where two corners on one side would overlap, which
/// is a shape already at its limit.
fn clamp_radii(radii: CornerRadii, w: f32, h: f32) -> CornerRadii {
    let r = CornerRadii {
        top_left: radii.top_left.max(0.0),
        top_right: radii.top_right.max(0.0),
        bottom_right: radii.bottom_right.max(0.0),
        bottom_left: radii.bottom_left.max(0.0),
    };
    let ratio = |extent: f32, sum: f32| if sum > extent { extent / sum } else { 1.0 };
    let f = ratio(w, r.top_left + r.top_right)
        .min(ratio(w, r.bottom_left + r.bottom_right))
        .min(ratio(h, r.top_left + r.bottom_left))
        .min(ratio(h, r.top_right + r.bottom_right));
    r.scaled(f)
}
