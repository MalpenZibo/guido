//! The caret, the selection band and the placeholder are `TextInput`'s own
//! methods.
//!
//! The imports are half of what this file says. Nothing here reaches for the
//! prelude, and nothing imports a trait: a caller who takes `text_input` from
//! `guido::widgets` can say what colour the field's own furniture is, with no
//! second name to remember. That was #354, where the three lived on a trait
//! with one live impl.
//!
//! The other half is that the colours arrive: the caret and the band are the
//! only two rectangles a focused field draws, so what they are filled with is
//! readable from the paint. `placeholder_color` is declared here for the
//! spelling and asserted in `tests/placeholder.rs`, where the text a field
//! draws when it is empty already lives.

mod common;

use common::Harness;
use guido::reactive::create_signal;
use guido::widgets::{Color, Event, Key, Modifiers, TextInput, text_input};

const CARET: Color = Color::rgb(0.4, 0.8, 1.0);
const BAND: Color = Color::rgba(0.4, 0.6, 1.0, 0.4);

/// A field that says all three, in the order a caller would write them.
fn field() -> TextInput {
    text_input(create_signal("hi".to_owned()))
        .cursor_color(CARET)
        .selection_color(BAND)
        .placeholder_color(Color::rgb(0.5, 0.5, 0.5))
}

/// The fill colour of every rectangle a focused field draws, in paint order.
fn fills(input: TextInput, select_all: bool) -> Vec<Color> {
    let mut harness = Harness::focused(input, 200.0, 40.0);
    if select_all {
        harness.send(Event::KeyDown {
            key: Key::Char('a'),
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        });
    }

    harness
        .painted_rounded_rects()
        .into_iter()
        .map(|(_, color)| color)
        .collect()
}

#[test]
fn a_caret_is_drawn_in_the_colour_the_field_declared() {
    assert_eq!(
        fills(field(), false),
        vec![CARET],
        "the caret is the only rectangle an unselected field draws, and it is \
         the declared colour"
    );
}

#[test]
fn a_selection_band_is_drawn_in_the_colour_the_field_declared() {
    let drawn = fills(field(), true);

    assert_eq!(
        drawn,
        vec![BAND, CARET],
        "a field with all of its text selected draws the band it declared, \
         and the caret on top of it"
    );
}
