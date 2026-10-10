//! Shared types for the renderer.

use crate::transform::Transform;
use crate::widgets::font::{FontFamily, FontWeight, LineHeight};
use crate::widgets::{Color, Rect, TextAlign};

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
    /// re-composites the shadow instead of leaving the old one behind:
    /// [`signed_extent`](Self::signed_extent) floored at zero, since a shadow
    /// pulled inside its box reaches nothing past it.
    pub fn extent(&self) -> f32 {
        // `clamp` rather than `max`, so a `NaN` stays one.
        self.signed_extent().clamp(0.0, f32::INFINITY)
    }

    /// How far this shadow reaches past its box, below zero for one pulled
    /// inside it.
    ///
    /// What orders two shadows and what a spring's overshoot is measured
    /// across: every shadow pulled inside the box has an
    /// [`extent`](Self::extent) of zero, but one pulled in by 16 and one by 4
    /// are still a step apart.
    pub(crate) fn signed_extent(&self) -> f32 {
        if self.color.a <= 0.0 {
            return 0.0;
        }
        self.spill() + self.offset.0.abs().max(self.offset.1.abs())
    }

    /// How far the blurred edge is drawn past the box on every side, before
    /// the offset moves it.
    ///
    /// The shader fades the shadow out with
    /// `smoothstep(-shadow_blur, shadow_blur * 2.0, distance - spread)`
    /// (`shader.wgsl`), so it is drawn until it is two blurs and the spread
    /// past the edge.
    ///
    /// A blur below zero, which a spring can overshoot to, counts as none: the
    /// quad the shader draws into is then grown by no more than the spread and
    /// the offset, so nothing is drawn further out than that. A `NaN` blur stays
    /// `NaN`, which is what `shrunk_to` answers by leaving the shadow alone.
    pub(crate) fn spill(&self) -> f32 {
        2.0 * self.blur.clamp(0.0, f32::INFINITY) + self.spread
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
    /// `NaN` field, since every comparison against it is false. A reach below
    /// zero counts as zero, so the factor is never above one or below zero.
    pub fn shrunk_to(self, reach: f32) -> Self {
        let reach = reach.clamp(0.0, f32::INFINITY);
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
    /// How tall each line is
    pub line_height: LineHeight,
    /// Extra advance after every glyph, in logical pixels
    pub letter_spacing: f32,
    /// Where each line sits across `rect`. Anything but `Start` is shaped at
    /// exactly the box's width, since that is the width the lines are aligned
    /// in.
    pub align: TextAlign,
    /// How opaque it is drawn, on top of `color`'s own alpha. Kept apart from
    /// the colour because the transformed-text path caches its rasterised
    /// texture by colour, and a fade folded into the colour would rasterise
    /// the text again on every frame of it.
    pub opacity: f32,
    /// The lines it is cut to, when it is cut.
    pub fit: Option<super::text_measurer::LineFit>,
    /// The clip this text was flattened under, as the shape it is rather than
    /// the box around it. The quad path cuts in the clip's own space; the
    /// glyphon path asks for the box, because `TextBounds` is four integers.
    pub clip: Option<crate::shape::PlacedShape>,
    /// Transform to apply to this text
    pub transform: Transform,
}

/// The two halves of "how far a shadow reaches", checked against each other.
///
/// `shrunk_to` is only correct because `extent` is homogeneous of degree one in
/// the three lengths — scale them all by `k` and the extent scales by `k`. Add a
/// constant term to `extent`, or a floor anywhere but zero, and the clamp
/// silently stops fitting, which paints a ring outside the damage rect a
/// container reserved. The floor at zero keeps it for any `k >= 0`, which is
/// every factor `shrunk_to` uses. That is what these say, at the definition
/// rather than through a hover test.
#[cfg(test)]
mod shadow_extent_tests {
    use super::*;

    const DEEP: Shadow = Shadow::new((3.0, -8.0), 10.0, 4.0, Color::rgba(0.0, 0.0, 0.0, 0.5));

    #[test]
    fn the_extent_is_what_scaling_a_shadow_scales() {
        assert_eq!(
            DEEP.extent(),
            32.0,
            "two blurs + spread + the longer offset"
        );
        for k in [0.0, 0.25, 1.0, 3.0] {
            assert!(
                (DEEP.scaled(k).extent() - DEEP.extent() * k).abs() < 1e-4,
                "scaling by {k} has to scale the extent by {k}"
            );
        }
    }

    /// `spill` is the shader's falloff written again in Rust, and the GPU test
    /// that compares the two skips on a machine without lavapipe. Changing the
    /// falloff without changing `spill` fails here instead.
    #[test]
    fn the_spill_is_where_the_shader_fades_a_shadow_out() {
        let shader = include_str!("shader.wgsl");
        for (line, what) in [
            (
                "rounded_rect_sdf(shadow_pos, in.shape_rect, radii, curvature) - shadow_spread",
                "the distance a shadow fades over",
            ),
            (
                "1.0 - smoothstep(-shadow_blur, shadow_blur * 2.0, shadow_dist)",
                "the falloff",
            ),
        ] {
            assert!(
                shader.contains(line),
                "{what} changed in shader.wgsl: `Shadow::spill` has to say how far the \
                 new one reaches"
            );
        }
        // The quad a shadow is drawn into has to make room for all of it.
        let fadeout: f32 = shader
            .split_once("let fadeout = ")
            .and_then(|(_, rest)| rest.split_once(';'))
            .and_then(|(value, _)| value.trim().parse().ok())
            .expect("the shader grows a shadow's quad by `fadeout` blurs");
        assert!(
            fadeout >= 2.0,
            "a quad grown by {fadeout} blurs cuts off a shadow drawn two blurs out"
        );
    }

    #[test]
    fn a_blur_overshot_below_zero_spills_no_further_than_the_spread() {
        let overshot = Shadow::new((0.0, 1.0), -1.0, 1.0, Color::BLACK);
        assert_eq!(overshot.spill(), 1.0);
        assert_eq!(overshot.extent(), 2.0, "the spread and the offset");
        // And one that is not a number stays one, so `shrunk_to` leaves it be.
        let unknown = Shadow::new((0.0, 1.0), f32::NAN, 1.0, Color::BLACK);
        assert!(unknown.extent().is_nan());
        assert_eq!(unknown.shrunk_to(1.0).spread, 1.0);
    }

    #[test]
    fn scaling_leaves_the_colour_alone() {
        // A shadow made smaller is smaller, not fainter — and the alpha is what
        // `extent` gates on, so fading it would break the identity above.
        assert_eq!(DEEP.scaled(0.5).color, DEEP.color);
    }

    #[test]
    fn a_shadow_shrunk_to_a_reach_fits_inside_it() {
        let fitted = DEEP.shrunk_to(16.0);
        assert!(fitted.extent() <= 16.0 + 1e-4, "{}", fitted.extent());
        assert_eq!(fitted.blur, 5.0, "and uniformly, so the shape survives");
        assert_eq!(fitted.spread, 2.0);
        assert_eq!(fitted.offset, (1.5, -4.0));
    }

    #[test]
    fn a_shadow_already_inside_the_reach_is_untouched() {
        assert_eq!(DEEP.shrunk_to(32.0), DEEP, "exactly at the reach");
        assert_eq!(DEEP.shrunk_to(100.0), DEEP);
    }

    /// Every comparison against `NaN` is false, so the guard is written to
    /// return the shadow rather than to scale it by a `NaN` factor — which
    /// would spread the `NaN` into all four fields.
    #[test]
    fn a_reach_that_is_not_a_number_shrinks_nothing() {
        assert_eq!(DEEP.shrunk_to(f32::NAN), DEEP);
    }

    /// A spread more negative than the rest pulls the shadow inside its box,
    /// where it reaches nothing past it.
    const PULLED_IN: Shadow = Shadow::new((0.0, 0.0), 2.0, -10.0, Color::BLACK);

    #[test]
    fn a_shadow_pulled_inside_its_box_reaches_nothing_past_it() {
        assert_eq!(PULLED_IN.extent(), 0.0);
    }

    /// A reach below the shadow's own depth inside the box is not a reason to
    /// scale it: grown by a spring's overshoot, such a reach was more
    /// negative still and scaled the shadow *up*.
    #[test]
    fn a_reach_of_zero_or_less_leaves_a_pulled_in_shadow_untouched() {
        assert_eq!(PULLED_IN.shrunk_to(0.0), PULLED_IN);
        assert_eq!(PULLED_IN.shrunk_to(-6.0 * 1.17), PULLED_IN);
        // One that reaches exactly to the edge has nothing to divide by.
        let flush = Shadow::new((0.0, 0.0), 2.0, -4.0, Color::BLACK);
        assert_eq!(flush.shrunk_to(-1.0), flush);
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
