//! `.autofocus()`: an input that takes the keyboard when it first appears.
//!
//! The rule has two halves, and each has a test here because each is what makes
//! the other safe: the offer is made *once*, at the first layout, and only when
//! no widget already holds focus.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::reactive::focus::{clear_focus, focused_widget, has_focus, request_focus};
use guido::tree::{Tree, WidgetId};

/// A tree the test can lay out repeatedly, as a running app does.
struct Screen {
    tree: Tree,
}

impl Screen {
    fn new() -> Self {
        clear_focus();
        Self { tree: Tree::new() }
    }

    fn add(&mut self, input: TextInput) -> WidgetId {
        let id = self.tree.register(Box::new(input));
        self.tree
            .with_widget_mut(id, |w, id, t| w.register_children(t, id));
        id
    }

    fn layout(&mut self, id: WidgetId) {
        self.tree.with_widget_mut(id, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 40.0))
        });
    }
}

#[test]
fn an_autofocused_input_takes_the_keyboard_when_it_is_laid_out() {
    let mut screen = Screen::new();
    let field = screen.add(text_input(create_signal(String::new())).autofocus());

    assert_eq!(
        focused_widget(),
        None,
        "nothing should be focused before the first layout — the tree is where \
         focus lives, and the widget is not in one yet"
    );

    screen.layout(field);

    assert!(has_focus(field));
}

#[test]
fn an_input_without_autofocus_waits_to_be_clicked() {
    let mut screen = Screen::new();
    let field = screen.add(text_input(create_signal(String::new())));

    screen.layout(field);

    assert_eq!(focused_widget(), None);
}

#[test]
fn autofocus_does_not_take_focus_from_whoever_has_it() {
    let mut screen = Screen::new();
    let clicked = screen.add(text_input(create_signal(String::new())));
    let latecomer = screen.add(text_input(create_signal(String::new())).autofocus());
    screen.layout(clicked);
    request_focus(&screen.tree, clicked);

    screen.layout(latecomer);

    assert!(
        has_focus(clicked),
        "an input appearing later must not pull the keyboard out from under the \
         one being typed into"
    );
}

#[test]
fn the_first_of_several_autofocusing_inputs_wins() {
    // A lock screen with two monitors is this: the same view built per output,
    // every copy asking for the focus.
    let mut screen = Screen::new();
    let first = screen.add(text_input(create_signal(String::new())).autofocus());
    let second = screen.add(text_input(create_signal(String::new())).autofocus());

    screen.layout(first);
    screen.layout(second);

    assert!(has_focus(first));
    assert!(!has_focus(second));
}

#[test]
fn a_relayout_does_not_ask_again() {
    let mut screen = Screen::new();
    let field = screen.add(text_input(create_signal(String::new())).autofocus());
    let other = screen.add(text_input(create_signal(String::new())));
    screen.layout(field);
    screen.layout(other);
    request_focus(&screen.tree, other);

    // Whatever a running app relayouts for — a resize, a scale change, an edit.
    screen.layout(field);

    assert!(
        has_focus(other),
        "autofocus is about appearing, not about being laid out; asking again on \
         every relayout would drag focus back on any resize"
    );
}
