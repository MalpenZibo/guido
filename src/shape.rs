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
//! scroller cut the card inside it, and a turned clip cuts what it covers. It
//! does not undo #397: a region is narrowed by one clip, and the chain that
//! produced it was collapsed before it arrived.

use crate::layout::Axis;
use crate::renderer::CornerRadii;
use crate::transform::Transform;
use crate::widgets::Rect;
use smallvec::SmallVec;

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
/// [`world_aabb`](Self::world_aabb) for one, and `world_radii` to cut it
/// with.
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
    pub(crate) fn from_clip(
        clip: &crate::renderer::tree::ClipRegion,
        placement: Transform,
    ) -> Self {
        Self::placed(clip.rect, clip.corner_radius, clip.curvature, placement)
    }

    /// The same, spelled out, for a caller holding four loose values.
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
    pub(crate) fn world_radii(&self) -> CornerRadii {
        self.radii.scaled(self.placement.extract_scale())
    }

    /// This shape reduced to its world box — what a clip would have been
    /// before it carried a transform.
    ///
    /// The shape is lost here, deliberately: this is the fallback for the one
    /// case a rect and a matrix cannot describe.
    pub(crate) fn flattened(&self) -> Self {
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

    /// Where the shape starts and stops, vertically, on the surface.
    #[inline]
    pub(crate) fn y_bounds(&self) -> (f32, f32) {
        self.world_aabb().span(Axis::Vertical)
    }

    /// Whether this shape is simply a rectangle on the surface: no turn, no
    /// skew, and corners too small to round away a whole pixel.
    ///
    /// Half a pixel is a *surface* measurement and the radii are in the
    /// widget's own space, so the two only mean the same thing at a scale of
    /// one: a corner of 0.4 under a `scale(8.0)` is three pixels of curve.
    pub(crate) fn is_a_box(&self) -> bool {
        self.placement.keeps_axes() && self.smallest_corner_on_surface() <= 0.5
    }

    /// The largest corner this shape has, measured where it will be drawn.
    ///
    /// Only meaningful once [`keeps_axes`](Transform::keeps_axes) holds, which
    /// is what makes the larger of the two factors the whole of what the
    /// transform does to a radius.
    fn smallest_corner_on_surface(&self) -> f32 {
        let (sx, sy) = self.placement.extract_scale_components();
        self.clamped_radii().max() * sx.max(sy)
    }

    /// The outline, transformed once and ready to be asked about scanlines.
    ///
    /// **Once, because the band loop asks hundreds of times.** A tall shape is
    /// cut into five hundred bands and every boundary wants a span; rebuilding
    /// four edges and four corners, clamping the radii and transforming eight
    /// points for each of them is the same answer computed five hundred times.
    ///
    /// **A corner flatter than a circle becomes a chord.** The arc solve
    /// parameterises a circle, and `curvature` says the corner is a
    /// superellipse — `k = 0` a bevel, `1` a circle, `2` a squircle. Above a
    /// circle the drawn shape is *larger*, so cutting the circle inscribes it
    /// and the tessellation's promise holds. Below one it is smaller, and
    /// cutting the circle would claim ground the shape does not cover: a
    /// bevelled container would blur the desktop past its own diagonal and take
    /// clicks there. The chord between the arc's two ends is inside every
    /// superellipse with `k >= 0`, and is exactly the bevel itself.
    pub(crate) fn outline(&self) -> Outline {
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
        let mut edges: SmallVec<[Edge; 8]> = SmallVec::new();
        for (from, to) in [
            ((x0 + r.top_left, y0), (x1 - r.top_right, y0)),
            ((x1, y0 + r.top_right), (x1, y1 - r.bottom_right)),
            ((x1 - r.bottom_right, y1), (x0 + r.bottom_left, y1)),
            ((x0, y1 - r.bottom_left), (x0, y0 + r.top_left)),
        ] {
            edges.push(Edge::placed(from, to, &self.placement));
        }

        let mut arcs: SmallVec<[Arc; 4]> = SmallVec::new();
        for corner in corners.iter().filter(|c| c.radius > 0.0) {
            if self.curvature >= 1.0 {
                arcs.push(Arc::placed(corner, &self.placement));
            } else {
                edges.push(Edge::placed(
                    corner.on_x_side(),
                    corner.on_y_side(),
                    &self.placement,
                ));
            }
        }

        Outline { edges, arcs }
    }

    /// The exact horizontal extent of the shape at surface scanline `y`.
    ///
    /// A convenience over [`Outline::span_at`] for a caller asking once; the
    /// band loop places the outline itself and keeps it.
    #[cfg(test)]
    pub(crate) fn span_at(&self, y: f32) -> Option<(f32, f32)> {
        self.outline().span_at(y)
    }
}

/// A shape's outline in *surface* coordinates: the straight parts and the
/// curved ones, each already carrying what a scanline query needs.
pub(crate) struct Outline {
    edges: SmallVec<[Edge; 8]>,
    arcs: SmallVec<[Arc; 4]>,
}

impl Outline {
    /// The exact horizontal extent of the shape at surface scanline `y`.
    ///
    /// **The boundary, piece by piece.** The shape is convex and its outline is
    /// straight edges and corner arcs; where a horizontal line meets the
    /// outline is where the shape begins and ends on that line. Every piece is
    /// already transformed, so nothing here cares whether the shape is turned —
    /// a rotation is not a case, it is what the matrix happened to hold. An
    /// axis-aligned shape falls out of the same arithmetic, which is why there
    /// is no separate corner inset: that was this, solved in advance for the
    /// one transform that keeps the axes.
    pub(crate) fn span_at(&self, y: f32) -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut hit = |x: f32| {
            lo = lo.min(x);
            hi = hi.max(x);
        };

        for edge in &self.edges {
            let ((ax, ay), (bx, by)) = (edge.from, edge.to);
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

        for arc in &self.arcs {
            arc.hits(y, &mut hit);
        }

        (lo <= hi).then_some((lo, hi))
    }
}

/// One straight part of an outline, in *surface* coordinates.
#[derive(Debug, Clone, Copy)]
struct Edge {
    from: (f32, f32),
    to: (f32, f32),
}

impl Edge {
    fn placed(from: (f32, f32), to: (f32, f32), placement: &Transform) -> Self {
        Self {
            from: placement.transform_point(from.0, from.1),
            to: placement.transform_point(to.0, to.1),
        }
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

/// A corner arc with everything a scanline query needs worked out in advance.
///
/// The arc is `centre + A·r·(cos t, sin t)`, so its height above a scanline is
/// `p·cos t + q·sin t = y - centre_y`. That is one cosine with a phase, which
/// is why `amplitude` and `phase` can be found once and every scanline costs an
/// `acos` rather than a solve.
#[derive(Debug, Clone, Copy)]
struct Arc {
    /// The circle, still in the shape's own space: the answer is a point on it.
    corner: Corner,
    /// Where its centre sits on the surface, vertically.
    centre_y: f32,
    solve: Solve,
    placement: Transform,
}

/// How to get from a scanline to a point on the arc.
#[derive(Debug, Clone, Copy)]
enum Solve {
    /// Nothing turned it, so the scanline gives `sin t` directly.
    Upright { half_height: f32 },
    /// Something did, and the height is a cosine with a phase.
    Turned { amplitude: f32, phase: f32 },
}

impl Arc {
    fn placed(corner: &Corner, placement: &Transform) -> Self {
        let (_, centre_y) = placement.transform_point(corner.centre.0, corner.centre.1);
        // An axis-keeping placement leaves `c` at zero, so the height equation
        // loses its cosine term and `sin t` falls straight out of the scanline.
        // That is one square root where the general case is an `atan2`, an
        // `acos` and two more trig calls — per corner, per scanline, on every
        // painted frame, for shapes that are almost all of them.
        if placement.keeps_axes() {
            return Self {
                corner: *corner,
                centre_y,
                solve: Solve::Upright {
                    half_height: placement.d() * corner.radius,
                },
                placement: *placement,
            };
        }
        // `c` and `d`, the matrix's *second row*: the layout is
        // `[a, b, tx, c, d, ty]` and `y ↦ c·x + d·y + ty`, so those are the two
        // coefficients that decide a point's height. Reading `b` here — the
        // x-from-y term — is inert for anything axis-aligned, where `b` and `c`
        // are both zero, and mirrors every solution about the corner's centre
        // the moment something turns.
        let (p, q) = (placement.c() * corner.radius, placement.d() * corner.radius);
        Self {
            corner: *corner,
            centre_y,
            solve: Solve::Turned {
                amplitude: (p * p + q * q).sqrt(),
                phase: q.atan2(p),
            },
            placement: *placement,
        }
    }

    /// Where this arc crosses scanline `y`, if it does — at most twice.
    fn hits(&self, y: f32, hit: &mut impl FnMut(f32)) {
        let target = y - self.centre_y;
        let (cx, cy) = self.corner.centre;
        let r = self.corner.radius;

        let pair = match self.solve {
            Solve::Upright { half_height } => {
                if half_height.abs() < f32::EPSILON || target.abs() > half_height.abs() {
                    return;
                }
                let st = target / half_height;
                let ct = (1.0 - st * st).max(0.0).sqrt();
                [(ct, st), (-ct, st)]
            }
            Solve::Turned { amplitude, phase } => {
                if amplitude < f32::EPSILON || target.abs() > amplitude {
                    return;
                }
                let base = (target / amplitude).clamp(-1.0, 1.0).acos();
                let (a, b) = (phase + base, phase - base);
                [(a.cos(), a.sin()), (b.cos(), b.sin())]
            }
        };

        for (ct, st) in pair {
            // Only this corner's own quadrant of the circle. Asked in the
            // circle's own frame, which is where the quadrant is defined — the
            // transform is applied after.
            if ct * self.corner.quadrant.0 < -1e-6 || st * self.corner.quadrant.1 < -1e-6 {
                continue;
            }
            let (wx, _) = self.placement.transform_point(cx + r * ct, cy + r * st);
            hit(wx);
        }
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
