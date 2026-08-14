//! How text looks — declared on the container, not on the text.
//!
//! Guido keeps one styling widget. A container decides what its box looks
//! like, and that includes the text inside it: colour, metrics, and the
//! caret and selection of an input. [`Text`](crate::widgets::Text) and
//! [`TextInput`](crate::widgets::TextInput) carry the content and nothing
//! else.
//!
//! The payoff is that text stops being a second styling system with its own
//! parallel setters, its own caches, and no access to anything else. Because
//! the declaration lives on the container, text colour reaches the state
//! layers ([`StateStyle::text_color`](crate::widgets::StateStyle::text_color))
//! and the animation machinery
//! ([`animate_text_color`](crate::widgets::Container::animate_text_color))
//! that already exist there, instead of needing a second copy of both.
//!
//! # Inheritance
//!
//! A text resolves each property from the nearest ancestor that declares it.
//! Containers in between that say nothing about text are transparent, so the
//! declaration can sit on the card while the layout rows underneath stay
//! plain:
//!
//! ```ignore
//! container().text_color(theme.text).font_size(14.0)
//!     .child(container().layout(Flex::row())
//!         .child(text("inherits both"))
//!         .child(container().font_size(21.0).child(text("own size, inherited colour"))))
//! ```
//!
//! Resolution is **per property**, like CSS computed values: the inner
//! container above overrides the size without disturbing the colour. Whole
//! struct nearest-wins would have made every partial declaration reset
//! everything it did not mention.
//!
//! # Why the walk starts at the text
//!
//! The obvious alternative is for each container to compute a resolved style
//! and hand it down. It is wrong here, and quietly so: guido skips layout for
//! subtrees whose constraints did not change, so a container could recompute
//! its style, its children skip their layout, and the text would keep
//! rendering at the old size.
//!
//! Walking up from the text instead makes the reactivity fall out. The walk
//! happens inside the text's own
//! [`with_signal_tracking`](crate::reactive::with_signal_tracking) scope, so
//! the text subscribes to exactly the ancestor signals it actually read, and
//! stops at the first ancestor that answers. A container changing its colour
//! invalidates the descendants that inherited it and not the ones that
//! declared their own — no invalidation plumbing, and no window in which a
//! stale value can be drawn.
//!
//! Which ancestors *declare* what is fixed once the tree is built, so the
//! shape of the walk cannot change under a text without the container being
//! rebuilt, and a rebuild re-registers the subtree anyway.

use smallvec::SmallVec;

use crate::reactive::Signal;

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
    /// Same trick as the stroke, and the approximation lands far better here:
    /// a shadow is already soft and low-contrast, so the banding a ring of
    /// samples produces is invisible where a scalloped stroke would not be.
    /// The rings composite with ordinary source-over blending, so they do not
    /// sum to a gaussian — they saturate towards the centre, which is the
    /// shape a shadow wants anyway.
    pub(crate) fn samples(&self) -> SmallVec<[(f32, f32, Color); 25]> {
        let (ox, oy) = self.offset;
        let mut out = SmallVec::new();

        if self.blur <= 0.0 {
            out.push((ox, oy, self.color));
            return out;
        }

        // Outermost first: the faint wide ring, then the tighter one, then the
        // solid core on top.
        let taps = (self.blur * 1.5).clamp(8.0, 12.0) as usize;
        for (radius, alpha) in [(self.blur, 0.35), (self.blur * 0.5, 0.6)] {
            for i in 0..taps {
                let angle = std::f32::consts::TAU * i as f32 / taps as f32;
                out.push((
                    ox + radius * angle.cos(),
                    oy + radius * angle.sin(),
                    self.color.with_alpha(self.color.a * alpha),
                ));
            }
        }
        out.push((ox, oy, self.color));
        out
    }
}

/// The text style a container declares for its descendants.
///
/// Every field is optional and resolved independently: a container that sets
/// only `color` leaves the metrics to whatever an ancestor said, rather than
/// resetting them. Properties no ancestor declares fall back to
/// [`Text`](crate::widgets::Text)'s defaults — white, 14 logical pixels, the
/// registered default family, normal weight.
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
    /// Caret colour. Only [`TextInput`](crate::widgets::TextInput) reads it.
    pub cursor_color: Option<Signal<Color>>,
    /// Selection highlight colour. Only
    /// [`TextInput`](crate::widgets::TextInput) reads it.
    pub selection_color: Option<Signal<Color>>,
}

impl TextStyle {
    /// Whether nothing at all is declared.
    ///
    /// A container in this state is not recorded on its node, so the walk
    /// skips it with a null check instead of a dereference.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Take from `outer` every property this style does not already declare.
    ///
    /// Called as the walk moves away from the text, so a nearer container
    /// always wins: whatever is already set was found closer.
    pub(crate) fn inherit_from(&mut self, outer: &Self) {
        self.color = self.color.or(outer.color);
        self.font_size = self.font_size.or(outer.font_size);
        self.font_family = self.font_family.or(outer.font_family);
        self.font_weight = self.font_weight.or(outer.font_weight);
        self.stroke = self.stroke.or(outer.stroke);
        self.shadow = self.shadow.or(outer.shadow);
        self.cursor_color = self.cursor_color.or(outer.cursor_color);
        self.selection_color = self.selection_color.or(outer.selection_color);
    }

    /// Whether every property has been resolved, so the walk can stop early.
    pub(crate) fn is_complete(&self) -> bool {
        self.color.is_some()
            && self.font_size.is_some()
            && self.font_family.is_some()
            && self.font_weight.is_some()
            && self.stroke.is_some()
            && self.shadow.is_some()
            && self.cursor_color.is_some()
            && self.selection_color.is_some()
    }
}
