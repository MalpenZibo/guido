//! The rectangles a `wl_region` is made of, and how a rounded shape becomes
//! them.
//!
//! Two regions travel from a frame to the compositor: the backdrop blur's, and
//! the surface's input region. Both are `wl_region`s, both are read off the
//! flattened frame, and both need the same thing from a rounded container —
//! the axis-aligned rectangles that stand in for a shape the protocol has no
//! way to describe. That translation lives here so there is one of it.

use crate::renderer::{CornerRadii, DrawCommand, FlattenedCommand};
use crate::shape::PlacedShape;

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

/// Approximate a placed rounded rectangle as a union of axis-aligned rects that
/// **inscribe** the shape, narrowed to `clip`.
///
/// A band per pixel, because that is what the output can tell apart: adjacent
/// bands that round to the same span merge back into one rectangle, so the
/// straight parts collapse and a curve — moving about a pixel a band — does
/// not. The merging is what keeps an upright panel at a handful of rectangles
/// instead of one per band.
///
/// **The clip is a shape too.** Both are convex, so at any scanline the
/// overlap is the overlap of the two spans — which is exact, turned clip or
/// not, and gets the corners right for free. A card filling a rounded scroller
/// is cornered by the scroller: publishing the scroller's square box instead
/// blurs the desktop in the four corners where the panel is not drawn, and
/// squaring off the card's own corners along a cut edge is the same mistake
/// the other way.
///
/// The coordinates are logical surface pixels, which is what `wl_region`
/// expects.
pub(crate) fn placed_shape_to_rects(
    shape: PlacedShape,
    clip: Option<PlacedShape>,
) -> Vec<RegionRect> {
    if shape.rect.width <= 0.0 || shape.rect.height <= 0.0 {
        return Vec::new();
    }

    let (mut top, mut bottom) = shape.y_bounds();
    if let Some(clip) = clip {
        let (clip_top, clip_bottom) = clip.y_bounds();
        top = top.max(clip_top);
        bottom = bottom.min(clip_bottom);
    }
    let height = bottom - top;
    if height <= 0.0 {
        return Vec::new();
    }

    // A square upright shape is its own rectangle, and a square upright clip
    // cuts it to another one.
    //
    // The band loop below would arrive at the same answer — it merges bands
    // that round alike, and for a square shape that is all of them — but it
    // would take about six microseconds to do it, against twenty nanoseconds
    // here. That is per region, per frame, and a bar of thirty pills declaring
    // input regions pays it thirty times. Worth a branch.
    if shape.is_a_box() && clip.is_none_or(|c| c.is_a_box()) {
        let boxed = shape.world_aabb();
        let boxed = match clip {
            Some(clip) => intersection(boxed, clip.world_aabb()),
            None => Some(boxed),
        };
        return boxed.and_then(to_region_rect).into_iter().collect();
    }

    let outline = shape.outline();
    let clip_outline = clip.map(|clip| clip.outline());

    // At a scanline: where the shape is, narrowed to where the clip is.
    let span = |y: f32| -> Option<(f32, f32)> {
        let (mut left, mut right) = outline.span_at(y)?;
        if let Some(ref clip) = clip_outline {
            let (clip_left, clip_right) = clip.span_at(y)?;
            left = left.max(clip_left);
            right = right.min(clip_right);
        }
        (left < right).then_some((left, right))
    };

    // One band per pixel: `to_region_rect` rounds each band's edges to whole
    // numbers, so a band finer than a pixel cannot produce a row the output can
    // tell apart — it only tightens the inscribed span by a fraction. Half-pixel
    // bands cost twice the spans for that fraction, which on a small shape is
    // the whole of the work.
    let bands = (height.ceil() as usize).clamp(4, 512);
    let band_h = height / bands as f32;

    let mut out: Vec<RegionRect> = Vec::new();
    // Every interior boundary is shared by the band above and the band below,
    // so each span is computed once and carried forward.
    let mut upper = span(top);
    for i in 0..bands {
        let y0 = top + i as f32 * band_h;
        let lower = span(y0 + band_h);
        let (Some((a0, b0)), Some((a1, b1))) = (upper, lower) else {
            upper = lower;
            continue;
        };
        upper = lower;
        // The tighter of the band's two edges keeps the slab inside the curve:
        // both shapes are convex, so between them neither is narrower.
        let (left, right) = (a0.max(a1), b0.min(b1));
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
fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
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
            curvature,
            takes,
        } = &*cmd.command
        else {
            continue;
        };
        out.extend(
            shape_of(cmd, *rect, *corner_radii, *curvature)
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
/// The clip is a shape here too, and intersected scanline by scanline rather
/// than as a box — so a card inside a rounded scroller is cornered by the
/// scroller, and a turned clip cuts what it actually covers.
///
/// One clip, though. `cmd.clip` is `effective_clip`, which `intersect_clips`
/// has already collapsed. Since #397 that collapse is exact whenever the two
/// spaces are a quarter turn or a mirror apart as well as a scale; what still
/// arrives as the box around both is a pair turned by some other angle. What is
/// escaped here is the *shape against clip* box intersection, not the chain's
/// own.
pub(crate) fn shape_of(
    cmd: &FlattenedCommand,
    rect: Rect,
    corner_radii: CornerRadii,
    curvature: f32,
) -> Vec<RegionRect> {
    placed_shape_to_rects(
        PlacedShape::placed(rect, corner_radii, curvature, cmd.world_transform),
        cmd.clip,
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
    use crate::transform::Transform;

    fn covers(rects: &[RegionRect], x: i32, y: i32) -> bool {
        rects.iter().any(|r| r.contains(x, y))
    }

    /// The commonest shape there is: no transform at all.
    fn upright(rect: Rect, radii: CornerRadii) -> Vec<RegionRect> {
        placed_shape_to_rects(shape(rect, radii, Transform::IDENTITY), None)
    }

    fn shape(rect: Rect, radii: CornerRadii, placement: Transform) -> PlacedShape {
        PlacedShape::placed(rect, radii, 1.0, placement)
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
            shape(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                round(16.0),
                Transform::scale_xy(2.0, 1.0),
            ),
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
            PlacedShape::placed(
                Rect::new(0.0, 0.0, w, h),
                radii,
                1.0,
                Transform::scale_xy(sx, sy),
            ),
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

    /// Each side runs between the two corners that are actually on it.
    ///
    /// Asked of `span_at` directly, at the exact scanline of the bottom edge,
    /// because the tessellation cannot see this: a band is evaluated at both of
    /// its ends and takes the narrower, so the bottom edge's own scanline is
    /// always paired with one above it and the narrower one wins. The bottom
    /// edge ran from `x1 - bottom_left` — the wrong corner, and `bottom_right`
    /// appeared nowhere in it — which is invisible while all four radii agree
    /// and produces a side that overshoots its corner when they do not.
    #[test]
    fn a_side_ends_where_its_own_corners_begin() {
        let shape = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii {
                top_left: 0.0,
                top_right: 0.0,
                bottom_right: 30.0,
                bottom_left: 0.0,
            },
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };

        // Within a thousandth: the corner's own end of the side comes back
        // through the arc solve, which is trigonometry and not arithmetic.
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;

        let (left, right) = shape.span_at(100.0).expect("the bottom edge is there");
        assert!(
            close(left, 0.0),
            "square at the bottom left, so the side reaches — got {left}"
        );
        assert!(
            close(right, 70.0),
            "and stops where the 30-radius bottom-right corner starts — got {right}"
        );

        // The opposite corner, so a fix that swaps the two is not a fix.
        let mirrored = PlacedShape {
            radii: CornerRadii {
                bottom_right: 0.0,
                bottom_left: 30.0,
                ..shape.radii
            },
            ..shape
        };
        let (left, right) = mirrored.span_at(100.0).expect("the bottom edge is there");
        assert!(
            close(left, 30.0) && close(right, 100.0),
            "got ({left}, {right})"
        );
    }

    /// The single-rectangle shortcut asks whether a corner is small *on the
    /// surface*, not in the space the widget wrote it in.
    ///
    /// A radius of 0.4 under a `scale(8.0)` is three pixels of curve. Measured
    /// where it was declared it looks like nothing and the whole box is
    /// published, corners and all.
    #[test]
    fn a_corner_is_small_enough_to_ignore_only_once_it_is_on_screen() {
        let scaled = placed_shape_to_rects(
            PlacedShape::placed(
                Rect::new(0.0, 0.0, 20.0, 20.0),
                round(0.4),
                1.0,
                Transform::scale(8.0),
            ),
            None,
        );
        assert!(
            !covers(&scaled, 0, 0),
            "3 pixels of curve is a cut corner, not a rounding error"
        );

        // And the shortcut still fires where it should, or every upright panel
        // pays for a stack of bands.
        let tiny = placed_shape_to_rects(
            PlacedShape::placed(
                Rect::new(0.0, 0.0, 100.0, 50.0),
                round(0.4),
                1.0,
                Transform::IDENTITY,
            ),
            None,
        );
        assert_eq!(tiny.len(), 1, "one rectangle, as it has always been");
    }

    /// A bigger corner is sampled at more points, so the accuracy holds as the
    /// shape grows.
    ///
    /// `samples_per_corner` is the one number the whole argument rests on — the
    /// sag of a chord across an angle is `r·θ²/8`, so the count has to rise
    /// with the square root of the radius *as it lands on the surface*, not as
    /// it was declared. Nothing watched it: replacing the body with a constant
    /// 2 passed the entire suite, including the coverage test above, because a
    /// small corner needs about two points anyway.
    ///
    /// This is the same shape twice, once scaled eight times, and the second
    /// one is where a fixed count runs out.
    #[test]
    fn a_corner_is_sampled_finely_enough_at_the_size_it_is_drawn() {
        use crate::widgets::Corners;

        let coverage = |scale: f32| {
            let placement = Transform::scale(scale);
            let shape = PlacedShape::placed(
                Rect::new(0.0, 0.0, 120.0, 120.0),
                round(40.0),
                2.0,
                placement,
            );
            let rects = placed_shape_to_rects(shape, None);
            let local = Rect::new(0.0, 0.0, 120.0, 120.0);
            let corners = Corners::superellipse(40.0, 2.0);
            let to_local = placement.inverse().expect("invertible");
            let inside = |x: i32, y: i32| {
                let (lx, ly) = to_local.transform_point(x as f32 + 0.5, y as f32 + 0.5);
                local.contains_shape(lx, ly, corners)
            };

            let covered: i64 = rects.iter().map(|r| r.width as i64 * r.height as i64).sum();
            let side = (120.0 * scale) as i32 + 4;
            let mut drawn = 0i64;
            for y in -2..side {
                for x in -2..side {
                    if inside(x, y) {
                        drawn += 1;
                    }
                }
            }
            (covered, drawn)
        };

        // A corner eight times the size gets more points, so it is cut *more*
        // accurately, not less. A fixed count goes the other way and flattens
        // out: measured, 98.85% then 99.75% with the count as written, against
        // 97.71% and 98.16% with it replaced by a constant 2. The bar at scale
        // 8 is what tells the two apart.
        let (covered, drawn) = coverage(1.0);
        assert!(
            covered * 1000 >= drawn * 985,
            "at scale 1 the region covers {covered} of {drawn}"
        );
        let (covered, drawn) = coverage(8.0);
        assert!(
            covered * 1000 >= drawn * 995,
            "at scale 8 the region covers {covered} of {drawn} — a corner eight \
             times the size needs more points, not the same number spread \
             further apart"
        );
    }

    /// What sampling a corner costs the compositor, in rectangles.
    ///
    /// A region is a list `wl_region` receives and the compositor diffs, so the
    /// count is the price of the accuracy above. A circle keeps its closed form
    /// and is unchanged; the sampled curvatures pay for the bands their extra
    /// vertices introduce.
    #[test]
    fn sampling_a_corner_does_not_multiply_the_rectangles() {
        let count = |k: f32| {
            placed_shape_to_rects(
                PlacedShape::placed(
                    Rect::new(0.0, 0.0, 300.0, 120.0),
                    round(40.0),
                    k,
                    Transform::IDENTITY,
                ),
                None,
            )
            .len()
        };
        let circle = count(1.0);
        for k in [0.5_f32, 2.0] {
            assert!(
                count(k) <= circle * 2,
                "k={k} publishes {} rectangles against the circle's {circle}",
                count(k)
            );
        }
    }

    /// The region is inside the shape, and nearly all of it.
    ///
    /// Two promises, and they pull opposite ways. A region that claims a pixel
    /// the shape does not cover sends a click to a surface that did not draw
    /// there — so every published rectangle has to be inside. A region that
    /// gives up pixels the shape does cover drops clicks on painted pixels, and
    /// they fall through to whatever is behind — so it has to give up almost
    /// none.
    ///
    /// Checked against the same signed distance the shader draws with, over
    /// every curvature the public API names and one between them, upright and
    /// turned. #400 was the second promise broken at k > 1: the corner was
    /// published as the circle inscribed in the squircle, 14% of each corner
    /// box short. A scoop still breaks it, by more — that is #410, and this
    /// test holds it to the first promise in the meantime.
    #[test]
    fn a_region_is_inside_its_shape_and_almost_all_of_it() {
        use crate::widgets::Corners;

        let size = 120.0;
        for k in [0.0_f32, 0.5, 1.0, 2.0, -1.0] {
            for degrees in [0.0_f32, 20.0] {
                let placement = Transform::rotate_degrees(degrees);
                let shape =
                    PlacedShape::placed(Rect::new(0.0, 0.0, size, size), round(40.0), k, placement);
                let rects = placed_shape_to_rects(shape, None);
                let local = Rect::new(0.0, 0.0, size, size);
                let corners = Corners::superellipse(40.0, k);
                let to_local = placement.inverse().expect("invertible");

                let inside = |x: i32, y: i32| {
                    let (lx, ly) = to_local.transform_point(x as f32 + 0.5, y as f32 + 0.5);
                    local.contains_shape(lx, ly, corners)
                };

                let mut claimed_outside = 0;
                let mut covered = 0;
                for rect in &rects {
                    for y in rect.y..rect.y + rect.height {
                        for x in rect.x..rect.x + rect.width {
                            covered += 1;
                            if !inside(x, y) {
                                claimed_outside += 1;
                            }
                        }
                    }
                }

                let mut drawn = 0;
                for y in -200..260 {
                    for x in -200..260 {
                        if inside(x, y) {
                            drawn += 1;
                        }
                    }
                }

                // Zero, not a tolerance. This is the promise that sends a
                // click to a surface which did not draw there, and it is zero
                // in every configuration here — a bound set near the observed
                // value is the one that notices it breaking.
                assert_eq!(
                    claimed_outside, 0,
                    "k={k} at {degrees}°: {claimed_outside} of {covered} published \
                     pixels are outside the shape, which has {drawn}"
                );
                // A scoop is the exception, and #410 is why: its corner is
                // cut by a chord tangent to the bite rather than by the bite,
                // which never over-claims and gives up about a sixth of the
                // shape. Held to the first promise only until that is fixed.
                if k >= 0.0 {
                    assert!(
                        covered * 100 >= drawn * 97,
                        "k={k} at {degrees}°: the region covers {covered} of \
                         the shape's {drawn} pixels, and has given up too many"
                    );
                }
            }
        }
    }

    /// Each curvature is cut as itself, and the two that are not sampled are
    /// the two that need no sampling.
    ///
    /// `Corners::bevel` is K = 0 — a straight diagonal across the corner. The
    /// chord between the arc's ends is not an approximation of it, it *is* it,
    /// and cutting a circle there would have a bevelled container blur the
    /// desktop past its own cut and take clicks there. A circle is K = 1 and
    /// keeps the closed-form solve. Everything between and above them is
    /// sampled, which is where #400 lived: a squircle was cut as the circle
    /// inscribed in it.
    #[test]
    fn each_curvature_is_cut_as_the_curve_it_is() {
        let corner = |curvature: f32| {
            placed_shape_to_rects(
                PlacedShape::placed(
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    round(40.0),
                    curvature,
                    Transform::IDENTITY,
                ),
                None,
            )
        };

        // 12 in from the left, 12 down: inside a 40 circle (√2·12 = 17 < 40 of
        // inset), outside the bevel, whose chord runs from (0,40) to (40,0).
        assert!(
            covers(&corner(1.0), 12, 12),
            "the circle reaches this pixel"
        );
        assert!(
            !covers(&corner(0.0), 12, 12),
            "and the bevel does not, because the chord has already cut it"
        );

        // A squircle is larger than the circle drawn inside it, so a point the
        // circle reaches is one the squircle reaches too.
        assert!(covers(&corner(2.0), 12, 12));

        // The other way round is what #400 was. 9 in and 9 down sits inside the
        // squircle — 2·31^4 is 1,847,042 against 40^4 of 2,560,000 — and
        // outside the circle, since 31·√2 is 43.8 and the radius is 40. The
        // region published the circle, so the compositor was told the surface
        // stops 7.6 pixels short of where it is drawn, along each corner
        // diagonal. For the blur that is a corner slightly under-blurred; for
        // the input region it is a wedge where a click falls through to
        // whatever is behind.
        assert!(
            covers(&corner(2.0), 9, 9),
            "a squircle reaches past the circle inscribed in it, and the \
             region has to reach with it"
        );

        let area = |rects: Vec<RegionRect>| -> i64 {
            rects.iter().map(|r| r.width as i64 * r.height as i64).sum()
        };
        assert!(
            area(corner(0.0)) < area(corner(1.0)),
            "a bevel takes more off the corner than a circle does"
        );

        // And the chord runs between the two points where the corner meets its
        // sides — (0, 40) and (40, 0) for a radius of 40 — so `x + y = 40` is
        // the cut. A chord that starts from the wrong end of either side still
        // removes *a* triangle, and every assertion above is satisfied by the
        // wrong one; only the line itself says which.
        let bevel = corner(0.0);
        assert!(
            !covers(&bevel, 18, 18),
            "18 + 18 is short of the cut at 40, so the bevel has taken it"
        );
        assert!(
            covers(&bevel, 25, 25),
            "25 + 25 clears it, so the shape holds this one — a chord hung off \
             the wrong end of either side does not run through x + y = 40 and \
             gets one of these two wrong"
        );
        assert!(
            covers(&bevel, 2, 45) && covers(&bevel, 45, 2),
            "and just past each end of the chord the straight sides take over"
        );
    }

    /// A concave corner is cut outside the bite, not through it.
    ///
    /// `Corners::scoop` is K = -1, and the shader draws it as the square corner
    /// **minus a disc** of the same radius centred on that corner — so the
    /// shape reaches nearly to the corner along each edge and is bitten away
    /// around the diagonal. It is the one corner that is *not* a subset of the
    /// square, and the chord that makes a bevel safe runs straight through the
    /// bite: at `r/√2` from the corner, well inside a disc of `r`.
    ///
    /// So the chord retreats to where it is tangent, `r·√2` along each axis,
    /// and the straight edges retreat with it — an edge that stopped at `r`
    /// would cover the bite on its own.
    ///
    /// Checked against what the shader actually draws rather than against a
    /// radius: in the box, and no nearer any corner than its own radius.
    #[test]
    fn a_scooped_corner_is_cut_outside_the_bite() {
        let (w, h, r) = (120.0f32, 90.0f32, 28.0f32);
        let rects = placed_shape_to_rects(
            PlacedShape::placed(
                Rect::new(0.0, 0.0, w, h),
                round(r),
                -1.0,
                Transform::IDENTITY,
            ),
            None,
        );
        assert!(!rects.is_empty(), "a scooped box is not empty");

        for rect in &rects {
            for (px, py) in [
                (rect.x, rect.y),
                (rect.x + rect.width, rect.y),
                (rect.x, rect.y + rect.height),
                (rect.x + rect.width, rect.y + rect.height),
            ] {
                let (px, py) = (px as f32, py as f32);
                assert!(
                    (-0.5..=w + 0.5).contains(&px) && (-0.5..=h + 0.5).contains(&py),
                    "({px}, {py}) is outside the box"
                );
                for (cx, cy) in [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)] {
                    let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
                    assert!(
                        d >= r - 0.75,
                        "({px}, {py}) is {d} from the scooped corner ({cx}, {cy}), \
                         inside a bite of {r}"
                    );
                }
            }
        }

        // And it still publishes the middle, rather than retreating to nothing.
        assert!(covers(&rects, 60, 45));
    }

    /// Adjacent bands that round to the same span are one rectangle.
    ///
    /// **This is what keeps a region small.** A band is computed per pixel, so
    /// an upright 900-tall panel produces nine hundred of them and all but the
    /// corner ones are identical; merged, it publishes about fifteen
    /// rectangles. Without the merge it publishes nine hundred, every frame,
    /// down a socket — and every test here would still pass, because each of
    /// those rectangles is inside the shape and together they cover it.
    ///
    /// So the count is the assertion. It cannot be exact — it depends on how
    /// the corner curve rounds — but "tens, not hundreds" is the whole property
    /// and a broken merge misses it by two orders of magnitude.
    #[test]
    fn bands_that_round_alike_are_published_as_one_rectangle() {
        let tall = placed_shape_to_rects(
            PlacedShape::placed(
                Rect::new(0.0, 0.0, 200.0, 900.0),
                round(24.0),
                1.0,
                Transform::IDENTITY,
            ),
            None,
        );
        assert!(
            tall.len() < 80,
            "a 900-tall panel is tens of rectangles, not one per band — got {}",
            tall.len()
        );
        assert!(covers(&tall, 100, 450), "and it still covers its middle");
        assert!(!covers(&tall, 1, 1), "with its corners cut");

        // The straight middle is the part that merges: the same panel with no
        // corners at all is a single rectangle by that route.
        let square = placed_shape_to_rects(
            PlacedShape::placed(
                Rect::new(0.0, 0.0, 200.0, 900.0),
                round(24.0),
                1.0,
                // A turn keeps it off the single-rectangle shortcut, so the
                // merge is what is being asked about rather than the fast path.
                Transform::rotate_degrees(90.0).center_at(100.0, 450.0),
            ),
            None,
        );
        assert!(
            square.len() < 80,
            "a quarter turn is still an upright shape — got {}",
            square.len()
        );
    }

    /// Two boxes are cut to their overlap, and boxes that miss leave nothing.
    ///
    /// The single-rectangle shortcut does this intersection itself rather than
    /// through the band loop, so it is the one path where an edge comparison
    /// going the wrong way produces a rectangle nobody checked.
    #[test]
    fn two_boxes_are_cut_to_what_they_share() {
        let boxed = |x: f32, y: f32, w: f32, h: f32| {
            PlacedShape::placed(Rect::new(x, y, w, h), round(0.0), 1.0, Transform::IDENTITY)
        };

        assert_eq!(
            placed_shape_to_rects(
                boxed(0.0, 0.0, 100.0, 100.0),
                Some(boxed(40.0, 30.0, 200.0, 200.0))
            ),
            vec![RegionRect {
                x: 40,
                y: 30,
                width: 60,
                height: 70
            }],
            "the overlap, on both axes at once"
        );

        assert!(
            placed_shape_to_rects(
                boxed(0.0, 0.0, 50.0, 50.0),
                Some(boxed(50.0, 0.0, 50.0, 50.0))
            )
            .is_empty(),
            "boxes that touch along an edge share no area"
        );
        assert!(
            placed_shape_to_rects(
                boxed(0.0, 0.0, 50.0, 50.0),
                Some(boxed(500.0, 0.0, 50.0, 50.0))
            )
            .is_empty(),
            "and ones that miss entirely leave nothing"
        );
    }

    /// Two corners on one side cannot eat more than the side has, and the
    /// shrink is one factor so the shape keeps its proportions.
    ///
    /// The CSS `border-radius` rule, and it is four sums: the two horizontal
    /// pairs against the width, the two vertical pairs against the height.
    /// Every other test here uses radii that fit, or uniform ones where all
    /// four sums agree — so any one of them could be read off the wrong pair
    /// and nothing would move.
    ///
    /// One pair over budget at a time, therefore, with the other three well
    /// inside it. A pill is the everyday shape this protects: `corners(999)` on
    /// a bar is exactly the case where the rule decides the whole silhouette.
    #[test]
    fn each_pair_of_corners_is_shrunk_by_its_own_side() {
        let cut = |w: f32, h: f32, radii: CornerRadii| {
            placed_shape_to_rects(
                PlacedShape::placed(Rect::new(0.0, 0.0, w, h), radii, 1.0, Transform::IDENTITY),
                None,
            )
        };
        let pair = |a: f32, b: f32, c: f32, d: f32| CornerRadii {
            top_left: a,
            top_right: b,
            bottom_right: c,
            bottom_left: d,
        };

        // Each case puts 60 + 60 across a side of 60, so that pair must halve
        // to 30. Which way the silhouette moves when it does not depends on
        // which side the pair spans: two corners sharing the *short* side pull
        // their curves inward and the shape narrows, two sharing the long one
        // let them reach further along it and the shape swells. So each case
        // names a pixel and whether the shape should hold it.
        for (label, rects, (x, y), inside) in [
            (
                "top pair, across the width",
                cut(60.0, 200.0, pair(60.0, 60.0, 0.0, 0.0)),
                (15, 5),
                true,
            ),
            (
                "bottom pair, across the width",
                cut(60.0, 200.0, pair(0.0, 0.0, 60.0, 60.0)),
                (15, 195),
                true,
            ),
            (
                "left pair, down the height",
                cut(200.0, 60.0, pair(60.0, 0.0, 0.0, 60.0)),
                (2, 15),
                false,
            ),
            (
                "right pair, down the height",
                cut(200.0, 60.0, pair(0.0, 60.0, 60.0, 0.0)),
                (197, 15),
                false,
            ),
        ] {
            assert_eq!(
                covers(&rects, x, y),
                inside,
                "{label}: the pair halves to 30, and ({x}, {y}) is on the \
                 {} of the curve that leaves",
                if inside { "inside" } else { "outside" }
            );
        }
    }

    // -- clipped shapes -------------------------------------------------

    /// A card filling a rounded scroller is cornered by the scroller.
    ///
    /// The clip is intersected scanline by scanline rather than as a box, so
    /// its curve shows. Published as the scroller's square bounding box — which
    /// is what a box intersection gives — the compositor blurs the desktop in
    /// the four corners where the panel is not drawn, and on a click-through
    /// surface those corners swallow clicks.
    #[test]
    fn a_corner_the_clip_supplies_belongs_to_the_clip() {
        let square_card = PlacedShape::placed(
            Rect::new(-10.0, -10.0, 120.0, 120.0),
            round(0.0),
            1.0,
            Transform::IDENTITY,
        );
        let rounded_scroller = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            round(24.0),
            1.0,
            Transform::IDENTITY,
        );

        let rects = placed_shape_to_rects(square_card, Some(rounded_scroller));
        assert!(
            !covers(&rects, 0, 0) && !covers(&rects, 99, 99),
            "the card declares square corners and the scroller cuts them anyway"
        );
        assert!(covers(&rects, 50, 50), "and the middle is still published");
    }

    /// A cut edge is straight, and only the corners along it are squared.
    ///
    /// A card scrolled half out of a viewport has a straight edge where the cut
    /// is. Published as though that side were still round, the region loses a
    /// wedge the size of the radius at each of its two corners.
    #[test]
    fn a_clip_squares_off_only_the_corners_it_cuts() {
        let card = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            round(20.0),
            1.0,
            Transform::IDENTITY,
        );
        let cuts_the_right = PlacedShape::placed(
            Rect::new(-50.0, -50.0, 90.0, 200.0),
            round(0.0),
            1.0,
            Transform::IDENTITY,
        );

        let rects = placed_shape_to_rects(card, Some(cuts_the_right));
        assert!(
            covers(&rects, 39, 0) && covers(&rects, 39, 99),
            "the cut edge is straight, so its corners reach it"
        );
        assert!(
            !covers(&rects, 0, 0) && !covers(&rects, 0, 99),
            "and the card's own corners are still round"
        );
    }

    /// The tighter of the two curves is the one that shows, whichever shape it
    /// belongs to.
    #[test]
    fn two_curves_over_one_another_leave_the_tighter() {
        let same = Rect::new(0.0, 0.0, 100.0, 100.0);
        let rounded = |r: f32| PlacedShape::placed(same, round(r), 1.0, Transform::IDENTITY);

        let sharp_clip = placed_shape_to_rects(rounded(4.0), Some(rounded(24.0)));
        assert!(
            !covers(&sharp_clip, 2, 2),
            "the clip rounds harder, so the clip's corner is what is left"
        );
        let sharp_shape = placed_shape_to_rects(rounded(24.0), Some(rounded(4.0)));
        assert!(
            !covers(&sharp_shape, 2, 2),
            "and the other way round gives the same answer"
        );
    }

    /// A clip is placed by its own transform, so one inside a translated
    /// subtree cuts where it is rather than where it was declared.
    #[test]
    fn a_clip_is_placed_before_it_cuts() {
        let card = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            round(0.0),
            1.0,
            Transform::translate(500.0, 300.0),
        );
        let clip = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            round(0.0),
            1.0,
            Transform::translate(500.0, 300.0),
        );

        let rects = placed_shape_to_rects(card, Some(clip));
        assert!(
            covers(&rects, 550, 350),
            "the clip covers the whole card, so nothing is cut"
        );

        // Compared without being placed, the clip would sit at the origin and
        // take the whole card with it.
        let unplaced = PlacedShape {
            curvature: 1.0,
            placement: Transform::IDENTITY,
            ..clip
        };
        assert!(placed_shape_to_rects(card, Some(unplaced)).is_empty());
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
            shape(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                round(0.0),
                Transform::rotate_degrees(45.0).center_at(50.0, 50.0),
            ),
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
                curvature: 1.0,
                placement: transform,
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
            shape(
                Rect::new(0.0, 0.0, w, h),
                round(0.0),
                Transform::rotate_degrees(20.0).center_at(w / 2.0, h / 2.0),
            ),
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

    /// A turned shape keeps its rounded corners: the arcs are transformed with
    /// everything else, and the rectangles still inscribe the result.
    ///
    /// **Every other turned test here uses a radius of zero**, which skips the
    /// arc solve entirely — so the corner arithmetic had no rotated case at all
    /// and shipped reading the wrong row of the matrix. The symptom was a
    /// region in stripes covering about half the shape while reaching several
    /// pixels past its outline, and the test that stood here asked only that
    /// rounding *removes* area, which a broken solve does more enthusiastically
    /// than a working one.
    ///
    /// So: inside, by an oracle that maps each rectangle back, and not much
    /// smaller than the shape.
    #[test]
    fn a_turned_rounded_shape_is_inside_itself_and_most_of_itself() {
        let (w, h, r) = (110.0f32, 70.0f32, 16.0f32);
        for degrees in [25.0f32, 45.0, 115.0, -30.0] {
            let transform = Transform::rotate_degrees(degrees).center_at(w / 2.0, h / 2.0);
            let rects = placed_shape_to_rects(
                PlacedShape {
                    rect: Rect::new(0.0, 0.0, w, h),
                    radii: round(r),
                    curvature: 1.0,
                    placement: transform,
                },
                None,
            );
            assert!(!rects.is_empty(), "at {degrees}°");

            let back = transform.inverse().expect("a rotation is invertible");
            let inside = |px: f32, py: f32| {
                let (x, y) = back.transform_point(px, py);
                let cx = x.clamp(r, w - r);
                let cy = y.clamp(r, h - r);
                ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= r + 1.0
                    && (-1.0..=w + 1.0).contains(&x)
                    && (-1.0..=h + 1.0).contains(&y)
            };
            for rect in &rects {
                for (px, py) in [
                    (rect.x, rect.y),
                    (rect.x + rect.width, rect.y),
                    (rect.x, rect.y + rect.height),
                    (rect.x + rect.width, rect.y + rect.height),
                ] {
                    assert!(
                        inside(px as f32, py as f32),
                        "at {degrees}°, ({px}, {py}) is outside the shape"
                    );
                }
            }

            // And most of it: rectangles that are merely *inside* can still be
            // stripes, which is what the wrong row produced.
            let covered: i64 = rects.iter().map(|r| r.width as i64 * r.height as i64).sum();
            let shape_area = (w * h - (4.0 - std::f32::consts::PI) * r * r) as i64;
            assert!(
                covered > shape_area * 9 / 10,
                "at {degrees}°, {covered} px of a {shape_area} px shape"
            );
        }
    }

    /// A clip narrows the shape, and an axis-aligned one narrows it to a box.
    #[test]
    fn a_clip_narrows_a_turned_shape() {
        let diamond = shape(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            round(0.0),
            Transform::rotate_degrees(45.0).center_at(50.0, 50.0),
        );
        let box_clip = |r: Rect| Some(shape(r, round(0.0), Transform::IDENTITY));
        let clipped =
            placed_shape_to_rects(diamond, box_clip(Rect::new(50.0, -50.0, 100.0, 200.0)));

        assert!(
            clipped.iter().all(|r| r.x >= 50),
            "nothing reaches left of the clip"
        );
        assert!(
            covers(&clipped, 60, 50),
            "and what is on show is still there"
        );
        assert!(
            placed_shape_to_rects(diamond, box_clip(Rect::new(500.0, 0.0, 10.0, 10.0))).is_empty(),
            "a clip that misses leaves nothing to publish"
        );
    }
}
