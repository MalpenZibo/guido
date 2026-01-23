use crate::widgets::Rect;

/// Horizontal anchor position for transform origin
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HorizontalAnchor {
    /// Anchor at the left edge (0%)
    Left,
    /// Anchor at the center (50%)
    Center,
    /// Anchor at the right edge (100%)
    Right,
    /// Anchor at a percentage from the left (0-100)
    Percent(f32),
    /// Anchor at a fixed pixel offset from the left
    Px(f32),
}

/// Vertical anchor position for transform origin
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerticalAnchor {
    /// Anchor at the top edge (0%)
    Top,
    /// Anchor at the center (50%)
    Center,
    /// Anchor at the bottom edge (100%)
    Bottom,
    /// Anchor at a percentage from the top (0-100)
    Percent(f32),
    /// Anchor at a fixed pixel offset from the top
    Px(f32),
}

/// Specifies the pivot point for transforms, similar to CSS `transform-origin`.
///
/// The origin is the point around which rotations and scales are applied.
/// By default, transforms are centered on the widget (50%, 50%).
///
/// # Example
/// ```ignore
/// // Rotate around the top-left corner
/// container()
///     .rotate(45.0)
///     .transform_origin(TransformOrigin::TOP_LEFT)
///
/// // Scale from the bottom-right corner
/// container()
///     .scale(1.5)
///     .transform_origin(TransformOrigin::BOTTOM_RIGHT)
///
/// // Use percentage-based origin (25% from left, 75% from top)
/// container()
///     .rotate(30.0)
///     .transform_origin(TransformOrigin::percent(25.0, 75.0))
///
/// // Use pixel-based offset (10px from left, 20px from top)
/// container()
///     .scale(2.0)
///     .transform_origin(TransformOrigin::px(10.0, 20.0))
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOrigin {
    /// Horizontal position of the origin
    pub horizontal: HorizontalAnchor,
    /// Vertical position of the origin
    pub vertical: VerticalAnchor,
}

impl TransformOrigin {
    /// Center of the widget (50%, 50%) - the default
    pub const CENTER: Self = Self {
        horizontal: HorizontalAnchor::Center,
        vertical: VerticalAnchor::Center,
    };

    /// Top-left corner (0%, 0%)
    pub const TOP_LEFT: Self = Self {
        horizontal: HorizontalAnchor::Left,
        vertical: VerticalAnchor::Top,
    };

    /// Top center (50%, 0%)
    pub const TOP: Self = Self {
        horizontal: HorizontalAnchor::Center,
        vertical: VerticalAnchor::Top,
    };

    /// Top-right corner (100%, 0%)
    pub const TOP_RIGHT: Self = Self {
        horizontal: HorizontalAnchor::Right,
        vertical: VerticalAnchor::Top,
    };

    /// Center left (0%, 50%)
    pub const LEFT: Self = Self {
        horizontal: HorizontalAnchor::Left,
        vertical: VerticalAnchor::Center,
    };

    /// Center right (100%, 50%)
    pub const RIGHT: Self = Self {
        horizontal: HorizontalAnchor::Right,
        vertical: VerticalAnchor::Center,
    };

    /// Bottom-left corner (0%, 100%)
    pub const BOTTOM_LEFT: Self = Self {
        horizontal: HorizontalAnchor::Left,
        vertical: VerticalAnchor::Bottom,
    };

    /// Bottom center (50%, 100%)
    pub const BOTTOM: Self = Self {
        horizontal: HorizontalAnchor::Center,
        vertical: VerticalAnchor::Bottom,
    };

    /// Bottom-right corner (100%, 100%)
    pub const BOTTOM_RIGHT: Self = Self {
        horizontal: HorizontalAnchor::Right,
        vertical: VerticalAnchor::Bottom,
    };

    /// Create a new transform origin with explicit anchors
    pub fn new(horizontal: HorizontalAnchor, vertical: VerticalAnchor) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }

    /// Create a transform origin from percentages (0-100 scale)
    ///
    /// # Example
    /// ```ignore
    /// // 25% from left, 75% from top
    /// TransformOrigin::percent(25.0, 75.0)
    /// ```
    pub fn percent(x_percent: f32, y_percent: f32) -> Self {
        Self {
            horizontal: HorizontalAnchor::Percent(x_percent),
            vertical: VerticalAnchor::Percent(y_percent),
        }
    }

    /// Create a transform origin from pixel offsets from the top-left corner
    ///
    /// # Example
    /// ```ignore
    /// // 10px from left, 20px from top
    /// TransformOrigin::px(10.0, 20.0)
    /// ```
    pub fn px(x: f32, y: f32) -> Self {
        Self {
            horizontal: HorizontalAnchor::Px(x),
            vertical: VerticalAnchor::Px(y),
        }
    }

    /// Resolve the transform origin to absolute coordinates within the given bounds.
    ///
    /// Returns `(x, y)` coordinates in the same coordinate system as the bounds.
    pub fn resolve(&self, bounds: Rect) -> (f32, f32) {
        let x = match self.horizontal {
            HorizontalAnchor::Left => bounds.x,
            HorizontalAnchor::Center => bounds.x + bounds.width / 2.0,
            HorizontalAnchor::Right => bounds.x + bounds.width,
            HorizontalAnchor::Percent(p) => bounds.x + bounds.width * (p / 100.0),
            HorizontalAnchor::Px(px) => bounds.x + px,
        };

        let y = match self.vertical {
            VerticalAnchor::Top => bounds.y,
            VerticalAnchor::Center => bounds.y + bounds.height / 2.0,
            VerticalAnchor::Bottom => bounds.y + bounds.height,
            VerticalAnchor::Percent(p) => bounds.y + bounds.height * (p / 100.0),
            VerticalAnchor::Px(px) => bounds.y + px,
        };

        (x, y)
    }

    /// Check if this is the center origin (default)
    pub fn is_center(&self) -> bool {
        matches!(
            self,
            Self {
                horizontal: HorizontalAnchor::Center,
                vertical: VerticalAnchor::Center
            }
        )
    }
}

impl Default for TransformOrigin {
    fn default() -> Self {
        Self::CENTER
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_constants() {
        let bounds = Rect::new(100.0, 50.0, 200.0, 100.0);

        // CENTER: (200, 100)
        let (x, y) = TransformOrigin::CENTER.resolve(bounds);
        assert!(approx_eq(x, 200.0));
        assert!(approx_eq(y, 100.0));

        // TOP_LEFT: (100, 50)
        let (x, y) = TransformOrigin::TOP_LEFT.resolve(bounds);
        assert!(approx_eq(x, 100.0));
        assert!(approx_eq(y, 50.0));

        // TOP: (200, 50)
        let (x, y) = TransformOrigin::TOP.resolve(bounds);
        assert!(approx_eq(x, 200.0));
        assert!(approx_eq(y, 50.0));

        // TOP_RIGHT: (300, 50)
        let (x, y) = TransformOrigin::TOP_RIGHT.resolve(bounds);
        assert!(approx_eq(x, 300.0));
        assert!(approx_eq(y, 50.0));

        // LEFT: (100, 100)
        let (x, y) = TransformOrigin::LEFT.resolve(bounds);
        assert!(approx_eq(x, 100.0));
        assert!(approx_eq(y, 100.0));

        // RIGHT: (300, 100)
        let (x, y) = TransformOrigin::RIGHT.resolve(bounds);
        assert!(approx_eq(x, 300.0));
        assert!(approx_eq(y, 100.0));

        // BOTTOM_LEFT: (100, 150)
        let (x, y) = TransformOrigin::BOTTOM_LEFT.resolve(bounds);
        assert!(approx_eq(x, 100.0));
        assert!(approx_eq(y, 150.0));

        // BOTTOM: (200, 150)
        let (x, y) = TransformOrigin::BOTTOM.resolve(bounds);
        assert!(approx_eq(x, 200.0));
        assert!(approx_eq(y, 150.0));

        // BOTTOM_RIGHT: (300, 150)
        let (x, y) = TransformOrigin::BOTTOM_RIGHT.resolve(bounds);
        assert!(approx_eq(x, 300.0));
        assert!(approx_eq(y, 150.0));
    }

    #[test]
    fn test_percent() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 200.0);

        // 25%, 75%
        let origin = TransformOrigin::percent(25.0, 75.0);
        let (x, y) = origin.resolve(bounds);
        assert!(approx_eq(x, 25.0));
        assert!(approx_eq(y, 150.0));

        // 0%, 0% should be same as TOP_LEFT
        let origin = TransformOrigin::percent(0.0, 0.0);
        let (x, y) = origin.resolve(bounds);
        assert!(approx_eq(x, 0.0));
        assert!(approx_eq(y, 0.0));

        // 100%, 100% should be same as BOTTOM_RIGHT
        let origin = TransformOrigin::percent(100.0, 100.0);
        let (x, y) = origin.resolve(bounds);
        assert!(approx_eq(x, 100.0));
        assert!(approx_eq(y, 200.0));
    }

    #[test]
    fn test_px() {
        let bounds = Rect::new(50.0, 100.0, 200.0, 300.0);

        // 10px, 20px from bounds origin
        let origin = TransformOrigin::px(10.0, 20.0);
        let (x, y) = origin.resolve(bounds);
        assert!(approx_eq(x, 60.0)); // 50 + 10
        assert!(approx_eq(y, 120.0)); // 100 + 20
    }

    #[test]
    fn test_is_center() {
        assert!(TransformOrigin::CENTER.is_center());
        assert!(TransformOrigin::default().is_center());
        assert!(!TransformOrigin::TOP_LEFT.is_center());
        assert!(!TransformOrigin::percent(50.0, 50.0).is_center());
    }

    #[test]
    fn test_default() {
        assert_eq!(TransformOrigin::default(), TransformOrigin::CENTER);
    }
}
