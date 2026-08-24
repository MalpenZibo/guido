/// A 2D affine transformation.
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
    /// The chainers below post-compose, so they read in the order they are
    /// written: `Transform::translate(10.0, 0.0).then_rotate(45.0)` moves and
    /// then turns.
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

    /// The X and Y scale components — `sqrt(a² + b²)` and `sqrt(c² + d²)`,
    /// so a rotated transform still reports the scale it carries.
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
