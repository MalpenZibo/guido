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
//! text is never asked about properties it cannot draw, and the next property
//! belonging to an input has an obvious place to go.
//!
//! Declared on the input itself, like every other part of how it looks — see
//! [`TextStyle`] for why the widget that draws is the widget that says.
//!
//! [`TextStyle`]: crate::widgets::TextStyle

use crate::reactive::{IntoSignal, Signal};

use super::widget::Color;

/// The caret, selection and placeholder an input declares for itself.
///
/// Every field is optional and independent, so declaring only the caret colour
/// leaves the selection at its default.
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

impl InputStyle {}

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

/// As for [`TextStyle`](crate::widgets::TextStyle): a partial style is
/// something to declare on.
impl InputStyled for InputStyle {
    fn input_style_mut(&mut self) -> &mut InputStyle {
        self
    }
}
