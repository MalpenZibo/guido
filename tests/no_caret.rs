//! `no_caret()`: nothing drawn, not merely nothing blinking.
//!
//! The first version of this option only stopped the blink. `cursor_visible`
//! stayed at what the constructor left it — true — so a field asked for no caret
//! got a permanent one instead: still, and still there. Scheduling tests could not
//! see that, because the wakeups were correctly absent. These look at what is
//! drawn.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::reactive::focus::{clear_focus, has_focus, request_focus};

/// Every rectangle a focused input draws. With no selection, the caret is the
/// only one there can be.
fn rects(input: TextInput) -> Vec<Rect> {
    clear_focus();
    let mut harness = Harness::laid_out(input, 200.0, 40.0);
    request_focus(&harness.tree, harness.root);
    harness.painted_rects()
}

#[test]
fn a_focused_field_draws_its_caret() {
    let drawn = rects(text_input(create_signal("hi".to_owned())));

    assert_eq!(drawn.len(), 1, "the caret, and nothing else: {drawn:?}");
}

#[test]
fn no_caret_draws_no_caret() {
    let drawn = rects(text_input(create_signal("hi".to_owned())).no_caret());

    assert!(
        drawn.is_empty(),
        "asked for no caret and got one anyway: {drawn:?}"
    );
}

#[test]
fn no_caret_still_takes_the_focus() {
    // The point of the option is losing the caret, not losing the keyboard.
    clear_focus();
    let harness = Harness::laid_out(
        text_input(create_signal(String::new()))
            .no_caret()
            .autofocus(),
        200.0,
        40.0,
    );

    assert!(has_focus(harness.root));
}
