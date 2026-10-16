//! `password_input`: a field over a `Password`, driven the way a user and an
//! application drive it.
//!
//! What it must not leave behind in memory is `password_leaves_no_copy.rs`;
//! what it must not hand to the clipboard is `password_never_exports.rs`. This
//! file is the rest: the field and the handle agree about what it holds, and
//! the editing a text field offers is there, minus what would copy or reveal.
//! A write from outside any event — `set`, `clear` — reaches the field on the
//! layout it schedules, which only the unit tests beside the widget can pump.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::Harness;
use guido::prelude::*;
use guido::reactive::clipboard::{SelectionKind, answer_pastes, deliver_pastes};

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 40.0;

/// A focused password field, the handle it is bound to, and a clock.
struct Field {
    harness: Harness,
    password: Password,
    now: Instant,
}

impl Field {
    fn new(build: impl FnOnce(PasswordInput, Password) -> PasswordInput) -> Self {
        let password = create_password();
        Self {
            harness: Harness::focused(build(password_input(password), password), WIDTH, HEIGHT),
            password,
            now: Instant::now(),
        }
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.now += Duration::from_millis(1);
        self.harness.send_at(
            Event::KeyDown {
                key,
                modifiers,
                repeat: false,
            },
            self.now,
        );
        self.harness.lay_out(WIDTH, HEIGHT);
    }

    fn key(&mut self, key: Key) {
        self.press(key, Modifiers::default());
    }

    fn ctrl(&mut self, c: char) {
        self.press(
            Key::Char(c),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        );
    }

    fn type_str(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Char(c));
        }
    }

    /// What the field holds — copied out, which only a test does.
    fn held(&self) -> String {
        self.password.with_untracked(|s| s.expose().to_owned())
    }
}

#[test]
fn typing_is_held_by_the_password_and_drawn_as_its_mask() {
    let mut field = Field::new(|f, _| f.mask_char('*'));
    field.type_str("hunter2");

    assert_eq!(field.held(), "hunter2");
    assert_eq!(field.harness.painted_texts(), ["*******"]);
}

#[test]
fn a_reader_of_the_password_follows_the_field() {
    let mut field = Field::new(|f, _| f);
    let password = field.password;
    let empty = Rc::new(Cell::new(None));
    let seen = empty.clone();
    create_effect(move || seen.set(Some(password.is_empty())));
    assert_eq!(empty.get(), Some(true));

    field.type_str("a");
    assert_eq!(empty.get(), Some(false), "typing did not notify the reader");

    field.key(Key::Backspace);
    assert_eq!(
        empty.get(),
        Some(true),
        "deleting did not notify the reader"
    );
}

#[test]
fn enter_can_take_the_secret_and_leave_the_field_empty() {
    let got = Rc::new(RefCell::new(None));
    let sink = got.clone();
    let mut field = Field::new(move |f, password| {
        f.on_submit(move || *sink.borrow_mut() = Some(password.take()))
    });
    field.type_str("hunter2");
    field.key(Key::Enter);

    let secret = got.borrow_mut().take().expect("on_submit was not called");
    assert_eq!(secret.expose(), "hunter2");
    assert_eq!(field.held(), "", "the field kept what it handed over");

    // And it goes on working from empty.
    field.type_str("ok");
    assert_eq!(field.held(), "ok");
}

/// A password typed past the field's width and taken on Enter leaves the
/// field drawing its caret at its start, not where the long one had scrolled
/// it, out of sight (#723).
#[test]
fn a_password_taken_on_enter_draws_its_caret_in_view() {
    let mut field = Field::new(|f, password| f.on_submit(move || drop(password.take())));
    field.type_str(&"a".repeat(120));
    let caret = |field: &mut Field| field.harness.painted_rects()[0].x;
    assert!(caret(&mut field) > 100.0, "typed past its width");

    field.key(Key::Enter);
    assert_eq!(field.held(), "");
    assert_eq!(caret(&mut field), 0.0);
}

/// A password field is one line too, and a password copied with its line
/// break is where it hurts most: the mask hides the break, and the password is
/// refused with it.
#[test]
fn a_pasted_line_break_never_enters_the_password() {
    let got = Rc::new(RefCell::new(None));
    let sink = got.clone();
    let mut field = Field::new(move |f, password| {
        f.on_submit(move || *sink.borrow_mut() = Some(password.take()))
    });

    field.ctrl('v');
    answer_pastes(SelectionKind::Clipboard, Some("hunter2\r\n"));
    deliver_pastes(&mut field.harness.tree);
    field.key(Key::Enter);

    let secret = got.borrow_mut().take().expect("on_submit was not called");
    assert_eq!(secret.expose(), "hunter2");
}

/// A lock screen that shows its dots while the password is checked hands over
/// a duplicate, and the field keeps what it had — caret and all.
#[test]
fn enter_can_hand_over_a_duplicate_and_keep_the_field() {
    let got = Rc::new(RefCell::new(None));
    let sink = got.clone();
    let mut field = Field::new(move |f, password| {
        f.mask_char('*').on_submit(move || {
            *sink.borrow_mut() = Some(password.with_untracked(Secret::duplicate))
        })
    });
    field.type_str("hunter2");
    field.key(Key::Enter);

    let copy = got.borrow_mut().take().expect("on_submit was not called");
    assert_eq!(copy.expose(), "hunter2");
    assert_eq!(field.held(), "hunter2", "a duplicate emptied the field");
    assert_eq!(field.harness.painted_texts(), ["*******"]);

    // Typing goes on from where the caret was.
    field.type_str("!");
    assert_eq!(field.held(), "hunter2!");
}

#[test]
fn undo_does_nothing_because_there_is_no_history() {
    let mut field = Field::new(|f, _| f);
    field.type_str("abc");
    field.key(Key::Backspace);

    field.ctrl('z');
    assert_eq!(field.held(), "ab", "an undo brought back a hidden text");
    field.ctrl('y');
    assert_eq!(field.held(), "ab");
}

#[test]
fn a_word_jump_goes_to_the_edge_rather_than_to_a_word() {
    let mut field = Field::new(|f, _| f);
    field.type_str("one two");
    field.press(
        Key::Left,
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    );
    field.type_str("X");
    assert_eq!(
        field.held(),
        "Xone two",
        "Ctrl+Left stopped at a word boundary, which says where the space is"
    );
}

/// A mask draws one bullet per character, so a step is one character: stepping
/// a whole cluster would move the caret over two bullets and say there is a
/// cluster there. Flutter steps an obscured field the same way.
#[test]
fn an_arrow_steps_one_character_even_inside_a_cluster() {
    let mut field = Field::new(|f, _| f);
    field.type_str("e\u{301}");
    field.key(Key::Left);
    field.type_str("X");
    assert_eq!(field.held(), "eX\u{301}");
}

#[test]
fn a_readonly_field_refuses_edits() {
    let busy = create_signal(false);
    let mut field = Field::new(move |f, _| f.readonly(busy));
    field.type_str("ab");

    busy.set(true);
    field.type_str("c");
    field.key(Key::Backspace);
    assert_eq!(field.held(), "ab");
}
