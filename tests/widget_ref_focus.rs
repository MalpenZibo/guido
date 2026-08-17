//! `WidgetRef::focus()`: moving the keyboard from application code.
//!
//! The interesting case is the early one. Focus is resolved against the tree, so
//! a request from a startup effect names a widget that does not exist yet — it has
//! to wait rather than be dropped. Compose, iced and Flutter all have a version of
//! this problem; the tests here pin guido's answer to it.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::reactive::focus::{apply_pending_focus, clear_focus, focused_widget, has_focus};
use guido::tree::{Tree, WidgetId};

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

    /// One frame: lay the widget out, then apply whatever focus was asked for —
    /// the order the loop uses, and the reason a handle attached this frame works.
    fn frame(&mut self, id: WidgetId) {
        self.tree.with_widget_mut(id, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 40.0))
        });
        apply_pending_focus(&self.tree);
    }
}

#[test]
fn focus_moves_the_keyboard_to_the_named_input() {
    let mut screen = Screen::new();
    let handle = create_widget_ref();
    let field = screen.add(text_input(create_signal(String::new())).widget_ref(handle));
    let other = screen.add(text_input(create_signal(String::new())));
    screen.frame(field);
    screen.frame(other);

    handle.focus();
    screen.frame(field);

    assert!(has_focus(field));
    assert!(!has_focus(other));
}

#[test]
fn a_request_before_the_first_layout_waits_instead_of_being_dropped() {
    // What `focus()` from a startup effect looks like: the view is composed, the
    // handle exists, the widget does not.
    let mut screen = Screen::new();
    let handle = create_widget_ref();

    handle.focus();
    assert_eq!(
        focused_widget(),
        None,
        "there is nothing to focus yet, and nothing should have been invented"
    );

    let field = screen.add(text_input(create_signal(String::new())).widget_ref(handle));
    screen.frame(field);

    assert!(
        has_focus(field),
        "the request has to survive until the widget it names exists"
    );
}

#[test]
fn the_last_request_of_a_frame_is_the_one_that_lands() {
    let mut screen = Screen::new();
    let first = create_widget_ref();
    let second = create_widget_ref();
    let field_a = screen.add(text_input(create_signal(String::new())).widget_ref(first));
    let field_b = screen.add(text_input(create_signal(String::new())).widget_ref(second));
    screen.frame(field_a);
    screen.frame(field_b);

    first.focus();
    second.focus();
    screen.frame(field_a);

    assert!(has_focus(field_b), "two answers, the later one wins");
    assert!(!has_focus(field_a));
}

#[test]
fn blur_gives_the_keyboard_back() {
    let mut screen = Screen::new();
    let handle = create_widget_ref();
    let field = screen.add(text_input(create_signal(String::new())).widget_ref(handle));
    screen.frame(field);
    handle.focus();
    screen.frame(field);
    assert!(has_focus(field));

    handle.blur();

    assert_eq!(focused_widget(), None);
}

#[test]
fn a_handle_reports_whether_it_holds_the_focus() {
    let mut screen = Screen::new();
    let handle = create_widget_ref();
    let field = screen.add(text_input(create_signal(String::new())).widget_ref(handle));
    screen.frame(field);

    assert!(!handle.is_focused());

    handle.focus();
    screen.frame(field);

    assert!(handle.is_focused());
}

#[test]
fn an_unattached_handle_answers_rather_than_panicking() {
    // Nothing has ever been laid out with this handle.
    let handle = create_widget_ref();

    assert_eq!(handle.widget(), None);
    assert!(!handle.is_focused());
    handle.blur(); // must be a no-op, not a panic
}
