//! How an input's own furniture looks — caret, selection, placeholder.
//!
//! These three are not text style. A [`Text`](crate::widgets::Text) draws
//! glyphs and nothing else; only a [`TextInput`](crate::widgets::TextInput)
//! draws a caret, a selection band or a placeholder. They lived in
//! [`TextStyle`](crate::widgets::TextStyle) for a while, each with a doc
//! comment admitting that only one widget ever read it, which is the shape of
//! a property in the wrong struct.
//!
//! Splitting them out costs nothing at the call site and buys two things: a
//! text no longer walks its ancestors looking for properties it cannot draw,
//! and the next property belonging to an input has an obvious place to go.
//!
//! Resolution works exactly like [`TextStyle`]'s — per property, nearest
//! declaration wins, walked from the input itself so the subscription lands
//! where the value is read. See that module for why the walk starts there.
//!
//! [`TextStyle`]: crate::widgets::TextStyle

use crate::reactive::{IntoSignal, Signal};

use super::widget::Color;

/// The input style a container declares for the inputs below it.
///
/// Every field is optional and resolved independently, so a container setting
/// only the caret colour leaves the selection to whatever an ancestor said.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct InputStyle {
    /// Colour of the caret. Defaults to the resolved text colour — an input
    /// that only sets `text_color` should not sprout a blue cursor.
    pub cursor_color: Option<Signal<Color>>,
    /// Colour of the selection band drawn behind the selected glyphs.
    pub selection_color: Option<Signal<Color>>,
    /// Colour of the placeholder. Defaults to the text colour at reduced
    /// alpha — a placeholder is the same text, quieter.
    pub placeholder_color: Option<Signal<Color>>,
}

impl InputStyle {
    /// Whether nothing at all is declared.
    ///
    /// A container in this state is not recorded on its node, so the walk
    /// skips it with a null check instead of a dereference.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Take from `outer` every property this style does not already declare.
    ///
    /// Called as the walk moves away from the input, so a nearer container
    /// always wins: whatever is already set was found closer.
    pub(crate) fn inherit_from(&mut self, outer: &Self) {
        self.cursor_color = self.cursor_color.or(outer.cursor_color);
        self.selection_color = self.selection_color.or(outer.selection_color);
        self.placeholder_color = self.placeholder_color.or(outer.placeholder_color);
    }

    /// Whether every property has been resolved, so the walk can stop early.
    pub(crate) fn is_complete(&self) -> bool {
        self.cursor_color.is_some()
            && self.selection_color.is_some()
            && self.placeholder_color.is_some()
    }
}

/// The vocabulary for declaring an input's own furniture, written once.
///
/// Implemented by [`TextInput`](crate::widgets::TextInput) alone, because it
/// is the only widget that draws any of it. That is the whole point of the
/// trait existing separately from
/// [`TextStyled`](crate::widgets::TextStyled): a placeholder colour has
/// nowhere to land except on the widget that draws the placeholder.
pub trait InputStyled: Sized {
    #[doc(hidden)]
    fn input_style_mut(&mut self) -> &mut InputStyle;

    /// Colour of the caret.
    fn cursor_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().cursor_color = Some(color.into_signal());
        self
    }

    /// Colour of the selection band behind the selected glyphs.
    fn selection_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().selection_color = Some(color.into_signal());
        self
    }

    /// Colour of the placeholder.
    fn placeholder_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().placeholder_color = Some(color.into_signal());
        self
    }
}
