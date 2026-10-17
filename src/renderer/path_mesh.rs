//! A path as triangles, with an anti-aliased edge.
//!
//! lyon fills and strokes a path into a triangle mesh, and does nothing about
//! the edges: without MSAA — and guido has none, its pipelines draw at one
//! sample — every edge of that mesh is a staircase. So the edge is drawn here,
//! as egui and Skia's non-MSAA path renderers draw it: a ramp of coverage one
//! physical pixel wide, centred on the true edge.
//!
//! The mesh is walked for its *boundary* — the edges only one triangle uses —
//! which is the outline lyon resolved, holes, crossings and joins included,
//! with no fill rule or winding left to guess at: the inside of each boundary
//! edge is wherever its own triangle is. Every boundary vertex is moved half a
//! pixel in along the averaged normal of its two edges, a twin is put half a
//! pixel out at coverage zero, and each boundary edge gets the quad between the
//! two. Coverage is interpolated across that quad, so it falls from one to zero
//! over the pixel the edge crosses.
//!
//! Everything here is in *physical* pixels of the path's own space — the
//! logical coordinates times the surface scale — because the fringe is one
//! device pixel wide. A transform applied afterwards carries the mesh with it,
//! fringe and all.

use lyon_tessellation::math::{Point as LyonPoint, point};
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::path::PathEvent;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};
use rustc_hash::FxHashMap;

use super::path::{Cap, Dashes, FillRule, Join, Path, PathElement, Stroke};

/// How far a flattened curve may stray from the true one, in physical pixels.
/// lyon's own default, and a tenth of the fringe.
const TOLERANCE: f32 = 0.1;

/// The narrowest a stroke is tessellated, in physical pixels. A thinner one
/// would leave its core inside out once both edges are moved half a pixel in,
/// so it is drawn this wide at a coverage that keeps its weight — egui draws a
/// hairline the same way.
const THINNEST: f32 = 1.0;

/// The longest a vertex's normal is allowed to grow at a sharp corner, in
/// units of the half-pixel it moves. A spike's miter is unbounded; past this
/// the fringe there is simply narrower.
const LONGEST_MITER: f32 = 4.0;

/// One corner of a triangle, in physical pixels of the path's own space, and
/// how much of the colour it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MeshVertex {
    pub position: [f32; 2],
    pub coverage: f32,
}

/// The triangles a fill or a stroke is drawn with.
#[derive(Debug, Clone, Default)]
pub(crate) struct PathMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

/// The lyon path for `path`, at `scale` physical pixels to the logical one.
///
/// SVG's rule for what follows a close: the next segment starts where the
/// closed subpath did. A path whose first element is not a `MoveTo` starts at
/// that element's first point, as Canvas starts one.
fn to_lyon(path: &Path, scale: f32) -> LyonPath {
    let at = |p: crate::widgets::Point| point(p.x * scale, p.y * scale);
    let mut builder = LyonPath::builder();
    let mut open = false;
    // Where the next subpath starts if a segment arrives with none open:
    // where the last closed one did, or — before any — that segment's own
    // first point, as Canvas has it.
    let mut start = None;
    for element in path.elements() {
        if !open && !matches!(element, PathElement::MoveTo(_) | PathElement::Close) {
            let first = element.points().next().map_or(point(0.0, 0.0), at);
            builder.begin(*start.get_or_insert(first));
            open = true;
        }
        match *element {
            PathElement::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                start = Some(at(p));
                builder.begin(at(p));
                open = true;
            }
            PathElement::LineTo(p) => {
                builder.line_to(at(p));
            }
            PathElement::QuadTo { control, to } => {
                builder.quadratic_bezier_to(at(control), at(to));
            }
            PathElement::CubicTo { first, second, to } => {
                builder.cubic_bezier_to(at(first), at(second), at(to));
            }
            PathElement::Close => {
                if open {
                    builder.end(true);
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

/// The most dashes and gaps a path is cut into. A pattern so fine against so
/// long a path that it would make more is drawn solid instead, as Skia gives
/// up past a million — and a pattern whose lengths are below what the
/// path's coordinates can resolve would otherwise never move along it.
const MOST_INTERVALS: usize = 200_000;

/// `path` cut into the dashes `dashes` describes, each an open subpath of its
/// own — or `None` if that would be more than [`MOST_INTERVALS`].
///
/// The curves are flattened first, to the tolerance the stroker would have
/// used, and a dash keeps every vertex it passes, so the stroker still joins
/// it at each corner and caps it only at its two ends. The pattern starts
/// again on every subpath. On a closed one, a dash that is on at the start
/// and on again when the subpath comes back to it is one dash through the
/// start, as Skia and cairo draw it, and a dash longer than the whole subpath
/// leaves it closed.
fn dash(
    path: &LyonPath,
    dashes: &Dashes,
    scale: f32,
    scratch: &mut DashScratch,
) -> Option<LyonPath> {
    let mut dasher = Dasher::new(dashes, scale, scratch);
    for event in path.iter().flattened(TOLERANCE) {
        match event {
            PathEvent::Begin { at } => dasher.begin(at),
            PathEvent::Line { to, .. } => dasher.line_to(to)?,
            PathEvent::End { first, close, .. } => {
                if close {
                    dasher.line_to(first)?;
                }
                dasher.end(close);
            }
            // Flattened: there are no curves left.
            PathEvent::Quadratic { .. } | PathEvent::Cubic { .. } => {}
        }
    }
    Some(dasher.out.build())
}

/// The buffers [`dash`] walks a path in, kept between tessellations so a
/// dash offset that moves every frame does not grow them again every frame.
#[derive(Default)]
struct DashScratch {
    /// The pattern, in physical pixels.
    pattern: Vec<f32>,
    /// The dash being drawn — empty in a gap.
    current: Vec<LyonPoint>,
    /// The subpath's first dash, held until its end says whether it is closed
    /// and the last dash runs into it.
    first: Vec<LyonPoint>,
}

/// The walk [`dash`] makes along a flattened path.
struct Dasher<'a> {
    /// Where each subpath starts in the pattern: the interval, and how much of
    /// it is left.
    start: (usize, f32),
    index: usize,
    left: f32,
    at: LyonPoint,
    scratch: &'a mut DashScratch,
    intervals: usize,
    out: lyon_tessellation::path::path::Builder,
}

impl<'a> Dasher<'a> {
    fn new(dashes: &Dashes, scale: f32, scratch: &'a mut DashScratch) -> Self {
        scratch.pattern.clear();
        scratch
            .pattern
            .extend(dashes.pattern().iter().map(|l| l * scale));
        let pattern = &scratch.pattern;
        // Walk the offset into the pattern. An offset that ends exactly where
        // an interval does starts on the next, unless it is zero: a pattern
        // that opens on a dot draws that dot at the start.
        let mut offset = dashes.offset() * scale;
        let mut index = 0;
        while offset > 0.0 && offset >= pattern[index] {
            offset -= pattern[index];
            index = (index + 1) % pattern.len();
        }
        let left = pattern[index] - offset;
        Self {
            start: (index, left),
            index,
            left,
            at: point(0.0, 0.0),
            scratch,
            intervals: 0,
            out: LyonPath::builder(),
        }
    }

    /// A dash is on the even intervals, a gap on the odd.
    fn on(&self) -> bool {
        self.index.is_multiple_of(2)
    }

    /// Whether the dash being drawn is the subpath's first, which began at
    /// its start: the subpath began on a dash, and that dash has not ended.
    fn in_first(&self) -> bool {
        self.start.0.is_multiple_of(2) && self.scratch.first.is_empty()
    }

    fn begin(&mut self, at: LyonPoint) {
        (self.index, self.left) = self.start;
        self.at = at;
        self.scratch.current.clear();
        self.scratch.first.clear();
        if self.on() {
            self.scratch.current.push(at);
        }
    }

    fn line_to(&mut self, to: LyonPoint) -> Option<()> {
        let from = self.at;
        let length = (to - from).length();
        let mut travelled = 0.0;
        while self.left <= length - travelled {
            self.intervals += 1;
            if self.intervals > MOST_INTERVALS {
                return None;
            }
            travelled += self.left;
            let p = if length > 0.0 {
                from.lerp(to, travelled / length)
            } else {
                from
            };
            self.scratch.current.push(p);
            if self.on() {
                self.finish_dash();
            }
            self.index = (self.index + 1) % self.scratch.pattern.len();
            self.left = self.scratch.pattern[self.index];
        }
        self.left -= length - travelled;
        if self.on() {
            self.scratch.current.push(to);
        }
        self.at = to;
        Some(())
    }

    fn finish_dash(&mut self) {
        let in_first = self.in_first();
        let DashScratch { current, first, .. } = &mut *self.scratch;
        if in_first {
            std::mem::swap(first, current);
        } else {
            emit(&mut self.out, current, false);
        }
        current.clear();
    }

    fn end(&mut self, close: bool) {
        let on = self.on();
        let in_first = self.in_first();
        let DashScratch { current, first, .. } = &mut *self.scratch;
        if close && on && in_first {
            // One dash all the way round: the subpath as it was, closed. Its
            // last point is its first again.
            current.pop();
            emit(&mut self.out, current, true);
        } else if close && on && !first.is_empty() {
            current.extend_from_slice(&first[1..]);
            emit(&mut self.out, current, false);
        } else {
            if on {
                emit(&mut self.out, current, false);
            }
            emit(&mut self.out, first, false);
        }
    }
}

/// One dash, as a subpath. A dash of a single point is drawn as one of no
/// length, which lyon gives a cap at either end of — a round cap's dot.
fn emit(out: &mut lyon_tessellation::path::path::Builder, points: &[LyonPoint], close: bool) {
    let Some((&start, rest)) = points.split_first() else {
        return;
    };
    out.begin(start);
    if rest.is_empty() {
        out.line_to(start);
    }
    for &p in rest {
        out.line_to(p);
    }
    out.end(close);
}

/// lyon's two tessellators, and everything a mesh is built in on its way to
/// being one — kept between meshes, so that tessellating a path allocates the
/// mesh it returns and little else.
#[derive(Default)]
pub(crate) struct Tessellator {
    fill: FillTessellator,
    stroke: StrokeTessellator,
    /// What lyon hands back, before it is welded and feathered.
    lyon: VertexBuffers<[f32; 2], u32>,
    scratch: Scratch,
}

/// The working buffers of [`Tessellator::feather`], emptied before each use.
#[derive(Default)]
struct Scratch {
    welded: Vec<[f32; 2]>,
    by_position: FxHashMap<(u32, u32), u32>,
    remap: Vec<u32>,
    indices: Vec<u32>,
    edges: FxHashMap<(u32, u32), (u32, u32, u32, u32)>,
    gathered: Vec<Gathered>,
    boundary: Vec<(u32, u32)>,
    outer: Vec<u32>,
    dash: DashScratch,
}

impl Tessellator {
    /// The inside of `path`, by `rule`, at `scale`.
    pub(crate) fn fill(&mut self, path: &Path, rule: FillRule, scale: f32) -> PathMesh {
        let lyon = to_lyon(path, scale);
        let options = FillOptions::tolerance(TOLERANCE).with_fill_rule(match rule {
            FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
            FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
        });
        self.lyon.vertices.clear();
        self.lyon.indices.clear();
        let tessellated = self.fill.tessellate_path(
            &lyon,
            &options,
            &mut BuffersBuilder::new(&mut self.lyon, |v: FillVertex| v.position().to_array()),
        );
        if tessellated.is_err() {
            // lyon gives up on input it cannot order — a NaN, an overflow.
            // What it managed to emit is not a shape anybody drew, so nothing
            // is drawn.
            return PathMesh::default();
        }
        self.feather(1.0)
    }

    /// The outline of `path`, as `stroke` draws it, at `scale`.
    pub(crate) fn stroke(&mut self, path: &Path, stroke: &Stroke, scale: f32) -> PathMesh {
        let width = stroke.width * scale;
        if width.is_nan() || width <= 0.0 {
            return PathMesh::default();
        }
        let mut lyon = to_lyon(path, scale);
        if let Some(dashes) = &stroke.dashes {
            lyon = dash(&lyon, dashes, scale, &mut self.scratch.dash).unwrap_or(lyon);
        }
        let options = StrokeOptions::tolerance(TOLERANCE)
            .with_line_width(width.max(THINNEST))
            .with_line_cap(match stroke.cap {
                Cap::Butt => LineCap::Butt,
                Cap::Round => LineCap::Round,
                Cap::Square => LineCap::Square,
            })
            .with_line_join(match stroke.join {
                Join::Miter => LineJoin::MiterClip,
                Join::Round => LineJoin::Round,
                Join::Bevel => LineJoin::Bevel,
            })
            .with_miter_limit(stroke.miter_limit.max(1.0));
        self.lyon.vertices.clear();
        self.lyon.indices.clear();
        let tessellated = self.stroke.tessellate_path(
            &lyon,
            &options,
            &mut BuffersBuilder::new(&mut self.lyon, |v: StrokeVertex| v.position().to_array()),
        );
        if tessellated.is_err() {
            return PathMesh::default();
        }
        self.feather((width / THINNEST).min(1.0))
    }

    /// Put an anti-aliased edge on the triangles lyon just made.
    ///
    /// `coverage` is what the inside is drawn at — one, or less for a stroke
    /// thinner than a pixel.
    fn feather(&mut self, coverage: f32) -> PathMesh {
        let Scratch {
            welded,
            by_position,
            remap,
            indices,
            edges,
            gathered,
            boundary,
            outer,
            dash: _,
        } = &mut self.scratch;

        // Weld first. A boundary is the edges one triangle uses, so two
        // vertices at one place under two names would turn the seam between
        // their triangles into an edge with a fringe drawn across the inside
        // of the shape.
        welded.clear();
        by_position.clear();
        remap.clear();
        remap.extend(self.lyon.vertices.iter().map(|&p| {
            *by_position
                .entry((p[0].to_bits(), p[1].to_bits()))
                .or_insert_with(|| {
                    welded.push(p);
                    (welded.len() - 1) as u32
                })
        }));
        indices.clear();
        indices.extend(self.lyon.indices.iter().map(|&i| remap[i as usize]));

        // Each edge, how many triangles use it, and — for the one that uses
        // it when it is a boundary — the corner opposite, which says which
        // side the inside is on.
        edges.clear();
        for &[a, b, c] in indices.as_chunks::<3>().0 {
            let (pa, pb, pc) = (welded[a as usize], welded[b as usize], welded[c as usize]);
            let ab = sub(pb, pa);
            let ac = sub(pc, pa);
            let area = ab[0] * ac[1] - ab[1] * ac[0];
            // A triangle with no area has no inside to put on either side of
            // its edges. lyon emits them where points are collinear, always
            // between two triangles that do have one.
            if area.abs() <= 1e-6 {
                continue;
            }
            for (u, v, w) in [(a, b, c), (b, c, a), (c, a, b)] {
                let entry = edges.entry((u.min(v), u.max(v))).or_insert((0, u, v, w));
                entry.0 += 1;
            }
        }

        gathered.clear();
        gathered.resize(welded.len(), Gathered::default());
        boundary.clear();
        for &(count, u, v, w) in edges.values() {
            if count != 1 {
                continue;
            }
            let (pu, pv, pw) = (welded[u as usize], welded[v as usize], welded[w as usize]);
            let along = sub(pv, pu);
            let length = dot(along, along).sqrt();
            if length <= 1e-6 {
                continue;
            }
            let mut normal = [-along[1] / length, along[0] / length];
            if dot(normal, sub(pw, pu)) > 0.0 {
                normal = [-normal[0], -normal[1]];
            }
            gathered[u as usize].add(normal);
            gathered[v as usize].add(normal);
            boundary.push((u, v));
        }
        // The map's order is the hasher's; the mesh's should not be.
        boundary.sort_unstable();

        let fringe = gathered.iter().filter(|g| g.edges > 0).count();
        let mut vertices = Vec::with_capacity(welded.len() + fringe);
        vertices.extend(
            welded
                .iter()
                .map(|&position| MeshVertex { position, coverage }),
        );
        outer.clear();
        outer.resize(welded.len(), u32::MAX);
        for (i, g) in gathered.iter().enumerate() {
            if g.edges == 0 {
                continue;
            }
            let out = g.outward();
            let p = welded[i];
            vertices[i].position = [p[0] - out[0] * 0.5, p[1] - out[1] * 0.5];
            outer[i] = vertices.len() as u32;
            vertices.push(MeshVertex {
                position: [p[0] + out[0] * 0.5, p[1] + out[1] * 0.5],
                coverage: 0.0,
            });
        }

        let mut mesh_indices = Vec::with_capacity(indices.len() + boundary.len() * 6);
        mesh_indices.extend_from_slice(indices);
        for &(u, v) in boundary.iter() {
            let (ou, ov) = (outer[u as usize], outer[v as usize]);
            mesh_indices.extend_from_slice(&[u, v, ov, u, ov, ou]);
        }

        PathMesh {
            vertices,
            indices: mesh_indices,
        }
    }
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn dot(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// What one boundary vertex has gathered: the outward normals of the boundary
/// edges that meet at it, the first two kept apart for the miter.
#[derive(Clone, Copy, Default)]
struct Gathered {
    first: [f32; 2],
    second: [f32; 2],
    sum: [f32; 2],
    edges: u32,
}

impl Gathered {
    fn add(&mut self, normal: [f32; 2]) {
        match self.edges {
            0 => self.first = normal,
            1 => self.second = normal,
            _ => {}
        }
        self.sum = [self.sum[0] + normal[0], self.sum[1] + normal[1]];
        self.edges += 1;
    }

    /// The direction to move the vertex out by, scaled so that both of its
    /// edges move by the same distance: the miter of the two normals,
    /// `(n1 + n2) / (1 + n1·n2)`, whose length is `1 / cos(θ/2)`.
    ///
    /// A vertex where more than two boundary edges meet — two subpaths that
    /// touch at a point — takes their average instead, which is the best one
    /// direction can do for edges going three ways.
    fn outward(&self) -> [f32; 2] {
        if self.edges == 2 {
            let along = 1.0 + dot(self.first, self.second);
            let sum = [
                self.first[0] + self.second[0],
                self.first[1] + self.second[1],
            ];
            if along > 1.0 / (LONGEST_MITER * LONGEST_MITER) {
                let m = [sum[0] / along, sum[1] / along];
                let length = dot(m, m).sqrt();
                if length <= LONGEST_MITER {
                    return m;
                }
                return [m[0] / length * LONGEST_MITER, m[1] / length * LONGEST_MITER];
            }
            // Folded back on itself: the two normals all but cancel, and the
            // direction they still agree on is the edge's own, not theirs.
            let length = dot(sum, sum).sqrt();
            if length > 1e-6 {
                return [
                    sum[0] / length * LONGEST_MITER,
                    sum[1] / length * LONGEST_MITER,
                ];
            }
            return self.first;
        }
        let length = dot(self.sum, self.sum).sqrt();
        if length > 1e-6 {
            [self.sum[0] / length, self.sum[1] / length]
        } else {
            self.first
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Color;

    fn square(side: f32) -> Path {
        Path::new()
            .move_to((0.0, 0.0))
            .line_to((side, 0.0))
            .line_to((side, side))
            .line_to((0.0, side))
            .close()
    }

    /// The area the mesh paints, coverage-weighted, found by integrating it
    /// over a fine grid. What a viewer would call the shape's weight.
    fn painted_area(mesh: &PathMesh, from: [f32; 2], to: [f32; 2], step: f32) -> f32 {
        let mut total = 0.0;
        let mut y = from[1] + step * 0.5;
        while y < to[1] {
            let mut x = from[0] + step * 0.5;
            while x < to[0] {
                // Triangles a mesh draws may overlap, as two blends would.
                let mut covered: f32 = 0.0;
                for t in mesh.indices.as_chunks::<3>().0.iter() {
                    if let Some(c) = coverage_at(mesh, t, [x, y]) {
                        covered = covered + c - covered * c;
                    }
                }
                total += covered * step * step;
                x += step;
            }
            y += step;
        }
        total
    }

    fn coverage_at(mesh: &PathMesh, t: &[u32; 3], p: [f32; 2]) -> Option<f32> {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices[t[k] as usize]);
        let (pa, pb, pc) = (a.position, b.position, c.position);
        let d = (pb[1] - pc[1]) * (pa[0] - pc[0]) + (pc[0] - pb[0]) * (pa[1] - pc[1]);
        if d.abs() < 1e-12 {
            return None;
        }
        let l1 = ((pb[1] - pc[1]) * (p[0] - pc[0]) + (pc[0] - pb[0]) * (p[1] - pc[1])) / d;
        let l2 = ((pc[1] - pa[1]) * (p[0] - pc[0]) + (pa[0] - pc[0]) * (p[1] - pc[1])) / d;
        let l3 = 1.0 - l1 - l2;
        (l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0)
            .then_some(l1 * a.coverage + l2 * b.coverage + l3 * c.coverage)
    }

    /// A filled square paints its own area — no more, though the fringe
    /// reaches half a pixel past it, and no less, though the inside was moved
    /// half a pixel in.
    #[test]
    fn a_feathered_fill_keeps_its_area() {
        let mesh = Tessellator::default().fill(&square(10.0), FillRule::NonZero, 1.0);
        let area = painted_area(&mesh, [-2.0, -2.0], [12.0, 12.0], 0.05);
        assert!((area - 100.0).abs() < 0.5, "a 10×10 square painted {area}");
    }

    /// The edge is a ramp, one pixel wide and centred on it: half covered on
    /// the line itself, full half a pixel in, nothing half a pixel out.
    #[test]
    fn the_edge_ramps_over_one_pixel() {
        let mesh = Tessellator::default().fill(&square(10.0), FillRule::NonZero, 1.0);
        let at = |x: f32| -> f32 {
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| coverage_at(&mesh, t, [x, 5.0]))
                .fold(0.0, f32::max)
        };
        assert!((at(0.0) - 0.5).abs() < 1e-4, "on the edge: {}", at(0.0));
        assert!((at(0.5) - 1.0).abs() < 1e-4, "half a pixel in: {}", at(0.5));
        assert!(at(-0.5) < 1e-4, "half a pixel out: {}", at(-0.5));
        assert_eq!(at(-0.6), 0.0, "past the fringe nothing is drawn");
    }

    /// The fringe is in physical pixels: at scale 2 the square is twice the
    /// size and its edge is still one pixel wide.
    #[test]
    fn the_fringe_is_one_physical_pixel_at_any_scale() {
        let mesh = Tessellator::default().fill(&square(10.0), FillRule::NonZero, 2.0);
        let xs: Vec<f32> = mesh.vertices.iter().map(|v| v.position[0]).collect();
        let min = xs.iter().copied().fold(f32::MAX, f32::min);
        let max = xs.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            (min + 0.5).abs() < 1e-4 && (max - 20.5).abs() < 1e-4,
            "{min}..{max}"
        );
    }

    /// A hole cut by the even-odd rule has an edge of its own, ramping the
    /// other way: the inner square is empty, and so is its fringe's far side.
    #[test]
    fn a_hole_is_feathered_inward() {
        let ring = Path::new()
            .move_to((0.0, 0.0))
            .line_to((20.0, 0.0))
            .line_to((20.0, 20.0))
            .line_to((0.0, 20.0))
            .close()
            .move_to((5.0, 5.0))
            .line_to((15.0, 5.0))
            .line_to((15.0, 15.0))
            .line_to((5.0, 15.0))
            .close();
        let mesh = Tessellator::default().fill(&ring, FillRule::EvenOdd, 1.0);
        let area = painted_area(&mesh, [-2.0, -2.0], [22.0, 22.0], 0.1);
        assert!(
            (area - 300.0).abs() < 1.5,
            "a 20×20 less a 10×10 painted {area}"
        );
    }

    /// A stroked polyline has an outline and nothing else: the seams lyon
    /// leaves between its triangles are not mistaken for edges, which would
    /// draw a faint line down the middle of the stroke.
    #[test]
    fn a_stroke_is_one_band_with_no_seam_inside() {
        let line = Path::new()
            .move_to((0.0, 10.0))
            .line_to((20.0, 4.0))
            .line_to((40.0, 12.0));
        let stroke = Stroke::new(3.0, Color::WHITE).joins(Join::Round);
        let mesh = Tessellator::default().stroke(&line, &stroke, 1.0);
        let fringe = mesh.vertices.iter().filter(|v| v.coverage == 0.0).count();
        let inside = mesh.vertices.len() - fringe;
        assert_eq!(fringe, inside, "every vertex of a stroke is on its outline");
        // Along the centre line the stroke is fully covered everywhere.
        for i in 1..40 {
            let x = i as f32;
            let y = if x < 20.0 {
                10.0 - 6.0 * x / 20.0
            } else {
                4.0 + 8.0 * (x - 20.0) / 20.0
            };
            let covered = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| coverage_at(&mesh, t, [x, y]))
                .fold(0.0, f32::max);
            assert!(
                covered > 0.999,
                "at x = {x} the centre is covered {covered}"
            );
        }
    }

    /// A dense line — segments far shorter than it is wide, as a sparkline of
    /// many samples in a narrow box is — still covers its centre everywhere
    /// and paints about its own area. lyon folds the inside of such joins back
    /// over itself, and the fringe has to survive that.
    #[test]
    fn a_dense_stroke_keeps_its_centre_and_its_weight() {
        let samples = 200;
        let point = |i: usize| {
            let x = i as f32 * 80.0 / (samples - 1) as f32;
            (x, 10.0 + 6.0 * (i as f32 * 0.9).sin())
        };
        let line = (1..samples).fold(Path::new().move_to(point(0)), |p, i| p.line_to(point(i)));
        let stroke = Stroke::new(1.6, Color::WHITE).joins(Join::Round);
        let mesh = Tessellator::default().stroke(&line, &stroke, 1.0);
        // The two ends are butt, on the edge, and half covered by right.
        for i in 1..samples - 1 {
            let (x, y) = point(i);
            let covered = mesh
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|t| coverage_at(&mesh, t, [x, y]))
                .fold(0.0, f32::max);
            assert!(
                covered > 0.99,
                "sample {i} at ({x}, {y}) is covered {covered}"
            );
        }
        // What the line covers, by definition: every point within half its
        // width of it, round joins and butt ends being what lie within that.
        let points: Vec<(f32, f32)> = (0..samples).map(point).collect();
        let near = |x: f32, y: f32| {
            points.windows(2).any(|w| {
                let ((ax, ay), (bx, by)) = (w[0], w[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                let (px, py) = (ax + t * dx - x, ay + t * dy - y);
                px * px + py * py <= 0.8 * 0.8
            })
        };
        let step = 0.1;
        let mut ideal = 0.0;
        let mut y = step * 0.5;
        while y < 20.0 {
            let mut x = -2.0 + step * 0.5;
            while x < 82.0 {
                if near(x, y) {
                    ideal += step * step;
                }
                x += step;
            }
            y += step;
        }
        let area = painted_area(&mesh, [-2.0, 0.0], [82.0, 20.0], step);
        assert!(
            (area - ideal).abs() < ideal * 0.05,
            "the line covers {ideal} and the mesh painted {area}"
        );
    }

    /// A hairline is drawn a pixel wide at the weight it asked for, rather
    /// than inside out.
    #[test]
    fn a_hairline_keeps_its_weight() {
        let line = Path::new().move_to((0.0, 5.0)).line_to((20.0, 5.0));
        let mesh = Tessellator::default().stroke(&line, &Stroke::new(0.5, Color::WHITE), 1.0);
        let area = painted_area(&mesh, [-2.0, 0.0], [22.0, 10.0], 0.05);
        assert!(
            (area - 10.0).abs() < 0.3,
            "20 pixels at half a pixel painted {area}"
        );
    }

    #[test]
    fn nothing_is_drawn_for_nothing() {
        let mesh = Tessellator::default().fill(&Path::new(), FillRule::NonZero, 1.0);
        assert!(mesh.indices.is_empty());
        let zero =
            Tessellator::default().stroke(&square(4.0), &Stroke::new(0.0, Color::WHITE), 1.0);
        assert!(zero.indices.is_empty());
    }

    /// The dashes `stroke` cuts `path` into at `scale`, each as its points and
    /// whether it is closed.
    fn dashes_of(path: &Path, stroke: &Stroke, scale: f32) -> Vec<(Vec<[f32; 2]>, bool)> {
        let lyon = dash(
            &to_lyon(path, scale),
            stroke.dashes.as_ref().unwrap(),
            scale,
            &mut DashScratch::default(),
        )
        .expect("within the interval limit");
        let mut out = Vec::new();
        for event in lyon.iter() {
            match event {
                PathEvent::Begin { at } => out.push((vec![at.to_array()], false)),
                PathEvent::Line { to, .. } => out.last_mut().unwrap().0.push(to.to_array()),
                PathEvent::End { close, .. } => out.last_mut().unwrap().1 = close,
                _ => unreachable!("a dash is straight lines"),
            }
        }
        out
    }

    fn line(length: f32) -> Path {
        Path::new().move_to((0.0, 0.0)).line_to((length, 0.0))
    }

    fn close(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(p, q)| (p[0] - q[0]).abs() < 1e-4 && (p[1] - q[1]).abs() < 1e-4)
    }

    /// The dashes, in any order: the first dash of a subpath is held back
    /// until its end says whether the last one runs into it, and a mesh's
    /// triangles are one colour whatever order they come in.
    fn assert_dashes(got: &[(Vec<[f32; 2]>, bool)], want: &[(&[[f32; 2]], bool)]) {
        assert!(
            got.len() == want.len()
                && want
                    .iter()
                    .all(|(w, wc)| got.iter().any(|(g, gc)| gc == wc && close(g, w))),
            "dashed into {got:?}, not {want:?}"
        );
    }

    /// The pattern is laid along the line from its start, and the offset is
    /// how far into the pattern that start is.
    #[test]
    fn a_line_is_cut_where_its_pattern_says() {
        let stroke = Stroke::new(1.0, Color::WHITE);
        assert_dashes(
            &dashes_of(&line(20.0), &stroke.clone().dashes(&[4.0, 4.0], 0.0), 1.0),
            &[
                (&[[0.0, 0.0], [4.0, 0.0]], false),
                (&[[8.0, 0.0], [12.0, 0.0]], false),
                (&[[16.0, 0.0], [20.0, 0.0]], false),
            ],
        );
        assert_dashes(
            &dashes_of(&line(20.0), &stroke.dashes(&[4.0, 4.0], 2.0), 1.0),
            &[
                (&[[0.0, 0.0], [2.0, 0.0]], false),
                (&[[6.0, 0.0], [10.0, 0.0]], false),
                (&[[14.0, 0.0], [18.0, 0.0]], false),
            ],
        );
    }

    /// The pattern is in logical pixels, like the path, so a scale cuts the
    /// same dashes into a larger path.
    #[test]
    fn the_pattern_scales_with_the_path() {
        let stroke = Stroke::new(1.0, Color::WHITE).dashes(&[4.0, 4.0], 2.0);
        assert_dashes(
            &dashes_of(&line(10.0), &stroke, 2.0),
            &[
                (&[[0.0, 0.0], [4.0, 0.0]], false),
                (&[[12.0, 0.0], [20.0, 0.0]], false),
            ],
        );
    }

    /// A dash that runs round a corner keeps it, so the stroker joins it there
    /// rather than cutting the corner off.
    #[test]
    fn a_dash_keeps_the_corners_it_turns() {
        let corner = Path::new()
            .move_to((0.0, 0.0))
            .line_to((10.0, 0.0))
            .line_to((10.0, 10.0));
        let stroke = Stroke::new(1.0, Color::WHITE).dashes(&[14.0, 2.0], 0.0);
        assert_dashes(
            &dashes_of(&corner, &stroke, 1.0),
            &[
                (&[[0.0, 0.0], [10.0, 0.0], [10.0, 4.0]], false),
                (&[[10.0, 6.0], [10.0, 10.0]], false),
            ],
        );
    }

    /// On a closed subpath the dash that is on when the path comes back to
    /// its start is the dash that began there: one dash, through the corner.
    #[test]
    fn a_dash_through_the_start_of_a_closed_path_is_one_dash() {
        // A perimeter of 40 is four periods, so the pattern is where it began
        // when the square closes: three pixels into a dash.
        let stroke = Stroke::new(1.0, Color::WHITE).dashes(&[6.0, 4.0], 3.0);
        let dashes = dashes_of(&square(10.0), &stroke, 1.0);
        assert_eq!(dashes.len(), 4, "{dashes:?}");
        let through = dashes.last().unwrap();
        assert!(
            close(&through.0, &[[0.0, 3.0], [0.0, 0.0], [3.0, 0.0]]),
            "{dashes:?}"
        );
    }

    /// A dash longer than the subpath it is laid on leaves it as it was,
    /// closed, with joins all the way round and no cap anywhere.
    #[test]
    fn a_dash_longer_than_a_closed_path_leaves_it_closed() {
        let stroke = Stroke::new(1.0, Color::WHITE).dashes(&[100.0, 4.0], 0.0);
        assert_dashes(
            &dashes_of(&square(10.0), &stroke, 1.0),
            &[(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], true)],
        );
    }

    /// The pattern starts again on each subpath.
    #[test]
    fn each_subpath_starts_the_pattern_again() {
        let two = line(6.0).move_to((0.0, 10.0)).line_to((6.0, 10.0));
        let stroke = Stroke::new(1.0, Color::WHITE).dashes(&[4.0, 4.0], 0.0);
        assert_dashes(
            &dashes_of(&two, &stroke, 1.0),
            &[
                (&[[0.0, 0.0], [4.0, 0.0]], false),
                (&[[0.0, 10.0], [4.0, 10.0]], false),
            ],
        );
    }

    /// A dash of no length with round caps is a dot; with butt caps it is
    /// nothing.
    #[test]
    fn a_dash_of_no_length_is_a_dot_with_round_caps() {
        let dotted = Stroke::new(4.0, Color::WHITE).dashes(&[0.0, 10.0], 0.0);
        let round =
            Tessellator::default().stroke(&line(25.0), &dotted.clone().caps(Cap::Round), 1.0);
        let area = painted_area(&round, [-4.0, -4.0], [30.0, 4.0], 0.05);
        let dots = 3.0 * std::f32::consts::PI * 4.0;
        assert!(
            (area - dots).abs() < dots * 0.1,
            "three dots of radius 2 cover {dots}, and the mesh painted {area}"
        );
        let butt = Tessellator::default().stroke(&line(25.0), &dotted, 1.0);
        let area = painted_area(&butt, [-4.0, -4.0], [30.0, 4.0], 0.05);
        assert!(area < 0.5, "dashes of no length painted {area}");
    }

    /// A pattern of no length, or one no length could be laid out by, draws
    /// the line solid — the mesh a stroke with no pattern draws.
    #[test]
    fn a_pattern_of_no_length_draws_the_line_solid() {
        let solid = Stroke::new(2.0, Color::WHITE);
        let expected = Tessellator::default().stroke(&square(10.0), &solid, 1.0);
        for pattern in [&[][..], &[0.0, 0.0], &[0.0], &[4.0, -1.0], &[f32::NAN, 2.0]] {
            let stroke = solid.clone().dashes(pattern, 3.0);
            assert_eq!(stroke.dashes, None, "{pattern:?} was kept");
            let mesh = Tessellator::default().stroke(&square(10.0), &stroke, 1.0);
            assert_eq!(mesh.vertices, expected.vertices, "{pattern:?}");
            assert_eq!(mesh.indices, expected.indices, "{pattern:?}");
        }
    }

    /// A pattern too fine for the path it is laid along — more intervals than
    /// [`MOST_INTERVALS`] — is drawn solid rather than walked for ever.
    #[test]
    fn a_pattern_too_fine_for_its_path_draws_it_solid() {
        let fine = Stroke::new(1.0, Color::WHITE).dashes(&[1e-3, 1e-3], 0.0);
        let lyon = to_lyon(&line(1000.0), 1.0);
        let scratch = &mut DashScratch::default();
        assert!(dash(&lyon, fine.dashes.as_ref().unwrap(), 1.0, scratch).is_none());
        let mesh = Tessellator::default().stroke(&line(1000.0), &fine, 1.0);
        let solid =
            Tessellator::default().stroke(&line(1000.0), &Stroke::new(1.0, Color::WHITE), 1.0);
        assert_eq!(mesh.indices, solid.indices);
    }
}
