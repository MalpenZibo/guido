//! How text looks — declared on the widget that draws the glyphs.
//!
//! [`Text`](crate::widgets::Text) and [`TextInput`](crate::widgets::TextInput)
//! are the two widgets that put glyphs on the screen, so they are the two that
//! say how those glyphs look. A container draws a box and says nothing about
//! what is written inside it.
//!
//! ```ignore
//! text("Hello").font_size(16.0).color(theme.text).bold()
//! ```
//!
//! # A style is a partial record
//!
//! Every field is an `Option`, so a declaration says only what it means to
//! say. What fills the rest is the widget's own default — white, 14 logical
//! pixels, the registered family, normal weight — not a neighbouring
//! declaration: nothing is inherited from anywhere.
//!
//! The partiality earns its keep on state overrides, which *are* merged:
//! `when_hovered(|s| s.color(..))` changes the colour of a hovered label and
//! leaves its metrics alone. `TextStyle::inherit_from` is what
//! folds them, nearest declaration first.
//!
//! # The same style, many times
//!
//! Write a function. It keeps the declaration next to the widget that draws
//! it, costs no wrapper node, and gives the style a name:
//!
//! ```ignore
//! let label = |s: &str| text(s).color(theme.weak).font_size(12.0);
//! container().children([label("one"), label("two"), label("three")])
//! ```
//!
//! # A state that reaches the glyphs
//!
//! The pointer is over the box and the colour belongs to the glyphs. Declare
//! each where it happens and let [`control()`](crate::widgets::Container::control)
//! join them: a leaf resolves its own states from the nearest control above it.
//!
//! ```ignore
//! container().control().when_hovered(|s| s.lighter(0.1))
//!     .child(text("Label").color(weak).when_hovered(|s| s.color(strong)))
//! ```

use smallvec::SmallVec;

use crate::reactive::{IntoSignal, Signal};

use super::font::{FontFamily, FontWeight};
use super::widget::Color;

/// A contour drawn around the glyphs.
///
/// The legibility fix for text over an image, where no single colour works
/// against both the light and dark parts of the picture.
///
/// It is drawn *under* the fill, like SVG's `paint-order: stroke fill` and
/// unlike a naive implementation: painted over, a stroke eats half the weight
/// of every stem and the text reads as thinner rather than outlined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStroke {
    /// Half-width of the contour, in logical pixels — how far the stroke
    /// reaches outwards from the glyph edge.
    pub width: f32,
    pub color: Color,
}

impl TextStroke {
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }

    /// The offsets to draw the glyphs at, in logical pixels.
    ///
    /// A ring of copies around the original. This is the classic approximation
    /// and it costs nothing new: glyphon keys its atlas on glyph, size and
    /// weight, and takes colour per draw, so the extra copies re-use the
    /// rasterization already there and cost fill rate only.
    ///
    /// Its ceiling is the corners, which scallop once the gaps between samples
    /// exceed a pixel — so the tap count grows with the width instead of being
    /// fixed at the usual eight. Past a few pixels the honest fix is a dilate
    /// on an offscreen mask, not more taps.
    ///
    /// That fix exists for one case: a text with a
    /// [`backdrop_blur`](crate::widgets::Text::backdrop_blur) has a coverage
    /// mask already, and takes its stroke as a contour dilated from it — which
    /// it must, since copies under the fill would fill the glass as well as
    /// ring it.
    pub(crate) fn samples(&self) -> SmallVec<[(f32, f32); 24]> {
        let taps = (self.width * 6.0).clamp(8.0, 24.0) as usize;
        (0..taps)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / taps as f32;
                (self.width * angle.cos(), self.width * angle.sin())
            })
            .collect()
    }
}

/// How far apart neighbouring shadow copies may land, in logical pixels.
///
/// Wider than this and the copies stop blending into one halo — see
/// [`TextShadow::samples`].
const SAMPLE_SPACING: f32 = 2.0;

/// Roughly the most copies one shadow may cost. The spacing is derived from it,
/// so a larger radius spreads the same budget thinner instead of costing more —
/// the realised count lands about a third above this, since each ring rounds its
/// tap count up and none goes below eight.
const SAMPLE_BUDGET: usize = 128;

/// A soft shadow cast by the glyphs, as CSS `text-shadow`.
///
/// Usually the better of the two for legibility over a photograph: it darkens
/// the neighbourhood the glyph sits in rather than only its edge, which is
/// what actually separates the text from a busy background.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextShadow {
    /// Offset in logical pixels, positive being right and down.
    pub offset: (f32, f32),
    /// How far the shadow spreads past the glyphs.
    pub blur: f32,
    pub color: Color,
}

impl TextShadow {
    pub fn new(offset_x: f32, offset_y: f32, blur: f32, color: Color) -> Self {
        Self {
            offset: (offset_x, offset_y),
            blur,
            color,
        }
    }

    /// The offsets and colours to draw the glyphs at, back to front.
    ///
    /// Same trick as the stroke, applied to a *disc* rather than a couple of
    /// rings: rings every [`SAMPLE_SPACING`] out to the radius, each with enough
    /// taps that neighbouring copies land that far apart along it too. Alpha
    /// falls off with distance, and the copies composite with ordinary
    /// source-over blending, so they do not sum to a gaussian — they saturate
    /// towards the centre, which is the shape a shadow wants anyway.
    ///
    /// The spacing is the whole game. Two rings of twelve taps — what this used
    /// to be — leaves five-pixel gaps at blur 10, and then the copies stop
    /// reading as one halo: a glyph's square features, a colon's dots or the
    /// stem of a 4, come out as a mosaic of separate rectangles. Round glyphs
    /// hide it, which is why it survived review. Filling the disc costs about
    /// four times the draws (fill rate only — every copy re-uses the same glyph
    /// rasters, only the position differs), and past [`SAMPLE_BUDGET`] the
    /// spacing widens rather than the count growing without bound, so a very
    /// large radius degrades towards the old look instead of costing hundreds of
    /// draws. That ceiling is where the honest fix takes over: a dilate and blur
    /// on an offscreen mask, not more taps.
    pub(crate) fn samples(&self) -> SmallVec<[(f32, f32, Color); 32]> {
        let (ox, oy) = self.offset;
        let mut out = SmallVec::new();

        if self.blur <= 0.0 {
            out.push((ox, oy, self.color));
            return out;
        }

        // A disc of radius r at this spacing holds about πr²/spacing² samples,
        // so this is the spacing that fits the budget for the radius asked for.
        let spacing =
            SAMPLE_SPACING.max(self.blur * (std::f32::consts::PI / SAMPLE_BUDGET as f32).sqrt());
        let rings = (self.blur / spacing).ceil().max(1.0);

        // Outermost first: each copy paints over the ones before it, so the
        // faint wide ones have to go down before the core.
        for ring in (1..=rings as usize).rev() {
            let radius = self.blur * ring as f32 / rings;
            let taps = ((std::f32::consts::TAU * radius / spacing).ceil() as usize).max(8);
            let color = self.color.with_alpha(self.ring_alpha(radius, rings));
            for i in 0..taps {
                let angle = std::f32::consts::TAU * i as f32 / taps as f32;
                out.push((ox + radius * angle.cos(), oy + radius * angle.sin(), color));
            }
        }
        // The core keeps the colour as asked, unscaled: it sits under the fill,
        // so it is not what the eye reads as the shadow's strength, and dimming
        // it only made a tight blur weaker than the same shadow with none.
        out.push((ox, oy, self.color));
        out
    }

    /// How opaque one copy on the ring at `radius` is.
    ///
    /// A gaussian in the distance, divided by how many rings there are to stack:
    /// without that, a wide blur — many more rings, all overlapping — would come
    /// out as a slab where a tight one is a halo.
    fn ring_alpha(&self, radius: f32, rings: f32) -> f32 {
        const PEAK: f32 = 0.6;
        let t = radius / self.blur;
        (self.color.a * PEAK / rings.sqrt() * (-2.0 * t * t).exp()).min(self.color.a)
    }
}

/// The text style a container declares for its descendants.
///
/// Every field is optional and resolved independently: a state override that
/// sets only `color` leaves the metrics the widget declared alone. Properties
/// nothing declares fall back to [`Text`](crate::widgets::Text)'s defaults —
/// white, 14 logical pixels, the registered default family, normal weight.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct TextStyle {
    /// Colour of the glyphs.
    pub color: Option<Signal<Color>>,
    /// Font size in logical pixels.
    pub font_size: Option<Signal<f32>>,
    /// Font family.
    pub font_family: Option<Signal<FontFamily>>,
    /// Font weight on the CSS 100-900 scale.
    pub font_weight: Option<Signal<FontWeight>>,
    /// Contour drawn around the glyphs, under the fill.
    pub stroke: Option<Signal<TextStroke>>,
    /// Soft shadow cast by the glyphs.
    pub shadow: Option<Signal<TextShadow>>,
}

impl TextStyle {
    /// Take from `outer` every property this style does not already declare.
    ///
    /// Called as the fold moves outward from the most specific declaration —
    /// an active state override, then the widget's own — so the nearer one
    /// always wins: whatever is already set was found first.
    pub(crate) fn inherit_from(&mut self, outer: &Self) {
        self.color = self.color.or(outer.color);
        self.font_size = self.font_size.or(outer.font_size);
        self.font_family = self.font_family.or(outer.font_family);
        self.font_weight = self.font_weight.or(outer.font_weight);
        self.stroke = self.stroke.or(outer.stroke);
        self.shadow = self.shadow.or(outer.shadow);
    }
}

/// The vocabulary for declaring text style, written once.
///
/// Implemented by whoever *draws* glyphs — [`Text`](crate::widgets::Text) and
/// [`TextInput`](crate::widgets::TextInput) — and by [`TextStyle`] itself, so
/// a state override is built with the same words as the widget:
/// `when_hovered(|s| s.color(..))`.
///
/// ```ignore
/// container()
///     .child(text("quiet").color(theme.weak))
///     .child(text("loud").color(theme.strong))
/// ```
pub trait TextStyled: Sized {
    #[doc(hidden)]
    fn text_style_mut(&mut self) -> &mut TextStyle;

    /// Colour of the glyphs.
    fn color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.text_style_mut().color = Some(color.into_signal());
        self
    }

    /// Font size in logical pixels.
    fn font_size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.text_style_mut().font_size = Some(size.into_signal());
        self
    }

    /// Font family.
    fn font_family<M>(mut self, family: impl IntoSignal<FontFamily, M>) -> Self {
        self.text_style_mut().font_family = Some(family.into_signal());
        self
    }

    /// Font weight on the CSS 100-900 scale.
    fn font_weight<M>(mut self, weight: impl IntoSignal<FontWeight, M>) -> Self {
        self.text_style_mut().font_weight = Some(weight.into_signal());
        self
    }

    /// Shorthand for [`font_weight`](Self::font_weight) at `FontWeight::BOLD`.
    fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    /// Shorthand for [`font_family`](Self::font_family) at the monospace family.
    fn mono(self) -> Self {
        self.font_family(FontFamily::Monospace)
    }

    /// Contour drawn around the glyphs, under the fill.
    fn text_stroke<M>(mut self, stroke: impl IntoSignal<TextStroke, M>) -> Self {
        self.text_style_mut().stroke = Some(stroke.into_signal());
        self
    }

    /// Soft shadow cast by the glyphs.
    fn text_shadow<M>(mut self, shadow: impl IntoSignal<TextShadow, M>) -> Self {
        self.text_style_mut().shadow = Some(shadow.into_signal());
        self
    }
}

/// A partial text style is itself something to declare style on, which is what
/// lets a state override use the same builder as the widget:
/// `when_hovered(|s| s.color(..))`.
impl TextStyled for TextStyle {
    fn text_style_mut(&mut self) -> &mut TextStyle {
        self
    }
}
