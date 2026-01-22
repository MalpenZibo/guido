/// A 4x4 transformation matrix stored in row-major order.
///
/// Used for 2D transformations (translate, rotate, scale) that compose
/// parent→child and are passed to the GPU shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Matrix data in row-major order: [row0, row1, row2, row3]
    pub data: [f32; 16],
}

impl Transform {
    /// Identity matrix (no transformation)
    pub const IDENTITY: Self = Self {
        data: [
            1.0, 0.0, 0.0, 0.0, // row 0
            0.0, 1.0, 0.0, 0.0, // row 1
            0.0, 0.0, 1.0, 0.0, // row 2
            0.0, 0.0, 0.0, 1.0, // row 3
        ],
    };

    /// Create an identity transform
    pub fn identity() -> Self {
        Self::IDENTITY
    }

    /// Create a translation transform
    pub fn translate(x: f32, y: f32) -> Self {
        Self {
            data: [
                1.0, 0.0, 0.0, x, // row 0
                0.0, 1.0, 0.0, y, // row 1
                0.0, 0.0, 1.0, 0.0, // row 2
                0.0, 0.0, 0.0, 1.0, // row 3
            ],
        }
    }

    /// Create a rotation transform around the Z axis (2D rotation)
    pub fn rotate(angle_radians: f32) -> Self {
        let cos = angle_radians.cos();
        let sin = angle_radians.sin();
        Self {
            data: [
                cos, -sin, 0.0, 0.0, // row 0
                sin, cos, 0.0, 0.0, // row 1
                0.0, 0.0, 1.0, 0.0, // row 2
                0.0, 0.0, 0.0, 1.0, // row 3
            ],
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
            data: [
                sx, 0.0, 0.0, 0.0, // row 0
                0.0, sy, 0.0, 0.0, // row 1
                0.0, 0.0, 1.0, 0.0, // row 2
                0.0, 0.0, 0.0, 1.0, // row 3
            ],
        }
    }

    /// Compose this transform with another: self * other
    /// Applies `other` first, then `self`.
    pub fn then(&self, other: &Transform) -> Transform {
        let a = &self.data;
        let b = &other.data;

        // Matrix multiplication: result[i][j] = sum(a[i][k] * b[k][j])
        // Row-major indexing: element at row i, col j is at index i*4 + j
        let mut result = [0.0f32; 16];

        for i in 0..4 {
            for j in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += a[i * 4 + k] * b[k * 4 + j];
                }
                result[i * 4 + j] = sum;
            }
        }

        Transform { data: result }
    }

    /// Compute the inverse of this transform.
    /// For affine 2D transforms (translate, rotate, scale), this uses a simplified inverse.
    pub fn inverse(&self) -> Transform {
        // For a 2D affine transform, the matrix has the form:
        // | a  b  0  tx |
        // | c  d  0  ty |
        // | 0  0  1  0  |
        // | 0  0  0  1  |
        //
        // The inverse is:
        // | d/det  -b/det  0  (-d*tx + b*ty)/det |
        // | -c/det  a/det  0  (c*tx - a*ty)/det  |
        // | 0       0      1  0                   |
        // | 0       0      0  1                   |

        let a = self.data[0];
        let b = self.data[1];
        let c = self.data[4];
        let d = self.data[5];
        let tx = self.data[3];
        let ty = self.data[7];

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
                0.0,
                (-d * tx + b * ty) * inv_det,
                -c * inv_det,
                a * inv_det,
                0.0,
                (c * tx - a * ty) * inv_det,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ],
        }
    }

    /// Transform a 2D point by this matrix
    pub fn transform_point(&self, x: f32, y: f32) -> (f32, f32) {
        // Homogeneous coordinates: (x, y, 0, 1)
        // Result: (a*x + b*y + tx, c*x + d*y + ty)
        let new_x = self.data[0] * x + self.data[1] * y + self.data[3];
        let new_y = self.data[4] * x + self.data[5] * y + self.data[7];
        (new_x, new_y)
    }

    /// Get the rows of the matrix for passing to the shader
    pub fn rows(&self) -> [[f32; 4]; 4] {
        [
            [self.data[0], self.data[1], self.data[2], self.data[3]],
            [self.data[4], self.data[5], self.data[6], self.data[7]],
            [self.data[8], self.data[9], self.data[10], self.data[11]],
            [self.data[12], self.data[13], self.data[14], self.data[15]],
        ]
    }

    /// Check if this is the identity transform
    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
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
        let t = Transform::identity();
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
    fn test_rows() {
        let t = Transform::translate(1.0, 2.0);
        let rows = t.rows();
        assert_eq!(rows[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(rows[1], [0.0, 1.0, 0.0, 2.0]);
        assert_eq!(rows[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(rows[3], [0.0, 0.0, 0.0, 1.0]);
    }
}
