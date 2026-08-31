//! A masked field must not hand its contents to anything outside itself.
//!
//! Both routes out are tested, because the dangerous one is not the obvious one:
//! Ctrl+C takes a keystroke, while the *primary selection* is filled by an
//! ordinary mouse drag and pasted by a middle click anywhere else. A lock screen
//! is where that matters — the clipboard outlives the unlock.
//!
//! Pasting *in* is checked too, and must keep working: blocking it stops
//! password managers, not attackers.

use guido::layout::Constraints;
use guido::prelude::*;
use guido::reactive::clipboard::{
    clear_system_clipboard, clipboard_paste, primary_paste, set_system_clipboard,
    take_clipboard_change,
};
use guido::reactive::focus::request_focus;
use guido::tree::{Tree, WidgetId};

const SECRET: &str = "correct horse battery staple";

struct Field {
    tree: Tree,
    id: WidgetId,
    value: RwSignal<String>,
}

impl Field {
    /// A focused, laid-out input holding `SECRET`.
    fn new(password: bool) -> Self {
        // Anything a previous test left behind would make this one lie.
        clear_system_clipboard();
        let _ = take_clipboard_change();

        let value = create_signal(SECRET.to_owned());
        let input = text_input(value).password(password);

        let mut tree = Tree::new();
        let id = tree.register(Box::new(input));
        tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(id, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 40.0))
        });
        request_focus(&tree, id);

        Self { tree, id, value }
    }

    fn key(&mut self, key: Key, ctrl: bool) {
        let event = Event::KeyDown {
            key,
            modifiers: Modifiers {
                ctrl,
                ..Default::default()
            },
        };
        self.event(&event);
    }

    fn event(&mut self, event: &Event) {
        let id = self.id;
        self.tree.with_widget_mut(id, |w, id, t| {
            w.event(t, id, event);
        });
    }

    /// Drag across the whole field, which is how the primary selection is filled.
    fn drag_select_all(&mut self) {
        // The field is only `font_size * 1.2` tall, so the y has to stay well
        // inside it or the hit test drops the press and the drag selects nothing.
        self.event(&Event::mouse_down(0.0, 4.0, MouseButton::Left));
        self.event(&Event::mouse_move(399.0, 4.0));
        self.event(&Event::mouse_up(399.0, 4.0, MouseButton::Left));
    }
}

/// Everything that could carry the value out of the widget.
fn exported() -> Vec<String> {
    [take_clipboard_change(), clipboard_paste(), primary_paste()]
        .into_iter()
        .flatten()
        .collect()
}

#[test]
fn a_password_field_does_not_copy_itself() {
    let mut field = Field::new(true);
    field.key(Key::Char('a'), true);
    field.key(Key::Char('c'), true);

    let exported = exported();
    assert!(
        !exported.iter().any(|text| text.contains(SECRET)),
        "the password left the field: {exported:?}"
    );
}

#[test]
fn a_password_field_does_not_leak_through_the_primary_selection() {
    // The one nobody presses a key for.
    let mut field = Field::new(true);
    field.drag_select_all();

    let exported = exported();
    assert!(
        !exported.iter().any(|text| text.contains(SECRET)),
        "a mouse drag published the password: {exported:?}"
    );
}

#[test]
fn a_password_field_refuses_the_cut_rather_than_deleting_quietly() {
    let mut field = Field::new(true);
    field.key(Key::Char('a'), true);
    field.key(Key::Char('x'), true);

    let exported = exported();
    assert!(
        !exported.iter().any(|text| text.contains(SECRET)),
        "the password left the field: {exported:?}"
    );
    assert_eq!(
        field.value.get_untracked(),
        SECRET,
        "a cut that cannot copy must not delete either — otherwise Ctrl+X is a \
         delete the user believes filled the clipboard"
    );
}

#[test]
fn an_ordinary_field_still_copies() {
    // The guard has to be about password mode, not about copying.
    let mut field = Field::new(false);
    field.key(Key::Char('a'), true);
    field.key(Key::Char('c'), true);

    assert_eq!(take_clipboard_change().as_deref(), Some(SECRET));
}

#[test]
fn an_ordinary_field_still_fills_the_primary_selection() {
    let mut field = Field::new(false);
    field.drag_select_all();

    assert_eq!(primary_paste().as_deref(), Some(SECRET));
}

#[test]
fn a_password_field_still_takes_a_paste() {
    let mut field = Field::new(true);
    field.key(Key::Char('a'), true);
    field.key(Key::Backspace, false);
    set_system_clipboard("hunter2".to_owned());

    field.key(Key::Char('v'), true);

    assert_eq!(field.value.get_untracked(), "hunter2");
}
