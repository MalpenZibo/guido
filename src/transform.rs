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

// A closure has to accept whatever the constant form accepts, or the same
// expression compiles in one position and not the other — the asymmetry
// `IntoSignal` exists to remove. `From` covers the constants; these cover the
// closures.
macro_rules! into_val_pairs {
    ($t:ty $(, $n:ty)*) => {$(
        impl crate::reactive::IntoVal<$t> for ($n, $n) {
            fn into_val(self) -> $t {
                self.into()
            }
        }
        impl crate::reactive::IntoVal<$t> for [$n; 2] {
            fn into_val(self) -> $t {
                self.into()
            }
        }
    )*};
}
into_val_pairs!(Translate, f32, f64, i32, u32);
into_val_pairs!(Scale, f32, f64, i32, u32);

macro_rules! into_val_scalar {
    ($($n:ty),*) => {$(
        impl crate::reactive::IntoVal<Scale> for $n {
            fn into_val(self) -> Scale {
                Scale::uniform(self as f32)
            }
        }
    )*};
}
into_val_scalar!(f32, f64, i32, u32);

// A signal whose value converts. `IntoSignal` accepts a `Signal<T>` only for
// the same `T`, so without these a signal is the one form that does not take
// what the others do: `scale(1.5)` and `scale(move || f.get())` both go through
// `From<f32> for Scale`, and `scale(f)` on a `Signal<f32>` did not.
//
// Written out per source type rather than as a blanket `T: Into<U>`, which does
// not work: `Into` is reflexive, so such an impl also covers `Signal<Scale> ->
// Scale` and collides with the passthrough, leaving the marker undecidable.
// Naming the source excludes the reflexive case by construction.
macro_rules! converting_signal {
    ($($from:ty => $to:ty),* $(,)?) => {$(
        impl crate::reactive::IntoSignal<$to, crate::reactive::ConvertedSignalMarker>
            for crate::reactive::Signal<$from>
        {
            fn into_signal(self) -> crate::reactive::Signal<$to> {
                crate::reactive::create_derived(move || self.get().into())
            }
        }
        impl crate::reactive::IntoSignal<$to, crate::reactive::ConvertedSignalMarker>
            for crate::reactive::RwSignal<$from>
        {
            fn into_signal(self) -> crate::reactive::Signal<$to> {
                let read = self.read_only();
                crate::reactive::create_derived(move || read.get().into())
            }
        }
    )*};
}
converting_signal!(
    f32 => Scale,
    (f32, f32) => Scale,
    (f32, f32) => Translate,
);

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
    /// applied here — [`center_at`](Self::center_at) does that at flatten
    /// time, and because it is a translation it commutes with this one, so
    /// which side it lands on does not change the result.
    pub(crate) fn compose(translate: Translate, rotate_degrees: f32, scale: Scale) -> Self {
        // Called for every container on every paint and every pointer event,
        // and almost none of them turn, so the trig is worth skipping rather
        // than trusting to be cheap.
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

    /// Compute the inverse of this transform.
    ///
    /// Returns the identity for degenerate (zero-determinant) transforms.
    pub fn inverse(&self) -> Transform {
        let [a, b, tx, c, d, ty] = self.data;

        let det = a * d - b * c;

        // Handle degenerate case (zero determinant)
        if det.abs() < 1e-10 {
            return Self::IDENTITY;
        }

        let inv_det = 1.0 / det;

        Transform {
            data: [
                d * inv_det,
                -b * inv_det,
                (-d * tx + b * ty) * inv_det,
                -c * inv_det,
                a * inv_det,
                (c * tx - a * ty) * inv_det,
            ],
        }
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

    /// Set the X translation component
    #[inline]
    pub(crate) fn set_tx(&mut self, val: f32) {
        self.data[2] = val;
    }

    /// Set the Y translation component
    #[inline]
    pub(crate) fn set_ty(&mut self, val: f32) {
        self.data[5] = val;
    }

    /// The X and Y scale components — the norms of the two *columns*,
    /// `sqrt(a² + c²)` and `sqrt(b² + d²)`, so a rotated transform still
    /// reports the scale it carries.
    ///
    /// Columns and not rows, which is the standard decomposition and also the
    /// only one that is right for the order [`compose`](Self::compose) builds.
    /// For `T·R·S` the matrix is `[cos·sx, -sin·sy, tx, sin·sx, cos·sy, ty]`:
    /// a column holds one axis's scale times a unit vector, so its norm is that
    /// scale, while a row mixes both axes and reports neither. Turning 45° with
    /// `scale((2.0, 0.5))` reads as `(2.0, 0.5)` by columns and as
    /// `(1.458, 1.458)` by rows.
    ///
    /// Rows were correct for `S·R`, which is the shape `scale_xy(..).then_rotate(..)`
    /// used to build and which no longer has a public spelling.
    pub(crate) fn extract_scale_components(&self) -> (f32, f32) {
        let (a, b, c, d) = (self.a(), self.b(), self.c(), self.d());
        ((a * a + c * c).sqrt(), (b * b + d * d).sqrt())
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
        let inv = t.inverse();
        let composed = t.then(&inv);

        // Should be identity
        let (x, y) = composed.transform_point(5.0, 7.0);
        assert!(approx_eq(x, 5.0));
        assert!(approx_eq(y, 7.0));
    }

    #[test]
    fn test_inverse_rotate() {
        let t = Transform::rotate_degrees(45.0);
        let inv = t.inverse();
        let composed = t.then(&inv);

        let (x, y) = composed.transform_point(3.0, 4.0);
        assert!(approx_eq(x, 3.0));
        assert!(approx_eq(y, 4.0));
    }

    #[test]
    fn test_inverse_scale() {
        let t = Transform::scale(2.0);
        let inv = t.inverse();
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

    /// The scale a transform reports has to be the scale it was built with,
    /// rotation included — it sizes the corner radii of the blur region a
    /// container publishes to the compositor.
    #[test]
    fn a_rotated_transform_reports_the_scale_it_carries() {
        for deg in [0.0f32, 30.0, 45.0, 90.0, 200.0] {
            let t = Transform::compose(Translate::new(9.0, -4.0), deg, Scale::new(2.0, 0.5));
            let (sx, sy) = t.extract_scale_components();
            assert!(
                approx_eq(sx, 2.0) && approx_eq(sy, 0.5),
                "at {deg}° expected (2.0, 0.5), got ({sx}, {sy})"
            );
        }
    }

    /// And a uniform one reads the same on both axes at any angle.
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

    #[test]
    fn test_inverse_degenerate() {
        // Zero scale has zero determinant - should return identity
        let t = Transform::scale(0.0);
        let inv = t.inverse();
        assert!(inv.is_identity());
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
