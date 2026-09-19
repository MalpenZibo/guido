//! The rectangles a `wl_region` is made of, and how a rounded shape becomes
//! them.
//!
//! Two regions travel from a frame to the compositor: the backdrop blur's, and
//! the surface's input region. Both are `wl_region`s, both are read off the
//! flattened frame, and both need the same thing from a rounded container —
//! the axis-aligned rectangles that stand in for a shape the protocol has no
//! way to describe. That translation lives here so there is one of it.

use crate::renderer::{CornerRadii, DrawCommand, FlattenedCommand, PlacedClip};
use crate::transform::Transform;
use crate::widgets::Rect;

/// An axis-aligned rectangle of a region, in logical surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegionRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl RegionRect {
    /// Whether this rectangle holds `(x, y)`, half-open on the far edges as
    /// [`Rect::contains`] is: the pixel a rectangle ends at belongs to the next
    /// one.
    ///
    /// Nothing the library does asks this — it is how a test reads back the
    /// region the compositor was handed, here and in `Headless::input_reaches`.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    /// A whole-pixel rectangle covering all of `r`.
    ///
    /// Outward, because a region is a claim about where something is: a
    /// fractional bound rounded inward leaves a dead line along an edge
    /// somebody drew. It is the rule the surface's own declared rectangles
    /// have always taken to the compositor.
    pub(crate) fn covering(r: Rect) -> Option<Self> {
        let x = r.x.floor() as i32;
        let y = r.y.floor() as i32;
        let width = (r.x + r.width).ceil() as i32 - x;
        let height = (r.y + r.height).ceil() as i32 - y;
        (width > 0 && height > 0).then_some(Self {
            x,
            y,
            width,
            height,
        })
    }
}

/// A rounded rect as its widget declared it, and the transform that places it
/// on the surface.
///
/// The shape, not the box around it. `wl_region` is a union of rectangles and
/// a turned shape has to become one somehow — but *which* rectangles is the
/// whole question, and the answer used to be "the one box that contains it".
/// A 150×90 card turned 20° has a 172×136 box, 73% more area, and the
/// compositor blurred the desktop in the four triangles where the card is not.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedShape {
    /// The rect in the widget's own coordinates.
    pub rect: Rect,
    /// Its corner radii, in that same space — circles there, whatever the
    /// transform makes of them out here.
    pub radii: CornerRadii,
    /// That space to logical surface pixels.
    pub transform: Transform,
}

impl PlacedShape {
    /// The corners, clamped so no two sharing an edge overlap.
    fn clamped_radii(&self) -> CornerRadii {
        clamp_radii(self.radii, self.rect.width, self.rect.height)
    }

    /// Centre and radius of each corner's circle, in local coordinates, with
    /// the quadrant it occupies as a direction pair.
    fn corners(&self) -> [(f32, f32, f32, f32, f32); 4] {
        let r = self.clamped_radii();
        let (x0, y0) = (self.rect.x, self.rect.y);
        let (x1, y1) = (x0 + self.rect.width, y0 + self.rect.height);
        [
            // centre x, centre y, radius, x direction, y direction
            (x0 + r.top_left, y0 + r.top_left, r.top_left, -1.0, -1.0),
            (x1 - r.top_right, y0 + r.top_right, r.top_right, 1.0, -1.0),
            (
                x1 - r.bottom_right,
                y1 - r.bottom_right,
                r.bottom_right,
                1.0,
                1.0,
            ),
            (
                x0 + r.bottom_left,
                y1 - r.bottom_left,
                r.bottom_left,
                -1.0,
                1.0,
            ),
        ]
    }

    /// The straight part of each side, in local coordinates: the four edges
    /// with their corners bitten out.
    fn edges(&self) -> [(f32, f32, f32, f32); 4] {
        let r = self.clamped_radii();
        let (x0, y0) = (self.rect.x, self.rect.y);
        let (x1, y1) = (x0 + self.rect.width, y0 + self.rect.height);
        [
            (x0 + r.top_left, y0, x1 - r.top_right, y0),
            (x1, y0 + r.top_right, x1, y1 - r.bottom_right),
            (x1 - r.bottom_left, y1, x0 + r.bottom_left, y1),
            (x0, y1 - r.bottom_left, x0, y0 + r.top_left),
        ]
    }

    /// Where the shape starts and stops, vertically, on the surface.
    fn y_bounds(&self) -> (f32, f32) {
        let b = self.transform.map_rect(self.rect);
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
    fn span_at(&self, y: f32) -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut hit = |x: f32| {
            lo = lo.min(x);
            hi = hi.max(x);
        };

        for (ax, ay, bx, by) in self.edges() {
            let (ax, ay) = self.transform.transform_point(ax, ay);
            let (bx, by) = self.transform.transform_point(bx, by);
            // Horizontal after transforming: it either lies on this scanline
            // or misses it, and if it lies on it both ends are hits.
            if (by - ay).abs() < f32::EPSILON {
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

        for (cx, cy, r, dir_x, dir_y) in self.corners() {
            if r <= 0.0 {
                // A square corner is the meeting of two edges, both already
                // asked above.
                continue;
            }
            let (_, tcy) = self.transform.transform_point(cx, cy);
            // The arc is `centre + A·r·(cos t, sin t)`. Its height above the
            // scanline is `P cos t + Q sin t = R`, which is one cosine with a
            // phase — so there are two answers, or none.
            let (p, q) = (self.transform.b() * r, self.transform.d() * r);
            let r_target = y - tcy;
            let amplitude = (p * p + q * q).sqrt();
            if amplitude < f32::EPSILON || r_target.abs() > amplitude {
                continue;
            }
            let phase = q.atan2(p);
            let base = (r_target / amplitude).clamp(-1.0, 1.0).acos();
            for theta in [phase + base, phase - base] {
                let (ct, st) = (theta.cos(), theta.sin());
                // Only this corner's own quadrant of the circle.
                if ct * dir_x < -1e-6 || st * dir_y < -1e-6 {
                    continue;
                }
                let (lx, ly) = (cx + r * ct, cy + r * st);
                let (wx, _) = self.transform.transform_point(lx, ly);
                hit(wx);
            }
        }

        (lo <= hi).then_some((lo, hi))
    }
}

/// Approximate a placed rounded rectangle as a union of axis-aligned rects that
/// **inscribe** the shape, optionally narrowed to `clip`.
///
/// Half-pixel bands, because that is fine enough that the straight parts round
/// to the same integers and merge back into one rectangle, while a curve moves
/// by about a pixel a band and does not. The merging is what keeps an upright
/// panel at a handful of rectangles instead of one per band.
///
/// Every corner separately, and every corner its own size: a shape cut square
/// on one side by a clip must not be tessellated as though that side were
/// round, which loses a wedge the size of the radius along the cut.
///
/// The coordinates are logical surface pixels, which is what `wl_region`
/// expects.
pub(crate) fn placed_shape_to_rects(shape: PlacedShape, clip: Option<Rect>) -> Vec<RegionRect> {
    if shape.rect.width <= 0.0 || shape.rect.height <= 0.0 {
        return Vec::new();
    }

    let (mut top, mut bottom) = shape.y_bounds();
    if let Some(clip) = clip {
        top = top.max(clip.y);
        bottom = bottom.min(clip.y + clip.height);
    }
    let height = bottom - top;
    if height <= 0.0 {
        return Vec::new();
    }

    // A square upright shape is its own rectangle, and saying so keeps the
    // commonest region in the tree a single rect rather than a stack that
    // rounds back to one.
    if shape.clamped_radii().max() <= 0.5 && shape.transform.keeps_axes() {
        let b = shape.transform.map_rect(shape.rect);
        let b = match clip {
            Some(clip) => intersect(b, clip),
            None => Some(b),
        };
        return b.and_then(RegionRect::covering).into_iter().collect();
    }

    let bands = ((height * 2.0).ceil() as usize).clamp(4, 512);
    let band_h = height / bands as f32;

    let mut out: Vec<RegionRect> = Vec::new();
    for i in 0..bands {
        let y0 = top + i as f32 * band_h;
        let y1 = y0 + band_h;
        // The tighter of the band's two edges keeps the slab inside the curve:
        // the shape is convex, so between them it is no narrower than either.
        let (Some((a0, b0)), Some((a1, b1))) = (shape.span_at(y0), shape.span_at(y1)) else {
            continue;
        };
        let (mut left, mut right) = (a0.max(a1), b0.min(b1));
        if let Some(clip) = clip {
            left = left.max(clip.x);
            right = right.min(clip.x + clip.width);
        }
        if right <= left {
            continue;
        }
        let Some(rect) = to_region_rect(Rect::new(left, y0, right - left, band_h)) else {
            continue;
        };
        // Bands that rounded to the same span are one rectangle — which is the
        // whole straight middle of an upright shape, and nothing at all of a
        // turned one.
        match out.last_mut() {
            Some(prev)
                if prev.x == rect.x
                    && prev.width == rect.width
                    && prev.y + prev.height == rect.y =>
            {
                prev.height += rect.height;
            }
            _ => out.push(rect),
        }
    }
    out
}

/// The overlap of two rects, or `None` where there is none.
fn intersect(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
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

/// Round-to-nearest on all four edges so adjacent slabs tile without gaps
/// (inward rounding would drop sub-pixel slabs). `None` when the rounding
/// collapses the rectangle to nothing.
fn to_region_rect(b: Rect) -> Option<RegionRect> {
    let x0 = b.x.round() as i32;
    let y0 = b.y.round() as i32;
    let x1 = (b.x + b.width).round() as i32;
    let y1 = (b.y + b.height).round() as i32;
    (x1 > x0 && y1 > y0).then_some(RegionRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    })
}

/// One declaration of where input reaches a surface, in the order it was
/// painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputRegionOp {
    /// Whether input reaches this rectangle. `false` cuts it out of whatever
    /// came before.
    pub takes: bool,
    pub rect: RegionRect,
}

/// What a surface asks the compositor about where input reaches it: the area
/// it takes before any container has spoken, and the declarations that came
/// out of the frame.
///
/// Two fields rather than one resolved list, because a rounded hole cut out of
/// a surface is not a union of rectangles anyone should have to compute:
/// `wl_region` subtracts, and this is that program in the order it is applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct InputRegionRequest {
    pub base: Vec<RegionRect>,
    pub ops: Vec<InputRegionOp>,
}

/// The input declarations of the frame that was just drawn, in paint order.
///
/// Read off the flattened commands for the reason
/// [`crate::blur::regions_from_commands`] gives at length: a container that did
/// not paint has no command here, so a hidden, culled or replaced declaration
/// takes itself back without anyone having to remember it.
///
/// Not sorted, unlike a blur region: these compose, and the order they compose
/// in is the order they were painted in.
pub(crate) fn input_ops_from_commands(commands: &[FlattenedCommand]) -> Vec<InputRegionOp> {
    let mut out = Vec::new();
    for cmd in commands {
        let DrawCommand::InputRegion {
            rect,
            corner_radii,
            takes,
        } = &*cmd.command
        else {
            continue;
        };
        out.extend(
            shape_of(cmd, *rect, *corner_radii)
                .into_iter()
                .map(|rect| InputRegionOp {
                    takes: *takes,
                    rect,
                }),
        );
    }
    out
}

/// The rectangles one command's rounded rect becomes, cut to what the command
/// is allowed to show.
///
/// **The shape, turned.** A command carries the transform it is drawn through,
/// so the region is tessellated from the placed shape rather than from the box
/// around it — a rotated container takes clicks where it is, and blurs the
/// desktop where it is, rather than over the square it happens to sit inside.
///
/// The *clip* is still a box. Narrowing a turned shape by a turned clip is the
/// case one rect and one matrix cannot describe — the same wall
/// `intersect_clips` meets — so the clip arrives here as its world bounds and
/// is over-generous exactly where it is itself turned.
pub(crate) fn shape_of(
    cmd: &FlattenedCommand,
    rect: Rect,
    corner_radii: CornerRadii,
) -> Vec<RegionRect> {
    let clip = cmd.clip.as_ref().map(PlacedClip::world_aabb);
    placed_shape_to_rects(
        PlacedShape {
            rect,
            radii: corner_radii,
            transform: cmd.world_transform,
        },
        clip,
    )
}

/// The area a surface takes input in before any container has spoken: the
/// whole of it, or whatever the surface itself declared, at creation or since.
///
/// A surface that says nothing takes input everywhere, which is what the
/// protocol's null region means. `click_through` is the same declaration with
/// nothing in it, and a declaration cuts the base down to the rectangles it
/// names.
pub(crate) fn base_rects(declared: Option<&[Rect]>, width: f32, height: f32) -> Vec<RegionRect> {
    match declared {
        None => Vec::from_iter(RegionRect::covering(Rect::new(0.0, 0.0, width, height))),
        Some(rects) => rects
            .iter()
            .copied()
            .filter_map(RegionRect::covering)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(rects: &[RegionRect], x: i32, y: i32) -> bool {
        rects.iter().any(|r| r.contains(x, y))
    }

    /// The commonest shape there is: no transform at all.
    fn upright(rect: Rect, radii: CornerRadii) -> Vec<RegionRect> {
        placed_shape_to_rects(
            PlacedShape {
                rect,
                radii,
                transform: Transform::IDENTITY,
            },
            None,
        )
    }

    fn round(radius: f32) -> CornerRadii {
        CornerRadii::uniform(radius)
    }

    #[test]
    fn zero_radius_is_bounding_box() {
        let rects = upright(Rect::new(10.0, 20.0, 100.0, 50.0), round(0.0));
        assert_eq!(
            rects,
            vec![RegionRect {
                x: 10,
                y: 20,
                width: 100,
                height: 50
            }]
        );
    }

    #[test]
    fn degenerate_rect_is_empty() {
        assert!(upright(Rect::new(0.0, 0.0, 0.0, 40.0), round(8.0)).is_empty());
        assert!(upright(Rect::new(0.0, 0.0, 40.0, -1.0), round(8.0)).is_empty());
    }

    #[test]
    fn rounded_corners_are_cut() {
        let rects = upright(Rect::new(0.0, 0.0, 100.0, 60.0), round(20.0));
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
        let rects = upright(Rect::new(0.0, 0.0, 80.0, 80.0), round(r));
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
        let rects = upright(Rect::new(0.0, 0.0, 40.0, 40.0), round(100.0));
        assert!(!rects.is_empty());
        assert!(covers(&rects, 20, 20), "the middle is covered");
        assert!(!covers(&rects, 0, 0), "the corner is not");
    }

    /// A square corner is square. This is the seam the region used to be lost
    /// at: a clip squared off two corners, the caller collapsed the four radii
    /// with `.max()`, and the shape arrived here as though all four were round —
    /// so the region gave back a wedge the size of the radius along the cut,
    /// which is the artefact the clipping exists to prevent.
    #[test]
    fn a_square_corner_is_not_rounded_because_another_one_is() {
        let rects = upright(
            Rect::new(0.0, 0.0, 40.0, 100.0),
            CornerRadii {
                top_left: 16.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 16.0,
            },
        );

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
        let rects = upright(Rect::new(0.0, 0.0, 100.0, 60.0), CornerRadii::top(20.0));

        assert!(!covers(&rects, 0, 0) && !covers(&rects, 99, 0), "top cut");
        assert!(
            covers(&rects, 0, 59) && covers(&rects, 99, 59),
            "bottom square"
        );
    }

    /// An unevenly scaled corner is an ellipse: a circle of 16 under a 2×1
    /// scale is 32 wide and 16 tall, and reaches the left edge 16 down where a
    /// circle of either radius would not.
    ///
    /// The ellipse now comes from the transform rather than from a radius pair
    /// carried alongside it, which is the only place it ever came from: the
    /// renderer scaled all four corners by one `(sx, sy)`, so four corners with
    /// four different axis ratios was a shape nothing could draw.
    #[test]
    fn an_elliptical_corner_is_cut_on_both_of_its_axes() {
        let rects = placed_shape_to_rects(
            PlacedShape {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                radii: round(16.0),
                transform: Transform::scale_xy(2.0, 1.0),
            },
            None,
        );

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
    /// that differ per corner under a scale that differs per axis — the
    /// property the whole tessellation exists to have, checked where the shape
    /// is least uniform.
    #[test]
    fn slabs_stay_inside_a_shape_with_four_different_corners() {
        let radii = CornerRadii {
            top_left: 30.0,
            top_right: 10.0,
            bottom_right: 0.0,
            bottom_left: 20.0,
        };
        let (sx, sy) = (1.2f32, 2.0f32);
        let (w, h) = (100.0f32, 50.0f32);
        let rects = placed_shape_to_rects(
            PlacedShape {
                rect: Rect::new(0.0, 0.0, w, h),
                radii,
                transform: Transform::scale_xy(sx, sy),
            },
            None,
        );
        assert!(!rects.is_empty());

        // Each corner's ellipse centre and semi-axes, in surface pixels.
        let (ww, hh) = (w * sx, h * sy);
        let corners = [
            (
                radii.top_left * sx,
                radii.top_left * sy,
                radii.top_left * sx,
                radii.top_left * sy,
                true,
                true,
            ),
            (
                ww - radii.top_right * sx,
                radii.top_right * sy,
                radii.top_right * sx,
                radii.top_right * sy,
                false,
                true,
            ),
            (
                radii.bottom_left * sx,
                hh - radii.bottom_left * sy,
                radii.bottom_left * sx,
                radii.bottom_left * sy,
                true,
                false,
            ),
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

    // -- turned shapes --------------------------------------------------

    /// A turned rectangle is tessellated as the turned rectangle, and not as
    /// the box around it.
    ///
    /// A 100×100 square turned 45° has a 141×141 bounding box whose corners are
    /// nowhere near the shape — 29 pixels of empty diagonal at each one. The
    /// compositor blurred the desktop in all four, which is what #198 is about.
    #[test]
    fn a_turned_square_does_not_claim_its_corners() {
        let rects = placed_shape_to_rects(
            PlacedShape {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                radii: round(0.0),
                transform: Transform::rotate_degrees(45.0).center_at(50.0, 50.0),
            },
            None,
        );
        assert!(!rects.is_empty());

        // The diamond's own points, which the bounding box shares.
        assert!(covers(&rects, 50, 20), "the top point is in the shape");
        assert!(covers(&rects, 50, 50), "and so is the middle");

        // The bounding box's corners, which are 29 pixels of nothing.
        for (x, y) in [(-15, -15), (115, -15), (-15, 115), (115, 115)] {
            assert!(
                !covers(&rects, x, y),
                "({x}, {y}) is a corner of the box, not of the shape"
            );
        }
    }

    /// Every rectangle of a turned shape is inside it — the same promise the
    /// upright tessellation makes, asked of the case that used to have no
    /// answer at all.
    #[test]
    fn slabs_stay_inside_a_turned_shape() {
        let (w, h) = (120.0f32, 60.0f32);
        let transform = Transform::rotate_degrees(25.0).center_at(w / 2.0, h / 2.0);
        let rects = placed_shape_to_rects(
            PlacedShape {
                rect: Rect::new(0.0, 0.0, w, h),
                radii: round(0.0),
                transform,
            },
            None,
        );
        assert!(!rects.is_empty());

        let back = transform.inverse().expect("a rotation is invertible");
        for rect in &rects {
            for (px, py) in [
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x, rect.y + rect.height),
                (rect.x + rect.width, rect.y + rect.height),
            ] {
                let (lx, ly) = back.transform_point(px as f32, py as f32);
                assert!(
                    lx >= -1.0 && ly >= -1.0 && lx <= w + 1.0 && ly <= h + 1.0,
                    "({px}, {py}) maps back to ({lx}, {ly}), outside the shape"
                );
            }
        }
    }

    /// A turned shape covers far less than its box, and the number is the
    /// point: this is the area the compositor was blurring where nothing was
    /// drawn.
    #[test]
    fn a_turned_shape_claims_about_its_own_area() {
        let (w, h) = (150.0f32, 90.0f32);
        let rects = placed_shape_to_rects(
            PlacedShape {
                rect: Rect::new(0.0, 0.0, w, h),
                radii: round(0.0),
                transform: Transform::rotate_degrees(20.0).center_at(w / 2.0, h / 2.0),
            },
            None,
        );
        let covered: i64 = rects.iter().map(|r| r.width as i64 * r.height as i64).sum();
        let shape = (w * h) as i64;

        assert!(
            covered <= shape,
            "the rectangles inscribe the shape, so they cannot exceed its \
             {shape} px — got {covered}"
        );
        assert!(
            covered > shape * 9 / 10,
            "and they should not give away much of it either — got {covered} \
             of {shape}"
        );
        // The issue's own arithmetic: the box is 73% larger than the shape.
        let boxed = {
            let b = Transform::rotate_degrees(20.0)
                .center_at(w / 2.0, h / 2.0)
                .map_rect(Rect::new(0.0, 0.0, w, h));
            (b.width * b.height) as i64
        };
        assert!(
            boxed > shape * 17 / 10,
            "the box this replaces is {boxed} px against the shape's {shape}"
        );
    }

    /// A turned shape keeps its rounded corners too — the arcs are transformed
    /// with everything else rather than approximated away.
    #[test]
    fn a_turned_shape_keeps_its_corners() {
        let square = |radius: f32| {
            placed_shape_to_rects(
                PlacedShape {
                    rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                    radii: round(radius),
                    transform: Transform::rotate_degrees(45.0).center_at(50.0, 50.0),
                },
                None,
            )
        };
        let area = |rects: Vec<RegionRect>| -> i64 {
            rects.iter().map(|r| r.width as i64 * r.height as i64).sum()
        };

        assert!(
            area(square(30.0)) < area(square(0.0)),
            "rounding the corners of a turned shape takes area off it, which it \
             cannot do if the corners were lost in a bounding box"
        );
    }

    /// The clip still narrows the shape, and still arrives as a box.
    #[test]
    fn a_clip_narrows_a_turned_shape() {
        let shape = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: round(0.0),
            transform: Transform::rotate_degrees(45.0).center_at(50.0, 50.0),
        };
        let clipped = placed_shape_to_rects(shape, Some(Rect::new(50.0, -50.0, 100.0, 200.0)));

        assert!(
            clipped.iter().all(|r| r.x >= 50),
            "nothing reaches left of the clip"
        );
        assert!(
            covers(&clipped, 60, 50),
            "and what is on show is still there"
        );
        assert!(
            placed_shape_to_rects(shape, Some(Rect::new(500.0, 0.0, 10.0, 10.0))).is_empty(),
            "a clip that misses leaves nothing to publish"
        );
    }
}
