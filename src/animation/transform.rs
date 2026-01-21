use super::Animatable;

/// 2D transformation that doesn't trigger layout recalculation
/// Applied during paint only
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Translation in x and y
    pub translate: (f32, f32),
    /// Scale in x and y (1.0 = no scale)
    pub scale: (f32, f32),
    /// Rotation in radians (clockwise)
    pub rotate: f32,
    /// Transform origin as fraction of size (0.5, 0.5 = center)
    pub origin: (f32, f32),
}

impl Transform {
    /// Identity transform (no transformation)
    pub const IDENTITY: Self = Self {
        translate: (0.0, 0.0),
        scale: (1.0, 1.0),
        rotate: 0.0,
        origin: (0.5, 0.5),
    };

    /// Create a translation transform
    pub fn translate(x: f32, y: f32) -> Self {
        Self {
            translate: (x, y),
            ..Self::IDENTITY
        }
    }

    /// Create a scale transform
    pub fn scale(x: f32, y: f32) -> Self {
        Self {
            scale: (x, y),
            ..Self::IDENTITY
        }
    }

    /// Create a uniform scale transform
    pub fn scale_uniform(scale: f32) -> Self {
        Self::scale(scale, scale)
    }

    /// Create a rotation transform (in radians)
    pub fn rotate(radians: f32) -> Self {
        Self {
            rotate: radians,
            ..Self::IDENTITY
        }
    }

    /// Set the transform origin (default is center: 0.5, 0.5)
    pub fn with_origin(mut self, x: f32, y: f32) -> Self {
        self.origin = (x, y);
        self
    }

    /// Apply this transform to a point relative to a bounding box
    pub fn apply_to_point(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
        // Calculate origin point
        let origin_x = width * self.origin.0;
        let origin_y = height * self.origin.1;

        // Translate to origin
        let mut tx = x - origin_x;
        let mut ty = y - origin_y;

        // Apply scale
        tx *= self.scale.0;
        ty *= self.scale.1;

        // Apply rotation
        if self.rotate.abs() > 1e-6 {
            let cos = self.rotate.cos();
            let sin = self.rotate.sin();
            let rx = tx * cos - ty * sin;
            let ry = tx * sin + ty * cos;
            tx = rx;
            ty = ry;
        }

        // Translate back from origin
        tx += origin_x;
        ty += origin_y;

        // Apply translation
        tx += self.translate.0;
        ty += self.translate.1;

        (tx, ty)
    }
}

impl Animatable for Transform {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            translate: (
                from.translate.0 + (to.translate.0 - from.translate.0) * t,
                from.translate.1 + (to.translate.1 - from.translate.1) * t,
            ),
            scale: (
                from.scale.0 + (to.scale.0 - from.scale.0) * t,
                from.scale.1 + (to.scale.1 - from.scale.1) * t,
            ),
            rotate: from.rotate + (to.rotate - from.rotate) * t,
            origin: from.origin, // Origin doesn't animate
        }
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

    #[test]
    fn test_identity_transform() {
        let t = Transform::IDENTITY;
        let (x, y) = t.apply_to_point(10.0, 20.0, 100.0, 100.0);
        assert_eq!(x, 10.0);
        assert_eq!(y, 20.0);
    }

    #[test]
    fn test_translate() {
        let t = Transform::translate(5.0, 10.0);
        let (x, y) = t.apply_to_point(10.0, 20.0, 100.0, 100.0);
        assert_eq!(x, 15.0);
        assert_eq!(y, 30.0);
    }

    #[test]
    fn test_scale() {
        let t = Transform::scale_uniform(2.0);
        let (x, y) = t.apply_to_point(10.0, 10.0, 100.0, 100.0);
        // Scaled around center (50, 50)
        // Point (10, 10) is -40, -40 from center
        // Scaled: -80, -80
        // Back from center: -30, -30
        assert!((x - (-30.0)).abs() < 0.1);
        assert!((y - (-30.0)).abs() < 0.1);
    }

    #[test]
    fn test_transform_lerp() {
        let t1 = Transform::translate(0.0, 0.0);
        let t2 = Transform::translate(10.0, 20.0);
        let mid = Transform::lerp(&t1, &t2, 0.5);
        assert_eq!(mid.translate.0, 5.0);
        assert_eq!(mid.translate.1, 10.0);
    }
}
