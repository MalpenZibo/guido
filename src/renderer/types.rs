//! Shared types for the renderer.

use crate::transform::Transform;
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::{Color, Rect};

/// Gradient direction for linear gradients
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientDir {
    Horizontal,
    Vertical,
    Diagonal,
    DiagonalReverse,
}

/// Optional gradient for shapes
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub start_color: Color,
    pub end_color: Color,
    pub direction: GradientDir,
}

/// Shadow configuration for shapes
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    /// Shadow offset in logical pixels (x, y)
    pub offset: (f32, f32),
    /// Blur radius in logical pixels
    pub blur: f32,
    /// Spread amount in logical pixels (expands shadow)
    pub spread: f32,
    /// Shadow color
    pub color: Color,
}

impl Shadow {
    /// Create a shadow with the given parameters.
    ///
    /// `const`, along with [`simple`](Self::simple) and [`none`](Self::none),
    /// so an application can write its own ladder of shadows down as constants
    /// — which is where a design system's elevation scale lives now that the
    /// library ships no table of its own.
    pub const fn new(offset: (f32, f32), blur: f32, spread: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread,
            color,
        }
    }

    /// How far this shadow reaches past the box that casts it.
    ///
    /// The amount the damage rect has to grow by, so repainting the box also
    /// re-composites the shadow instead of leaving the old one behind.
    pub fn extent(&self) -> f32 {
        if self.color.a <= 0.0 {
            return 0.0;
        }
        self.blur + self.spread + self.offset.0.abs().max(self.offset.1.abs())
    }

    /// The same shadow, every length multiplied by `k`.
    ///
    /// The colour does not scale — a shadow made smaller should be smaller, not
    /// fainter — and neither does the alpha `extent` gates on, so
    /// `scaled(k).extent() == extent() * k` for any `k >= 0`. That identity is
    /// what [`shrunk_to`](Self::shrunk_to) rests on, and it is why the two live
    /// beside [`extent`](Self::extent) rather than beside either caller: add a
    /// constant term to `extent` and both stop holding, here, where it is
    /// tested.
    ///
    /// Two callers, one rule: the HiDPI pass multiplies a shadow by the surface
    /// scale before it reaches the instance buffer, and a container shrinks one
    /// to fit the damage rect its layout reserved.
    pub fn scaled(self, k: f32) -> Self {
        Self {
            offset: (self.offset.0 * k, self.offset.1 * k),
            blur: self.blur * k,
            spread: self.spread * k,
            color: self.color,
        }
    }

    /// This shadow, shrunk until it reaches no further than `reach` past its
    /// box.
    ///
    /// Uniform, so the shape survives rather than the shadow collapsing along
    /// one axis. A shadow already inside the rect is returned untouched, and so
    /// is one whose extent is not a positive number — which is what answers a
    /// `NaN` field, since every comparison against it is false.
    pub fn shrunk_to(self, reach: f32) -> Self {
        let extent = self.extent();
        match extent.partial_cmp(&reach) {
            Some(std::cmp::Ordering::Greater) => self.scaled(reach / extent),
            // Inside the reach, exactly at it, or not comparable at all — a
            // `NaN` on either side lands here rather than scaling by a `NaN`
            // factor, which would spread it into all four fields.
            _ => self,
        }
    }

    /// Create a shadow with no spread
    pub const fn simple(offset: (f32, f32), blur: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread: 0.0,
            color,
        }
    }

    /// Create a default shadow (no shadow)
    pub const fn none() -> Self {
        Self {
            offset: (0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
        }
    }
}

/// A text entry for rendering, containing all information needed to render text.
#[derive(Debug, Clone)]
pub struct TextEntry {
    /// The text string to render
    pub text: String,
    /// The bounding rectangle for the text in logical pixels
    pub rect: Rect,
    /// The text color
    pub color: Color,
    /// The font size in logical pixels
    pub font_size: f32,
    /// The font family
    pub font_family: FontFamily,
    /// The font weight
    pub font_weight: FontWeight,
    /// Optional clip rectangle to constrain text rendering
    pub clip_rect: Option<Rect>,
    /// Transform to apply to this text
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
}

/// An image entry for rendering.
#[derive(Clone)]
pub struct ImageEntry {
    /// The image source
    pub source: ImageSource,
    /// The bounding rectangle for the image in logical pixels
    pub rect: Rect,
    /// How the image content should fit within its bounds
    pub content_fit: ContentFit,
    /// Optional clip rectangle to constrain image rendering
    pub clip_rect: Option<Rect>,
    /// Transform to apply to this image
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
}

/// The two halves of "how far a shadow reaches", checked against each other.
///
/// `shrunk_to` is only correct because `extent` is homogeneous of degree one in
/// the three lengths — scale them all by `k` and the extent scales by `k`. Add a
/// constant term or a floor to `extent` and the clamp silently stops fitting,
/// which paints a ring outside the damage rect a container reserved. That is
/// what these say, at the definition rather than through a hover test.
#[cfg(test)]
mod shadow_extent_tests {
    use super::*;

    const DEEP: Shadow = Shadow::new((3.0, -8.0), 10.0, 4.0, Color::rgba(0.0, 0.0, 0.0, 0.5));

    #[test]
    fn the_extent_is_what_scaling_a_shadow_scales() {
        assert_eq!(DEEP.extent(), 22.0, "blur + spread + the longer offset");
        for k in [0.0, 0.25, 1.0, 3.0] {
            assert!(
                (DEEP.scaled(k).extent() - DEEP.extent() * k).abs() < 1e-4,
                "scaling by {k} has to scale the extent by {k}"
            );
        }
    }

    #[test]
    fn scaling_leaves_the_colour_alone() {
        // A shadow made smaller is smaller, not fainter — and the alpha is what
        // `extent` gates on, so fading it would break the identity above.
        assert_eq!(DEEP.scaled(0.5).color, DEEP.color);
    }

    #[test]
    fn a_shadow_shrunk_to_a_reach_fits_inside_it() {
        let fitted = DEEP.shrunk_to(11.0);
        assert!(fitted.extent() <= 11.0 + 1e-4, "{}", fitted.extent());
        assert_eq!(fitted.blur, 5.0, "and uniformly, so the shape survives");
        assert_eq!(fitted.spread, 2.0);
        assert_eq!(fitted.offset, (1.5, -4.0));
    }

    #[test]
    fn a_shadow_already_inside_the_reach_is_untouched() {
        assert_eq!(DEEP.shrunk_to(22.0), DEEP, "exactly at the reach");
        assert_eq!(DEEP.shrunk_to(100.0), DEEP);
    }

    /// Every comparison against `NaN` is false, so the guard is written to
    /// return the shadow rather than to scale it by a `NaN` factor — which
    /// would spread the `NaN` into all four fields.
    #[test]
    fn a_reach_that_is_not_a_number_shrinks_nothing() {
        assert_eq!(DEEP.shrunk_to(f32::NAN), DEEP);
    }

    /// A transparent shadow reaches nowhere however large its numbers, so there
    /// is nothing to divide by and nothing to shrink.
    #[test]
    fn an_invisible_shadow_has_no_extent_to_clamp() {
        let ghost = Shadow::new((9.0, 9.0), 9.0, 9.0, Color::TRANSPARENT);
        assert_eq!(ghost.extent(), 0.0);
        assert_eq!(ghost.shrunk_to(1.0), ghost);
        assert_eq!(Shadow::none().extent(), 0.0);
    }
}
