//! How a widget is moved, turned and resized, and the matrix that carries the
//! result to the GPU.
//!
//! The three are separate values because that is the only form in which they
//! can be animated. A matrix cannot say how far round a turn went: a rotation
//! of 360° and no rotation at all are the same six numbers, so interpolating
//! them is interpolating something that has already lost the answer. Kept
//! apart, a rotation is an angle and an angle is a number.
//!
//! They compose in one order, always — translate, then rotate, then scale —
//! which is the order CSS fixed for the same three properties, and for the
//! same reason: with no declared order, two declarations that read the same
//! would mean different things.

/// How far a widget is displaced, in logical pixels.
///
/// Unaffected by [`Pivot`](crate::pivot::Pivot): moving a box is the same
/// movement wherever the pivot sits.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Translate {
    /// Rightward displacement.
    pub x: f32,
    /// Downward displacement.
    pub y: f32,
}

impl Translate {
    /// No displacement.
    pub const NONE: Self = Self { x: 0.0, y: 0.0 };

    /// A displacement of `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// How much a widget is resized, as a factor per axis.
///
/// A bare number scales both axes; a pair scales them apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    /// Horizontal factor. `1.0` is unscaled.
    pub x: f32,
    /// Vertical factor. `1.0` is unscaled.
    pub y: f32,
}

impl Scale {
    /// Unscaled.
    pub const NONE: Self = Self { x: 1.0, y: 1.0 };

    /// The same factor on both axes.
    pub const fn uniform(factor: f32) -> Self {
        Self {
            x: factor,
            y: factor,
        }
    }

    /// A factor per axis.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::NONE
    }
}

macro_rules! from_pairs {
    ($t:ty $(, $n:ty)*) => {$(
        impl From<($n, $n)> for $t {
            fn from((x, y): ($n, $n)) -> Self {
                Self { x: x as f32, y: y as f32 }
            }
        }
        impl From<[$n; 2]> for $t {
            fn from([x, y]: [$n; 2]) -> Self {
                Self { x: x as f32, y: y as f32 }
            }
        }
    )*};
}
from_pairs!(Translate, f32, f64, i32, u32);
from_pairs!(Scale, f32, f64, i32, u32);

macro_rules! scale_from_scalar {
    ($($n:ty),*) => {$(
        impl From<$n> for Scale {
            fn from(factor: $n) -> Self {
                Self::uniform(factor as f32)
            }
        }
    )*};
}
scale_from_scalar!(f32, f64, i32, u32);

// Every conversion a scale or a translation accepts, declared once. `From`
// covers the constants, above; this covers the closure and the three signal
// forms, so the same expression compiles in every position.
crate::reactive::converts!(
    f32 => Scale,
    f64 => Scale,
    i32 => Scale,
    u32 => Scale,
    (f32, f32) => Scale,
    (f64, f64) => Scale,
    (i32, i32) => Scale,
    (u32, u32) => Scale,
    [f32; 2] => Scale,
    [f64; 2] => Scale,
    [i32; 2] => Scale,
    [u32; 2] => Scale,
    (f32, f32) => Translate,
    (f64, f64) => Translate,
    (i32, i32) => Translate,
    (u32, u32) => Translate,
    [f32; 2] => Translate,
    [f64; 2] => Translate,
    [i32; 2] => Translate,
    [u32; 2] => Translate,
);

/// Below this a determinant is zero: the transform has collapsed the plane
/// onto a line or a point, and cannot be undone.
const DEGENERATE: f32 = 1e-10;

/// A 2D affine transformation.
///
/// Not part of the public vocabulary: an application says `translate`,
/// `rotate` and `scale`, and this is what those compose into on the way to the
/// renderer. It is reachable from [`widget_prelude`](crate::widget_prelude) so
/// that a widget written outside the crate can position what it paints.
///
/// Stored as 6 floats in the layout `[a, b, tx, c, d, ty]`, representing:
///
/// ```text
/// | a  b  tx |   (x maps to a*x + b*y + tx)
/// | c  d  ty |   (y maps to c*x + d*y + ty)
/// ```
///
/// This is exactly the data the GPU shader consumes. The previous
/// implementation stored a full 4×4 matrix (64 bytes) of which 8 of the 16
/// floats were structurally constant; the affine form is 24 bytes, composes
/// with 6 multiplies per output element instead of a 4×4 multiply, and cuts
/// the size of every render node and flattened command that embeds one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Affine coefficients: `[a, b, tx, c, d, ty]`
    pub data: [f32; 6],
}

impl Transform {
    /// Identity transform (no transformation)
    pub const IDENTITY: Self = Self {
        data: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    };

    /// The three declared components, composed: translate, then rotate, then
    /// scale.
    ///
    /// Folded into the affine form directly rather than built by two `then`
    /// calls, since two of the three are known to be sparse. The pivot is not
    /// applied here — [`center_at`](Self::center_at) does that, from both of
    /// the places that need the composed matrix: `flatten_node` on the way to
    /// the renderer, and `untransform_point` on the way back for a pointer
    /// event.
    ///
    /// It very much changes the rotate and scale halves — turning about a
    /// corner is not turning about the centre, and that is the whole point of
    /// it. What survives it untouched is the *translate* component, because
    /// `C·T·R·S·C⁻¹` is `T·(C·R·S·C⁻¹)`: a translation commutes past the
    /// conjugating one. Which is why a `Pivot` moves what `rotate` and `scale`
    /// do and does nothing at all to `translate`.
    pub fn compose(translate: Translate, rotate_degrees: f32, scale: Scale) -> Self {
        // Not about correctness: this runs for every container on every paint
        // and every pointer event, and almost none of them turn.
        let (sin, cos) = if rotate_degrees == 0.0 {
            (0.0, 1.0)
        } else {
            rotate_degrees.to_radians().sin_cos()
        };

        Self {
            data: [
                cos * scale.x,
                -sin * scale.y,
                translate.x,
                sin * scale.x,
                cos * scale.y,
                translate.y,
            ],
        }
    }

    /// Create a translation transform
    pub fn translate(x: f32, y: f32) -> Self {
        Self {
            data: [1.0, 0.0, x, 0.0, 1.0, y],
        }
    }

    /// Create a rotation transform around the Z axis (2D rotation)
    pub fn rotate(angle_radians: f32) -> Self {
        let cos = angle_radians.cos();
        let sin = angle_radians.sin();
        Self {
            data: [cos, -sin, 0.0, sin, cos, 0.0],
        }
    }

    /// Create a rotation transform from degrees
    pub fn rotate_degrees(angle_degrees: f32) -> Self {
        Self::rotate(angle_degrees.to_radians())
    }

    /// Create a uniform scale transform
    pub fn scale(s: f32) -> Self {
        Self::scale_xy(s, s)
    }

    /// Create a non-uniform scale transform
    pub fn scale_xy(sx: f32, sy: f32) -> Self {
        Self {
            data: [sx, 0.0, 0.0, 0.0, sy, 0.0],
        }
    }

    /// The `a` (x-from-x) coefficient.
    #[inline]
    pub fn a(&self) -> f32 {
        self.data[0]
    }

    /// The `b` (x-from-y) coefficient.
    #[inline]
    pub fn b(&self) -> f32 {
        self.data[1]
    }

    /// The `c` (y-from-x) coefficient.
    #[inline]
    pub fn c(&self) -> f32 {
        self.data[3]
    }

    /// The `d` (y-from-y) coefficient.
    #[inline]
    pub fn d(&self) -> f32 {
        self.data[4]
    }

    /// Create a transform that applies this transform centered around a point.
    ///
    /// This is equivalent to: translate(cx, cy) * self * translate(-cx, -cy)
    /// Which means: move to origin, apply transform, move back.
    ///
    /// Useful for rotating or scaling around a specific point rather than the origin.
    pub fn center_at(self, cx: f32, cy: f32) -> Self {
        // Directly fold the two translations into the affine form instead of
        // composing three transforms: only the translation column changes.
        let [a, b, tx, c, d, ty] = self.data;
        Self {
            data: [
                a,
                b,
                tx + cx - (a * cx + b * cy),
                c,
                d,
                ty + cy - (c * cx + d * cy),
            ],
        }
    }

    /// Then translate by `(x, y)`.
    ///
    /// The chainers read in the order CSS `transform` reads: each one applies
    /// in the frame the previous ones left, so
    /// `Transform::translate(10.0, 0.0).then_rotate(45.0)` moves and then
    /// turns about where it has moved to.
    ///
    /// In matrix terms each is a right-multiplication — `self * other`, which
    /// is [`then`](Self::then) — so the *point* meets them in the reverse
    /// order. The two readings are the same movement described from opposite
    /// ends; the one above is the one the call site is written in.
    pub fn then_translate(self, x: f32, y: f32) -> Self {
        self.then(&Self::translate(x, y))
    }

    /// Then rotate by `degrees`.
    pub fn then_rotate(self, degrees: f32) -> Self {
        self.then(&Self::rotate_degrees(degrees))
    }

    /// Then scale uniformly by `factor`.
    pub fn then_scale(self, factor: f32) -> Self {
        self.then(&Self::scale(factor))
    }

    /// Then scale each axis.
    pub fn then_scale_xy(self, x: f32, y: f32) -> Self {
        self.then(&Self::scale_xy(x, y))
    }

    /// Compose this transform with another: self * other
    /// Applies `other` first, then `self`.
    pub fn then(&self, other: &Transform) -> Transform {
        let [a1, b1, tx1, c1, d1, ty1] = self.data;
        let [a2, b2, tx2, c2, d2, ty2] = other.data;
        Transform {
            data: [
                a1 * a2 + b1 * c2,
                a1 * b2 + b1 * d2,
                a1 * tx2 + b1 * ty2 + tx1,
                c1 * a2 + d1 * c2,
                c1 * b2 + d1 * d2,
                c1 * tx2 + d1 * ty2 + ty1,
            ],
        }
    }

    /// Compute the inverse of this transform, or `None` where there is none.
    ///
    /// A scale of zero on either axis squashes the plane onto a line or a
    /// point, and the map stops being one-to-one: every point in the box lands
    /// on top of every other, so "which point did this come from" has
    /// infinitely many answers and the arithmetic divides by zero.
    ///
    /// This used to answer the identity there, on the grounds that it was the
    /// least wrong finite answer. It was not: the caller that undoes a
    /// transform before hit testing read it as "nothing to undo" and compared
    /// the point against the *uncollapsed* bounds, so an invisible subtree
    /// answered for the whole area it covers when it is open. Saying `None`
    /// hands the caller a decision instead of a wrong number — which is how
    /// Qt spells it too, where `inverted` sets an `invertible` flag beside the
    /// identity it returns.
    pub fn inverse(&self) -> Option<Transform> {
        let [a, b, tx, c, d, ty] = self.data;

        let det = a * d - b * c;
        if det.abs() < DEGENERATE {
            return None;
        }

        let inv_det = 1.0 / det;

        Some(Transform {
            data: [
                d * inv_det,
                -b * inv_det,
                (-d * tx + b * ty) * inv_det,
                -c * inv_det,
                a * inv_det,
                (c * tx - a * ty) * inv_det,
            ],
        })
    }

    /// This transform as it acts about `pivot` within `bounds`.
    ///
    /// A transform is written about the origin and applied about a pivot, and
    /// resolving the one into the other is what `flatten` does before it draws
    /// anything. Everything that reasons about transformed geometry ahead of
    /// the draw — where a clip lands, how far a box can move — has to resolve
    /// it the same way, so there is one function rather than four
    /// transcriptions agreeing by prose.
    #[inline]
    pub fn about(&self, pivot: crate::pivot::Pivot, bounds: crate::widgets::Rect) -> Self {
        let (px, py) = pivot.resolve(bounds);
        self.center_at(px, py)
    }

    /// The smallest axis-aligned rect containing `rect` once this transform
    /// has been applied to it.
    ///
    /// Under rotation the image of a rect is not a rect, so this is the
    /// enclosing box: bigger than the shape, never smaller. Everything that
    /// asks a rect question about transformed geometry — where a clip lands,
    /// which children a viewport can reach — wants that answer rather than an
    /// exact one, because being too generous only costs work while being too
    /// mean loses pixels.
    #[inline]
    pub fn map_rect(&self, rect: crate::widgets::Rect) -> crate::widgets::Rect {
        let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
        let corners = [
            self.transform_point(rect.x, rect.y),
            self.transform_point(x1, rect.y),
            self.transform_point(rect.x, y1),
            self.transform_point(x1, y1),
        ];
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (x, y) in corners {
            (min_x, min_y) = (min_x.min(x), min_y.min(y));
            (max_x, max_y) = (max_x.max(x), max_y.max(y));
        }
        crate::widgets::Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// Transform a 2D point by this transform
    #[inline]
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, tx, c, d, ty] = self.data;
        (a * x + b * y + tx, c * x + d * y + ty)
    }

    /// Check if this is the identity transform
    #[inline]
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// This placement undone, for a consumer working in physical pixels while
    /// the space it places is written in logical ones.
    ///
    /// The surface scale multiplies the world *after* the placement has been
    /// applied, so undoing it is this transform's inverse composed with a
    /// shrink — which reaches the matrix and not the offset. Whatever is
    /// written in the placed space keeps the logical units it was written in,
    /// and scaling that as well would apply the surface scale twice.
    ///
    /// `None` where there is no inverse: a collapsed placement maps every point
    /// onto a line, and nothing can be carried back through it.
    ///
    /// The identity is most placements in most frames, and taking it the long
    /// way is a reciprocal and twelve multiplies to arrive back at a shrink.
    #[inline]
    pub fn physical_inverse(&self, scale: f32) -> Option<Self> {
        if self.is_identity() {
            return Some(Self::scale(1.0 / scale));
        }
        Some(self.inverse()?.then_scale(1.0 / scale))
    }

    /// Whether this sends a rect to a rect, however that rect is turned about.
    ///
    /// Wider than [`keeps_axes`](Self::keeps_axes) by the two cases that swap
    /// or flip the axes rather than leaning one into the other: a quarter turn,
    /// where `a == 0 && d == 0`, and a mirror, where a factor is negative. The
    /// image is still a rect in both — it is the *corners* that move, and a
    /// caller that can follow them can use this where `keeps_axes` would have
    /// given up.
    ///
    /// Skia draws the same line and calls it `rectStaysRect`: identity, scale,
    /// mirror and multiples of 90°, with `SkRRect::transform` cycling the
    /// corner radii for a quarter turn and swapping them for a flip. WebRender
    /// allows the mirror and still sends a quarter turn to a clip mask; GTK4's
    /// GSK stops at translate plus axis-aligned scale and rasterises a mask for
    /// everything else. Guido has no mask, so the wider predicate is the one
    /// that buys something.
    ///
    /// A degenerate map — either axis collapsed — is not one of these: the
    /// image is a segment or a point, and `false` sends the caller to whatever
    /// it does when there is no answer.
    ///
    /// Unlike [`keeps_axes`](Self::keeps_axes) this is not exact, and it cannot
    /// be: `cos(90°)` in floating point is 6e-17 and never 0, so an exact test
    /// would answer `false` to every quarter turn anybody can actually build
    /// and the wider half of this predicate would be unreachable. The tolerance
    /// is relative — each row against its own magnitude, so an uneven scale is
    /// judged on its shape rather than on its size — and it bounds the error
    /// rather than hiding it: a map this accepts is within `1e-6` of the
    /// rect-preserving one it is treated as, so a clip is out by at most a
    /// millionth of its own coordinates. `rotate_degrees(89.0)` and `91.0` are
    /// refused.
    #[inline]
    pub fn rect_stays_rect(&self) -> bool {
        let [a, b, _, c, d, _] = self.data;
        // Each row against its own magnitude, not the matrix's. Against the
        // matrix's, a wildly uneven scale — `scale_xy(2000.0, 0.001)` — would
        // find its own smaller axis inside the tolerance and be refused, which
        // would make this narrower than `keeps_axes` at the extremes while
        // claiming to be wider.
        let dominates = |big: f32, small: f32| {
            let magnitude = big.abs().max(small.abs());
            magnitude.is_finite() && magnitude > 0.0 && small.abs() <= magnitude * 1e-6
        };
        let upright = dominates(a, b) && dominates(d, c);
        let quarter_turn = dominates(b, a) && dominates(c, d);
        upright || quarter_turn
    }

    /// Where this map sends each corner of a rect, as indices into
    /// [`CornerRadii`](crate::renderer::CornerRadii)'s fixed order — top-left,
    /// top-right, bottom-right, bottom-left.
    ///
    /// `permutation()[i]` is where corner `i` lands. Only meaningful when
    /// [`rect_stays_rect`](Self::rect_stays_rect) holds; otherwise the corners
    /// are not corners of an axis-aligned rect any more and there is nothing to
    /// permute them into.
    ///
    /// Read off the map rather than enumerated: each corner is a sign pair, the
    /// linear part sends it to another sign pair, and that names the
    /// destination. Enumerating the eight cases by hand is where a table gets
    /// one entry wrong and a rounded corner appears on the wrong side of a
    /// mirrored scroller.
    pub fn corner_permutation(&self) -> [usize; 4] {
        // Top-left, top-right, bottom-right, bottom-left as signs of (x, y)
        // measured from the rect's centre.
        const CORNERS: [(f32, f32); 4] = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let [a, b, _, c, d, _] = self.data;
        CORNERS.map(|(sx, sy)| {
            let x = a * sx + b * sy;
            let y = c * sx + d * sy;
            CORNERS
                .iter()
                .position(|&(tx, ty)| (tx > 0.0) == (x > 0.0) && (ty > 0.0) == (y > 0.0))
                .expect("a sign pair is one of the four corners")
        })
    }

    /// Whether this keeps a rect a rect *the same way round* — no turn, no
    /// skew, no flip.
    ///
    /// Narrower than [`rect_stays_rect`](Self::rect_stays_rect) on purpose, and
    /// the two are not interchangeable. This is the question a fast path asks
    /// before assuming a shape is still upright: `PlacedShape::is_a_box` and
    /// the arc solve both read a corner's radius straight out of the slot it
    /// was written in, which a quarter turn or a mirror would invalidate even
    /// though the rect survives them. Widening this would break those quietly.
    ///
    /// Exact rather than epsilon'd, unlike
    /// [`is_translation_only`](Self::is_translation_only) and unlike
    /// `rect_stays_rect`, which cannot be: the maps this accepts have `b` and
    /// `c` exactly zero, so there is no rounding to absorb.
    #[inline]
    pub fn keeps_axes(&self) -> bool {
        let [a, b, _, c, d, _] = self.data;
        b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0
    }

    /// Whether this moves without turning, scaling or skewing — so that
    /// undoing it is negating its two components and nothing more.
    #[inline]
    pub fn is_pure_translation(&self) -> bool {
        let [a, b, _, c, d, _] = self.data;
        a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0
    }

    /// Get the X translation component
    #[inline]
    pub fn tx(&self) -> f32 {
        self.data[2]
    }

    /// Get the Y translation component
    #[inline]
    pub fn ty(&self) -> f32 {
        self.data[5]
    }

    /// The same transform, moved by `(dx, dy)` in the space it maps *into*.
    ///
    /// Not [`then_translate`](Self::then_translate), which is the other side:
    /// that one translates in the transform's own space, so under a rotation it
    /// moves along the rotated axes. This one adds to the translation the
    /// matrix already carries, which is the displacement applied last — what a
    /// caller shifting something across the surface means.
    #[inline]
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        let [a, b, tx, c, d, ty] = self.data;
        Self {
            data: [a, b, tx + dx, c, d, ty + dy],
        }
    }

    /// How far the image of a unit circle reaches horizontally and vertically
    /// in world space — `sqrt(a² + b²)` and `sqrt(c² + d²)`, the norms of the
    /// two **rows**.
    ///
    /// This is not "the scale the transform was built from", and reading it
    /// that way is how it has been got wrong twice. A caller pairs the result
    /// with a world-space axis-aligned box, where the question is how wide and
    /// how tall a corner becomes rather than how much each of the transform's
    /// own axes was stretched. Under a rotation those are different numbers.
    ///
    /// Rows answer it exactly, for any affine and any composition order,
    /// because the image of the unit circle is
    /// `{ (a·cosθ + b·sinθ, c·cosθ + d·sinθ) }` and the extreme of each
    /// coordinate is the norm of its row, by Cauchy–Schwarz. Nothing here is
    /// an approximation and nothing depends on how the matrix was arrived at.
    ///
    /// The two plausible alternatives are both wrong, measured against the
    /// image of the circle for `rotate(45°)` with `scale((2.0, 0.5))`, where
    /// the truth is `(1.458, 1.458)`:
    ///
    /// | | rot45 then scale | scale then rot45 | rot90 then scale |
    /// |---|---|---|---|
    /// | truth | (1.458, 1.458) | (2.0, 0.5) | (0.5, 2.0) |
    /// | rows | **(1.458, 1.458)** | **(2.0, 0.5)** | **(0.5, 2.0)** |
    /// | columns | (2.0, 0.5) | (1.458, 1.458) | (2.0, 0.5) |
    /// | singular values | (2.0, 0.5) | (2.0, 0.5) | (2.0, 0.5) |
    ///
    /// The singular values are the ellipse's own semi-axes, which is a true
    /// fact about the shape and the wrong question: they are ordered by size
    /// rather than by axis, so they transpose the corner for a quarter turn and
    /// for any scale with `sy > sx`.
    pub(crate) fn extract_scale_components(&self) -> (f32, f32) {
        let (a, b, c, d) = (self.a(), self.b(), self.c(), self.d());
        ((a * a + b * b).sqrt(), (c * c + d * d).sqrt())
    }

    /// One scale factor for a transform that may not have exactly one: the
    /// geometric mean of the two axes.
    pub(crate) fn extract_scale(&self) -> f32 {
        let (sx, sy) = self.extract_scale_components();
        (sx * sy).sqrt()
    }

    /// Check if this transform contains only translation (no rotation or scale).
    pub fn is_translation_only(&self) -> bool {
        let (a, b, c, d) = (self.a(), self.b(), self.c(), self.d());
        // For pure translation: a=1, b=0, c=0, d=1
        (a - 1.0).abs() < 1e-6 && b.abs() < 1e-6 && c.abs() < 1e-6 && (d - 1.0).abs() < 1e-6
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wider than `keeps_axes` by exactly the two cases that move corners
    /// without leaning one axis into the other.
    #[test]
    fn a_quarter_turn_and_a_mirror_still_send_a_rect_to_a_rect() {
        assert!(Transform::IDENTITY.rect_stays_rect());
        assert!(Transform::translate(3.0, -9.0).rect_stays_rect());
        assert!(Transform::scale_xy(0.5, 3.0).rect_stays_rect());

        for degrees in [90.0, 180.0, 270.0, -90.0] {
            assert!(
                Transform::rotate_degrees(degrees).rect_stays_rect(),
                "{degrees}° is a quarter turn or a half one"
            );
        }
        assert!(Transform::scale_xy(-1.0, 1.0).rect_stays_rect());
        assert!(Transform::scale_xy(1.0, -1.0).rect_stays_rect());
        assert!(Transform::scale_xy(-2.0, -3.0).rect_stays_rect());
        assert!(
            Transform::rotate_degrees(90.0)
                .then(&Transform::scale_xy(-1.0, 1.0))
                .rect_stays_rect(),
            "a quarter turn and a mirror together is a transpose, still a rect"
        );

        // Genuinely wider than `keeps_axes`, including at the extremes: an
        // uneven scale is judged on its shape, not on its size.
        let lopsided = Transform::scale_xy(2000.0, 0.001);
        assert!(lopsided.keeps_axes());
        assert!(
            lopsided.rect_stays_rect(),
            "a row compared against the whole matrix would find 0.001 inside \
             the tolerance and refuse what `keeps_axes` accepts"
        );

        // What it still refuses, and has to: no reordering of four corners
        // makes these an axis-aligned rect again.
        for degrees in [30.0, 45.0, 89.0, 91.0] {
            assert!(
                !Transform::rotate_degrees(degrees).rect_stays_rect(),
                "{degrees}° is not a quarter turn"
            );
        }
        // Collapsed: the image is a segment, not a rect.
        assert!(!Transform::scale_xy(0.0, 1.0).rect_stays_rect());
        assert!(!Transform::scale_xy(1.0, 0.0).rect_stays_rect());
        assert!(!Transform { data: [0.0; 6] }.rect_stays_rect());

        // Half of a quarter turn and half of a skew. Each row has to answer the
        // same way: one row looking like a turn while the other leans is a
        // shear, and a shear sends a rect to a parallelogram. Written because
        // nothing else here has a matrix where the two rows disagree — every
        // named constructor makes them agree, so the conjunction went untested
        // and `||` would have passed the whole suite.
        for data in [
            [0.0, 1.0, 0.0, 1.0, 1.0, 0.0],
            [1.0, 1.0, 0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
        ] {
            assert!(
                !Transform { data }.rect_stays_rect(),
                "one row turned and the other leaning is a shear: {data:?}"
            );
        }
    }

    /// The permutation says where each corner lands, in `CornerRadii`'s order:
    /// top-left, top-right, bottom-right, bottom-left.
    #[test]
    fn the_corner_permutation_follows_the_turn() {
        assert_eq!(
            Transform::IDENTITY.corner_permutation(),
            [0, 1, 2, 3],
            "nothing moves"
        );
        assert_eq!(
            Transform::scale(4.0).corner_permutation(),
            [0, 1, 2, 3],
            "a scale grows the corners and does not move them"
        );
        assert_eq!(
            Transform::rotate_degrees(90.0).corner_permutation(),
            [1, 2, 3, 0],
            "a quarter turn clockwise: top-left becomes top-right"
        );
        assert_eq!(
            Transform::rotate_degrees(-90.0).corner_permutation(),
            [3, 0, 1, 2],
            "and the other way round"
        );
        assert_eq!(
            Transform::rotate_degrees(180.0).corner_permutation(),
            [2, 3, 0, 1],
            "a half turn sends every corner to its opposite"
        );
        assert_eq!(
            Transform::scale_xy(-1.0, 1.0).corner_permutation(),
            [1, 0, 3, 2],
            "mirrored across x: the left corners swap with the right ones"
        );
        assert_eq!(
            Transform::scale_xy(1.0, -1.0).corner_permutation(),
            [3, 2, 1, 0],
            "and across y, the top with the bottom"
        );
        assert_eq!(
            Transform::rotate_degrees(90.0)
                .then(&Transform::scale_xy(-1.0, 1.0))
                .corner_permutation(),
            [2, 1, 0, 3],
            "a turn and a mirror together: the reflection across y = -x, which \
             holds two corners still and swaps the other two"
        );
    }

    /// Applying the permutation twice for a quarter turn is a half turn, four
    /// times is the identity. A table with one entry wrong fails this and a
    /// spot check of one angle does not.
    #[test]
    fn four_quarter_turns_bring_every_corner_home() {
        use crate::renderer::CornerRadii;

        let start = CornerRadii {
            top_left: 1.0,
            top_right: 2.0,
            bottom_right: 3.0,
            bottom_left: 4.0,
        };
        let quarter = Transform::rotate_degrees(90.0).corner_permutation();

        let mut radii = start;
        for _ in 0..4 {
            radii = radii.permuted(quarter);
        }
        assert_eq!(radii, start, "four quarter turns is where you started");

        let half = start.permuted(quarter).permuted(quarter);
        assert_eq!(
            half,
            start.permuted(Transform::rotate_degrees(180.0).corner_permutation()),
            "two quarters is a half"
        );
    }

    /// Which transforms let a clip keep its shape when it is rewritten in
    /// another clip's coordinates — and, just as load-bearing, which do not.
    ///
    /// A quarter turn and a flip both send a rect to a rect, so a predicate
    /// about *rects* would accept them. This one is about corners: each moves
    /// the top-left corner somewhere else, and `CornerRadii` is four numbers in
    /// a fixed order that nothing permutes. Accepting them would rebase the
    /// rect correctly and leave the radii on the wrong corners, which is worse
    /// than the box the caller falls back to. See #397.
    #[test]
    fn keeping_the_axes_is_about_corners_and_not_only_rects() {
        assert!(Transform::IDENTITY.keeps_axes());
        assert!(Transform::translate(12.0, -4.0).keeps_axes());
        assert!(Transform::scale(2.0).keeps_axes());
        assert!(Transform::scale_xy(0.5, 3.0).keeps_axes());
        assert!(
            Transform::translate(3.0, 3.0)
                .then(&Transform::scale(0.5))
                .keeps_axes(),
            "a move and a scale together still keep them"
        );

        assert!(
            !Transform::rotate_degrees(30.0).keeps_axes(),
            "the ordinary rotation, where the rect stops being a rect at all"
        );
        assert!(
            !Transform::rotate_degrees(90.0).keeps_axes(),
            "a quarter turn sends a rect to a rect and the top-left corner to \
             the top-right, and the radii do not follow it"
        );
        assert!(
            !Transform::scale_xy(-1.0, 1.0).keeps_axes(),
            "a mirror likewise: the rect survives, the corner order does not"
        );
        assert!(!Transform::scale_xy(1.0, -1.0).keeps_axes());

        // One wrong component at a time: every case above is wrong in more
        // than one, so a predicate that had lost a condition would still
        // answer them correctly on the strength of the others.
        // `data` is `[a, b, tx, c, d, ty]`.
        for (label, data) in [
            ("x leans into y", [1.0, 0.5, 0.0, 0.0, 1.0, 0.0]),
            ("y leans into x", [1.0, 0.0, 0.0, 0.5, 1.0, 0.0]),
            ("x flipped", [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            ("y flipped", [1.0, 0.0, 0.0, 0.0, -1.0, 0.0]),
            // Zero, not merely negative: `>= 0.0` would call a collapsed axis
            // axis-keeping, and a space squashed onto a line has no rect in it
            // to keep. Asked because the mutation job asked it.
            ("x collapsed", [0.0, 0.0, 0.0, 0.0, 1.0, 0.0]),
            ("y collapsed", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        ] {
            assert!(!Transform { data }.keeps_axes(), "{label}");
        }
    }

    /// What the paint cache leans on: it undoes a child's old placement by
    /// negating the two translation components, which is only undoing when
    /// there is nothing else in the matrix. The `debug_assert` in
    /// `reuse_cached` asks this, so a predicate that answered `true` for
    /// everything would take the guard with it.
    #[test]
    fn only_a_move_is_a_pure_translation() {
        assert!(Transform::IDENTITY.is_pure_translation());
        assert!(Transform::translate(12.0, -4.0).is_pure_translation());

        assert!(
            !Transform::scale(2.0).is_pure_translation(),
            "a scale survives negating the translation, which is the bug"
        );
        assert!(!Transform::rotate_degrees(90.0).is_pure_translation());
        assert!(
            !Transform::translate(3.0, 3.0)
                .then(&Transform::scale(0.5))
                .is_pure_translation(),
            "and so does one composed with a move"
        );

        // One wrong component at a time, because every case above has more
        // than one wrong at once — and a predicate that had lost a condition
        // would still answer those correctly on the strength of the others.
        // `data` is `[a, b, tx, c, d, ty]`.
        for (label, data) in [
            ("x scaled", [2.0, 0.0, 7.0, 0.0, 1.0, 9.0]),
            ("sheared along x", [1.0, 0.5, 7.0, 0.0, 1.0, 9.0]),
            ("sheared along y", [1.0, 0.0, 7.0, 0.5, 1.0, 9.0]),
            ("y scaled", [1.0, 0.0, 7.0, 0.0, 2.0, 9.0]),
        ] {
            assert!(
                !Transform { data }.is_pure_translation(),
                "{label}: one component is enough to make undoing it more than \
                 negating the move"
            );
        }
    }

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_identity() {
        let t = Transform::IDENTITY;
        assert_eq!(t, Transform::IDENTITY);
        assert!(t.is_identity());
    }

    #[test]
    fn test_translate() {
        let t = Transform::translate(10.0, 20.0);
        let (x, y) = t.transform_point(0.0, 0.0);
        assert!(approx_eq(x, 10.0));
        assert!(approx_eq(y, 20.0));

        let (x2, y2) = t.transform_point(5.0, 5.0);
        assert!(approx_eq(x2, 15.0));
        assert!(approx_eq(y2, 25.0));
    }

    #[test]
    fn test_rotate() {
        let t = Transform::rotate_degrees(90.0);
        let (x, y) = t.transform_point(1.0, 0.0);
        assert!(approx_eq(x, 0.0));
        assert!(approx_eq(y, 1.0));
    }

    #[test]
    fn test_scale() {
        let t = Transform::scale(2.0);
        let (x, y) = t.transform_point(3.0, 4.0);
        assert!(approx_eq(x, 6.0));
        assert!(approx_eq(y, 8.0));
    }

    #[test]
    fn test_scale_xy() {
        let t = Transform::scale_xy(2.0, 3.0);
        let (x, y) = t.transform_point(1.0, 1.0);
        assert!(approx_eq(x, 2.0));
        assert!(approx_eq(y, 3.0));
    }

    #[test]
    fn test_compose() {
        // Translate then scale
        let translate = Transform::translate(10.0, 0.0);
        let scale = Transform::scale(2.0);

        // scale.then(translate): first translate, then scale
        // Point (0,0) -> translate -> (10,0) -> scale -> (20,0)
        let composed = scale.then(&translate);
        let (x, y) = composed.transform_point(0.0, 0.0);
        assert!(approx_eq(x, 20.0));
        assert!(approx_eq(y, 0.0));
    }

    #[test]
    fn test_inverse_translate() {
        let t = Transform::translate(10.0, 20.0);
        let inv = t.inverse().expect("invertible");
        let composed = t.then(&inv);

        // Should be identity
        let (x, y) = composed.transform_point(5.0, 7.0);
        assert!(approx_eq(x, 5.0));
        assert!(approx_eq(y, 7.0));
    }

    #[test]
    fn test_inverse_rotate() {
        let t = Transform::rotate_degrees(45.0);
        let inv = t.inverse().expect("invertible");
        let composed = t.then(&inv);

        let (x, y) = composed.transform_point(3.0, 4.0);
        assert!(approx_eq(x, 3.0));
        assert!(approx_eq(y, 4.0));
    }

    #[test]
    fn test_inverse_scale() {
        let t = Transform::scale(2.0);
        let inv = t.inverse().expect("invertible");
        let composed = t.then(&inv);

        let (x, y) = composed.transform_point(3.0, 4.0);
        assert!(approx_eq(x, 3.0));
        assert!(approx_eq(y, 4.0));
    }

    #[test]
    fn test_center_at_rotation() {
        // Rotate 90 degrees around point (10, 10)
        let t = Transform::rotate_degrees(90.0).center_at(10.0, 10.0);

        // Point at center should stay at center
        let (x, y) = t.transform_point(10.0, 10.0);
        assert!(approx_eq(x, 10.0));
        assert!(approx_eq(y, 10.0));

        // Point (11, 10) should rotate to (10, 11) - 1 unit right becomes 1 unit up
        let (x2, y2) = t.transform_point(11.0, 10.0);
        assert!(approx_eq(x2, 10.0));
        assert!(approx_eq(y2, 11.0));

        // Point (10, 11) should rotate to (9, 10) - 1 unit up becomes 1 unit left
        let (x3, y3) = t.transform_point(10.0, 11.0);
        assert!(approx_eq(x3, 9.0));
        assert!(approx_eq(y3, 10.0));
    }

    #[test]
    fn test_center_at_scale() {
        // Scale 2x around point (5, 5)
        let t = Transform::scale(2.0).center_at(5.0, 5.0);

        // Point at center should stay at center
        let (x, y) = t.transform_point(5.0, 5.0);
        assert!(approx_eq(x, 5.0));
        assert!(approx_eq(y, 5.0));

        // Point (6, 5) should scale to (7, 5) - 1 unit from center becomes 2 units
        let (x2, y2) = t.transform_point(6.0, 5.0);
        assert!(approx_eq(x2, 7.0));
        assert!(approx_eq(y2, 5.0));

        // Point (3, 3) should scale to (1, 1) - 2 units from center in each axis becomes 4
        let (x3, y3) = t.transform_point(3.0, 3.0);
        assert!(approx_eq(x3, 1.0));
        assert!(approx_eq(y3, 1.0));
    }

    #[test]
    fn test_center_at_identity() {
        // Identity centered at any point should still be identity
        let t = Transform::IDENTITY.center_at(100.0, 200.0);
        let (x, y) = t.transform_point(50.0, 75.0);
        assert!(approx_eq(x, 50.0));
        assert!(approx_eq(y, 75.0));
    }

    /// center_at must agree with the composed definition:
    /// translate(cx, cy) * self * translate(-cx, -cy)
    #[test]
    fn test_center_at_matches_composed_form() {
        let t = Transform::rotate_degrees(30.0).then(&Transform::scale_xy(2.0, 0.5));
        let (cx, cy) = (17.0, -4.0);

        let direct = t.center_at(cx, cy);
        let composed = Transform::translate(cx, cy)
            .then(&t)
            .then(&Transform::translate(-cx, -cy));

        for (a, b) in direct.data.iter().zip(composed.data.iter()) {
            assert!(approx_eq(*a, *b), "{direct:?} != {composed:?}");
        }
    }

    /// The extents the function reports, checked against the extents the shape
    /// actually has: the image of a unit circle, sampled.
    ///
    /// Written against the definition rather than against expected numbers on
    /// purpose. The numbers are what got this wrong twice — it is easy to write
    /// down what a transform "should" scale by and lock in an answer to a
    /// question nobody asked.
    fn true_extents(t: &Transform) -> (f32, f32) {
        let (mut x, mut y) = (0.0f32, 0.0f32);
        for i in 0..3600 {
            let th = (i as f32) * std::f32::consts::TAU / 3600.0;
            let (sin, cos) = th.sin_cos();
            x = x.max((t.a() * cos + t.b() * sin).abs());
            y = y.max((t.c() * cos + t.d() * sin).abs());
        }
        (x, y)
    }

    fn assert_reports_its_extents(t: Transform, what: &str) {
        let (want_x, want_y) = true_extents(&t);
        let (got_x, got_y) = t.extract_scale_components();
        assert!(
            (got_x - want_x).abs() < 1e-3 && (got_y - want_y).abs() < 1e-3,
            "{what}: the shape reaches ({want_x:.4}, {want_y:.4}), \
             the transform reports ({got_x:.4}, {got_y:.4})"
        );
    }

    /// A container's own transform, at every angle. This is the case the
    /// column norms and the singular values both get wrong.
    #[test]
    fn a_transform_reports_the_extents_its_shape_has() {
        for deg in [0.0f32, 30.0, 45.0, 90.0, 180.0, 200.0, 270.0] {
            for scale in [Scale::NONE, Scale::new(2.0, 0.5), Scale::new(0.5, 2.0)] {
                assert_reports_its_extents(
                    Transform::compose(Translate::new(9.0, -4.0), deg, scale),
                    &format!("rotate {deg}° with scale ({}, {})", scale.x, scale.y),
                );
            }
        }
    }

    /// And a world transform, which is a chain: an ancestor's scale over a
    /// descendant's rotation, three deep, and a shear built from two of each.
    #[test]
    fn a_chain_reports_the_extents_its_shape_has() {
        let scale_over_rotation =
            Transform::scale_xy(2.0, 0.5).then(&Transform::rotate_degrees(45.0));
        assert_reports_its_extents(scale_over_rotation, "scale above a rotation");

        let three = Transform::scale_xy(2.0, 0.5)
            .then(&Transform::rotate_degrees(30.0))
            .then(&Transform::scale(1.5));
        assert_reports_its_extents(three, "three levels");

        let sheared = Transform::scale_xy(2.0, 0.5)
            .then(&Transform::rotate_degrees(30.0))
            .then(&Transform::scale_xy(0.5, 2.0))
            .then(&Transform::rotate_degrees(20.0));
        assert_reports_its_extents(sheared, "two rotations with two scales, a shear");
    }

    /// A rotation on its own changes nothing about how far a circle reaches.
    #[test]
    fn a_rotation_alone_reports_no_scaling() {
        for deg in [0.0f32, 45.0, 180.0, 360.0] {
            let t = Transform::compose(Translate::NONE, deg, Scale::NONE);
            let (sx, sy) = t.extract_scale_components();
            assert!(
                approx_eq(sx, 1.0) && approx_eq(sy, 1.0),
                "at {deg}°: ({sx}, {sy})"
            );
        }
    }

    /// `compose` is the fixed order written out: translate, then rotate, then
    /// scale, which as matrices is `T * R * S`. Folding it by hand is only
    /// worth doing if it agrees with composing it.
    #[test]
    fn compose_is_translate_then_rotate_then_scale() {
        let (t, deg, s) = (Translate::new(7.0, -3.0), 30.0_f32, Scale::new(2.0, 0.5));

        let folded = Transform::compose(t, deg, s);
        let built = Transform::translate(t.x, t.y)
            .then(&Transform::rotate_degrees(deg))
            .then(&Transform::scale_xy(s.x, s.y));

        for (a, b) in folded.data.iter().zip(built.data.iter()) {
            assert!(approx_eq(*a, *b), "{folded:?} != {built:?}");
        }
    }

    /// The three declared apart do what the one composed value did, so the
    /// order is a rule and not an accident of which builder ran last.
    #[test]
    fn each_component_is_neutral_when_it_is_not_declared() {
        assert_eq!(
            Transform::compose(Translate::NONE, 0.0, Scale::NONE),
            Transform::IDENTITY
        );
        assert_eq!(
            Transform::compose(Translate::new(5.0, 6.0), 0.0, Scale::NONE),
            Transform::translate(5.0, 6.0)
        );
        assert_eq!(
            Transform::compose(Translate::NONE, 0.0, Scale::uniform(2.0)),
            Transform::scale(2.0)
        );
    }

    /// A rotation is stored as the number it was given, so a scale read back
    /// from a full turn is still a scale of one — and, unlike a matrix, the
    /// turn itself is still there to interpolate.
    #[test]
    fn a_full_turn_composes_to_the_size_it_started_at() {
        let full = Transform::compose(Translate::NONE, 360.0, Scale::NONE);
        assert!(approx_eq(full.extract_scale(), 1.0));
    }

    #[test]
    fn test_combined_rotate_and_scale() {
        // Rotate 45 degrees then scale 2x
        let rotate = Transform::rotate_degrees(45.0);
        let scale = Transform::scale(2.0);
        let combined = scale.then(&rotate);

        // Point (1, 0) rotated 45 degrees is (cos45, sin45) ≈ (0.707, 0.707)
        // Then scaled 2x is (1.414, 1.414)
        let (x, y) = combined.transform_point(1.0, 0.0);
        let expected = std::f32::consts::SQRT_2;
        assert!(approx_eq(x, expected));
        assert!(approx_eq(y, expected));
    }

    #[test]
    fn test_non_uniform_scale() {
        let t = Transform::scale_xy(2.0, 0.5);
        let (x, y) = t.transform_point(10.0, 10.0);
        assert!(approx_eq(x, 20.0));
        assert!(approx_eq(y, 5.0));
    }

    #[test]
    fn test_rotate_360_is_identity() {
        let t = Transform::rotate_degrees(360.0);
        let (x, y) = t.transform_point(3.0, 4.0);
        assert!(approx_eq(x, 3.0));
        assert!(approx_eq(y, 4.0));
    }

    #[test]
    fn test_rotate_negative() {
        // -90 degrees should be same as 270 degrees
        let t1 = Transform::rotate_degrees(-90.0);
        let t2 = Transform::rotate_degrees(270.0);

        let (x1, y1) = t1.transform_point(1.0, 0.0);
        let (x2, y2) = t2.transform_point(1.0, 0.0);

        assert!(approx_eq(x1, x2));
        assert!(approx_eq(y1, y2));
    }

    /// A collapsed transform has no inverse, and says so rather than handing
    /// back the identity — which the hit test read as "nothing to undo", so an
    /// invisible subtree answered for the area it covers when it is open.
    #[test]
    fn a_collapsed_transform_has_no_inverse() {
        assert!(Transform::scale(0.0).inverse().is_none());
        assert!(Transform::scale_xy(1.0, 0.0).inverse().is_none());
        assert!(Transform::scale_xy(0.0, 1.0).inverse().is_none());

        // Small is not collapsed: the map is still one-to-one, so undoing it
        // is a division by something rather than by nothing.
        assert!(Transform::scale_xy(1.0, 0.001).inverse().is_some());
    }

    #[test]
    fn test_multiple_composition() {
        // T1 * T2 * T3 applied to point
        let t1 = Transform::translate(10.0, 0.0);
        let t2 = Transform::scale(2.0);
        let t3 = Transform::translate(0.0, 5.0);

        // Compose: t3 * t2 * t1 (apply t1 first, then t2, then t3)
        let composed = t3.then(&t2).then(&t1);

        // Point (0, 0) -> t1 -> (10, 0) -> t2 -> (20, 0) -> t3 -> (20, 5)
        let (x, y) = composed.transform_point(0.0, 0.0);
        assert!(approx_eq(x, 20.0));
        assert!(approx_eq(y, 5.0));
    }
}
