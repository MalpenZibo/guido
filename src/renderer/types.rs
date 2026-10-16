//! Shared types for the renderer.

use crate::transform::Transform;
use crate::widgets::font::{FontFamily, FontFeatures, FontStyle, FontWeight, LineHeight};
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// Drawn inside the shape rather than outside it — see
    /// [`inset`](Self::inset).
    pub inset: bool,
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
            ..Self::none()
        }
    }

    /// The same shadow, cast inside the shape instead of outside it, as CSS's
    /// `inset` keyword casts one.
    ///
    /// It is drawn over the fill and inside the border, from the same distance
    /// the outer shadow is: dark at the edge and fading inwards, moved by the
    /// offset — so `(0.0, 4.0)` darkens the top edge, the way a control
    /// pressed in looks — and with a positive spread pushing the edge further
    /// in. A blur of zero and a spread is a hard ring inside the edge.
    pub const fn inset(mut self) -> Self {
        self.inset = true;
        self
    }

    /// How far this shadow reaches past the box that casts it.
    ///
    /// The amount the damage rect has to grow by, so repainting the box also
    /// re-composites the shadow instead of leaving the old one behind:
    /// `signed_extent` floored at zero, since a shadow
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
    ///
    /// An inset shadow reaches nothing past its box, whatever its numbers.
    pub(crate) fn signed_extent(&self) -> f32 {
        if self.color.a <= 0.0 || self.inset {
            return 0.0;
        }
        self.depth()
    }

    /// How far the shadow is drawn from the edge, on the side its offset
    /// carries it furthest: out of the box for an outer shadow, into it for
    /// an inset one.
    pub(crate) fn depth(&self) -> f32 {
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
            ..self
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
        match shrink_factor(self.extent(), reach) {
            Some(k) => self.scaled(k),
            None => self,
        }
    }

    /// Create a shadow with no spread
    pub const fn simple(offset: (f32, f32), blur: f32, color: Color) -> Self {
        Self::new(offset, blur, 0.0, color)
    }

    /// Create a default shadow (no shadow)
    pub const fn none() -> Self {
        Self {
            offset: (0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
            inset: false,
        }
    }
}

/// What a shadow reaching `extent` past its box is scaled by to reach no
/// further than `reach`, or `None` where it already fits.
///
/// A reach below zero counts as zero, so the factor is never above one or below
/// zero. Inside the reach, exactly at it, or not comparable at all is `None` —
/// a `NaN` on either side lands there rather than scaling by a `NaN` factor,
/// which would spread it into every field.
fn shrink_factor(extent: f32, reach: f32) -> Option<f32> {
    let reach = reach.clamp(0.0, f32::INFINITY);
    (extent > reach).then(|| reach / extent)
}

/// The shadows a box casts, drawn in order with the first on top — the order
/// CSS draws a `box-shadow` list in.
///
/// A short list, never longer than [`MAX`](Self::MAX), so it stays a value
/// like the one shadow it holds more of: `Copy`, animatable, and written down
/// as a constant. A design system whose elevation is a tight shadow under a
/// soft one writes each step as a pair:
///
/// ```
/// use guido::prelude::*;
///
/// const RAISED: Shadows = Shadows::new([
///     Shadow::new((0.0, 1.0), 2.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.3)),
///     Shadow::new((0.0, 4.0), 8.0, 3.0, Color::rgba(0.0, 0.0, 0.0, 0.15)),
/// ]);
/// let _ = container().shadow(RAISED);
/// // One shadow, or an array of them, is a list already.
/// let _ = container().shadow(Shadow::simple((0.0, 2.0), 4.0, Color::BLACK));
/// ```
///
/// Outer and [inset](Shadow::inset) shadows mix in one list: the outer ones
/// are drawn under the box, the inset ones over its fill.
#[derive(Debug, Clone, Copy)]
pub struct Shadows {
    list: [Shadow; Shadows::MAX],
    len: u8,
}

impl Shadows {
    /// The most shadows one box casts.
    pub const MAX: usize = 4;

    /// No shadow at all.
    pub const fn none() -> Self {
        Self {
            list: [Shadow::none(); Self::MAX],
            len: 0,
        }
    }

    /// These shadows, the first drawn on top. More than [`MAX`](Self::MAX)
    /// does not compile.
    pub const fn new<const N: usize>(shadows: [Shadow; N]) -> Self {
        const {
            assert!(
                N <= Shadows::MAX,
                "a box casts at most `Shadows::MAX` shadows"
            );
        }
        let mut list = [Shadow::none(); Self::MAX];
        let mut i = 0;
        while i < N {
            list[i] = shadows[i];
            i += 1;
        }
        Self { list, len: N as u8 }
    }

    /// The first [`MAX`](Self::MAX) of `shadows`.
    #[cfg(test)]
    pub(crate) fn from_slice(shadows: &[Shadow]) -> Self {
        let mut list = Self::none();
        for shadow in shadows {
            list.push(*shadow);
        }
        list
    }

    /// The shadows, first on top.
    pub fn as_slice(&self) -> &[Shadow] {
        &self.list[..usize::from(self.len)]
    }

    /// One more shadow on the end, past any [`MAX`](Self::MAX) dropped.
    pub(crate) fn push(&mut self, shadow: Shadow) {
        if let Some(slot) = self.list.get_mut(usize::from(self.len)) {
            *slot = shadow;
            self.len += 1;
        }
    }

    /// How far the furthest of these reaches past the box: what the damage
    /// rect grows by. See [`Shadow::extent`].
    pub fn extent(&self) -> f32 {
        self.signed_extent().clamp(0.0, f32::INFINITY)
    }

    /// The furthest [`Shadow::signed_extent`] of the list, and zero for none.
    pub(crate) fn signed_extent(&self) -> f32 {
        self.as_slice()
            .iter()
            .map(Shadow::signed_extent)
            .reduce(f32::max)
            .unwrap_or(0.0)
    }

    /// These shadows, shrunk until none reaches further than `reach` past the
    /// box.
    ///
    /// Every outer shadow by the same factor — the one that brings the
    /// furthest inside the reach — so the list keeps its proportions, as
    /// [`Shadow::shrunk_to`] keeps a shadow's. An inset shadow reaches nothing
    /// past the box and is left as it is.
    pub fn shrunk_to(mut self, reach: f32) -> Self {
        if let Some(k) = shrink_factor(self.extent(), reach) {
            for shadow in &mut self.list[..usize::from(self.len)] {
                if !shadow.inset {
                    *shadow = shadow.scaled(k);
                }
            }
        }
        self
    }
}

impl Default for Shadows {
    fn default() -> Self {
        Self::none()
    }
}

/// Two lists are equal when they hold the same shadows, whatever the unused
/// slots behind them hold.
impl PartialEq for Shadows {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl From<Shadow> for Shadows {
    fn from(shadow: Shadow) -> Self {
        Self::new([shadow])
    }
}

impl<const N: usize> From<[Shadow; N]> for Shadows {
    fn from(shadows: [Shadow; N]) -> Self {
        Self::new(shadows)
    }
}

crate::reactive::converts!(
    Shadow => Shadows,
    [Shadow; 1] => Shadows,
    [Shadow; 2] => Shadows,
    [Shadow; 3] => Shadows,
    [Shadow; 4] => Shadows,
);

/// A text entry for rendering, containing all information needed to render text.
#[derive(Debug, Clone)]
pub struct TextEntry {
    /// The text string to render, shared with the draw command it came from
    pub text: std::rc::Rc<str>,
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
    /// Upright, italic or oblique
    pub font_style: FontStyle,
    /// The OpenType features it is shaped with
    pub font_features: FontFeatures,
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
    /// The clip this text was flattened under, as the shapes it is rather
    /// than the box around them: a first shape, and the chain of those it
    /// could not be merged with in the frame's clip chains. The quad path cuts
    /// in each shape's own space; the glyphon path asks for the box they all
    /// leave open, because `TextBounds` is four integers.
    pub clip: Option<super::clip::ResolvedClip>,
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

#[cfg(test)]
mod shadows_tests {
    use super::*;
    use crate::animation::Animatable;

    const NEAR: Shadow = Shadow::new((0.0, 2.0), 2.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.3));
    const FAR: Shadow = Shadow::new((0.0, 8.0), 10.0, 2.0, Color::rgba(0.0, 0.0, 0.0, 0.2));
    const GLOW: Shadow = Shadow::new((0.0, 0.0), 30.0, 0.0, Color::BLACK).inset();

    #[test]
    fn a_list_reaches_as_far_as_its_furthest_outer_shadow() {
        assert_eq!(Shadows::new([NEAR, FAR]).extent(), FAR.extent());
        assert_eq!(Shadows::new([FAR, NEAR]).extent(), FAR.extent());
        assert_eq!(Shadows::none().extent(), 0.0);
    }

    #[test]
    fn an_inset_shadow_reaches_nothing_past_its_box() {
        assert_eq!(GLOW.extent(), 0.0, "for all its 30 px of blur");
        assert_eq!(Shadows::new([NEAR, GLOW]).extent(), NEAR.extent());
    }

    #[test]
    fn a_list_shrunk_to_a_reach_scales_its_outer_shadows_together() {
        let reach = FAR.extent() / 2.0;
        let shrunk = Shadows::new([NEAR, FAR, GLOW]).shrunk_to(reach);
        assert!((shrunk.extent() - reach).abs() < 1e-4);
        assert_eq!(shrunk.as_slice()[0], NEAR.scaled(0.5), "by the same factor");
        assert_eq!(shrunk.as_slice()[1], FAR.scaled(0.5));
        assert_eq!(shrunk.as_slice()[2], GLOW, "and the inset one not at all");
    }

    #[test]
    fn equal_lists_are_equal_whatever_their_unused_slots_hold() {
        let shortened = Shadows::from_slice(&Shadows::new([NEAR, FAR]).as_slice()[..1]);
        assert_eq!(shortened, Shadows::from(NEAR));
        assert_ne!(Shadows::from(NEAR), Shadows::new([NEAR, FAR]));
    }

    /// A shadow arriving in a longer list fades in from nothing, in its own
    /// colour, rather than through black.
    #[test]
    fn the_shorter_list_is_padded_with_shadows_that_draw_nothing() {
        let coloured = Shadow::new((0.0, 8.0), 10.0, 0.0, Color::rgba(1.0, 0.0, 0.0, 0.4));
        let half = Shadows::lerp(&Shadows::from(NEAR), &Shadows::new([NEAR, coloured]), 0.5);
        assert_eq!(half.as_slice().len(), 2);
        assert_eq!(half.as_slice()[0], NEAR);
        let arriving = half.as_slice()[1];
        assert_eq!(arriving.offset, (0.0, 4.0));
        assert_eq!(arriving.blur, 5.0);
        assert_eq!(arriving.color, Color::rgba(1.0, 0.0, 0.0, 0.2));
    }

    /// An outer shadow and an inset one have no shape between them, so the
    /// list steps rather than drawing a halfway that is neither.
    #[test]
    fn inset_against_outer_steps_halfway() {
        let (from, to) = (Shadows::from(FAR), Shadows::from(GLOW));
        assert_eq!(Shadows::lerp(&from, &to, 0.4), from);
        assert_eq!(Shadows::lerp(&from, &to, 0.6), to);
        // And padding takes the inset of the shadow it stands opposite, so a
        // glow arriving beside an outer shadow eases in.
        let arriving = Shadows::lerp(&from, &Shadows::new([FAR, GLOW]), 0.5);
        assert_eq!(arriving.as_slice()[1].blur, 15.0);
        assert!(arriving.as_slice()[1].inset);
    }

    #[test]
    fn two_inset_shadows_interpolate() {
        let deep = Shadow::new((0.0, 4.0), 10.0, 2.0, Color::BLACK).inset();
        let half = Shadows::lerp(&Shadows::from(GLOW), &Shadows::from(deep), 0.5);
        assert_eq!(half.as_slice()[0].blur, 20.0);
        assert_eq!(half.as_slice()[0].offset, (0.0, 2.0));
        assert!(half.as_slice()[0].inset);
    }

    /// Each list answers with its own shadows' channels, and a spring
    /// retargeted from a list of one to a list of two reads the second one's
    /// as starting from nothing rather than indexing past the first.
    #[test]
    fn lists_of_different_lengths_carry_velocity_between_them() {
        let lengths = [
            Shadows::none(),
            Shadows::from(NEAR),
            Shadows::new([NEAR, FAR, GLOW, NEAR]),
        ]
        .map(|s| s.channels().len());
        assert_eq!(lengths, [0, 8, 32]);

        let (one, two) = (Shadows::from(NEAR), Shadows::new([NEAR, FAR]));
        let carried = crate::animation::carry_velocity(1.0, &one, &two, &one, &two);
        assert!(
            (carried - 1.0).abs() < 1e-6,
            "the same segment, so the same speed: {carried}"
        );
    }

    #[test]
    fn a_list_rising_is_forward_and_falling_is_reverse() {
        let (low, high) = (Shadows::from(NEAR), Shadows::new([NEAR, FAR]));
        assert!(!Shadows::is_reverse(&low, &high));
        assert!(Shadows::is_reverse(&high, &low));
        assert!(Shadows::is_reverse(&low, &Shadows::none()));
        let (shallow, deep) = (
            Shadow::new((0.0, 0.0), 2.0, 0.0, Color::BLACK).inset(),
            Shadow::new((0.0, 0.0), 12.0, 0.0, Color::BLACK).inset(),
        );
        assert!(
            !Shadow::is_reverse(&shallow, &deep),
            "an inset shadow reaching deeper in is the larger"
        );
        assert!(Shadow::is_reverse(&deep, &shallow));
    }
}
