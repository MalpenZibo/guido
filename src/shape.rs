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
//! does not undo the clip chain's own collapse: a region is narrowed by one
//! clip, and the chain that produced it was reduced before it arrived — exactly
//! where the spaces are a quarter turn, a mirror or a scale apart, and to the
//! box around both otherwise.

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
/// two that are turned differently is not one — unless the turn between them is
/// a quarter of one, or a mirror, where the image of a rect is still a rect and
/// `intersect_clips` rewrites one in the other's space. For any other angle it
/// falls back to the box around both. The tessellator is the exception: it
/// never writes the intersection down, so it can take both shapes and meet them
/// a scanline at a time.
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
    /// What a clip used to be, and still is for the one consumer that cannot
    /// take a shape: glyphon clips text to an integer rect, which is
    /// over-generous under rotation (#405).
    ///
    /// It is also how a *viewport* is found, which is a different thing and not
    /// an approximation — the backdrop pass narrows the box it may write to and
    /// then cuts the difference back off per fragment. Everything else tests in
    /// the clip's own space, so this is the fallback rather than the answer.
    #[inline]
    pub fn world_aabb(&self) -> Rect {
        self.placement.map_rect(self.rect)
    }

    /// The corner radii that box is cut with.
    ///
    /// Beside [`world_aabb`](Self::world_aabb) because they are one answer:
    /// the box and the corners of the box.
    ///
    /// One radius per corner, so an unevenly scaled corner — an ellipse — has
    /// to be approximated. **The larger axis, not the mean of the two**: a
    /// corner that cuts at least as much as the ellipse leaves a region inside
    /// the shape it stands for, and blurs slightly less than it should. The
    /// other way round it blurs where nothing is drawn, which is the whole
    /// class of bug this type exists to close.
    ///
    /// The mean is what the geometric `extract_scale` gives and what this
    /// briefly returned when the elliptical radii were deleted — a direction
    /// change nobody asked for, in the one place still allowed to approximate.
    ///
    /// Nothing that has a choice reads this: the shader tests in the shape's
    /// own space where the corner is a circle again, and the tessellator cuts
    /// the placed shape rather than a box with radii bolted on. It is the
    /// fallback's approximation and nothing else.
    #[inline]
    pub(crate) fn world_radii(&self) -> CornerRadii {
        let (sx, sy) = self.placement.extract_scale_components();
        self.radii.scaled(sx.max(sy))
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
        self.placement.keeps_axes() && self.largest_corner_on_surface() <= 0.5
    }

    /// The largest corner this shape has, measured where it will be drawn.
    ///
    /// Only meaningful once [`keeps_axes`](Transform::keeps_axes) holds, which
    /// is what makes the larger of the two factors the whole of what the
    /// transform does to a radius.
    fn largest_corner_on_surface(&self) -> f32 {
        let (sx, sy) = self.placement.extract_scale_components();
        self.clamped_radii().max() * sx.max(sy)
    }

    /// How many points to sample a corner's curve at, when there is no closed
    /// form for it.
    ///
    /// The sag of a chord across an arc of angle `θ` is about `r·θ²/8`, so for
    /// a quarter turn cut into `n + 1` chords the worst gap between the curve
    /// and the polyline is roughly `r · (π/2)²/8 / (n+1)²`. Asking for under
    /// half a pixel of it gives `n ≈ sqrt(r · 0.31 / 0.5)`, which is 5 for a 40
    /// pixel corner and 8 for a 100 pixel one.
    ///
    /// Taken from the corner as it lands **on the surface**, not as it was
    /// declared: a shape scaled down needs fewer points for the same accuracy,
    /// and a shape scaled up needs more. Clamped low because each sample is an
    /// edge the scanline loop tests, and clamped high because a region is a
    /// list the compositor has to receive.
    fn samples_per_corner(&self) -> usize {
        let radius = self.largest_corner_on_surface();
        ((radius * 0.62).sqrt().ceil() as usize).clamp(2, 12)
    }

    /// The outline, transformed once and ready to be asked about scanlines.
    ///
    /// **Once, because the band loop asks hundreds of times.** A tall shape is
    /// cut into five hundred bands and every boundary wants a span; rebuilding
    /// four edges and four corners, clamping the radii and transforming eight
    /// points for each of them is the same answer computed five hundred times.
    ///
    /// **Every corner is cut as the curve it is.** `curvature` says the corner
    /// is a superellipse — `k = 0` a bevel, `1` a circle, `2` a squircle — and
    /// the arc solve parameterises a circle and nothing else. So a circle takes
    /// the closed form, a bevel takes the chord between the arc's two ends,
    /// which *is* the bevel, and everything else is sampled: chords of a convex
    /// curve lie inside it, so the tessellation's promise — never claim a pixel
    /// the shape does not cover — holds by construction.
    ///
    /// Cutting the circle for all of them was the old answer and it failed in
    /// both directions. Below a circle the shape is smaller, so a bevelled
    /// container blurred the desktop past its own diagonal and took clicks
    /// there. Above one the shape is larger, so a squircle gave up 14% of each
    /// corner box — a 7.6 pixel dead band along the diagonal of a 40 pixel
    /// corner, where a click fell through to whatever was behind (#400).
    ///
    /// **A scoop is not traced, it is taken away.** It curves inward, so
    /// chords of it fall *outside* the shape and sampling cannot be used —
    /// which is why it used to be cut by a chord tangent to the bite, never
    /// over-claiming and giving up 17% of the shape (#410). What the shader
    /// actually draws is the plain box minus a disc sitting on each outer
    /// corner, and that is what is built here: the sides run corner to
    /// corner, no arc is traced, and the four discs go in `bites` for
    /// [`Outline::take_bites_from`] to remove from a span. Exact,
    /// through the same closed-form circle solve every other curvature's arc
    /// uses.
    pub(crate) fn outline(&self) -> Outline {
        let declared = self.clamped_radii();
        // A scoop's sides reach the corners — the curve comes out of the shape
        // afterwards rather than being traced along it — so the outline is
        // built as the plain box and the radii travel in `bites` instead.
        let r = if self.curvature < 0.0 {
            CornerRadii::uniform(0.0)
        } else {
            declared
        };
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
            // An exact form where there is one, and a polyline where there is
            // not. The circle is the only curve here a scanline meets in closed
            // form; every other exponent would need a root-find per band per
            // corner, inside a loop that already runs one band per pixel.
            //
            // Chords of a convex curve lie inside it, so sampling keeps the
            // promise the whole tessellator rests on: the region never claims a
            // pixel the shape does not cover. It gives some back instead, and
            // `samples_per_corner` is what bounds how much.
            let k = self.curvature;
            if k == 1.0 {
                arcs.push(Arc::placed(corner, &self.placement));
            } else if k == 0.0 {
                // A bevel is a straight cut, and the chord between the arc's
                // ends is not an approximation of it — it is the shape.
                edges.push(Edge::placed(
                    corner.on_x_side(),
                    corner.on_y_side(),
                    &self.placement,
                ));
            } else {
                let mut previous = corner.on_x_side();
                for point in corner.superellipse(k, self.samples_per_corner()) {
                    edges.push(Edge::placed(previous, point, &self.placement));
                    previous = point;
                }
                edges.push(Edge::placed(previous, corner.on_y_side(), &self.placement));
            }
        }

        // With `r` zeroed above, every corner's centre already sits on the
        // box's outer corner, which is where a scoop's disc is centred. Only
        // the radius has to be put back — and put back the way the *shader*
        // clamps it, which is `min(radius, min(half_w, half_h))` per corner
        // rather than the proportional rule `clamp_radii` applies. The two
        // agree for uniform radii and not otherwise, and this is the one
        // consumer that can simply match: a bite is subtracted, so two of them
        // overlapping on one side is no more than a column removed twice,
        // which is what the proportional rule exists to prevent and what
        // nothing here needs prevented.
        let half = (self.rect.width * 0.5).min(self.rect.height * 0.5);
        let bites = if self.curvature < 0.0 {
            corners
                .iter()
                .zip(self.radii.to_array().map(|r| r.max(0.0).min(half)))
                .filter(|(_, radius)| *radius > 0.0)
                .map(|(corner, radius)| Arc::placed(&Corner { radius, ..*corner }, &self.placement))
                .collect()
        } else {
            SmallVec::new()
        };

        Outline { edges, arcs, bites }
    }

    /// The exact horizontal extent of the shape at surface scanline `y`,
    /// where the shape has only one.
    ///
    /// A convenience over [`Outline::span_at`] for a caller asking once; the
    /// band loop places the outline itself and keeps it. Panics on a scanline
    /// a scoop has split in two, which is the one case with no single answer
    /// to give — [`Outline::span_at`] is what asks about that.
    #[cfg(test)]
    pub(crate) fn span_at(&self, y: f32) -> Option<(f32, f32)> {
        let spans = self.outline().span_at(y);
        assert!(
            spans.len() <= 1,
            "scanline {y} meets this shape in {} intervals, not one",
            spans.len()
        );
        spans.first().copied()
    }
}

/// A shape's outline in *surface* coordinates: the straight parts and the
/// curved ones, each already carrying what a scanline query needs.
pub(crate) struct Outline {
    edges: SmallVec<[Edge; 8]>,
    arcs: SmallVec<[Arc; 4]>,
    /// The discs a scoop takes *out* of the shape, empty for every other
    /// curvature. Kept apart from `arcs` because they are not boundary the
    /// span runs to — they are boundary the span stops at and starts again
    /// past, which is the one place this shape is not convex.
    bites: SmallVec<[Arc; 4]>,
}

/// Where a shape is on one scanline: ascending, disjoint, and usually one.
///
/// One interval is the whole story for every convex shape, which is every
/// curvature but the scoop. A scoop is the box minus a disc at each corner,
/// and once something turns it a horizontal line can enter the shape, cross
/// into a bite and come out into the shape again — two intervals on that
/// line, with a gap that belongs to nobody. Claiming the gap sends clicks to
/// a surface that did not draw there; claiming only the wider half gives up
/// 5.5% of a shape at 45 degrees, which is well past the budget the whole
/// tessellation has. So the answer is a list.
pub(crate) type Spans = SmallVec<[(f32, f32); 2]>;

/// `spans` with `(lo, hi)` removed from it.
///
/// Each interval survives whole, is trimmed at one end, is split in two, or
/// disappears. The input is ascending and disjoint, and so is the output.
pub(crate) fn subtract(spans: &Spans, (lo, hi): (f32, f32)) -> Spans {
    let mut out = Spans::new();
    for &(a, b) in spans {
        if hi <= a || lo >= b {
            out.push((a, b));
            continue;
        }
        if a < lo {
            out.push((a, lo));
        }
        if hi < b {
            out.push((hi, b));
        }
    }
    out
}

impl Outline {
    /// The outer bounds of the shape at surface scanline `y`, before any bite
    /// is taken out of them.
    ///
    /// **The boundary, piece by piece.** The straight edges and the corner arcs
    /// bound a convex region, and where a horizontal line meets them is where
    /// that region begins and ends on the line. Every piece is already
    /// transformed, so nothing here cares whether the shape is turned — a
    /// rotation is not a case, it is what the matrix happened to hold. An
    /// axis-aligned shape falls out of the same arithmetic, which is why there
    /// is no separate corner inset: that was this, solved in advance for the
    /// one transform that keeps the axes.
    ///
    /// For every curvature but the scoop this *is* the shape's extent, and
    /// [`Outline::span_at`] just hands it back. A scoop's outline is the plain
    /// box, so this is the box, and the discs it takes out of it are
    /// [`Outline::without_its_bites`].
    pub(crate) fn bounds_at(&self, y: f32) -> Option<(f32, f32)> {
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

    /// Where the shape is on scanline `y`.
    ///
    /// The band loop does not go through this — it asks `bounds_at` once per
    /// boundary and carries the answer to the band below, then takes the
    /// bites out over the whole band. This is the same thing for a caller
    /// asking about one line, which is what a test does.
    #[cfg(test)]
    pub(crate) fn span_at(&self, y: f32) -> Spans {
        let mut spans = Spans::new();
        let Some(bounds) = self.bounds_at(y) else {
            return spans;
        };
        spans.push(bounds);
        self.take_bites_from(&mut spans, y, y);
        spans
    }

    /// Whether anything comes out of this outline's spans at all.
    ///
    /// False for every curvature but the scoop, and the band loop asks it once
    /// per region so that the shapes with nothing to subtract do not pay for
    /// the shape of the answer — see `placed_shape_to_rects`.
    pub(crate) fn is_bitten(&self) -> bool {
        !self.bites.is_empty()
    }

    /// Take this shape's scoops out of `spans`. Does nothing at all for every
    /// curvature but a scoop, which is the point: the band loop calls it once
    /// per band per shape, and a shape with no bites must not pay for the
    /// possibility of having some.
    ///
    /// A band is claimed only where the shape covers it at *every* height in
    /// it, so what comes out is the union of the disc across the band — see
    /// [`Arc::removed_between`]. Reading it off the two edges is what a convex
    /// boundary allows and a bite does not: the widest part of a bite can sit
    /// strictly between two scanlines.
    ///
    /// `y0 == y1` asks about a single scanline, which is what `span_at` does.
    pub(crate) fn take_bites_from(&self, spans: &mut Spans, y0: f32, y1: f32) {
        for bite in &self.bites {
            if let Some(cut) = bite.removed_between(y0, y1) {
                *spans = subtract(spans, cut);
            }
        }
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
    /// Points along the superellipse between the two sides, exclusive of both.
    ///
    /// Parameterised so the samples are spread evenly around the corner rather
    /// than evenly in x: `(cos t)^(2/n)` and `(sin t)^(2/n)` trace
    /// `|x|^n + |y|^n = r^n` for `t` in the first quadrant, which is where the
    /// curve bends most and where chords of it fall furthest inside.
    fn superellipse(&self, curvature: f32, samples: usize) -> impl Iterator<Item = (f32, f32)> {
        let n = 2f32.powf(curvature);
        let (cx, cy) = self.centre;
        let (qx, qy) = self.quadrant;
        let r = self.radius;
        (1..=samples).map(move |i| {
            let t = std::f32::consts::FRAC_PI_2 * i as f32 / (samples + 1) as f32;
            (
                cx + qx * r * t.cos().powf(2.0 / n),
                cy + qy * r * t.sin().powf(2.0 / n),
            )
        })
    }

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
    /// The parameter where the *placed* circle reaches furthest in x, as
    /// `(cos t, sin t)`. Surface x is `a·x + b·y`, so it turns where
    /// `-a·sin t + b·cos t` is zero — a property of the placement, not of any
    /// scanline, and [`Arc::removed_between`] asks for it once per band.
    widest_x: (f32, f32),
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
        let t = placement.b().atan2(placement.a());
        let widest_x = (t.cos(), t.sin());
        if placement.keeps_axes() {
            return Self {
                corner: *corner,
                centre_y,
                solve: Solve::Upright {
                    half_height: placement.d() * corner.radius,
                },
                placement: *placement,
                widest_x,
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
            widest_x,
        }
    }

    /// The two points of the circle at height `target` above its centre, in
    /// the circle's own frame, or `None` where the scanline misses it.
    ///
    /// Both solves answer the same question — the height of `centre + A·r·(cos
    /// t, sin t)` above the centre is `target` — and differ only in what the
    /// placement lets them assume about it.
    fn at_height(&self, target: f32) -> Option<[(f32, f32); 2]> {
        match self.solve {
            Solve::Upright { half_height } => {
                if half_height.abs() < f32::EPSILON || target.abs() > half_height.abs() {
                    return None;
                }
                let st = target / half_height;
                let ct = (1.0 - st * st).max(0.0).sqrt();
                Some([(ct, st), (-ct, st)])
            }
            Solve::Turned { amplitude, phase } => {
                if amplitude < f32::EPSILON || target.abs() > amplitude {
                    return None;
                }
                let base = (target / amplitude).clamp(-1.0, 1.0).acos();
                let (a, b) = (phase + base, phase - base);
                Some([(a.cos(), a.sin()), (b.cos(), b.sin())])
            }
        }
    }

    /// Where a point of the circle lands on the surface.
    fn point_of(&self, ct: f32, st: f32) -> (f32, f32) {
        let (cx, cy) = self.corner.centre;
        let r = self.corner.radius;
        self.placement.transform_point(cx + r * ct, cy + r * st)
    }

    /// Everything the circle takes out of the band `y0..=y1`, as one interval.
    ///
    /// Used where the circle is a **bite**: the disc a scoop takes out of its
    /// corner. No quadrant filter — the disc removes every point of the shape
    /// inside it, not just the part of it the boundary runs along.
    ///
    /// **The union across the band, not the chord at any one height.** A band
    /// is claimed only where the shape covers it at *every* height in it, so a
    /// column has to come out if the disc reaches it anywhere in the band.
    /// Asking at the widest single height is enough only while the placed disc
    /// is still a disc: a rotation maps one to another, so the chord nearest
    /// its centre contains every other. A rotation composed with a
    /// non-uniform scale does not — `container().rotate(20.0).scale(Scale::new(2.0,
    /// 1.0))` places a *tilted ellipse*, whose horizontal chord slides
    /// sideways as it grows, and a column the widest chord misses is one the
    /// disc still covers half a band away. That claimed pixels nothing drew,
    /// which is the promise here that is zero and not a tolerance.
    ///
    /// The band cuts the ellipse into a convex region, so its horizontal
    /// extent is reached on the boundary: either at an end of one of the two
    /// edge chords, or where the ellipse itself reaches furthest in x. Six
    /// candidates, and the extremes are among them. A band holding the whole
    /// disc has no edge chord at all and is bounded by the second pair alone.
    fn removed_between(&self, y0: f32, y1: f32) -> Option<(f32, f32)> {
        let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
        let (mut left, mut right) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut take = |x: f32| {
            left = left.min(x);
            right = right.max(x);
        };

        for edge in [lo, hi] {
            if let Some(pair) = self.at_height(edge - self.centre_y) {
                for (ct, st) in pair {
                    take(self.point_of(ct, st).0);
                }
            }
        }

        // And where the placed circle reaches furthest in x, from either side.
        let (ct, st) = self.widest_x;
        for (ct, st) in [(ct, st), (-ct, -st)] {
            let (x, y) = self.point_of(ct, st);
            if (lo..=hi).contains(&y) {
                take(x);
            }
        }

        (left <= right).then_some((left, right))
    }

    /// Where this arc crosses scanline `y`, if it does — at most twice.
    fn hits(&self, y: f32, hit: &mut impl FnMut(f32)) {
        let target = y - self.centre_y;
        let Some(pair) = self.at_height(target) else {
            return;
        };

        for (ct, st) in pair {
            // Only this corner's own quadrant of the circle. Asked in the
            // circle's own frame, which is where the quadrant is defined — the
            // transform is applied after.
            if ct * self.corner.quadrant.0 < -1e-6 || st * self.corner.quadrant.1 < -1e-6 {
                continue;
            }
            hit(self.point_of(ct, st).0);
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
