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

use std::time::Instant;

use crate::jobs::RequiredJob;
use crate::reactive::{IntoSignal, Signal};
use crate::widgets::container::AnimationState;

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

/// Where a declared text property keeps its motion.
///
/// Boxed and absent by default, like the style beside it: a text that declares
/// no timing pays a null check rather than two animation states. Only the two
/// interpolable properties are here — see [`declares_text_style`].
#[derive(Default)]
pub(crate) struct TextAnims {
    pub(crate) color: Option<AnimationState<Color>>,
    pub(crate) font_size: Option<AnimationState<f32>>,
}

impl TextAnims {
    /// Point both motions at what the style now resolves to, and say what has
    /// to happen next.
    ///
    /// Returns the job the frame after this one wants, and the size to measure
    /// with — the animated one while it is moving, the declared one otherwise.
    ///
    /// A motion that has never run is *seeded* rather than eased: its stored
    /// value is whatever `get_untracked` saw when the builder ran, and a write
    /// landing between construction and the first layout would otherwise make
    /// the first frame ease from a value that was already stale.
    pub(crate) fn retarget(
        &mut self,
        color: Color,
        size: f32,
        now: Instant,
    ) -> (Option<RequiredJob>, f32) {
        let mut wants = None;
        if let Some(a) = self.color.as_mut() {
            if a.is_initial() {
                a.set_immediate(color);
            } else {
                a.animate_to(color, now);
            }
            if a.is_animating() {
                wants = Some(RequiredJob::Paint);
            }
        }
        let mut measured = size;
        if let Some(a) = self.font_size.as_mut() {
            if a.is_initial() {
                a.set_immediate(size);
            } else {
                a.animate_to(size, now);
            }
            measured = a.displayed();
            // A size still moving has to be measured again, not merely redrawn.
            if a.is_animating() {
                wants = Some(RequiredJob::Layout);
            }
        }
        (wants, measured)
    }

    /// Move both motions on, and say what the next frame wants.
    ///
    /// `None` where nothing moved: a transition inside its `delay_ms` is
    /// animating and has produced no new value, and asking for a paint there
    /// would wake the loop every frame for a picture that has not changed.
    pub(crate) fn advance(&mut self, now: Instant) -> Option<RequiredJob> {
        let mut wants = None;
        if let Some(a) = self.color.as_mut()
            && a.advance(now).is_changed()
        {
            wants = Some(RequiredJob::Paint);
        }
        if let Some(a) = self.font_size.as_mut()
            && a.advance(now).is_changed()
        {
            wants = Some(RequiredJob::Layout);
        }
        wants
    }

    /// Whether either motion is still on its way, which is what keeps the
    /// frames coming while a delay is running.
    pub(crate) fn is_animating(&self) -> bool {
        self.color.as_ref().is_some_and(|a| a.is_animating())
            || self.font_size.as_ref().is_some_and(|a| a.is_animating())
    }

    /// Whether a colour motion was declared. The declared colour has to be read
    /// under layout tracking to retarget it, and that subscription is worth
    /// paying only where there is something to retarget — every other text
    /// reads its colour at paint alone, as it always did.
    pub(crate) fn animates_color(&self) -> bool {
        self.color.is_some()
    }
}

/// The vocabulary for declaring text style, on a widget that draws glyphs.
///
/// Written once and emitted for both [`Text`](crate::widgets::Text) and
/// [`TextInput`](crate::widgets::TextInput), which is what keeps the two in
/// step: a property added here reaches both, and one that reaches only one of
/// them cannot be written.
///
/// These are the *declaration* sites, so `color` and `font_size` carry how they
/// move as well as what they are — `color(theme.warn.transition(200.0))`. The
/// four below them take values only, because they are not values that can be
/// interpolated: a family and a weight snap to an installed face, and a stroke
/// and a shadow are records with no `Animatable` between them.
///
/// The *override* site is [`TextStyle`], and its setters take values alone.
/// That is not a second vocabulary but the rule falling out of the types: a
/// timing on `when_hovered(|s| s.color(..))` is a compile error rather than a
/// value quietly ignored, which is the same shape `Container` and `StateStyle`
/// already have.
macro_rules! declares_text_style {
    ($widget:ty, $style:ident, $anims:ident) => {
        impl $widget {
            fn text_style_mut(&mut self) -> &mut $crate::widgets::text_style::TextStyle {
                self.$style.get_or_insert_with(Default::default)
            }

            /// Point the declared motions at the resolved style, and hand back
            /// the size to measure with.
            ///
            /// Emitted rather than written per widget: the setters above are
            /// the same for both, and so is what has to happen behind them — a
            /// widget that took a transition and did not carry it would be the
            /// silently-dropped value the whole rule exists to prevent.
            fn retarget_text_anims(
                &mut self,
                tree: &$crate::tree::Tree,
                id: $crate::tree::WidgetId,
                color: $crate::widgets::Color,
                size: f32,
            ) -> f32 {
                let Some(anims) = self.$anims.as_deref_mut() else {
                    return size;
                };
                let (wants, measured) = anims.retarget(color, size, tree.frame_instant());
                if let Some(required) = wants {
                    $crate::jobs::request_job(id, $crate::jobs::JobRequest::Animation(required));
                }
                measured
            }

            /// Move the declared motions on, and ask for the frame that carries
            /// them further.
            fn advance_text_anims(
                &mut self,
                tree: &$crate::tree::Tree,
                id: $crate::tree::WidgetId,
            ) -> bool {
                let now = tree.frame_instant();
                let Some(anims) = self.$anims.as_deref_mut() else {
                    return false;
                };
                let moved = anims.advance(now);
                let running = anims.is_animating();
                if let Some(required) = moved {
                    $crate::jobs::request_job(id, $crate::jobs::JobRequest::Animation(required));
                } else if running {
                    // Inside a delay: still on its way, nothing new to draw, so
                    // it asks to be woken without asking for a picture.
                    $crate::jobs::request_job(
                        id,
                        $crate::jobs::JobRequest::Animation($crate::jobs::RequiredJob::None),
                    );
                }
                running
            }

            /// Whether a colour motion was declared — see
            /// [`TextAnims::animates_color`].
            fn animates_text_color(&self) -> bool {
                self.$anims
                    .as_deref()
                    .is_some_and($crate::widgets::text_style::TextAnims::animates_color)
            }

            /// Colour of the glyphs, and how it moves.
            pub fn color<M>(
                mut self,
                color: impl $crate::animation::IntoAnimated<$crate::widgets::Color, M>,
            ) -> Self {
                let signal = $crate::widgets::container::declare(
                    &mut self.$anims,
                    color,
                    |a: &mut $crate::widgets::text_style::TextAnims| &mut a.color,
                );
                self.text_style_mut().color = Some(signal);
                self
            }

            /// Font size in logical pixels, and how it moves.
            pub fn font_size<M>(
                mut self,
                size: impl $crate::animation::IntoAnimated<f32, M>,
            ) -> Self {
                let signal = $crate::widgets::container::declare(
                    &mut self.$anims,
                    size,
                    |a: &mut $crate::widgets::text_style::TextAnims| &mut a.font_size,
                );
                self.text_style_mut().font_size = Some(signal);
                self
            }

            /// Font family.
            pub fn font_family<M>(
                mut self,
                family: impl $crate::reactive::IntoSignal<$crate::widgets::FontFamily, M>,
            ) -> Self {
                self.text_style_mut().font_family = Some(family.into_signal());
                self
            }

            /// Font weight on the CSS 100-900 scale.
            pub fn font_weight<M>(
                mut self,
                weight: impl $crate::reactive::IntoSignal<$crate::widgets::FontWeight, M>,
            ) -> Self {
                self.text_style_mut().font_weight = Some(weight.into_signal());
                self
            }

            /// Shorthand for [`font_weight`](Self::font_weight) at `FontWeight::BOLD`.
            pub fn bold(self) -> Self {
                self.font_weight($crate::widgets::FontWeight::BOLD)
            }

            /// Shorthand for [`font_family`](Self::font_family) at the monospace family.
            pub fn mono(self) -> Self {
                self.font_family($crate::widgets::FontFamily::Monospace)
            }

            /// Contour drawn around the glyphs, under the fill.
            pub fn text_stroke<M>(
                mut self,
                stroke: impl $crate::reactive::IntoSignal<$crate::widgets::TextStroke, M>,
            ) -> Self {
                self.text_style_mut().stroke = Some(stroke.into_signal());
                self
            }

            /// Soft shadow cast by the glyphs.
            pub fn text_shadow<M>(
                mut self,
                shadow: impl $crate::reactive::IntoSignal<$crate::widgets::TextShadow, M>,
            ) -> Self {
                self.text_style_mut().shadow = Some(shadow.into_signal());
                self
            }
        }
    };
}

pub(crate) use declares_text_style;

/// The same vocabulary for an override, which supplies values and never a
/// timing — see [`declares_text_style`] for why the two differ.
impl TextStyle {
    /// Colour of the glyphs.
    ///
    /// An override supplies a value and never a timing: the motion belongs to
    /// whoever *declared* the property, and a state layer does not declare it.
    /// So this is a compile error rather than a value quietly ignored, which is
    /// the whole reason [`Animated`](crate::animation::Animated) is not an
    /// [`IntoSignal`]:
    ///
    /// ```compile_fail
    /// use guido::prelude::*;
    ///
    /// const HOT: Color = Color::rgb(0.9, 0.3, 0.2);
    /// let _ = text("label").when_hovered(|s| s.color(HOT.transition(80.0)));
    /// ```
    pub fn color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.color = Some(color.into_signal());
        self
    }

    /// Font size in logical pixels.
    pub fn font_size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.font_size = Some(size.into_signal());
        self
    }

    /// Font family.
    pub fn font_family<M>(mut self, family: impl IntoSignal<FontFamily, M>) -> Self {
        self.font_family = Some(family.into_signal());
        self
    }

    /// Font weight on the CSS 100-900 scale.
    pub fn font_weight<M>(mut self, weight: impl IntoSignal<FontWeight, M>) -> Self {
        self.font_weight = Some(weight.into_signal());
        self
    }

    /// Shorthand for [`font_weight`](Self::font_weight) at `FontWeight::BOLD`.
    pub fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    /// Shorthand for [`font_family`](Self::font_family) at the monospace family.
    pub fn mono(self) -> Self {
        self.font_family(FontFamily::Monospace)
    }

    /// Contour drawn around the glyphs, under the fill.
    pub fn text_stroke<M>(mut self, stroke: impl IntoSignal<TextStroke, M>) -> Self {
        self.stroke = Some(stroke.into_signal());
        self
    }

    /// Soft shadow cast by the glyphs.
    pub fn text_shadow<M>(mut self, shadow: impl IntoSignal<TextShadow, M>) -> Self {
        self.shadow = Some(shadow.into_signal());
        self
    }
}
