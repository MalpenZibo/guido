//! Paths, and how one is filled and stroked.
//!
//! A [`Path`] is a sequence of straight and curved segments in a widget's local
//! coordinates; [`PaintContext::draw_path`](super::PaintContext::draw_path)
//! fills it, strokes it, or both. The renderer tessellates it into triangles
//! with lyon and anti-aliases the edges with a one-pixel fringe of its own —
//! see `docs/RENDERER.md`, "Paths".

use std::cell::OnceCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::widgets::{Color, Point, Rect};

/// One step of a [`Path`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathElement {
    /// Start a new subpath here, leaving the current one open.
    MoveTo(Point),
    /// A straight segment to the point.
    LineTo(Point),
    /// A quadratic Bézier curve through `control` to `to`.
    QuadTo { control: Point, to: Point },
    /// A cubic Bézier curve through two control points to `to`.
    CubicTo {
        first: Point,
        second: Point,
        to: Point,
    },
    /// Close the current subpath with a straight segment back to its start.
    Close,
}

impl PathElement {
    /// The points this element names, in order — control points included.
    pub fn points(&self) -> impl Iterator<Item = Point> {
        let points = match *self {
            PathElement::MoveTo(p) | PathElement::LineTo(p) => [Some(p), None, None],
            PathElement::QuadTo { control, to } => [Some(control), Some(to), None],
            PathElement::CubicTo { first, second, to } => [Some(first), Some(second), Some(to)],
            PathElement::Close => [None, None, None],
        };
        points.into_iter().flatten()
    }
}

/// A shape made of straight and curved segments, in logical pixels.
///
/// Built a segment at a time, and cheap to clone: the segments are shared, so a
/// widget can keep one and draw it every frame, and the renderer recognises it
/// as the path it tessellated last time without comparing the segments again.
///
/// ```
/// # use guido::prelude::*;
/// # use guido::widget_prelude::*;
/// let sparkline = Path::new()
///     .move_to((0.0, 12.0))
///     .line_to((10.0, 4.0))
///     .line_to((20.0, 9.0))
///     .quad_to((25.0, 0.0), (30.0, 6.0));
/// assert_eq!(sparkline.elements().len(), 4);
/// ```
#[derive(Clone, Default)]
pub struct Path {
    inner: Rc<PathData>,
}

#[derive(Clone, Default)]
struct PathData {
    elements: Vec<PathElement>,
    /// The hash of `elements`, worked out once per path rather than once per
    /// frame it is drawn in. Emptied by every edit, as `bounds` is.
    hash: OnceCell<u64>,
    bounds: OnceCell<Rect>,
}

impl Path {
    /// An empty path.
    pub fn new() -> Self {
        Self::default()
    }

    fn push(mut self, element: PathElement) -> Self {
        // Unique while it is being built, so this is the vector itself and not
        // a copy of it.
        let data = Rc::make_mut(&mut self.inner);
        data.elements.push(element);
        data.hash = OnceCell::new();
        data.bounds = OnceCell::new();
        self
    }

    /// Start a new subpath at `to`.
    pub fn move_to(self, to: impl Into<Point>) -> Self {
        self.push(PathElement::MoveTo(to.into()))
    }

    /// A straight segment to `to`. A path that starts with one starts at
    /// `to`, and so draws nothing until the next segment.
    pub fn line_to(self, to: impl Into<Point>) -> Self {
        self.push(PathElement::LineTo(to.into()))
    }

    /// A quadratic Bézier curve through `control` to `to`.
    pub fn quad_to(self, control: impl Into<Point>, to: impl Into<Point>) -> Self {
        self.push(PathElement::QuadTo {
            control: control.into(),
            to: to.into(),
        })
    }

    /// A cubic Bézier curve through `first` and `second` to `to`.
    pub fn cubic_to(
        self,
        first: impl Into<Point>,
        second: impl Into<Point>,
        to: impl Into<Point>,
    ) -> Self {
        self.push(PathElement::CubicTo {
            first: first.into(),
            second: second.into(),
            to: to.into(),
        })
    }

    /// Close the current subpath back to where it started.
    pub fn close(self) -> Self {
        self.push(PathElement::Close)
    }

    /// The segments, in order.
    pub fn elements(&self) -> &[PathElement] {
        &self.inner.elements
    }

    /// Whether the path has no segments at all.
    pub fn is_empty(&self) -> bool {
        self.inner.elements.is_empty()
    }

    /// The box around every point the path names, control points included —
    /// so it always holds the curve, and is sometimes larger than it.
    pub fn bounds(&self) -> Rect {
        *self.inner.bounds.get_or_init(|| {
            let mut points = self.inner.elements.iter().flat_map(PathElement::points);
            let Some(first) = points.next() else {
                return Rect::default();
            };
            let (mut min, mut max) = (first, first);
            for p in points {
                min = Point::new(min.x.min(p.x), min.y.min(p.y));
                max = Point::new(max.x.max(p.x), max.y.max(p.y));
            }
            Rect::new(min.x, min.y, max.x - min.x, max.y - min.y)
        })
    }

    /// A hash of the segments, which is what the renderer's cache of
    /// tessellations is keyed by.
    pub(crate) fn content_hash(&self) -> u64 {
        *self.inner.hash.get_or_init(|| {
            let mut hasher = rustc_hash::FxHasher::default();
            for element in &self.inner.elements {
                std::mem::discriminant(element).hash(&mut hasher);
                for p in element.points() {
                    p.x.to_bits().hash(&mut hasher);
                    p.y.to_bits().hash(&mut hasher);
                }
            }
            hasher.finish()
        })
    }
}

/// The same segments: the same path as far as anything drawn is concerned.
impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner) || self.inner.elements == other.inner.elements
    }
}

impl std::fmt::Debug for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.elements()).finish()
    }
}

/// Which parts of a path that crosses itself are inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FillRule {
    /// Inside wherever the outline winds round a point other than zero times —
    /// the default in SVG, Canvas and Flutter.
    #[default]
    NonZero,
    /// Inside wherever a ray from the point crosses the outline an odd number
    /// of times, so a subpath inside another cuts a hole whichever way it
    /// winds.
    EvenOdd,
}

/// How the inside of a path is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fill {
    pub color: Color,
    pub rule: FillRule,
}

impl Fill {
    /// Fill with `color`, by the non-zero rule.
    pub fn color(color: Color) -> Self {
        Self {
            color,
            rule: FillRule::NonZero,
        }
    }

    /// Decide what is inside by `rule` instead.
    pub fn rule(mut self, rule: FillRule) -> Self {
        self.rule = rule;
        self
    }
}

/// How the end of an open line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Cap {
    /// Square, and exactly at the end.
    #[default]
    Butt,
    /// A half disc past the end, as wide as the line.
    Round,
    /// A half square past the end, as wide as the line.
    Square,
}

/// How two segments of a line meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Join {
    /// The outer edges extended until they meet, or cut square once they
    /// would reach past the [miter limit](Stroke::miter_limit).
    #[default]
    Miter,
    /// A disc at the corner.
    Round,
    /// The outer corners joined by a straight edge.
    Bevel,
}

/// How the outline of a path is painted: a line `width` logical pixels wide,
/// centred on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
    pub cap: Cap,
    pub join: Join,
    /// How far a miter may reach past the corner, as a multiple of half the
    /// width, before it is cut square. 4 is SVG's default.
    pub miter_limit: f32,
}

impl Stroke {
    /// A line `width` logical pixels wide, with butt caps and miter joins.
    pub fn new(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            cap: Cap::Butt,
            join: Join::Miter,
            miter_limit: 4.0,
        }
    }

    /// End both ends of an open line with `cap`.
    pub fn caps(mut self, cap: Cap) -> Self {
        self.cap = cap;
        self
    }

    /// Meet segments with `join`.
    pub fn joins(mut self, join: Join) -> Self {
        self.join = join;
        self
    }

    /// Cut a miter square once it reaches `limit` half-widths past its corner.
    /// Held at 1 or more, where SVG holds it.
    pub fn miter_limit(mut self, limit: f32) -> Self {
        self.miter_limit = limit.max(1.0);
        self
    }

    /// How far past the path the line can reach, in logical pixels: half its
    /// width, times what a miter or a square cap adds.
    pub(crate) fn reach(&self) -> f32 {
        let corner = match self.join {
            Join::Miter => self.miter_limit.max(1.0),
            Join::Round | Join::Bevel => 1.0,
        };
        let end = match self.cap {
            Cap::Square => std::f32::consts::SQRT_2,
            Cap::Butt | Cap::Round => 1.0,
        };
        self.width * 0.5 * corner.max(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path the builder made is the path that was described, and its hash is
    /// worked out from that and nothing else.
    #[test]
    fn the_same_segments_are_the_same_path() {
        let a = Path::new().move_to((0.0, 0.0)).line_to((4.0, 2.0)).close();
        let b = Path::new().move_to((0.0, 0.0)).line_to((4.0, 2.0)).close();
        let c = Path::new().move_to((0.0, 0.0)).line_to((4.0, 3.0)).close();
        assert_eq!(a, b);
        assert_eq!(a.content_hash(), b.content_hash());
        assert_ne!(a, c);
        assert_ne!(a.content_hash(), c.content_hash());
    }

    /// An edit after the hash was taken is seen by the next one.
    #[test]
    fn an_edit_forgets_the_hash() {
        let a = Path::new().move_to((0.0, 0.0)).line_to((4.0, 2.0));
        let before = a.content_hash();
        let shared = a.clone();
        let a = a.line_to((8.0, 0.0));
        assert_ne!(a.content_hash(), before);
        assert_eq!(
            shared.content_hash(),
            before,
            "a clone keeps its own segments"
        );
        assert_eq!(shared.elements().len(), 2);
    }

    #[test]
    fn the_bounds_hold_every_point_the_path_names() {
        let p = Path::new()
            .move_to((2.0, 3.0))
            .quad_to((10.0, -4.0), (6.0, 8.0))
            .line_to((-1.0, 5.0));
        assert_eq!(p.bounds(), Rect::new(-1.0, -4.0, 11.0, 12.0));
        assert_eq!(Path::new().bounds(), Rect::default());
    }

    #[test]
    fn a_stroke_reaches_as_far_as_its_corners_and_ends_can() {
        let s = Stroke::new(2.0, Color::WHITE);
        assert_eq!(s.reach(), 4.0, "a miter of four half-widths");
        assert_eq!(s.joins(Join::Round).reach(), 1.0);
        let square = s.joins(Join::Bevel).caps(Cap::Square).reach();
        assert!((square - std::f32::consts::SQRT_2).abs() < 1e-6);
    }
}
