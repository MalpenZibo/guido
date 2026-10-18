//! Editing a field: typing, deleting, moving, selecting, cutting and undoing.
//!
//! Every test here presses a key and asks what the text became, which nothing
//! did before. The placeholder, the caret's absence, password export and
//! autofocus each had a file; the twenty-five one-line deletions the mutation
//! job found in `text_input.rs` — the bodies of `delete`, `move_cursor`,
//! `undo`, `redo`, whole arms of `handle_key`, and the `has_focus` guard that
//! decides whether a field hears the keyboard at all — could each be removed
//! with all of them still green.
//!
//! So the question each test answers is not "does this work" but "what could
//! I take out of `text_input.rs` that this would not notice". That is why
//! several of them go one step past the obvious assertion: a backspace that
//! leaves the cursor in the wrong place still shows the right text, and an
//! undo that forgets to end the coalescing window still undoes.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use common::Harness;
use guido::prelude::*;
use guido::reactive::clipboard::{
    SelectionKind, answer_pastes, deliver_pastes, take_clipboard_change,
};
use guido::reactive::focus::{clear_focus, focus_path, request_focus};
use guido::renderer::{DrawCommand, RenderNode};
use guido::tree::WidgetId;

/// Narrow enough that a sentence does not fit, which is what the scrolling
/// tests below need and what the rest are indifferent to.
const WIDTH: f32 = 200.0;
const HEIGHT: f32 = 40.0;

/// Wider than `WIDTH` at the default 14px, so the caret can leave the viewport.
const SENTENCE: &str = "the quick brown fox jumps over the lazy dog";

/// Where a caret pushed off the right edge comes to rest: the viewport
/// `ensure_cursor_visible` keeps it inside is the field less one
/// `SCROLL_PADDING` at each end, and that constant is 2.0.
///
/// Pinning the number is what gives that test teeth — a viewport out by a
/// padding still lands the caret somewhere inside the field, so mere
/// containment would not notice. It is also the one thing here that would have
/// to change if the padding were retuned, which is why the test beside it
/// makes the same point without naming a number.
const SCROLL_PADDING_BOTH_ENDS: f32 = 4.0;

/// A laid-out field, and a clock the test moves by hand.
///
/// The clock is not decoration: the undo history coalesces edits less than
/// half a second apart into one entry, and the caret's blink turns on how long
/// ago the last one was. A test that let the wall clock answer would be asking
/// about whichever side of those windows the machine happened to land on.
struct Field {
    harness: Harness,
    value: RwSignal<String>,
    now: Instant,
}

impl Field {
    fn focused(text: &str) -> Self {
        let value = create_signal(text.to_owned());
        Self::focused_around(value, text_input(value))
    }

    /// A focused field around an input carrying more than the plain
    /// constructor gives it — the submit callback the Enter test listens to.
    fn focused_around(value: RwSignal<String>, input: TextInput) -> Self {
        let field = Self::around(value, input);
        request_focus(&field.harness.tree, field.harness.root);
        field
    }

    /// For the one test about a field that must not hear the keyboard.
    fn unfocused(text: &str) -> Self {
        let value = create_signal(text.to_owned());
        Self::around(value, text_input(value))
    }

    /// A focused field wrapped in whatever the test wants to say about it — a
    /// container declaring the whole subtree disabled, mostly. The *field*
    /// holds the focus, not the wrapper, which is what the tests below turn on.
    fn wrapped(value: RwSignal<String>, root: impl Widget + 'static) -> Self {
        let field = Self::around(value, root);
        request_focus(&field.harness.tree, field.input());
        field
    }

    /// The field itself: the last child of the wrapper, or the root when
    /// nothing wraps it.
    fn input(&self) -> WidgetId {
        self.harness
            .tree
            .get_children(self.harness.root)
            .last()
            .copied()
            .unwrap_or(self.harness.root)
    }

    fn around(value: RwSignal<String>, input: impl Widget + 'static) -> Self {
        // The focus and the selection a previous field in this thread published
        // would otherwise answer for this one.
        clear_focus();
        answer_pastes(SelectionKind::Clipboard, None);
        let _ = take_clipboard_change();

        Self {
            harness: Harness::laid_out(input, WIDTH, HEIGHT),
            value,
            now: Instant::now(),
        }
    }

    /// Press a key, a millisecond after the one before it.
    fn press(&mut self, key: Key, modifiers: Modifiers) -> EventResponse {
        self.now += Duration::from_millis(1);
        self.harness.send_at(
            Event::KeyDown {
                key,
                modifiers,
                repeat: false,
            },
            self.now,
        )
    }

    fn key(&mut self, key: Key) -> EventResponse {
        self.press(key, Modifiers::default())
    }

    fn shift(&mut self, key: Key) {
        self.press(
            key,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
    }

    fn ctrl(&mut self, c: char) {
        self.press(
            Key::Char(c),
            Modifiers {
                ctrl: true,
                ..Default::default()
            },
        );
    }

    fn ctrl_shift(&mut self, c: char) {
        self.press(
            Key::Char(c),
            Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        );
    }

    /// A middle press at a point, which is the primary-selection paste, with
    /// `offered` as what another application selected.
    fn middle_click(&mut self, x: f32, y: f32, offered: &str) {
        self.now += Duration::from_millis(1);
        self.harness
            .send_at(Event::mouse_down(x, y, MouseButton::Middle), self.now);
        self.paste_arrives(SelectionKind::Primary, offered);
    }

    /// The answer to the pastes asked for so far arrives, as the loop hands it
    /// over.
    fn paste_arrives(&mut self, kind: SelectionKind, text: &str) {
        answer_pastes(kind, Some(text));
        deliver_pastes(&mut self.harness.tree);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.key(Key::Char(c));
        }
    }

    /// Step past the coalescing window, so the next edit is its own undo entry.
    fn pause(&mut self) {
        self.now += Duration::from_millis(1000);
    }

    /// Let `ms` pass and run the pass that toggles the caret. The blink period
    /// is 530ms, so how far this steps decides whether the caret flips.
    fn wait(&mut self, ms: u64) {
        self.now += Duration::from_millis(ms);
        self.harness.advance(self.now);
    }

    fn text(&self) -> String {
        self.value.get_untracked()
    }

    /// Where the caret is drawn, if it is showing.
    /// Where the caret is drawn, if it is showing.
    fn caret_x(&mut self) -> Option<f32> {
        self.caret().map(|c| c.x)
    }

    /// The colour the field's own glyphs were drawn in.
    fn text_colour(&mut self) -> Option<Color> {
        fn find(node: &RenderNode) -> Option<Color> {
            for cmd in &node.commands {
                if let DrawCommand::Text { color, .. } = &**cmd {
                    return Some(*color);
                }
            }
            node.children.iter().find_map(|child| find(child))
        }
        self.harness.lay_out(WIDTH, HEIGHT);
        find(&self.harness.paint())
    }

    /// The caret's rectangle, if it is showing.
    fn caret(&mut self) -> Option<Rect> {
        let painted = self.harness.painted_rects();
        assert!(
            painted.len() <= 1,
            "a field paints a selection highlight and a caret and nothing else, \
             and the highlight comes first — so this answers for the caret only \
             where there is no selection: {painted:?}"
        );
        painted.into_iter().next()
    }
}

fn assert_caret_at(field: &mut Field, expected: f32, why: &str) {
    let caret = field.caret().expect("a focused field draws its caret");
    assert!(
        (caret.x - expected).abs() < 0.5,
        "{why}: expected the caret at {expected}, found it at {}",
        caret.x
    );
}

#[test]
fn an_unfocused_field_does_not_hear_the_keyboard() {
    // The sharpest of the twenty-five: a key travels the focus path, and a
    // field nobody focused is not on it. Route keys any wider and every field
    // on a surface takes every keystroke, with everything else here green.
    let mut field = Field::unfocused("");

    let response = field.key(Key::Char('a'));

    assert_eq!(response, EventResponse::Ignored);
    assert_eq!(
        field.text(),
        "",
        "a field nobody focused took a keystroke meant for whoever holds the \
         keyboard"
    );
}

#[test]
fn typing_puts_the_characters_in() {
    let mut field = Field::focused("");

    field.type_text("hi");

    assert_eq!(field.text(), "hi");
}

#[test]
fn enter_hands_the_text_as_it_stands_to_on_submit() {
    let submitted: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = Rc::clone(&submitted);
    let value = create_signal(String::new());
    let mut field = Field::focused_around(
        value,
        text_input(value).on_submit(move |text| sink.borrow_mut().push(text.to_owned())),
    );

    field.type_text("hi");
    field.key(Key::Enter);

    assert_eq!(
        submitted.borrow().as_slice(),
        ["hi".to_owned()],
        "Enter has to hand over what is in the field now, not what it held when \
         the callback was declared"
    );
}

#[test]
fn backspace_takes_the_character_before_the_cursor_and_leaves_it_there() {
    let mut field = Field::focused("");
    field.type_text("hit");

    field.key(Key::Backspace);
    assert_eq!(field.text(), "hi");

    // Where the cursor landed, asked the only way a test outside the crate can:
    // move one step and write. A backspace that deletes the right character and
    // leaves the cursor past the end of the text still reads as "hi".
    field.key(Key::Left);
    field.type_text("Z");
    assert_eq!(
        field.text(),
        "hZi",
        "backspace leaves the cursor where the character it removed was"
    );
}

/// After a deletion the field knows how long its text is: End goes to its
/// end, not as far past it as the text was long before.
#[test]
fn end_after_a_backspace_is_the_end_of_what_is_left() {
    let mut field = Field::focused("");
    field.type_text("hit");
    field.key(Key::Backspace);

    field.key(Key::End);
    field.key(Key::Backspace);
    assert_eq!(field.text(), "h");
}

/// Ctrl+Left stops at the start of a word and Ctrl+Right at its end, skipping
/// the spaces between them — asked, like the test above, by moving and then
/// writing.
#[test]
fn ctrl_arrows_move_by_word() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let mut field = Field::focused("");
    field.type_text("one  two three");

    field.press(Key::Left, ctrl);
    field.type_text("A");
    assert_eq!(
        field.text(),
        "one  two Athree",
        "back to the start of the last word"
    );

    field.press(Key::Left, ctrl);
    field.press(Key::Left, ctrl);
    field.type_text("B");
    assert_eq!(
        field.text(),
        "one  Btwo Athree",
        "back over a word and the spaces before it"
    );

    field.key(Key::Home);
    field.press(Key::Right, ctrl);
    field.type_text("C");
    assert_eq!(
        field.text(),
        "oneC  Btwo Athree",
        "forward to the end of a word, not past the spaces after it"
    );

    field.key(Key::End);
    field.press(Key::Right, ctrl);
    field.type_text("D");
    assert_eq!(
        field.text(),
        "oneC  Btwo AthreeD",
        "and no further than the end"
    );
}

#[test]
fn delete_takes_the_character_after_it() {
    // A field opens with its cursor at the start, so Delete has something in
    // front of it and Backspace does not.
    let mut field = Field::focused("hit");

    field.key(Key::Delete);

    assert_eq!(field.text(), "it");
}

#[test]
fn backspace_at_the_start_and_delete_at_the_end_do_nothing() {
    // Nothing at all: a text that comes out the same is still a change
    // `on_change` would be told about.
    let changes = Rc::new(RefCell::new(0));
    let counted = changes.clone();
    let value = create_signal("hi".to_owned());
    let mut field = Field::focused_around(
        value,
        text_input(value).on_change(move |_| *counted.borrow_mut() += 1),
    );

    field.key(Key::Backspace);
    assert_eq!(field.text(), "hi");

    field.key(Key::End);
    field.key(Key::Delete);
    assert_eq!(field.text(), "hi");
    assert_eq!(*changes.borrow(), 0, "on_change heard of a change");
}

#[test]
fn right_moves_the_cursor_forward_one_character() {
    let mut field = Field::focused("ac");

    field.key(Key::Right);
    field.type_text("b");

    assert_eq!(field.text(), "abc");
}

#[test]
fn left_moves_the_cursor_backward_one_character() {
    // Three characters, and the insert one in from the end: a cursor that goes
    // the wrong way and one that goes nowhere both write somewhere else.
    let mut field = Field::focused("bcd");
    field.key(Key::End);

    field.key(Key::Left);
    field.type_text("X");

    assert_eq!(field.text(), "bcXd");
}

/// An `e` and a combining acute are two characters drawn as one é. A caret
/// stopped between them is drawn before the é, and what is typed there takes
/// its accent.
const E_ACUTE: &str = "e\u{301}";
/// Two regional indicators, drawn as one flag.
const FLAG: &str = "🇮🇹";
/// Three people joined by zero-width joiners, drawn as one family.
const FAMILY: &str = "👨\u{200d}👩\u{200d}👧";

#[test]
fn right_steps_over_a_letter_and_its_mark_together() {
    let mut field = Field::focused(E_ACUTE);

    field.key(Key::Right);
    field.type_text("a");

    assert_eq!(
        field.text(),
        format!("{E_ACUTE}a"),
        "the accent stays on the e"
    );
}

#[test]
fn right_steps_over_a_joined_emoji_in_one_press() {
    let mut field = Field::focused(FAMILY);

    field.key(Key::Right);
    field.type_text("X");

    assert_eq!(field.text(), format!("{FAMILY}X"));
}

#[test]
fn left_steps_back_over_a_letter_and_its_mark_together() {
    let mut field = Field::focused(E_ACUTE);
    field.key(Key::End);

    field.key(Key::Left);
    field.type_text("X");

    assert_eq!(field.text(), format!("X{E_ACUTE}"));
}

#[test]
fn delete_takes_a_whole_flag() {
    let mut field = Field::focused(FLAG);

    field.key(Key::Delete);

    assert_eq!(field.text(), "", "half a flag is a broken glyph");
}

/// Backspace takes the whole of what is drawn as one character, as GTK does
/// after a Latin letter or an emoji, and as Flutter and iced do everywhere.
#[test]
fn backspace_takes_a_letter_and_its_mark_together() {
    let mut field = Field::focused(E_ACUTE);
    field.key(Key::End);

    field.key(Key::Backspace);

    assert_eq!(field.text(), "");
}

#[test]
fn backspace_takes_a_whole_flag() {
    let mut field = Field::focused(FLAG);
    field.key(Key::End);

    field.key(Key::Backspace);

    assert_eq!(field.text(), "");
}

/// A click just right of where the é starts is nearer its start than the
/// next letter's, and the character nearest it is the mark, which has no
/// boundary of its own.
#[test]
fn a_click_inside_a_letter_and_its_mark_lands_before_them() {
    let mut field = Field::focused(&format!("{E_ACUTE}a"));

    field.now += Duration::from_millis(1);
    field
        .harness
        .send_at(Event::mouse_down(1.0, 5.0, MouseButton::Left), field.now);
    field.type_text("X");

    assert_eq!(field.text(), format!("X{E_ACUTE}a"));
}

/// The cursor one character into "ab" is inside the é the signal then holds.
#[test]
fn a_text_set_from_outside_does_not_leave_the_cursor_inside_a_cluster() {
    let mut field = Field::focused("ab");
    field.key(Key::Right);

    field.value.set(E_ACUTE.to_owned());
    field.harness.tree.mark_needs_layout(field.harness.root);
    field.harness.lay_out(WIDTH, HEIGHT);
    field.type_text("X");

    assert_eq!(field.text(), format!("X{E_ACUTE}"));
}

#[test]
fn home_and_end_go_to_the_ends() {
    let mut field = Field::focused("bcd");

    field.key(Key::End);
    field.type_text("!");
    assert_eq!(field.text(), "bcd!");

    field.key(Key::Home);
    field.type_text("?");
    assert_eq!(field.text(), "?bcd!");
}

#[test]
fn shift_and_an_arrow_extend_the_selection() {
    let mut field = Field::focused("abcd");

    field.shift(Key::Right);
    field.shift(Key::Right);
    field.type_text("X");

    assert_eq!(
        field.text(),
        "Xcd",
        "typing over a selection replaces it, so what was replaced says how far \
         the selection reached"
    );
}

#[test]
fn shift_and_home_select_back_to_the_start() {
    let mut field = Field::focused("abcd");
    field.key(Key::End);

    field.shift(Key::Home);
    field.type_text("X");

    assert_eq!(field.text(), "X");
}

#[test]
fn end_without_shift_ends_the_selection() {
    let mut field = Field::focused("abcd");
    field.shift(Key::Right);
    field.shift(Key::Right);

    field.key(Key::End);
    field.type_text("X");

    assert_eq!(
        field.text(),
        "abcdX",
        "an edge without shift collapses what was selected, so what follows \
         inserts rather than replaces"
    );
}

#[test]
fn left_on_a_selection_goes_to_its_start_rather_than_stepping_past_it() {
    let mut field = Field::focused("abcd");
    field.ctrl('a');

    field.key(Key::Left);
    field.type_text("X");

    assert_eq!(
        field.text(),
        "Xabcd",
        "Left on a selection goes to its start, not one character back from the \
         cursor"
    );
}

#[test]
fn right_on_a_selection_goes_to_its_end_rather_than_stepping_past_it() {
    // The selection has to stop short of the end of the text: from a selection
    // that reaches it, collapsing to the end and stepping one on from the
    // cursor land in the same place, and the test could not fail.
    let mut field = Field::focused("abcd");
    field.shift(Key::Right);
    field.shift(Key::Right);

    field.key(Key::Right);
    field.type_text("Y");

    assert_eq!(
        field.text(),
        "abYcd",
        "Right on a selection goes to its end, not one character on from the \
         cursor"
    );
}

#[test]
fn ctrl_a_selects_all_of_it_from_wherever_the_cursor_is() {
    // From the end, because a field opens with its cursor at 0 and a select-all
    // that only reached forward from the cursor would look right from there.
    let mut field = Field::focused("abcd");
    field.key(Key::End);

    field.ctrl('a');
    field.type_text("X");

    assert_eq!(field.text(), "X");
}

#[test]
fn ctrl_x_cuts_the_selection_to_the_clipboard() {
    let mut field = Field::focused("abcd");
    field.ctrl('a');

    field.ctrl('x');

    assert_eq!(field.text(), "");
    assert_eq!(take_clipboard_change().as_deref(), Some("abcd"));
}

/// A paste is a request: the text goes in when the application that owns the
/// clipboard answers, at the caret.
#[test]
fn ctrl_v_inserts_the_paste_when_it_arrives() {
    let mut field = Field::focused("abcd");
    field.key(Key::End);
    field.key(Key::Left);

    field.ctrl('v');
    assert_eq!(field.text(), "abcd", "nothing is there until the answer is");

    field.paste_arrives(SelectionKind::Clipboard, "XY");
    assert_eq!(field.text(), "abcXYd");
}

/// A field is one line, and a line copied from a terminal or an editor comes
/// with its line break. The break is removed, as HTML's `<input type=text>`
/// strips newlines from its value: left in, it reaches `on_submit` and the
/// search misses or the password is refused, with nothing on screen to say why.
#[test]
fn a_pasted_line_break_never_enters_the_field() {
    let submitted: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = Rc::clone(&submitted);
    let value = create_signal(String::new());
    let mut field = Field::focused_around(
        value,
        text_input(value).on_submit(move |text| sink.borrow_mut().push(text.to_owned())),
    );

    field.ctrl('v');
    field.paste_arrives(SelectionKind::Clipboard, "hunter2\n");
    assert_eq!(field.text(), "hunter2");

    field.key(Key::Enter);
    assert_eq!(submitted.borrow().as_slice(), ["hunter2".to_owned()]);
}

/// Removed rather than replaced: a Windows line ending is two characters, and
/// neither of them is left behind.
#[test]
fn a_pasted_line_break_inside_the_text_joins_its_two_halves() {
    let mut field = Field::focused("");

    field.ctrl('v');
    field.paste_arrives(SelectionKind::Clipboard, "a\r\nb\nc\rd");

    assert_eq!(field.text(), "abcd");
}

/// A paste that was nothing but a line break inserts nothing, and nothing is
/// no edit: an undo after it takes back what came before, rather than an
/// entry for the paste that changed nothing.
#[test]
fn a_paste_of_only_a_line_break_leaves_nothing_to_undo() {
    // A delete before it, so the paste could not coalesce into its entry.
    let mut field = Field::focused("ab");
    field.key(Key::End);
    field.key(Key::Backspace);

    field.ctrl('v');
    field.paste_arrives(SelectionKind::Clipboard, "\n");
    field.ctrl('z');

    assert_eq!(field.text(), "ab");
}

#[test]
fn a_cut_leaves_the_cursor_where_the_selection_was() {
    let mut field = Field::focused("abcd");
    field.shift(Key::Right);
    field.shift(Key::Right);

    field.ctrl('x');
    assert_eq!(field.text(), "cd");

    field.type_text("X");
    assert_eq!(
        field.text(),
        "Xcd",
        "what is typed after a cut belongs where the cut text was, not wherever \
         the cursor was left"
    );
}

/// An undo back to a short text shows it from its start, as a field that only
/// ever held it would, not from where the long text had scrolled (#723).
#[test]
fn an_undo_back_to_a_short_text_shows_it_as_typed() {
    let mut fresh = Field::focused("");
    fresh.type_text("abc");
    let expected = fresh.caret_x().expect("a focused field draws its caret");

    let mut field = Field::focused("");
    field.type_text("abc");
    field.pause();
    field.type_text(&"a".repeat(120));
    assert!(field.caret_x().unwrap() > expected, "typed past its width");

    field.ctrl('z');
    assert_eq!(field.text(), "abc");
    assert_caret_at(&mut field, expected, "undone back to abc");
}

/// An `on_change` that cuts a long text back to a shorter one leaves the field
/// showing it as a field that only ever held the shorter one would (#723).
#[test]
fn a_text_cut_short_by_on_change_shows_it_as_typed() {
    let mut fresh = Field::focused("");
    fresh.type_text(&"a".repeat(40));
    let expected = fresh.caret_x().expect("a focused field draws its caret");

    let value = create_signal(String::new());
    let mut field = Field::focused_around(
        value,
        text_input(value).on_change(move |text| {
            if text.len() > 80 {
                value.set(text[..40].to_owned());
            }
        }),
    );
    field.type_text(&"a".repeat(80));
    field.key(Key::Char('a'));
    assert_eq!(field.text(), "a".repeat(40));
    assert_caret_at(&mut field, expected, "cut back to 40");
}

#[test]
fn ctrl_z_puts_back_what_was_typed_and_ctrl_y_takes_it_away_again() {
    let mut field = Field::focused("");
    field.type_text("ab");
    field.pause();
    field.type_text("c");
    assert_eq!(field.text(), "abc");

    field.ctrl('z');
    assert_eq!(
        field.text(),
        "ab",
        "the second thought is undone on its own; the first is a separate entry \
         because a second passed in between"
    );

    field.ctrl('z');
    assert_eq!(field.text(), "");

    field.ctrl('y');
    assert_eq!(field.text(), "ab");

    field.ctrl_shift('z');
    assert_eq!(field.text(), "abc", "Ctrl+Shift+Z is the other redo");
}

#[test]
fn a_delete_is_undoable_too() {
    let mut field = Field::focused("hi");
    field.key(Key::End);
    field.pause();

    field.key(Key::Backspace);
    assert_eq!(field.text(), "h");

    field.ctrl('z');
    assert_eq!(field.text(), "hi");
}

#[test]
fn undoing_coalesced_typing_leaves_an_empty_field_editable() {
    let mut field = Field::focused("");
    field.type_text("ab");
    field.ctrl('z');
    assert_eq!(field.text(), "");

    field.key(Key::Backspace);
    assert_eq!(field.text(), "");
    field.type_text("é");
    assert_eq!(field.text(), "é");
}

#[test]
fn undoing_coalesced_typing_restores_its_original_insertion_point() {
    let mut field = Field::focused("aéb");
    field.key(Key::Right);
    field.type_text("XY");
    field.ctrl('z');
    assert_eq!(field.text(), "aéb");

    field.type_text("Z");
    assert_eq!(field.text(), "aZéb");
}

#[test]
fn undoing_coalesced_replacement_restores_the_original_selection() {
    let mut field = Field::focused("abcd");
    field.key(Key::Right);
    field.shift(Key::Right);
    field.shift(Key::Right);
    field.type_text("XY");
    field.ctrl('z');
    assert_eq!(field.text(), "abcd");

    field.type_text("Z");
    assert_eq!(field.text(), "aZd");
}

#[test]
fn undoing_coalesced_deletion_restores_its_original_caret() {
    let mut field = Field::focused("aéb");
    field.key(Key::End);
    field.key(Key::Backspace);
    field.key(Key::Backspace);
    assert_eq!(field.text(), "a");
    field.ctrl('z');
    assert_eq!(field.text(), "aéb");
    field.ctrl('y');
    assert_eq!(field.text(), "a");
    field.ctrl('z');
    assert_eq!(field.text(), "aéb");

    field.type_text("Z");
    assert_eq!(field.text(), "aébZ");
}

#[test]
fn typing_after_an_undo_drops_what_was_undone() {
    // An undo has to end the coalescing window as well as move the text back.
    // If it does not, the next keystroke joins the entry the undo left behind
    // instead of starting one — which also leaves the redo stack standing, so
    // a redo jumps forward to a version the user has already typed over.
    let mut field = Field::focused("");
    field.type_text("ab");
    field.pause();
    field.type_text("c");
    field.ctrl('z');
    assert_eq!(field.text(), "ab");

    field.type_text("X");
    field.ctrl('y');

    assert_eq!(
        field.text(),
        "abX",
        "there is nothing to redo once the undone version has been typed over"
    );
}

#[test]
fn returning_to_saved_text_keeps_the_next_undo_group() {
    let mut field = Field::focused("a");
    field.ctrl('a');
    field.type_text("b");
    field.ctrl('a');
    field.type_text("a");
    field.pause();
    field.type_text("c");
    assert_eq!(field.text(), "ac");
    field.ctrl('z');
    assert_eq!(field.text(), "a");
    field.type_text("Z");
    assert_eq!(field.text(), "aZ");
}

#[test]
fn editing_an_externally_cleared_value_discards_redo() {
    let mut field = Field::focused("");
    field.type_text("a");
    field.pause();
    field.type_text("b");
    field.ctrl('z');
    assert_eq!(field.text(), "a");
    field.value.set(String::new());
    field.harness.tree.mark_needs_layout(field.harness.root);
    field.harness.lay_out(WIDTH, HEIGHT);
    field.type_text("X");
    assert_eq!(field.text(), "X");
    field.ctrl('y');
    assert_eq!(field.text(), "X");
}

#[test]
fn rapid_return_to_saved_text_keeps_the_original_group_selection() {
    let mut field = Field::focused("a");
    field.ctrl('a');
    field.type_text("b");
    field.ctrl('a');
    field.type_text("a");
    field.type_text("c");
    field.ctrl('z');
    assert_eq!(field.text(), "a");
    field.type_text("Z");
    assert_eq!(field.text(), "Z");
}

#[test]
fn typing_restarts_the_blink_rather_than_letting_the_caret_stay_dark() {
    let mut field = Field::focused("");
    assert!(field.caret().is_some(), "a focused field shows a caret");

    field.wait(600);
    assert_eq!(
        field.caret(),
        None,
        "a blink period on, the caret is in its dark half"
    );

    field.wait(400);
    field.type_text("a");
    assert!(
        field.caret().is_some(),
        "the caret has to come back the moment a key is pressed, or typing \
         happens somewhere the user cannot see"
    );

    // And the period has to restart with it. 400ms after the keystroke is
    // inside a blink of it, but 800ms after the toggle before it — so a caret
    // that was made visible without moving the clock forward goes dark here.
    field.wait(400);
    assert!(
        field.caret().is_some(),
        "the blink resumed from the last toggle instead of from the keystroke"
    );
}

#[test]
fn the_caret_stays_inside_the_field_when_the_text_runs_past_its_end() {
    let mut field = Field::focused(SENTENCE);

    field.key(Key::End);

    assert_caret_at(
        &mut field,
        WIDTH - SCROLL_PADDING_BOTH_ENDS,
        "a caret pushed off the right edge should be scrolled back to just \
         inside it",
    );
}

#[test]
fn the_view_follows_the_caret_no_further_than_it_has_to() {
    // The same property as the test above, without naming the padding: once the
    // caret has been scrolled to its resting place, more typing scrolls the
    // text under it and leaves it exactly where it was.
    let mut field = Field::focused(SENTENCE);
    field.key(Key::End);
    let settled = field.caret().expect("a focused field draws its caret").x;

    field.type_text(" and again");

    assert_caret_at(
        &mut field,
        settled,
        "typing at the right edge should move the text, not the caret",
    );
}

#[test]
fn a_step_back_inside_the_viewport_moves_the_caret_and_not_the_view() {
    // The comparison that decides *whether* to scroll, rather than how far. A
    // field that re-scrolls on a cursor move it should have left alone pins the
    // caret against the right edge and slides the text under it, so the caret
    // stops appearing to move at all.
    let mut field = Field::focused(SENTENCE);
    field.key(Key::End);
    let at_the_edge = field.caret().expect("a focused field draws its caret").x;

    field.key(Key::Left);

    let caret = field.caret().expect("a focused field draws its caret");
    assert!(
        caret.x < at_the_edge - 1.0,
        "one character back from the right edge is still inside the viewport, so \
         the caret should have moved left from {at_the_edge} and is at {}",
        caret.x
    );
}

#[test]
fn walking_back_through_a_scrolled_field_never_puts_the_caret_outside_it() {
    // Walking off the left edge is the other half, and the field has to bring
    // the view with it every step rather than letting the caret cross its own
    // border.
    let mut field = Field::focused(SENTENCE);
    field.key(Key::End);

    for step in 1..=SENTENCE.chars().count() {
        field.key(Key::Left);
        let caret = field.caret().expect("a focused field draws its caret");
        assert!(
            caret.x >= -0.5 && caret.x <= WIDTH + 0.5,
            "{step} characters back from the end, the caret is at {} — outside a \
             field {WIDTH} wide",
            caret.x
        );
    }
}

#[test]
fn the_view_comes_back_to_the_start_when_the_cursor_does() {
    let mut field = Field::focused(SENTENCE);
    field.key(Key::End);

    field.key(Key::Home);

    assert_caret_at(
        &mut field,
        0.0,
        "a cursor at the start of the text belongs at the start of the field",
    );
}

// ---------------------------------------------------------------------------
// A field that has been told to stop
// ---------------------------------------------------------------------------

/// The other half of the subtree gate: keys take the same dispatch path as the
/// pointer, so a container that refuses one refuses the other.
#[test]
fn a_field_below_a_disabled_container_does_not_hear_the_keyboard() {
    let value = create_signal("hi".to_owned());
    let mut field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .enabled(false)
            .child(text_input(value)),
    );

    let response = field.key(Key::Char('x'));

    assert_eq!(response, EventResponse::Ignored);
    assert_eq!(
        field.text(),
        "hi",
        "a field under a disabled container took a keystroke"
    );
}

/// A disabled subtree shows no focus ring — the same answer Qt, GTK and a
/// `<fieldset disabled>` give, and the reason `readonly` exists beside it.
///
/// The focus itself is not asserted here: what a caller can see is what its
/// control answers, and a disabled unit answers no.
#[test]
fn a_disabled_container_stops_claiming_the_focus_below_it() {
    let value = create_signal(String::new());
    let enabled = create_signal(true);
    let field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .enabled(enabled)
            .child(text_input(value)),
    );
    let control = field
        .harness
        .tree
        .nearest_control(field.input())
        .expect("declaring `enabled` makes a container an interaction unit");

    assert!(control.has_focus(), "the field below it holds the keyboard");
    enabled.set(false);
    assert!(
        !control.has_focus(),
        "a disabled unit is not where the keyboard is aimed"
    );
}

// ---------------------------------------------------------------------------
// Read-only: refusing the edit, and nothing else
// ---------------------------------------------------------------------------

/// #211: a lock screen that has sent the password to PAM must stop taking the
/// characters typed while it waits, because they can never be sent.
#[test]
fn a_read_only_field_refuses_every_edit() {
    let value = create_signal("hi".to_owned());
    let readonly = create_signal(true);
    let mut field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .child(text_input(value).readonly(readonly)),
    );

    // The response as well as the text. The refusal itself is at the writers,
    // so the text would be right even if the keyboard gate were gone entirely —
    // what says the gate is still there is the field declining the key, which
    // is what lets somebody else have it.
    for key in [Key::Char('x'), Key::Backspace, Key::Delete] {
        assert_eq!(
            field.key(key),
            EventResponse::Ignored,
            "a key it will not use is a key somebody else may"
        );
    }
    for c in ['x', 'v', 'z', 'y'] {
        assert_eq!(
            field.press(
                Key::Char(c),
                Modifiers {
                    ctrl: true,
                    ..Default::default()
                }
            ),
            EventResponse::Ignored,
            "ctrl+{c} edits, so it is refused too"
        );
    }
    // Ctrl+A only selects, so it is not refused.
    assert_eq!(
        field.press(
            Key::Char('a'),
            Modifiers {
                ctrl: true,
                ..Default::default()
            }
        ),
        EventResponse::Handled
    );
    assert_eq!(field.text(), "hi", "not one of those may change the text");

    readonly.set(false);
    field.key(Key::End);
    field.key(Key::Char('x'));
    assert_eq!(field.text(), "hix", "and it takes them again when it may");
}

/// A keystroke is not the only way into the text. A middle click pastes the
/// primary selection straight into `insert_text`, so a guard that sat at
/// `handle_key` would have let it through — on a lock screen, into the password
/// PAM is already answering.
#[test]
fn a_read_only_field_refuses_a_middle_click_paste() {
    let value = create_signal("hi".to_owned());
    let readonly = create_signal(true);
    let mut field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .child(text_input(value).readonly(readonly)),
    );
    field.middle_click(4.0, 5.0, "pasted");
    assert_eq!(field.text(), "hi", "the primary selection got in");

    readonly.set(false);
    field.middle_click(4.0, 5.0, "pasted");
    assert!(
        field.text().contains("pasted"),
        "and the same click works when the field is not read-only: {}",
        field.text()
    );
}

/// A field resolves its own state layers through its control, exactly as a
/// `Text` beside it does — nothing declared one on a `TextInput` before, so the
/// whole of `is_state_active` could be replaced by a constant unnoticed.
#[test]
fn a_field_styles_itself_from_its_control() {
    const CALM: Color = Color::rgb(0.5, 0.5, 0.5);
    const LIT: Color = Color::rgb(1.0, 1.0, 1.0);

    let value = create_signal("hi".to_owned());
    let enabled = create_signal(true);
    let mut field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .enabled(enabled)
            .child(text_input(value).color(CALM).when_focused(|s| s.color(LIT))),
    );

    assert_eq!(
        field.text_colour(),
        Some(LIT),
        "the field holds the focus, so its focused layer applies"
    );

    enabled.set(false);
    assert_eq!(
        field.text_colour(),
        Some(CALM),
        "a disabled unit is not where the keyboard is aimed, so the layer lifts"
    );
}

/// `Enter` is deliberately not an edit, and this is the half that carries
/// allock: `readonly` is set from inside the submit path, so a field that
/// swallowed its own `Enter` would go permanently unsubmittable.
#[test]
fn a_read_only_field_still_submits() {
    let value = create_signal("hi".to_owned());
    let submits = Rc::new(RefCell::new(Vec::new()));
    let sink = submits.clone();
    let mut field = Field::wrapped(
        value,
        container().width(WIDTH).height(HEIGHT).child(
            text_input(value)
                .readonly(true)
                .on_submit(move |text| sink.borrow_mut().push(text.to_owned())),
        ),
    );

    assert_eq!(field.key(Key::Enter), EventResponse::Handled);
    assert_eq!(submits.borrow().as_slice(), ["hi"]);
}

/// The whole reason this is not `enabled`. A field that has stopped taking
/// edits is still the one the keyboard is aimed at, and on a multi-monitor
/// lock screen the `when_focused` ring is the only thing naming the screen.
#[test]
fn a_read_only_field_keeps_the_focus_and_the_ring() {
    let value = create_signal("hi".to_owned());
    let readonly = create_signal(false);
    let field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .control()
            .child(text_input(value).readonly(readonly)),
    );
    let control = field.harness.tree.nearest_control(field.input()).unwrap();
    let before = focus_path();
    assert!(before.contains(field.input()));
    assert!(control.has_focus());

    readonly.set(true);

    assert_eq!(before, focus_path(), "the focus path is untouched");
    assert!(
        control.has_focus(),
        "read-only is not disabled: the ring stays"
    );
}

/// It refuses the edit and nothing else, which is Qt's line: the caret still
/// moves, so a caller can still see and copy what is there.
#[test]
fn a_read_only_field_still_moves_its_caret() {
    let value = create_signal(SENTENCE.to_owned());
    let mut field = Field::wrapped(
        value,
        container()
            .width(WIDTH)
            .height(HEIGHT)
            .child(text_input(value).readonly(true)),
    );

    assert_eq!(field.key(Key::End), EventResponse::Handled);
    assert_ne!(
        field.caret_x(),
        Some(0.0),
        "End moved the caret off the start"
    );
    assert_eq!(field.key(Key::Home), EventResponse::Handled);
}

/// Press at 4, drag to 40, optionally leave the surface, optionally move on to
/// `then_move_to`, then type over whatever is selected. What the field holds
/// afterwards is how far the selection had got, without pinning a number that
/// belongs to the font.
fn after_a_drag(leave: bool, then_move_to: Option<f32>) -> String {
    let value = create_signal(SENTENCE.to_owned());
    let mut field = Field::focused_around(value, text_input(value));

    field.now += Duration::from_millis(1);
    field
        .harness
        .send_at(Event::mouse_down(4.0, 5.0, MouseButton::Left), field.now);
    field.now += Duration::from_millis(1);
    field
        .harness
        .send_at(Event::mouse_move(40.0, 5.0), field.now);

    if leave {
        field.now += Duration::from_millis(1);
        field.harness.send_at(Event::MouseLeave, field.now);
    }
    if let Some(x) = then_move_to {
        field.now += Duration::from_millis(1);
        field.harness.send_at(Event::mouse_move(x, 5.0), field.now);
    }

    field.key(Key::Char('X'));
    field.text()
}

/// A selection drag has to survive the pointer leaving the *field* — that is
/// how you select past its edge — but not the pointer leaving the *surface*.
/// Nothing out there will deliver the release that ends the drag, so the field
/// would extend the selection on the next move that carried a position, with no
/// button held.
///
/// The pointer cannot normally reach that state: `wl_pointer`'s implicit grab
/// keeps a `leave` from arriving mid-drag. A touch gesture can, since #425 made
/// `wl_touch.cancel` deliver `MouseLeave` alone rather than synthesizing a
/// release that would have clicked whatever was under the finger.
#[test]
fn a_drag_does_not_outlive_the_pointer_leaving_the_surface() {
    let value = create_signal(SENTENCE.to_owned());
    let mut field = Field::focused_around(value, text_input(value));

    field.now += Duration::from_millis(1);
    field
        .harness
        .send_at(Event::mouse_down(4.0, 5.0, MouseButton::Left), field.now);
    field.now += Duration::from_millis(1);
    assert_eq!(
        field
            .harness
            .send_at(Event::mouse_move(40.0, 5.0), field.now),
        EventResponse::Handled,
        "the drag is live and extending the selection"
    );

    field.now += Duration::from_millis(1);
    field.harness.send_at(Event::MouseLeave, field.now);
    field.now += Duration::from_millis(1);
    // `Hit`, not `Handled`: the point is on the field, which covers what is
    // beneath it, but nothing took the move.
    assert_eq!(
        field
            .harness
            .send_at(Event::mouse_move(150.0, 5.0), field.now),
        EventResponse::Hit,
        "the leave ended the drag, so a later move is not the field's to take — \
         and 150 is inside the field, so only a live drag could have claimed it"
    );

    // The same thing said in the text rather than the response, and it has to
    // be said by comparison: how far a drag to x=40 reaches is the font's
    // business, so the assertion is that the move after the leave changed
    // nothing — while the identical move without the leave changes plenty.
    let left_then_moved = after_a_drag(true, Some(150.0));
    assert_eq!(
        left_then_moved,
        after_a_drag(true, None),
        "the move after the leave selected nothing further"
    );
    assert_ne!(
        left_then_moved,
        after_a_drag(false, Some(150.0)),
        "and it would have, had the drag still been live"
    );
}

impl Field {
    /// The `count`th press of a run at `x`, held.
    fn press_down(&mut self, count: u32, x: f32) {
        self.now += Duration::from_millis(1);
        self.harness
            .send_at(common::press_numbered(x, 5.0, count), self.now);
    }

    /// The pointer dragged to `x`, and released there.
    fn release_at(&mut self, x: f32) {
        self.now += Duration::from_millis(1);
        self.harness.send_at(Event::mouse_move(x, 5.0), self.now);
        self.harness
            .send_at(Event::mouse_up(x, 5.0, MouseButton::Left), self.now);
    }

    /// The `count`th press of a run at `x`, released where it landed.
    fn press_number(&mut self, count: u32, x: f32) {
        self.press_down(count, x);
        self.release_at(x);
    }

    /// Where the caret is drawn after `keys`, which is how a test finds a
    /// word on screen without pinning a number that belongs to the font.
    fn x_after(&mut self, keys: &[(Key, Modifiers)]) -> f32 {
        for &(key, modifiers) in keys {
            self.press(key, modifiers);
        }
        self.caret_x().expect("a caret with nothing selected")
    }
}

/// A double click inside "world" selects it, and typing replaces it.
#[test]
fn a_double_click_selects_the_word_under_the_pointer() {
    let mut field = Field::focused("hello world");
    let end = field.x_after(&[(Key::End, Modifiers::default())]);

    field.press_number(1, end - 4.0);
    field.press_number(2, end - 4.0);
    field.type_text("X");

    assert_eq!(field.text(), "hello X");
}

/// A third click selects everything, whatever word it landed on.
#[test]
fn a_triple_click_selects_everything() {
    let mut field = Field::focused("hello world");
    let end = field.x_after(&[(Key::End, Modifiers::default())]);

    for count in 1..=3 {
        field.press_number(count, end - 4.0);
    }
    field.type_text("X");

    assert_eq!(field.text(), "X");
}

/// A drag after a double click extends the selection a word at a time, from
/// the word it started on, in either direction: a drag that ends inside a
/// word takes all of it.
#[test]
fn a_drag_after_a_double_click_extends_by_words() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let mut field = Field::focused("one two three");
    let two = field.x_after(&[
        (Key::End, Modifiers::default()),
        (Key::Left, ctrl),
        (Key::Left, ctrl),
    ]) + 3.0;
    let three = field.x_after(&[(Key::End, Modifiers::default())]) - 4.0;

    // Forwards, from "one" into "two".
    field.press_down(2, 3.0);
    field.release_at(two);
    field.type_text("X");
    assert_eq!(field.text(), "X three");

    // Backwards, from "three" into "two".
    let mut field = Field::focused("one two three");
    field.press_down(2, three);
    field.release_at(two);
    field.type_text("X");
    assert_eq!(field.text(), "one X");
}

/// A press on the right half of a word's last letter is on that word: the
/// caret position nearest it is past the letter, before the space, and the
/// word is chosen by the character drawn under the pointer, not by that.
#[test]
fn a_double_click_on_the_last_letter_of_a_word_selects_that_word() {
    let mut field = Field::focused("hello world");
    let right = (Key::Right, Modifiers::default());
    let past_hello = field.x_after(&[
        (Key::Home, Modifiers::default()),
        right,
        right,
        right,
        right,
        right,
    ]);

    field.press_number(1, past_hello - 1.0);
    field.press_number(2, past_hello - 1.0);
    field.type_text("X");

    assert_eq!(field.text(), "X world");
}

/// A field scrolled to its end still selects the word drawn under the
/// pointer: the press is read against the text as scrolled, not as laid out
/// from the field's left edge.
#[test]
fn a_double_click_in_a_scrolled_field_selects_the_word_drawn_there() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    let mut field = Field::focused(SENTENCE);
    // Just inside the left edge of the "d": the caret position nearest it is
    // the one before it, and the character under it is the "d".
    let dog = field.x_after(&[(Key::End, Modifiers::default()), (Key::Left, ctrl)]) + 1.0;

    field.press_number(1, dog);
    field.press_number(2, dog);
    field.type_text("X");

    assert_eq!(field.text(), SENTENCE.replace("dog", "X"));
}

/// What a double click on `text`, at the x `keys` leave the caret at plus
/// `nudge`, leaves once `X` is typed over its selection.
fn double_click_then_type(text: &str, keys: &[(Key, Modifiers)], nudge: f32) -> String {
    let mut field = Field::focused(text);
    let x = field.x_after(keys) + nudge;
    field.press_number(1, x);
    field.press_number(2, x);
    field.type_text("X");
    field.text()
}

/// A key with nothing held, for [`Field::x_after`].
fn plain(key: Key) -> (Key, Modifiers) {
    (key, Modifiers::default())
}

/// A word is what Unicode's word segmentation (UAX #29) says it is, as GTK,
/// Qt and the browsers have it, so the full stop after the last word of a
/// sentence is not part of it: typing over the word keeps the stop.
#[test]
fn a_double_click_leaves_the_punctuation_beside_a_word() {
    assert_eq!(
        double_click_then_type("hello world.", &[plain(Key::End), plain(Key::Left)], -4.0),
        "hello X."
    );
}

/// A path is words between slashes, not one word: a double click in its last
/// part selects that part.
#[test]
fn a_double_click_in_a_path_selects_one_part_of_it() {
    assert_eq!(
        double_click_then_type("/usr/local/bin", &[plain(Key::End)], -4.0),
        "/usr/local/X"
    );
}

/// An address divides at the `@` and nowhere else: a full stop between two
/// letters is inside a word, so the domain is one, and so is `don't`.
#[test]
fn a_double_click_keeps_a_word_whole_across_a_full_stop_or_an_apostrophe() {
    assert_eq!(
        double_click_then_type("user@example.com", &[plain(Key::End)], -4.0),
        "user@X"
    );
    assert_eq!(
        double_click_then_type("user@example.com", &[plain(Key::Home)], 3.0),
        "X@example.com"
    );
    assert_eq!(
        double_click_then_type("say don't", &[plain(Key::End)], -4.0),
        "say X"
    );
}

/// A double click on punctuation selects the punctuation — less than a word,
/// and neither word beside it.
#[test]
fn a_double_click_on_punctuation_selects_only_the_punctuation() {
    let mut after_hello = vec![plain(Key::Home)];
    after_hello.extend([plain(Key::Right); 5]);
    assert_eq!(
        double_click_then_type("hello, world", &after_hello, 1.0),
        "helloX world"
    );
}

/// Ctrl+Right stops at the end of the next word and Ctrl+Left at the start of
/// the one before, stepping over the punctuation between them as the browsers
/// do — so a path is crossed a part at a time.
#[test]
fn ctrl_arrows_stop_at_the_parts_of_a_path() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let mut field = Field::focused("/usr/local/bin");
    field.key(Key::Home);
    field.press(Key::Right, ctrl);
    field.type_text("X");
    assert_eq!(
        field.text(),
        "/usrX/local/bin",
        "to the end of usr, index 4"
    );

    let mut field = Field::focused("/usr/local/bin");
    field.key(Key::End);
    field.press(Key::Left, ctrl);
    field.press(Key::Left, ctrl);
    field.type_text("X");
    assert_eq!(
        field.text(),
        "/usr/Xlocal/bin",
        "back to the start of bin, then of local"
    );
}

/// Punctuation is never a stop of its own: Ctrl+Right from the end of a word
/// goes over the comma and the space to the end of the next word, and stops
/// there rather than past the full stop; Ctrl+Left from the end goes back over
/// the full stop to the start of the last word.
#[test]
fn ctrl_arrows_step_over_punctuation() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    let mut field = Field::focused("hello, world.");
    field.key(Key::Home);
    field.press(Key::Right, ctrl);
    field.press(Key::Right, ctrl);
    field.type_text("X");
    assert_eq!(field.text(), "hello, worldX.");

    let mut field = Field::focused("hello, world.");
    field.key(Key::Home);
    for _ in 0..3 {
        field.press(Key::Right, ctrl);
    }
    field.type_text("Y");
    assert_eq!(
        field.text(),
        "hello, world.Y",
        "from the end of the last word, over the full stop to the end"
    );

    let mut field = Field::focused("hello, world.");
    field.key(Key::End);
    field.press(Key::Left, ctrl);
    field.type_text("X");
    assert_eq!(field.text(), "hello, Xworld.");
}
