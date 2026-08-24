//! The shape of a box's corners: how far they are rounded, and how.
//!
//! One value rather than two properties, because the second means nothing
//! without the first — a curvature applies to an arc, and with no radius there
//! is no arc to apply it to. Declaring them apart made it possible to write a
//! squircle with square corners and get no warning about it.
//!
//! The constructors name the *shape*, and take the size:
//!
//! ```ignore
//! container().corners(8.0)                        // rounded, uniform
//! container().corners([16.0, 0.0])                // rounded on the top pair only
//! container().corners(Corners::squircle(12.0))    // iOS-style continuous
//! container().corners(Corners::bevel(12.0))       // a diagonal cut
//! container().corners(Corners::scoop(12.0))       // concave
//! container().corners(Corners::superellipse(12.0, 1.5))
//! ```
//!
//! The radii take one value for all four, `[top, bottom]` for the two pairs,
//! or `[top-left, top-right, bottom-right, bottom-left]` clockwise as CSS
//! writes it — see [`CornerRadii`], which spells out why the two-value form
//! pairs adjacent corners where `padding` pairs opposite sides.

use crate::renderer::CornerRadii;

/// How far a box's corners are rounded, and the curve they are rounded with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners {
    /// How far each corner is rounded, in logical pixels.
    pub radii: CornerRadii,
    /// The superellipse exponent, on the CSS K-value scale: 1.0 is a circular
    /// arc, 2.0 an iOS-style squircle, 0.0 a diagonal cut, -1.0 concave.
    pub curvature: f32,
}

impl Corners {
    /// Circular corners — the ordinary rounded box.
    pub fn rounded(radii: impl Into<CornerRadii>) -> Self {
        Self::superellipse(radii, 1.0)
    }

    /// iOS-style continuous corners (K = 2).
    pub fn squircle(radii: impl Into<CornerRadii>) -> Self {
        Self::superellipse(radii, 2.0)
    }

    /// A diagonal cut across the corner (K = 0).
    pub fn bevel(radii: impl Into<CornerRadii>) -> Self {
        Self::superellipse(radii, 0.0)
    }

    /// Concave corners, curving inward (K = -1).
    pub fn scoop(radii: impl Into<CornerRadii>) -> Self {
        Self::superellipse(radii, -1.0)
    }

    /// Any curvature on the scale, for the shapes between the named ones.
    pub fn superellipse(radii: impl Into<CornerRadii>, curvature: f32) -> Self {
        Self {
            radii: radii.into(),
            curvature,
        }
    }

    /// Square corners.
    pub const SQUARE: Self = Self {
        radii: CornerRadii {
            top_left: 0.0,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        },
        curvature: 1.0,
    };
}

impl Default for Corners {
    fn default() -> Self {
        Self::SQUARE
    }
}

/// A bare size means rounded corners, which is what a bare size nearly always
/// means. Reach for a constructor to say otherwise.
impl<T: Into<CornerRadii>> From<T> for Corners {
    fn from(radii: T) -> Self {
        Self::rounded(radii)
    }
}

/// Animating a shape moves five numbers: the four radii and the curvature.
///
/// The curvature crosses one boundary the interpolation cannot smooth: below
/// zero a corner is *concave*, and both this crate's hit test and the shader
/// change formula there. A transition from a scoop to a squircle therefore
/// passes through a bevel and changes family in one frame, on screen and in
/// the clickable region alike. Transitions within a family — rounded to
/// squircle, any radius to any other — are continuous.
impl crate::animation::Animatable for Corners {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            radii: crate::animation::Animatable::lerp(&from.radii, &to.radii, t),
            curvature: from.curvature + (to.curvature - from.curvature) * t,
        }
    }

    fn is_reverse(from: &Self, to: &Self) -> bool {
        // Curvature counts too: a shape that only softens is still going
        // somewhere, and a spring that ignored it would never call it a
        // reversal.
        let total = |c: &Self| {
            c.radii.top_left
                + c.radii.top_right
                + c.radii.bottom_right
                + c.radii.bottom_left
                + c.curvature
        };
        total(to) < total(from)
    }

    fn channels(&self) -> crate::animation::Channels {
        crate::animation::Channels::from_slice(&[
            self.radii.top_left,
            self.radii.top_right,
            self.radii.bottom_right,
            self.radii.bottom_left,
            self.curvature,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape and the size arrive together, so neither can be declared
    /// without the other.
    #[test]
    fn a_constructor_names_the_shape_and_takes_the_size() {
        assert_eq!(Corners::squircle(12.0).curvature, 2.0);
        assert_eq!(Corners::bevel(12.0).curvature, 0.0);
        assert_eq!(Corners::scoop(12.0).curvature, -1.0);
        assert_eq!(Corners::rounded(12.0).curvature, 1.0);
        assert_eq!(Corners::squircle(12.0).radii, CornerRadii::uniform(12.0));
    }

    /// One, two or four values, and the two-value form pairs the top corners
    /// against the bottom ones.
    #[test]
    fn the_radii_take_one_two_or_four_values() {
        assert_eq!(Corners::from(8.0).radii, CornerRadii::uniform(8.0));

        let pairs = Corners::squircle([16.0, 0.0]).radii;
        assert_eq!((pairs.top_left, pairs.top_right), (16.0, 16.0));
        assert_eq!((pairs.bottom_right, pairs.bottom_left), (0.0, 0.0));

        let four = Corners::rounded([1.0, 2.0, 3.0, 4.0]).radii;
        assert_eq!(four.top_left, 1.0);
        assert_eq!(four.top_right, 2.0);
        assert_eq!(four.bottom_right, 3.0);
        assert_eq!(four.bottom_left, 4.0);
    }
}
