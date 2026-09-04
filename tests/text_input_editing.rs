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
    clear_system_clipboard, clipboard_paste, primary_copy, take_clipboard_change,
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
        clear_system_clipboard();
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
        self.harness
            .send_at(Event::KeyDown { key, modifiers }, self.now)
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

    /// A middle press at a point, which is the primary-selection paste.
    fn middle_click(&mut self, x: f32, y: f32) {
        self.now += Duration::from_millis(1);
        self.harness
            .send_at(Event::mouse_down(x, y, MouseButton::Middle), self.now);
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
    // The sharpest of the twenty-five: with the guard gone, every field on a
    // surface takes every keystroke, and everything else here stays green.
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
    let mut field = Field::focused("hi");

    field.key(Key::Backspace);
    assert_eq!(field.text(), "hi");

    field.key(Key::End);
    field.key(Key::Delete);
    assert_eq!(field.text(), "hi");
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
    assert_eq!(clipboard_paste().as_deref(), Some("abcd"));
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
    primary_copy("pasted");

    field.middle_click(4.0, 5.0);
    assert_eq!(field.text(), "hi", "the primary selection got in");

    readonly.set(false);
    field.middle_click(4.0, 5.0);
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
