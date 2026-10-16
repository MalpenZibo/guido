//! The surface losing the keyboard leaves the focus where it was (#726).
//!
//! `wl_keyboard.leave` is `Event::FocusOut` and `wl_keyboard.enter` is
//! `Event::FocusIn`. A text input used to answer the first by giving the focus
//! up, and nothing gave it back on the second, so the next key went nowhere; a
//! focusable container kept it through the same round trip. Both now keep it,
//! as a `GtkWindow` keeps its focus widget while it is not active. The field
//! draws no caret while its surface is without the keyboard.
//!
//! The popup half of the decision — a surface getting back the widget it had
//! focused when the focus went elsewhere — needs two surfaces and the loop, and
//! is in `tests/headless_app.rs`.

mod common;

use std::time::Instant;

use common::Harness;
use guido::prelude::*;
use guido::reactive::focus::has_focus;

fn key(c: char) -> Event {
    Event::KeyDown {
        key: Key::Char(c),
        modifiers: Modifiers::default(),
        repeat: false,
    }
}

#[test]
fn a_field_keeps_the_focus_while_its_surface_is_without_the_keyboard() {
    let value = create_signal(String::new());
    let mut field = Harness::focused(text_input(value), 200.0, 40.0);
    field.send_at(key('a'), Instant::now());
    assert_eq!(
        field.painted_rects().len(),
        1,
        "focused, with the keyboard: the caret is drawn"
    );

    field.send_at(Event::FocusOut, Instant::now());
    assert!(
        has_focus(field.root),
        "the keyboard leaving the surface does not take the focus off the field"
    );
    assert!(
        field.painted_rects().is_empty(),
        "but with the keyboard elsewhere there is no caret to draw: {:?}",
        field.painted_rects()
    );

    field.send_at(Event::FocusIn, Instant::now());
    assert_eq!(
        field.painted_rects().len(),
        1,
        "the keyboard back, the caret back"
    );
    field.send_at(key('b'), Instant::now());
    assert_eq!(
        value.get_untracked(),
        "ab",
        "and the next key is typed into the field"
    );
}

#[test]
fn a_focusable_container_keeps_the_focus_through_the_same_round_trip() {
    let mut card = Harness::focused(
        container().width(100.0).height(40.0).focusable(true),
        200.0,
        40.0,
    );

    card.send(Event::FocusOut);
    assert!(has_focus(card.root), "kept while the keyboard is away");
    card.send(Event::FocusIn);
    assert!(has_focus(card.root), "and still there when it comes back");
}
