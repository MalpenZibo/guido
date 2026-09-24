//! Single-line text editing: [`text_input`] and [`password_input`].
//!
//! The two are one widget, [`TextInput<C>`], over two answers to where the
//! text lives. [`Plain`] mirrors an `RwSignal<String>`, which is what any field
//! wants. [`Masked`] keeps it in the [`Secret`] behind a [`Password`] and edits
//! it there, in place: nothing about a password is copied, kept for undo, or
//! allowed out through the clipboard. Everything else — the caret, selection,
//! scrolling, styling, focus — is the same code for both, so a password field
//! cannot fall behind the text field it looks like.
//!
//! Styling (background, borders, etc.) should be handled by wrapping in a Container.

use std::collections::VecDeque;
use std::ops::Range;
use std::time::{Duration, Instant};

use zeroize::Zeroize;

use crate::clock::{EventInstant, FrameInstant};
use crate::default_font_family;
use crate::jobs::{JobRequest, RequiredJob, request_job, request_job_at};
use crate::layout::{Constraints, Size};
use crate::reactive::focus::focused_widget;
use crate::reactive::{
    CursorIcon, IntoSignal, Password, Prop, RwSignal, clipboard_copy, clipboard_paste, has_focus,
    primary_copy, primary_paste, release_focus, request_focus,
};
use crate::renderer::{PaintContext, char_index_from_x_styled};
use crate::secret::Secret;
use crate::tree::{LayoutCtx, Tree, WidgetId};
use crate::widget_ref::{WidgetRef, register_widget_ref};

use super::control::Control;
use super::font::{FontFamily, FontWeight};
use super::state_layer::{StateWhen, Stateful};
use super::text_style::{DEFAULT_FONT_SIZE, TextStyle};
use super::widget::{Color, Event, EventResponse, Key, MouseButton, Rect, Widget};

/// The caret, selection band and placeholder colours a field declares for
/// itself.
///
/// These three are not text style. A [`Text`](crate::widgets::Text) draws
/// glyphs and nothing else; only a field draws a caret, a band or a
/// placeholder, so only a field is asked about them. They lived in
/// [`TextStyle`] for a while, each with a doc comment admitting that one
/// widget ever read it, which is the shape of a property in the wrong struct.
///
/// Every field is optional and independent, so declaring only the caret colour
/// leaves the selection at its default.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct InputStyle {
    /// Colour of the caret. Defaults to the resolved text colour — a field
    /// that only sets its text colour should not sprout a blue cursor.
    pub(crate) cursor_color: Prop<Color>,
    /// Colour of the selection band drawn behind the selected glyphs.
    pub(crate) selection_color: Prop<Color>,
    /// Colour of the placeholder. Defaults to the text colour at reduced
    /// alpha — a placeholder is the same text, quieter.
    pub(crate) placeholder_color: Prop<Color>,
}

/// Cursor blink interval in milliseconds
const CURSOR_BLINK_MS: u64 = 530;

/// How much of the text colour a placeholder keeps when nothing overrides it.
/// A placeholder is the same text, quieter — not a different colour to pick.
const PLACEHOLDER_ALPHA: f32 = 0.45;

/// Maximum number of undo history entries
const MAX_HISTORY_SIZE: usize = 100;

/// Padding from edges before scrolling starts
const SCROLL_PADDING: f32 = 2.0;

/// Time window for coalescing similar edits (in milliseconds)
const HISTORY_COALESCE_MS: u64 = 500;

/// The ambient facts of one edit: the width the caret has to stay inside, and
/// when the event that caused it happened.
///
/// Built once in `event` and carried down, the way `Container` carries a
/// `HitContext`. They travelled as two positional parameters through thirteen
/// private methods, of which two actually read the instant — and the next
/// ambient fact would have been a fourteenth.
#[derive(Clone, Copy)]
struct Edit {
    width: f32,
    at: EventInstant,
}

/// Type alias for text input callbacks
type TextCallback = Box<dyn Fn(&str)>;

/// A snapshot of text input state for undo/redo
#[derive(Clone, Debug)]
struct HistoryEntry {
    /// The text content
    text: String,
    /// Cursor position
    cursor: usize,
    /// Selection anchor
    anchor: usize,
}

/// Type of edit operation for history coalescing
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditType {
    Insert,
    Delete,
}

/// Undo/redo history manager
struct History {
    /// Stack of past states (most recent at end)
    undo_stack: VecDeque<HistoryEntry>,
    /// Stack of undone states for redo
    redo_stack: VecDeque<HistoryEntry>,
    /// Time of last edit (for coalescing)
    last_edit_time: EventInstant,
    /// Type of last edit (for coalescing)
    last_edit_type: Option<EditType>,
}

impl History {
    fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            last_edit_time: Instant::now().into(),
            last_edit_type: None,
        }
    }

    /// Push a new state to history (clears redo stack)
    /// Uses coalescing to merge similar edits within a time window
    fn push(&mut self, entry: HistoryEntry, edit_type: EditType, at: EventInstant) {
        let since_last = at.duration_since(self.last_edit_time);

        // Don't push if it's the same as the last entry
        if let Some(last) = self.undo_stack.back()
            && last.text == entry.text
        {
            return;
        }

        // Coalesce similar edits within the time window
        let should_coalesce = self.last_edit_type == Some(edit_type)
            && since_last < Duration::from_millis(HISTORY_COALESCE_MS)
            && !self.undo_stack.is_empty();

        if should_coalesce {
            // Update the last entry instead of creating a new one
            if let Some(last) = self.undo_stack.back_mut() {
                last.cursor = entry.cursor;
                last.anchor = entry.anchor;
                // Keep the original text (state before the sequence of edits)
            }
        } else {
            self.undo_stack.push_back(entry);
            self.redo_stack.clear();

            // Limit history size
            if self.undo_stack.len() > MAX_HISTORY_SIZE {
                self.undo_stack.pop_front();
            }
        }

        self.last_edit_time = at;
        self.last_edit_type = Some(edit_type);
    }

    /// Reset coalescing state (call after non-edit operations like undo/redo)
    fn reset_coalescing(&mut self) {
        self.last_edit_type = None;
    }

    /// Undo: pop from undo stack, push current to redo stack
    fn undo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        if let Some(previous) = self.undo_stack.pop_back() {
            self.redo_stack.push_back(current);
            Some(previous)
        } else {
            None
        }
    }

    /// Redo: pop from redo stack, push current to undo stack
    fn redo(&mut self, current: HistoryEntry) -> Option<HistoryEntry> {
        if let Some(next) = self.redo_stack.pop_back() {
            self.undo_stack.push_back(current);
            Some(next)
        } else {
            None
        }
    }
}

/// Selection state tracking anchor and cursor positions
#[derive(Clone, Copy, Debug, Default)]
pub struct Selection {
    /// Where selection started (anchor point)
    pub anchor: usize,
    /// Current cursor position
    pub cursor: usize,
}

impl Selection {
    /// Create a new selection with cursor at given position (no selection)
    pub fn new(pos: usize) -> Self {
        Self {
            anchor: pos,
            cursor: pos,
        }
    }

    /// Check if there is an active selection (anchor != cursor)
    pub fn has_selection(&self) -> bool {
        self.anchor != self.cursor
    }

    /// Get the start and end of the selection (min, max)
    pub fn range(&self) -> (usize, usize) {
        if self.anchor <= self.cursor {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Collapse selection to cursor position
    pub fn collapse(&mut self) {
        self.anchor = self.cursor;
    }
}

/// Where a field's text lives, and what may be done with it.
///
/// Public so that [`TextInput<C>`] can name it, and in a private module so that
/// nothing outside the crate can implement it: the two answers are [`Plain`]
/// and [`Masked`], and a third would be a field the rules above were never
/// asked about.
mod content {
    use std::ops::Range;

    pub trait Content: 'static {
        /// Whether the text is hidden: never copied out, and a Ctrl+arrow
        /// does not reveal where its words end.
        const HIDDEN: bool;

        /// Lend the text, untracked: the editing below runs from events. For
        /// finding positions in it, never for copying it out — that is
        /// [`snapshot`](Self::snapshot)'s to allow.
        fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R;

        /// A copy for the undo history, or `None` where there may be no
        /// copy — which is also what turns undo off.
        fn snapshot(&self) -> Option<String>;

        /// Replace a byte range, in place, and tell whoever is listening.
        fn replace(&mut self, range: Range<usize>, with: &str);

        /// Read the text back in layout, tracked. `Some` with the new character
        /// count when what is displayed differs from `char_count` characters
        /// of what was displayed before.
        fn refresh(&mut self, char_count: usize) -> Option<usize>;

        /// What is drawn for `char_count` characters.
        fn display(&self, char_count: usize) -> String;

        /// Enter. `Some` with the new character count when the submit took
        /// the text with it.
        fn submit(&mut self) -> Option<usize>;
    }
}

use content::Content;

/// A field's text as an ordinary `String`, mirrored from an `RwSignal`.
///
/// What [`text_input`] builds. The signal is the application's, and the
/// field writes every edit back to it.
pub struct Plain {
    value: RwSignal<String>,
    text: String,
    on_change: Option<TextCallback>,
    on_submit: Option<TextCallback>,
}

impl Content for Plain {
    const HIDDEN: bool = false;

    fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        f(&self.text)
    }

    fn snapshot(&self) -> Option<String> {
        Some(self.text.clone())
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.text.replace_range(range, with);
        self.value.set(self.text.clone());
        if let Some(ref callback) = self.on_change {
            callback(&self.text);
        }
    }

    fn refresh(&mut self, _char_count: usize) -> Option<usize> {
        let text = &mut self.text;
        let changed = self.value.with(|value| {
            let changed = value != text;
            if changed {
                text.clone_from(value);
            }
            changed
        });
        changed.then(|| self.text.chars().count())
    }

    fn display(&self, _char_count: usize) -> String {
        self.text.clone()
    }

    fn submit(&mut self) -> Option<usize> {
        if let Some(ref callback) = self.on_submit {
            callback(&self.text);
        }
        None
    }
}

/// A field's text as a [`Secret`], behind a [`Password`].
///
/// What [`password_input`] builds. The field edits the secret where it lies,
/// and draws [`mask_char`](TextInput::mask_char) once per character without
/// ever reading what the characters are.
pub struct Masked {
    password: Password,
    mask_char: Prop<char>,
    cached_mask_char: char,
    on_submit: Option<Box<dyn Fn(Secret)>>,
}

impl Content for Masked {
    const HIDDEN: bool = true;

    fn with_text<R>(&self, f: impl FnOnce(&str) -> R) -> R {
        self.password.with_untracked(|secret| f(secret.expose()))
    }

    fn snapshot(&self) -> Option<String> {
        // A history of a password is copies of it. GTK turns undo off for
        // invisible text for the same reason.
        None
    }

    fn replace(&mut self, range: Range<usize>, with: &str) {
        self.password
            .edit(|secret| secret.replace_range(range, with));
    }

    fn refresh(&mut self, char_count: usize) -> Option<usize> {
        // The count, not the text: counting borrows the secret and keeps
        // nothing, and the count is all the display is made of.
        let now = self.password.with(|secret| secret.expose().chars().count());
        let mask_char = self.mask_char.get_or('•');
        let changed = now != char_count || mask_char != self.cached_mask_char;
        self.cached_mask_char = mask_char;
        changed.then_some(now)
    }

    fn display(&self, char_count: usize) -> String {
        std::iter::repeat_n(self.cached_mask_char, char_count).collect()
    }

    fn submit(&mut self) -> Option<usize> {
        // Moved out, not lent: the application holds the one copy there is,
        // and the field starts again empty — swaylock clears on submit too.
        let callback = self.on_submit.as_ref()?;
        callback(self.password.take());
        Some(0)
    }
}

/// A single-line text field, over where its text lives.
///
/// `TextInput` is the plain one, bound to an `RwSignal<String>` by
/// [`text_input`]; [`PasswordInput`] keeps its text in a [`Password`] and is
/// built by [`password_input`].
pub struct TextInput<C = Plain> {
    content: C,
    cached_char_count: usize,
    cached_display_text: String,
    display_text_dirty: bool,

    // Measurement cache (avoid repeated text shaping in paint)
    /// Total width of display text
    cached_text_width: f32,
    /// Cumulative width at each character index (length = char_count + 1)
    /// cached_glyph_positions[i] = width of text[0..i]
    cached_glyph_positions: Vec<f32>,
    /// Whether measurements need to be recalculated
    measurements_dirty: bool,

    // Metrics resolved from the enclosing container's style. Cached because a
    // change to any of them invalidates the glyph measurements below.
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,

    /// Whether the text refuses to change. Not the same as disabled: a
    /// read-only field still takes the focus, still says so, and still lets a
    /// selection be made and copied — it only refuses the edit.
    readonly: Prop<bool>,

    /// Whether a caret is drawn at all. Off costs nothing: no caret, no blink,
    /// nothing to wake the loop for.
    caret: Prop<bool>,
    cached_caret: bool,

    /// See [`cursor`](Self::cursor).
    cursor: Prop<CursorIcon>,

    /// Handle for application code to reach this input — to focus it, mostly.
    widget_ref: Option<WidgetRef>,
    /// Shown while the value is empty. Reactive: a prompt that changes — PAM
    /// asking a different question — changes what the empty field says.
    placeholder: Prop<String>,

    /// An unmade offer of the initial focus. Cleared once made, so autofocus is
    /// a *first layout* behaviour rather than something that fights the user on
    /// every relayout.
    autofocus_pending: bool,

    // Selection state
    selection: Selection,

    // Cursor blinking
    cursor_visible: bool,
    last_cursor_toggle: FrameInstant,

    // Mouse drag selection
    is_dragging: bool,

    // Mouse hover state (for cursor icon)
    is_hovered: bool,
    /// The same fact behind a signal, so a state override resolving it
    /// subscribes. Written next to `is_hovered`, never instead of it: the
    /// cursor logic reads a bool on the event path, where a subscription
    /// would be noise.
    hover: RwSignal<bool>,

    // Undo/redo history
    history: History,

    // Horizontal scroll offset for text overflow
    scroll_offset: f32,

    /// What this input declares about its own text, and about the furniture
    /// only it draws. Boxed and absent by default.
    text_style: Option<Box<TextStyle>>,
    /// How the declared colour and size move, when declared with a motion.
    text_anims: Option<Box<crate::widgets::text_style::TextAnims>>,
    input_style: Option<Box<InputStyle>>,
    /// Overrides that apply while this field's control is in a state, in
    /// declaration order.
    states: Vec<(StateWhen, TextStyle)>,
}

/// A field whose text is a [`Secret`] behind a [`Password`].
pub type PasswordInput = TextInput<Masked>;

impl TextInput<Plain> {
    /// Create a TextInput with a Signal for two-way binding.
    /// Changes made in the TextInput will be written back to the signal.
    pub fn new(signal: RwSignal<String>) -> Self {
        // Use get_untracked() to avoid registering layout dependencies during widget creation.
        // Layout dependencies should only be registered during the widget's own layout phase.
        let text = signal.get_untracked();
        let char_count = text.chars().count();
        Self::with_content(
            Plain {
                value: signal,
                text,
                on_change: None,
                on_submit: None,
            },
            char_count,
        )
    }

    /// Set callback for text changes
    pub fn on_change<F: Fn(&str) + 'static>(mut self, callback: F) -> Self {
        self.content.on_change = Some(Box::new(callback));
        self
    }

    /// Set callback for submit (Enter key)
    pub fn on_submit<F: Fn(&str) + 'static>(mut self, callback: F) -> Self {
        self.content.on_submit = Some(Box::new(callback));
        self
    }
}

impl TextInput<Masked> {
    /// A field over `password`, which it reads and edits in place.
    pub fn new_password(password: Password) -> Self {
        let char_count = password.with_untracked(|secret| secret.expose().chars().count());
        Self::with_content(
            Masked {
                password,
                mask_char: Prop::Unset,
                cached_mask_char: '•',
                on_submit: None,
            },
            char_count,
        )
    }

    /// The character drawn for each one typed (default: '•').
    pub fn mask_char<M>(mut self, c: impl IntoSignal<char, M>) -> Self {
        self.content.mask_char = c.into_prop();
        self
    }

    /// Called on Enter with the field's [`Secret`], moved out of it.
    ///
    /// The field is empty from that moment, and the application holds the one
    /// copy there is: hand it to PAM, on another thread if need be — `Secret`
    /// is `Send` — and let it drop, which wipes it.
    pub fn on_submit<F: Fn(Secret) + 'static>(mut self, callback: F) -> Self {
        self.content.on_submit = Some(Box::new(callback));
        self
    }
}

impl<C: Content> TextInput<C> {
    fn with_content(content: C, cached_char_count: usize) -> Self {
        let default_family = default_font_family();
        Self {
            content,
            cached_char_count,
            cached_display_text: String::new(),
            display_text_dirty: true,
            cached_text_width: 0.0,
            cached_glyph_positions: Vec::new(),
            measurements_dirty: true,
            cached_font_size: DEFAULT_FONT_SIZE,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
            readonly: Prop::Unset,
            caret: Prop::Unset,
            cached_caret: true,
            cursor: Prop::Const(CursorIcon::Text),
            widget_ref: None,
            placeholder: Prop::Unset,
            autofocus_pending: false,
            selection: Selection::new(0),
            cursor_visible: true,
            last_cursor_toggle: Instant::now().into(),
            is_dragging: false,
            is_hovered: false,
            hover: crate::reactive::signal::create_signal(false),
            history: History::new(),
            scroll_offset: 0.0,
            text_style: None,
            text_anims: None,
            input_style: None,
            states: Vec::new(),
        }
    }

    /// Give up the hover.
    ///
    /// Two events mean the same thing — the pointer moved out of the bounds, or
    /// it left the surface — and both used to spell it out. The
    /// guard lives here rather than at the call sites so that neither can
    /// forget it: an unchanged pointer move must not cost a signal write.
    fn release_pointer(&mut self) {
        if self.is_hovered {
            self.is_hovered = false;
            self.hover.set(false);
        }
    }

    /// Whether an override applies. Reading the answer subscribes the field to
    /// its control, so it is asked only for a state it declares.
    fn is_state_active(&self, id: WidgetId, control: Option<&Control>, when: &StateWhen) -> bool {
        match (when, control) {
            (StateWhen::When(condition), _) => condition.get(),
            (_, Some(control)) => control.is_active(when),
            // No control above: the field is its own unit. It already tracks
            // the pointer for its cursor, and it is the thing that holds the
            // focus, so both answers are its own.
            (StateWhen::Hovered, None) => self.hover.get(),
            (StateWhen::Focused, None) => crate::reactive::focus::focus_path().contains(id),
            // Nothing above it declared `enabled`, so nothing switched it off —
            // and being read-only is not being disabled.
            (StateWhen::Pressed | StateWhen::Disabled, None) => false,
        }
    }

    fn resolved_input_style(&self) -> InputStyle {
        self.input_style.as_deref().copied().unwrap_or_default()
    }

    /// Refuse edits, and nothing else.
    ///
    /// For the field whose answer is already in flight: a lock screen that has
    /// sent the password to PAM must stop taking the characters typed while it
    /// waits, because they can never be sent and are thrown away when the
    /// answer arrives.
    ///
    /// **It keeps the focus, and the ring that says so.** That is the whole
    /// reason this is not
    /// [`Container::enabled`](crate::widgets::Container::enabled): a disabled
    /// thing is not available and gives the keyboard up, while a read-only one
    /// is still where your typing is aimed — on a multi-monitor lock screen the
    /// `when_focused` ring is the only thing naming the screen. Every toolkit
    /// keeps these two apart for exactly this reason: `readonly` beside
    /// `disabled` on the web, `setReadOnly` beside `setEnabled` in Qt,
    /// `readOnly` beside `enabled` in Flutter.
    ///
    /// The caret is not part of it. A field that should stop blinking while it
    /// waits says so with the property that owns that —
    /// [`caret`](Self::caret) — driven by the same signal.
    ///
    /// ```
    /// use guido::prelude::*;
    ///
    /// let value = create_signal(String::new());
    /// let busy = create_signal(false);
    /// let _ = text_input(value).readonly(busy).caret(move || !busy.get());
    /// ```
    ///
    /// What it refuses is every *edit*: typing, backspace, delete, paste, cut
    /// and undo. What it keeps is everything that only reads — the caret moves,
    /// a selection can be made and copied, and `Enter` still reaches
    /// [`on_submit`](Self::on_submit), which is Qt's line and the one a caller
    /// can put its own guard behind.
    pub fn readonly<M>(mut self, readonly: impl IntoSignal<bool, M>) -> Self {
        self.readonly = readonly.into_prop();
        self
    }

    /// Attach a handle, so application code can move the keyboard here.
    ///
    /// The container has the same builder; put the ref on the *input* when what
    /// you mean is "focus this field", since a container cannot take focus.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// let field = create_widget_ref();
    /// // ...
    /// container().on_click(move || field.focus());
    /// ```
    pub fn widget_ref(mut self, widget_ref: WidgetRef) -> Self {
        self.widget_ref = Some(widget_ref);
        self
    }

    /// Text to show while the field is empty.
    ///
    /// Drawn in the placeholder colour — this field's text colour at reduced
    /// alpha unless it declares
    /// [`placeholder_color`](Self::placeholder_color) — and
    /// never masked, since it is a label rather than a value: a password field
    /// with a placeholder shows the word, not bullets.
    ///
    /// Reactive, so a prompt that changes changes the empty field with it.
    pub fn placeholder<M>(mut self, text: impl IntoSignal<String, M>) -> Self {
        self.placeholder = text.into_prop();
        self
    }

    /// Draw no caret, keeping the focus and everything it carries — click to
    /// position, drag to select, the keyboard.
    ///
    /// For a field where the caret says nothing: a masked one, where every
    /// character looks the same and you are always at the end. swaylock draws no
    /// caret for that reason.
    ///
    /// It is also the cheapest field there is. A blinking caret is the one thing
    /// a still screen redraws on its own, twice a second, forever; without it an
    /// idle surface wakes the loop for nothing at all.
    pub fn no_caret(self) -> Self {
        self.caret(false)
    }

    /// Whether a caret is drawn at all.
    ///
    /// The property [`no_caret`](Self::no_caret) is the shorthand for. Read
    /// where the rest of the declared values are read, so a field can be told
    /// to stop drawing one without being rebuilt and losing its focus.
    pub fn caret<M>(mut self, caret: impl IntoSignal<bool, M>) -> Self {
        self.caret = caret.into_prop();
        self
    }

    /// The shape the pointer takes over the field: the I-beam unless said
    /// otherwise.
    ///
    /// It is claimed on every pointer event inside the field, a press as well
    /// as a move, and the field is inside whatever declared a shape around it,
    /// so this is the one place a field's shape can be changed. A masked field
    /// with no caret has nothing a click could place, and on a surface with no
    /// pointer it should not bring one back:
    ///
    /// ```
    /// # use guido::prelude::*;
    /// # let secret = create_password();
    /// password_input(secret)
    ///     .no_caret()
    ///     .cursor(CursorIcon::Hidden);
    /// ```
    ///
    /// It takes a signal, as [`Container::cursor`](crate::widgets::Container::cursor)
    /// does, and a shape that changes under a still pointer goes out without
    /// waiting for it to move.
    pub fn cursor<M>(mut self, cursor: impl IntoSignal<CursorIcon, M>) -> Self {
        self.cursor = cursor.into_prop();
        self
    }

    /// Take keyboard focus when this input first appears, if nothing else has it.
    ///
    /// For a screen that exists to be typed into — a lock screen, a search
    /// overlay, a dialog with one field — where making the user click first is
    /// the wrong answer, and where there is no cursor to click *with* on a
    /// surface that has no pointer.
    ///
    /// The offer is made once, at the input's first layout, and only when no
    /// widget holds focus. Both halves matter:
    ///
    /// - *once*, so a relayout does not drag focus back from wherever the user
    ///   has since put it
    /// - *only when free*, so two autofocusing inputs do not fight — the first
    ///   laid out wins — and one on a second surface does not pull focus off the
    ///   surface being typed into. That last case is what a lock screen with two
    ///   monitors is: the same view built per output, all of them asking.
    ///
    /// The equivalent elsewhere: Flutter's `autofocus: true`, Floem's
    /// `autofocus` focus-nav flag, `forward-focus` on a Slint window.
    pub fn autofocus(mut self) -> Self {
        self.autofocus_pending = true;
        self
    }

    /// Get the display text (masked for a password), using cache when clean
    fn display_text(&mut self) -> &str {
        if self.display_text_dirty {
            self.cached_display_text = self.content.display(self.cached_char_count);
            self.display_text_dirty = false;
        }
        &self.cached_display_text
    }

    /// Get the display text for immutable contexts (for paint)
    fn display_text_cached(&self) -> &str {
        &self.cached_display_text
    }

    /// Update cached glyph positions if measurements are dirty.
    /// Call this from layout() to ensure measurements are ready for paint().
    fn update_measurements(&mut self) {
        if !self.measurements_dirty {
            return;
        }

        // Ensure display text is current
        let _ = self.display_text();
        let display = &self.cached_display_text;
        let font_size = self.cached_font_size;
        let font_family = &self.cached_font_family;
        let font_weight = self.cached_font_weight;

        // Build cumulative position array (positions[i] = x of the boundary
        // before character i) by shaping the text ONCE. The previous
        // implementation measured every prefix — O(n) shaping passes of
        // O(n) text per keystroke — and flooded the measurement cache with
        // one entry per prefix.
        self.cached_glyph_positions = crate::renderer::measure_char_positions_styled(
            display,
            font_size,
            *font_family,
            font_weight,
        );
        self.cached_text_width = self
            .cached_glyph_positions
            .last()
            .copied()
            .unwrap_or_default();

        self.measurements_dirty = false;
    }

    /// Get cached width at a character index (0 to char_count inclusive)
    fn cached_width_at_char(&self, char_index: usize) -> f32 {
        self.cached_glyph_positions
            .get(char_index)
            .copied()
            .unwrap_or(self.cached_text_width)
    }

    /// Convert a character index to a byte index in the text
    fn char_to_byte_index(&self, char_index: usize) -> usize {
        self.content.with_text(|text| {
            text.char_indices()
                .nth(char_index)
                .map(|(i, _)| i)
                .unwrap_or(text.len())
        })
    }

    /// Convert a character range to a byte range in the cached value
    fn char_range_to_byte_range(&self, start: usize, end: usize) -> (usize, usize) {
        let byte_start = self.char_to_byte_index(start);
        let byte_end = self.char_to_byte_index(end);
        (byte_start, byte_end)
    }

    /// Refresh cached values from the bound signal and this field's style.
    ///
    /// The reads belong to this input because the call its layout is made
    /// inside does — see `LayoutCtx::layout_child` — so a change to a declared
    /// metric re-lays-out this input and nothing else.
    fn refresh(&mut self, ctx: &mut LayoutCtx, id: WidgetId) -> f32 {
        let (new_font_size, new_font_family, new_font_weight, overflow, new_color) = {
            let style = self.resolved_text_style(ctx.tree_ref(), id);

            // Assigned here rather than returned, as the font metrics are:
            // the rule in this function is that a value comes back through
            // the tuple only when it is compared against its cache below to
            // raise a dirty flag.
            self.cached_caret = self.caret.get_or(true);

            (
                style.font_size(id),
                style.font_family(),
                style.font_weight(),
                crate::widgets::text::decoration_overflow(style.stroke(), style.shadow()),
                self.animates_text_color().then(|| style.color(id)),
            )
        };

        // Compared against what the field holds, in place: a read that
        // copied the value out would be a copy per layout.
        if let Some(char_count) = self.content.refresh(self.cached_char_count) {
            self.text_replaced(char_count);
        }

        // The declared motions, pointed at what the style now resolves to. The
        // same two properties `Text` animates, from the same list — see
        // `declares_text_style`.
        let new_font_size =
            self.retarget_text_anims(ctx, id, new_color.unwrap_or(Color::WHITE), new_font_size);

        // Check font properties - only set dirty flag if changed
        if (new_font_size - self.cached_font_size).abs() > f32::EPSILON {
            self.cached_font_size = new_font_size;
            self.measurements_dirty = true;
        }
        if new_font_family != self.cached_font_family {
            self.cached_font_family = new_font_family;
            self.measurements_dirty = true;
        }
        if new_font_weight != self.cached_font_weight {
            self.cached_font_weight = new_font_weight;
            self.measurements_dirty = true;
        }

        overflow
    }

    /// Advance the blink, and ask to be woken when it next changes.
    ///
    /// Returns whether the caret is drawn at all.
    ///
    /// The wake is *scheduled*, not animated. Asking for an animation frame —
    /// which is what this did — pins the loop at 60 fps for a square wave that
    /// changes twice a second, so 113 frames out of 114 repaint the same pixels.
    /// A focused field is the normal state of a lock screen, and that ran all
    /// night.
    fn update_cursor_blink(&mut self, id: WidgetId, now: FrameInstant) -> bool {
        if !self.cached_caret || !has_focus(id) {
            return false;
        }
        let period = Duration::from_millis(CURSOR_BLINK_MS);
        if now.duration_since(self.last_cursor_toggle) >= period {
            self.cursor_visible = !self.cursor_visible;
            self.last_cursor_toggle = now;
        }
        true
    }

    /// Reset cursor to visible (called on input)
    /// Restart the caret's blink from the edit that moved it.
    ///
    /// The blink runs on frames and the edit happened on an event, so the
    /// phase starts a loop's input latency earlier than the frame that will
    /// draw it — which shifts the first blink and nothing else. That is the
    /// crossing, made here rather than at each of the callers, which is the
    /// only reason this takes an [`EventInstant`] and not the clock it
    /// measures against.
    fn reset_cursor_blink(&mut self, at: EventInstant) {
        let at = at.as_frame_start();
        self.cursor_visible = true;
        self.last_cursor_toggle = at;
    }

    /// Get character index from x coordinate relative to text start.
    /// Uses cached glyph positions for O(log n) binary search.
    fn char_index_at_x(&self, x: f32, bounds: Rect) -> usize {
        let text_x = bounds.x;
        // Account for scroll offset
        let relative_x = x - text_x + self.scroll_offset;

        if relative_x <= 0.0 {
            return 0;
        }
        if relative_x >= self.cached_text_width {
            return self.cached_char_count;
        }

        // Binary search on cached glyph positions
        let positions = &self.cached_glyph_positions;
        if positions.is_empty() {
            // Fallback if cache not populated (shouldn't happen after layout)
            let display = self.display_text_cached();
            return char_index_from_x_styled(
                display,
                self.cached_font_size,
                relative_x,
                self.cached_font_family,
                self.cached_font_weight,
            );
        }

        // Find the insertion point using binary search
        let mut left = 0;
        let mut right = positions.len();
        while left < right {
            let mid = (left + right) / 2;
            if positions[mid] < relative_x {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // left now points to first position >= relative_x
        // Check if click is closer to the previous character
        if left > 0 && left < positions.len() {
            let prev_x = positions[left - 1];
            let curr_x = positions[left];
            if (relative_x - prev_x) < (curr_x - relative_x) {
                return left - 1;
            }
        }

        left.min(self.cached_char_count)
    }

    /// Ensure the cursor is visible by adjusting scroll offset
    fn ensure_cursor_visible(&mut self, width: f32) {
        // Ensure measurements are up to date
        self.update_measurements();

        let cursor_x = self.cached_width_at_char(self.selection.cursor);
        let visible_width = width - SCROLL_PADDING * 2.0;

        if visible_width <= 0.0 {
            return;
        }

        // If cursor is to the left of visible area, scroll left
        if cursor_x < self.scroll_offset + SCROLL_PADDING {
            self.scroll_offset = (cursor_x - SCROLL_PADDING).max(0.0);
        }
        // If cursor is to the right of visible area, scroll right
        else if cursor_x > self.scroll_offset + visible_width {
            self.scroll_offset = cursor_x - visible_width;
        }

        // Don't scroll past the start
        self.scroll_offset = self.scroll_offset.max(0.0);
    }

    /// Insert text at cursor, replacing any selection
    /// Whether the field is refusing edits right now.
    ///
    /// Asked by each of the four functions that write the text, rather than at
    /// the entrance: a keystroke is not the only way in — a middle click pastes
    /// the primary selection straight into `insert_text` — and a guard at one
    /// entrance is a guard the next one does not have. Untracked for the same
    /// reason the clipboard guards are: an event does not wait for a layout, so
    /// a field told to freeze must be frozen for the very next key rather than
    /// for the frame after the next pass.
    fn refuses_edits(&self) -> bool {
        self.readonly.get_or_untracked(false)
    }

    fn insert_text(&mut self, text: &str, edit: Edit) {
        if self.refuses_edits() {
            return;
        }
        // Save state before modification
        self.save_to_history(EditType::Insert, edit.at);

        let (start, end) = self.selection.range();
        let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);
        let inserted_char_count = text.chars().count();

        // Replace selection with new text
        self.content.replace(byte_start..byte_end, text);
        // Update cached char count: old - deleted + inserted
        self.cached_char_count = self.cached_char_count - (end - start) + inserted_char_count;
        self.display_text_dirty = true;
        self.measurements_dirty = true;
        self.selection = Selection::new(start + inserted_char_count);

        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Delete selected text or character before/after cursor
    fn delete(&mut self, forward: bool, edit: Edit) {
        if self.refuses_edits() {
            return;
        }
        // Check if there's anything to delete
        let has_content_to_delete = if self.selection.has_selection() {
            true
        } else if forward {
            self.selection.cursor < self.cached_char_count
        } else {
            self.selection.cursor > 0
        };

        // Save state before modification (only if we'll actually delete something)
        if has_content_to_delete {
            self.save_to_history(EditType::Delete, edit.at);
        }

        if self.selection.has_selection() {
            // Delete selection
            let (start, end) = self.selection.range();
            self.delete_range(start, end);
            self.selection = Selection::new(start);
        } else if forward {
            // Delete character after cursor
            if self.selection.cursor < self.cached_char_count {
                self.delete_range(self.selection.cursor, self.selection.cursor + 1);
            }
        } else {
            // Delete character before cursor (backspace)
            if self.selection.cursor > 0 {
                self.delete_range(self.selection.cursor - 1, self.selection.cursor);
                self.selection = Selection::new(self.selection.cursor - 1);
            }
        }
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Delete a range of characters
    fn delete_range(&mut self, start: usize, end: usize) {
        let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);

        self.content.replace(byte_start..byte_end, "");
        self.cached_char_count -= end - start;
        self.display_text_dirty = true;
        self.measurements_dirty = true;
    }

    /// The text was replaced wholesale — read back in layout, restored by an
    /// undo, or taken by a submit — and is now `char_count` characters long.
    fn text_replaced(&mut self, char_count: usize) {
        self.cached_char_count = char_count;
        self.display_text_dirty = true;
        self.measurements_dirty = true;
        // Clamp selection to valid range
        self.selection.cursor = self.selection.cursor.min(char_count);
        self.selection.anchor = self.selection.anchor.min(char_count);
    }

    /// Move cursor left/right, optionally extending selection
    fn move_cursor(&mut self, direction: i32, extend_selection: bool, word: bool, edit: Edit) {
        let new_pos = if word {
            self.find_word_boundary(self.selection.cursor, direction)
        } else if direction < 0 {
            self.selection.cursor.saturating_sub(1)
        } else {
            (self.selection.cursor + 1).min(self.cached_char_count)
        };

        self.selection.cursor = new_pos;
        if !extend_selection {
            self.selection.collapse();
        }
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Find word boundary in given direction
    fn find_word_boundary(&self, start: usize, direction: i32) -> usize {
        let len = self.cached_char_count;

        // Where a hidden text's words end is part of what is hidden, so a
        // word jump goes to the edge, as GTK's password entry does.
        if C::HIDDEN {
            return if direction < 0 { 0 } else { len };
        }

        if direction < 0 {
            // Move left - collect only the prefix up to cursor (not entire string)
            if start == 0 {
                return 0;
            }

            // Collect characters before cursor position
            let prefix: Vec<char> = self.content.with_text(|t| t.chars().take(start).collect());
            let mut pos = prefix.len() - 1;

            // Skip whitespace going backwards
            while pos > 0 && prefix[pos].is_whitespace() {
                pos -= 1;
            }
            // Skip word characters going backwards
            while pos > 0 && !prefix[pos - 1].is_whitespace() {
                pos -= 1;
            }
            pos
        } else {
            // Move right - use iterator directly, no allocation
            if start >= len {
                return len;
            }

            self.content.with_text(|text| {
                let mut pos = start;
                let mut chars = text.chars().skip(start).peekable();

                // Skip word characters
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    chars.next();
                    pos += 1;
                }
                // Skip whitespace
                for c in chars {
                    if !c.is_whitespace() {
                        break;
                    }
                    pos += 1;
                }
                pos.min(len)
            })
        }
    }

    /// Move cursor to start/end
    fn move_to_edge(&mut self, to_start: bool, extend_selection: bool, edit: Edit) {
        self.selection.cursor = if to_start { 0 } else { self.cached_char_count };
        if !extend_selection {
            self.selection.collapse();
        }
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Select all text
    fn select_all(&mut self, edit: Edit) {
        self.selection.anchor = 0;
        self.selection.cursor = self.cached_char_count;
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Get selected text
    fn get_selected_text(&self) -> Option<String> {
        if self.selection.has_selection() {
            let (start, end) = self.selection.range();
            let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);
            Some(
                self.content
                    .with_text(|text| text[byte_start..byte_end].to_string()),
            )
        } else {
            None
        }
    }

    /// The selection, when it is allowed to leave the widget.
    ///
    /// `None` for a password. What a masked field holds must not reach the
    /// clipboard or the primary selection, and the leak that matters is not
    /// Ctrl+C — it is the primary selection, which an ordinary mouse drag fills
    /// with no keystroke at all, ready for a middle-click anywhere else. GTK4's
    /// `GtkPasswordEntry` refuses to export for the same reason; GTK3 exported
    /// the mask instead, which is a row of bullets that is no use to paste.
    ///
    /// Pasting *into* the field stays allowed. Blocking that is the security
    /// theatre banking sites are mocked for: it stops password managers, not
    /// attackers, and pushes people towards passwords they can type.
    fn exportable_selection(&self) -> Option<String> {
        if C::HIDDEN {
            return None;
        }
        self.get_selected_text()
    }

    /// Copy selected text to clipboard
    fn copy_selection(&self) {
        if let Some(text) = self.exportable_selection() {
            clipboard_copy(&text);
        }
    }

    /// Cut selected text (copy and delete)
    fn cut_selection(&mut self, edit: Edit) {
        // A cut that cannot copy is not a cut. Refusing the gesture outright is
        // what GtkPasswordEntry does, and it keeps Ctrl+X from quietly becoming
        // a delete while the user believes the clipboard was filled.
        if C::HIDDEN {
            return;
        }
        if self.selection.has_selection() {
            self.copy_selection();
            self.delete(false, edit); // Delete the selection
        }
    }

    /// Paste text from clipboard
    fn paste(&mut self, edit: Edit) {
        if let Some(mut text) = clipboard_paste() {
            self.insert_pasted(&mut text, edit);
        }
    }

    /// Insert what a paste brought, and wipe the `String` it came in: pasting
    /// a password from a manager is the ordinary case, and the copy the
    /// clipboard handed over is ours to clear.
    fn insert_pasted(&mut self, text: &mut String, edit: Edit) {
        self.insert_text(text, edit);
        text.zeroize();
    }

    /// Save current state to history (call before making changes)
    fn save_to_history(&mut self, edit_type: EditType, at: EventInstant) {
        if let Some(entry) = self.current_history_entry() {
            self.history.push(entry, edit_type, at);
        }
    }

    /// Get current state as a history entry, where the text allows one.
    fn current_history_entry(&self) -> Option<HistoryEntry> {
        Some(HistoryEntry {
            text: self.content.snapshot()?,
            cursor: self.selection.cursor,
            anchor: self.selection.anchor,
        })
    }

    /// Undo the last change
    fn undo(&mut self, edit: Edit) {
        if self.refuses_edits() {
            return;
        }
        let Some(current) = self.current_history_entry() else {
            return;
        };
        if let Some(previous) = self.history.undo(current) {
            self.restore(previous, edit);
        }
    }

    /// Redo the last undone change
    fn redo(&mut self, edit: Edit) {
        if self.refuses_edits() {
            return;
        }
        let Some(current) = self.current_history_entry() else {
            return;
        };
        if let Some(next) = self.history.redo(current) {
            self.restore(next, edit);
        }
    }

    /// Put back a state from the history.
    fn restore(&mut self, entry: HistoryEntry, edit: Edit) {
        let whole = self.content.with_text(str::len);
        self.content.replace(0..whole, &entry.text);
        self.text_replaced(entry.text.chars().count());
        self.selection.cursor = entry.cursor;
        self.selection.anchor = entry.anchor;
        self.history.reset_coalescing();
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Handle key down event
    fn handle_key(&mut self, key: &Key, ctrl: bool, shift: bool, edit: Edit) -> EventResponse {
        // What the *answer* is for a key a read-only field will not use — the
        // refusal itself is at the four writers, which is what makes it hold
        // for the middle-click paste as well. `Ignored` rather than `Handled`,
        // because a keystroke this field will not use is a keystroke somebody
        // else may: Qt's read-only line edit ignores them for the same reason.
        if mutates(key, ctrl) && self.refuses_edits() {
            return EventResponse::Ignored;
        }
        match key {
            Key::Backspace => {
                self.delete(false, edit);
                EventResponse::Handled
            }
            Key::Delete => {
                self.delete(true, edit);
                EventResponse::Handled
            }
            Key::Enter => {
                if let Some(char_count) = self.content.submit() {
                    self.text_replaced(char_count);
                }
                EventResponse::Handled
            }
            Key::Left => {
                if !shift && self.selection.has_selection() {
                    // Collapse to start of selection
                    let (start, _) = self.selection.range();
                    self.selection = Selection::new(start);
                    self.reset_cursor_blink(edit.at);
                } else {
                    self.move_cursor(-1, shift, ctrl, edit);
                }
                EventResponse::Handled
            }
            Key::Right => {
                if !shift && self.selection.has_selection() {
                    // Collapse to end of selection
                    let (_, end) = self.selection.range();
                    self.selection = Selection::new(end);
                    self.reset_cursor_blink(edit.at);
                } else {
                    self.move_cursor(1, shift, ctrl, edit);
                }
                EventResponse::Handled
            }
            Key::Home => {
                self.move_to_edge(true, shift, edit);
                EventResponse::Handled
            }
            Key::End => {
                self.move_to_edge(false, shift, edit);
                EventResponse::Handled
            }
            Key::Char(c) => {
                if ctrl {
                    match c.to_ascii_lowercase() {
                        'a' => {
                            self.select_all(edit);
                            EventResponse::Handled
                        }
                        'c' => {
                            self.copy_selection();
                            EventResponse::Handled
                        }
                        'x' => {
                            self.cut_selection(edit);
                            EventResponse::Handled
                        }
                        'v' => {
                            self.paste(edit);
                            EventResponse::Handled
                        }
                        'z' => {
                            // Ctrl+Shift+Z = redo, Ctrl+Z = undo
                            if shift {
                                self.redo(edit);
                            } else {
                                self.undo(edit);
                            }
                            EventResponse::Handled
                        }
                        'y' => {
                            // Ctrl+Y = redo
                            self.redo(edit);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                } else if !c.is_control() {
                    // Encoded on the stack: a `String` per keystroke is a
                    // heap copy of every character of a password.
                    self.insert_text(c.encode_utf8(&mut [0; 4]), edit);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }
}

/// Whether this keystroke would change the text.
///
/// Only decides what a read-only field *answers* — the edit is already refused
/// at the writers — so a key missing from this list is a key answered
/// `Handled` that changes nothing, rather than a hole.
fn mutates(key: &Key, ctrl: bool) -> bool {
    match key {
        Key::Backspace | Key::Delete => true,
        Key::Char(c) if ctrl => matches!(c.to_ascii_lowercase(), 'x' | 'v' | 'z' | 'y'),
        Key::Char(c) => !c.is_control(),
        _ => false,
    }
}

impl<C: Content> Stateful for TextInput<C> {
    type Style = TextStyle;

    fn push_state_style(&mut self, when: StateWhen, style: TextStyle) {
        self.states.push((when, style));
    }
}

crate::widgets::text_style::declares_text_style!(impl[C: Content] TextInput<C>, text_style, text_anims);

/// The field's own furniture: the caret, the selection band and the
/// placeholder.
///
/// Inherent, and only here, because a field is the only widget that draws any
/// of it. A caret colour on a `Text` would be a property nothing reads, and a
/// container declares nothing about the text inside it.
impl<C: Content> TextInput<C> {
    fn input_style_mut(&mut self) -> &mut InputStyle {
        self.input_style.get_or_insert_with(Box::default)
    }

    /// Colour of the caret.
    pub fn cursor_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().cursor_color = color.into_prop();
        self
    }

    /// Colour of the selection band behind the selected glyphs.
    pub fn selection_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().selection_color = color.into_prop();
        self
    }

    /// Colour of the placeholder.
    pub fn placeholder_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_style_mut().placeholder_color = color.into_prop();
        self
    }
}

impl<C: Content> Widget for TextInput<C> {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let blinking = self.update_cursor_blink(id, tree.frame_instant());
        let animating = self.advance_text_anims(tree, id);
        blinking || animating
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        let id = ctx.id();
        // Text inputs are never relayout boundaries
        ctx.tree().set_relayout_boundary(id, false);

        // Refresh cached values from reactive properties. The reads belong to
        // this layout, which is the scope the call was made inside.
        let overflow = self.refresh(ctx, id);
        ctx.tree().set_own_paint_reach(id, overflow);

        // Update measurement cache (has internal dirty check)
        self.update_measurements();

        // Use cached text width for sizing (TextMeasurer caches the actual measurement)
        // Use previous height from tree to maintain stable sizing
        let prev_height = ctx
            .tree_ref()
            .cached_size(id)
            .map(|s| s.height)
            .unwrap_or(0.0);
        let height = (self.cached_font_size * 1.2).max(prev_height);

        // Text inputs should fill available width (like HTML input elements)
        // Use max_width if available, otherwise fall back to measured width
        let width = if constraints.max_width.is_finite() && constraints.max_width > 0.0 {
            constraints.max_width
        } else {
            self.cached_text_width.max(100.0) // Minimum 100px if unconstrained
        };

        let size = Size::new(
            width.max(constraints.min_width).min(constraints.max_width),
            height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        if let Some(widget_ref) = self.widget_ref {
            register_widget_ref(id, widget_ref);
        }

        // Here rather than at construction because focus needs the tree, and
        // this is the first moment the input is in one — the same reason Flutter
        // makes you wait for a post-frame callback to request focus by hand.
        if self.autofocus_pending {
            self.autofocus_pending = false;
            if focused_widget().is_none() {
                request_focus(ctx.tree(), id);
            }
        }

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let (tree, id) = (ctx.tree(), ctx.id());
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let bounds = tree.get_bounds(id).unwrap_or_default();
        let display = self.display_text_cached();
        let is_focused = has_focus(id);

        // Read inside the scope the framework opened around this call, so a
        // change to one repaints this input and nothing else.
        let (text_color, selection_color, cursor_color, stroke, shadow, placeholder) = {
            let style = self.resolved_text_style(tree, id);
            let input = self.resolved_input_style();
            let text_color = crate::widgets::container::get_animated_value(
                self.text_anims.as_ref().and_then(|a| a.color.as_ref()),
                || style.color(id),
            );
            // Only when there is nothing to show instead. Read inside the
            // tracking scope like every other paint input, so a prompt that
            // changes repaints the field.
            // The emptiness test first: reading the prop clones the prompt out
            // and subscribes to it, and a field with content shows no prompt —
            // so doing it the other way round allocates a `String` per paint
            // and repaints a field for a prompt it is not displaying.
            let placeholder = (self.cached_char_count == 0)
                .then(|| self.placeholder.get())
                .flatten()
                .map(|placeholder| {
                    let color = input.placeholder_color.get_or(Color::rgba(
                        text_color.r,
                        text_color.g,
                        text_color.b,
                        text_color.a * PLACEHOLDER_ALPHA,
                    ));
                    (placeholder, color)
                });
            (
                text_color,
                input
                    .selection_color
                    .get_or(Color::rgba(0.4, 0.6, 1.0, 0.4)),
                // The caret defaults to the text colour: an input that
                // only sets `text_color` should not sprout a blue cursor.
                input.cursor_color.get_or(text_color),
                style.stroke(),
                style.shadow(),
                placeholder,
            )
        };

        // The text scrolls horizontally under a fixed viewport, so whatever
        // slid past either edge has to be cut at the widget's bounds.
        //
        // Horizontally only: the vertical axis never scrolls, and closing it
        // would trim descenders and any glyph stroke or shadow, which the
        // viewport is not meant to touch. One em of slack each way is past
        // anything a single line can reach.
        let slack = self.cached_font_size;
        ctx.set_clip_rect(Rect::new(
            0.0,
            -slack,
            bounds.width,
            bounds.height + slack * 2.0,
        ));

        // Draw selection highlight if focused and has selection (LOCAL coords)
        if is_focused && self.selection.has_selection() {
            let (start, end) = self.selection.range();
            let start_x = self.cached_width_at_char(start) - self.scroll_offset;
            let end_x = self.cached_width_at_char(end) - self.scroll_offset;

            let selection_rect = Rect::new(start_x, 0.0, end_x - start_x, bounds.height);
            ctx.draw_rounded_rect(selection_rect, selection_color, 0.0);
        }

        // Draw text with scroll offset (LOCAL coords)
        let text_bounds = Rect::new(
            -self.scroll_offset,
            0.0,
            self.cached_text_width.max(bounds.width),
            bounds.height,
        );
        // The placeholder stands in for the text, never beside it: it is only
        // resolved when the value is empty, so there is nothing to overlap.
        let (drawn, drawn_color) = match &placeholder {
            Some((text, color)) => (text.as_str(), *color),
            None => (display, text_color),
        };
        ctx.draw_text_decorated(
            drawn,
            text_bounds,
            drawn_color,
            self.cached_font_size,
            self.cached_font_family,
            self.cached_font_weight,
            crate::widgets::TextAlign::Start,
            stroke,
            shadow,
            None,
        );

        // The caret, and the wake that keeps it blinking.
        //
        // `self.caret` gates the *drawing*, not only the blink: stopping the blink
        // leaves `cursor_visible` at whatever it last was — true, from the
        // constructor — and a field asked for no caret got a permanent one.
        if self.cached_caret && is_focused {
            // The toggle happens in `advance_animations`, and this is what asks
            // for the wake that runs it. It has to be here, not at the moment
            // focus arrives: focus comes from a click, from `autofocus`, or from
            // `WidgetRef::focus()`, and all three only queue a repaint. Asking
            // from the repaint covers every one of them, and covers the half of
            // the cycle where the caret is hidden and there is nothing to draw.
            request_job_at(
                id,
                JobRequest::Animation(RequiredJob::Paint),
                // A deadline the loop compares against the wall clock, so it
                // leaves the frame's sequence here.
                (self.last_cursor_toggle + Duration::from_millis(CURSOR_BLINK_MS)).into_inner(),
            );

            if self.cursor_visible {
                let cursor_x =
                    self.cached_width_at_char(self.selection.cursor) - self.scroll_offset;
                let cursor_rect = Rect::new(
                    cursor_x,
                    0.0,
                    1.5, // cursor width
                    bounds.height,
                );
                ctx.draw_rounded_rect(cursor_rect, cursor_color, 0.0);
            }
        }
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        // When this event happened, which every edit below is timed against —
        // the undo-coalescing window and the caret restarting on input.
        let edit = Edit {
            width: tree.get_bounds(id).unwrap_or_default().width,
            at: tree.event_instant(),
        };
        // Get bounds from Tree for hit testing
        let bounds = tree.get_bounds(id).unwrap_or_default();

        let pointed_at = bounds.contains_at(event.coords());
        if pointed_at {
            tree.point_shows_cursor(self.cursor);
        }

        match event {
            Event::MouseDown {
                at: Some(at),
                button,
                ..
            } if bounds.contains(at.x, at.y) && *button == MouseButton::Left => {
                // Focus, then repaint to show the caret where the click landed.
                // The blink schedules its own next wake from `paint`.
                request_focus(tree, id);
                request_job(id, JobRequest::Paint);

                // Set cursor position
                let char_index = self.char_index_at_x(at.x, bounds);
                self.selection = Selection::new(char_index);
                self.is_dragging = true;
                self.reset_cursor_blink(edit.at);
                self.ensure_cursor_visible(bounds.width);

                return EventResponse::Handled;
            }
            // Matched whether or not it has a position, because the two
            // halves want different answers: a move with no position is over
            // nothing, so the hover below has to fall — but it says nothing
            // about where a selection has got to, so the drag keeps what it
            // had rather than being dragged to the start of the line.
            Event::MouseMove { at, .. } => {
                if pointed_at && !self.is_hovered {
                    self.is_hovered = true;
                    self.hover.set(true);
                } else if !pointed_at {
                    self.release_pointer();
                }

                if let Some(at) = at
                    && self.is_dragging
                {
                    // Extend selection while dragging
                    let char_index = self.char_index_at_x(at.x, bounds);
                    self.selection.cursor = char_index;
                    self.ensure_cursor_visible(bounds.width);
                    request_job(id, JobRequest::Paint);
                    return EventResponse::Handled;
                }
            }
            Event::MouseUp { button, .. } if *button == MouseButton::Left && self.is_dragging => {
                self.is_dragging = false;
                // Select-to-copy: a completed mouse selection becomes the
                // primary selection (middle-click paste elsewhere)
                if let Some(text) = self.exportable_selection() {
                    primary_copy(&text);
                }
                return EventResponse::Handled;
            }
            Event::MouseDown {
                at: Some(at),
                button,
                ..
            } if bounds.contains(at.x, at.y) && *button == MouseButton::Middle => {
                // Middle-click paste from the primary selection
                request_focus(tree, id);
                let char_index = self.char_index_at_x(at.x, bounds);
                self.selection = Selection::new(char_index);
                if let Some(mut text) = primary_paste() {
                    self.insert_pasted(&mut text, edit);
                }
                self.reset_cursor_blink(edit.at);
                request_job(id, JobRequest::Paint);
                return EventResponse::Handled;
            }
            // A press inside a field that holds the keyboard is the field's,
            // whatever the button pressed. The dispatcher takes the focus off a
            // press nobody claimed, and the arms above claim only left and
            // middle — so a right press on the caret used to blur the very
            // field it landed on.
            //
            // Said on the tree rather than by returning `Handled`, which also
            // means consumed: a row wrapping the field declaring `on_right_click`
            // never saw the press, and whether it fired depended on where the
            // keyboard was. The same conflation as #308, one widget over.
            Event::MouseDown { at: Some(at), .. }
                if has_focus(id) && bounds.contains(at.x, at.y) =>
            {
                tree.keep_the_focus_this_press_landed_on();
            }
            Event::KeyDown { key, modifiers } if has_focus(id) => {
                let response = self.handle_key(key, modifiers.ctrl, modifiers.shift, edit);
                if response == EventResponse::Handled {
                    request_job(id, JobRequest::Paint);
                }
                return response;
            }
            Event::FocusOut if has_focus(id) => {
                release_focus(id);
                self.cursor_visible = false;
                self.is_dragging = false;
                request_job(id, JobRequest::Paint);
            }
            Event::MouseLeave => {
                // A selection drag survives the pointer leaving the *field* —
                // that is how you select past its edge, and `release_pointer`
                // above is called for it. It must not survive the pointer
                // leaving the *surface*, because nothing out there will ever
                // deliver the release that ends it: the drag would then extend
                // on the next move that arrived, with no button held. A touch
                // gesture the compositor cancels is how that now happens.
                self.is_dragging = false;
                self.release_pointer();
            }
            _ => {}
        }

        EventResponse::Ignored
    }
}

/// Create a text input widget with two-way signal binding.
///
/// Changes made in the text input will be written back to the signal.
/// ```no_run
/// # use guido::prelude::*;
/// let username = create_signal(String::new());
/// text_input(username);
/// ```
pub fn text_input(signal: RwSignal<String>) -> TextInput {
    TextInput::new(signal)
}

/// Create a password field over `password`.
///
/// The field reads and edits the [`Secret`] the [`Password`] holds, in place,
/// and draws one [`mask_char`](TextInput::mask_char) per character. Nothing
/// about the text is copied: there is no undo history, nothing can be copied
/// or cut out of it, and Enter moves the secret out to
/// [`on_submit`](TextInput::<Masked>::on_submit), leaving the field empty.
///
/// ```no_run
/// # use guido::prelude::*;
/// let password = create_password();
/// let busy = create_signal(false);
///
/// password_input(password)
///     .placeholder("Password")
///     .readonly(busy)
///     .on_submit(move |secret: Secret| {
///         busy.set(true);
///         // hand `secret` to PAM; dropping it wipes it
///         # let _ = secret;
///     });
///
/// // Wiped in place, for instance after an idle timeout.
/// password.clear();
/// ```
pub fn password_input(password: Password) -> PasswordInput {
    TextInput::new_password(password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobType;
    use crate::jobs::{clear_pending_jobs, clear_scheduled_jobs, next_deadline, queued_job_types};
    use crate::layout::Constraints;
    use crate::reactive::create_signal;
    use crate::widgets::PointerKind;

    /// A move with no position says nothing about where a drag has got to.
    ///
    /// A pointer event that has descended into a subtree collapsed to nothing
    /// arrives without a position (#227), and the arm that extends a selection
    /// has to leave it alone rather than read the absence as an origin — which
    /// is what an earlier attempt's far-away sentinel did, putting the cursor
    /// at index 0 for as long as the box was closed.
    ///
    /// A guard rather than a fix: the shape that ships already holds this,
    /// because the drag branch asks for a position before it moves anything.
    /// What it guards against is somebody restoring the unwrap — the `None`
    /// arriving here is indistinguishable from a click at the origin the
    /// moment it is given a default.
    #[test]
    fn a_move_with_no_position_leaves_the_selection_where_it_was() {
        let mut tree = crate::tree::Tree::new();
        // An id with no bounds: the drag branch does not consult them — that
        // is what lets a selection continue past the edge of the field — so
        // the empty rect is exactly the right stand-in here.
        let id = tree.register(Box::new(crate::widgets::container()));

        let text = create_signal(String::from("hello world"));
        let mut input = text_input(text);
        input.selection = Selection {
            anchor: 2,
            cursor: 7,
        };
        input.is_dragging = true;

        let moved_to = Event::mouse_move(60.0, 5.0);
        Widget::event(&mut input, &mut tree, id, &moved_to);
        let dragged = input.selection.cursor;
        assert_ne!(
            dragged, 7,
            "a positioned move has to extend the selection, or this test is \
             asserting against a drag that was never running"
        );

        Widget::event(
            &mut input,
            &mut tree,
            id,
            &Event::MouseMove {
                at: None,
                pointer: PointerKind::Mouse,
            },
        );
        assert_eq!(
            (input.selection.anchor, input.selection.cursor),
            (2, dragged),
            "and one with no position leaves it exactly where it was"
        );
    }

    /// Two keystrokes inside the window are one undo, and two outside it are
    /// two — which is what `HISTORY_COALESCE_MS` promises and nothing could
    /// ask before.
    ///
    /// The window is half a second. A test that wanted to stand on either side
    /// of it had to sleep for real, so the far side cost five seconds of wall
    /// clock and the near side was a race against how long the assertions took.
    /// Both moments are named now.
    #[test]
    fn the_coalescing_window_has_two_sides_and_they_can_both_be_asked_about() {
        let entry = |text: &str| HistoryEntry {
            text: text.to_string(),
            cursor: text.chars().count(),
            anchor: text.chars().count(),
        };

        let inside = {
            let t0: EventInstant = Instant::now().into();
            let mut history = History::new();
            history.push(entry("a"), EditType::Insert, t0);
            history.push(
                entry("ab"),
                EditType::Insert,
                t0 + Duration::from_millis(50),
            );
            history.undo_stack.len()
        };
        assert_eq!(
            inside, 1,
            "two keystrokes fifty milliseconds apart are one thing to undo"
        );

        let outside = {
            let t0: EventInstant = Instant::now().into();
            let mut history = History::new();
            history.push(entry("a"), EditType::Insert, t0);
            history.push(entry("ab"), EditType::Insert, t0 + Duration::from_secs(5));
            history.undo_stack.len()
        };
        assert_eq!(
            outside, 2,
            "and five seconds apart they are two, because the sentence in \
             between was a different thought"
        );
    }

    /// A field inside a container, laid out from the container, focused.
    ///
    /// The container is what makes these tests about *reactivity* rather than
    /// about plumbing. A `TextInput` re-runs its own layout on every call, so a
    /// value read without tracking still reaches a second direct `layout(..)`
    /// and the test passes while nothing subscribes. A container takes its
    /// unchanged-constraints early-out and never asks its children again, so
    /// the second pass reaches the field only if the write marked it.
    fn field_in_container(input: impl Widget + 'static) -> (Tree, WidgetId, WidgetId) {
        clear_pending_jobs();
        clear_scheduled_jobs();
        crate::reactive::focus::clear_focus();

        let mut tree = Tree::new();
        let root = tree.register(Box::new(crate::widgets::container().child(input)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let id = tree.get_children(root)[0];
        relayout(&mut tree, root);
        request_focus(&tree, id);
        clear_pending_jobs();
        (tree, root, id)
    }

    /// Re-lay out from `root` after draining the jobs a signal write queued.
    ///
    /// The drain is what turns a write into `needs_layout` on the widget that
    /// read it; without it the second pass measures the cache.
    fn relayout(tree: &mut Tree, root: WidgetId) {
        crate::jobs::pump_and_layout(tree, root, Constraints::new(0.0, 0.0, 200.0, 40.0));
    }

    /// Put the caret at the end, which is where its x becomes a measurement of
    /// the displayed text rather than a constant zero.
    fn caret_to_end(tree: &mut Tree, id: WidgetId) {
        tree.with_widget_mut(id, |w, wid, t| {
            w.event(
                t,
                wid,
                &Event::KeyDown {
                    key: Key::End,
                    modifiers: crate::widgets::widget::Modifiers::default(),
                },
            )
        });
    }

    /// The colour and size the field drew its glyphs at, on a frame at a named
    /// instant.
    ///
    /// Named because an eased value is a function of how long it has been
    /// running, and a test that let the clock decide would assert on whatever
    /// the machine managed between two calls.
    fn drawn_text(tree: &mut Tree, id: WidgetId, at: std::time::Instant) -> Option<(Color, f32)> {
        fn find(node: &crate::renderer::RenderNode) -> Option<(Color, f32)> {
            for cmd in &node.commands {
                if let crate::renderer::DrawCommand::Text {
                    color, font_size, ..
                } = &**cmd
                {
                    return Some((*color, *font_size));
                }
            }
            node.children.iter().find_map(|child| find(child))
        }
        tree.set_frame_instant(Some(at));
        relayout(tree, id);
        let node = paint_once(tree, id);
        tree.set_frame_instant(None);
        find(&node)
    }

    /// Every rectangle the field draws. Focused with no selection, the caret is
    /// the only one there can be, and its x is where the displayed text ends.
    fn drawn_rects(tree: &mut Tree, id: WidgetId) -> Vec<crate::widgets::Rect> {
        fn collect(node: &crate::renderer::RenderNode, out: &mut Vec<crate::widgets::Rect>) {
            for cmd in &node.commands {
                if let crate::renderer::DrawCommand::RoundedRect { rect, .. } = &**cmd {
                    out.push(*rect);
                }
            }
            for child in &node.children {
                collect(child, out);
            }
        }
        let mut out = Vec::new();
        collect(&paint_once(tree, id), &mut out);
        out
    }

    /// A password is drawn as its mask, never as its text.
    ///
    /// Read off the caret's x, which sits at the end of the *displayed* text:
    /// bullets and `iiii` are different widths in any font, so the assertion
    /// does not depend on which one is installed — only that they differ.
    #[test]
    fn a_password_is_drawn_as_its_mask() {
        let password = crate::reactive::create_password();
        password.set(Secret::from("iiiiiiii".to_owned()));
        let (mut tree, _, id) = field_in_container(password_input(password));
        caret_to_end(&mut tree, id);
        let masked = drawn_rects(&mut tree, id);
        assert_eq!(masked.len(), 1, "the caret, and nothing else");

        let (mut tree, _, id) =
            field_in_container(text_input(create_signal("iiiiiiii".to_owned())));
        caret_to_end(&mut tree, id);
        let plain = drawn_rects(&mut tree, id);

        assert_ne!(
            masked[0].x, plain[0].x,
            "the password field drew its text rather than its mask"
        );
    }

    /// Every text a subtree drew.
    fn drawn_strings(tree: &mut Tree, id: WidgetId) -> Vec<String> {
        fn collect(node: &crate::renderer::RenderNode, out: &mut Vec<String>) {
            for cmd in &node.commands {
                if let crate::renderer::DrawCommand::Text { text, .. } = &**cmd {
                    out.push(text.clone());
                }
            }
            for child in &node.children {
                collect(child, out);
            }
        }
        let mut out = Vec::new();
        collect(&paint_once(tree, id), &mut out);
        out
    }

    /// The application fills and wipes the field through the handle, outside
    /// any event, and the field follows on its next layout — which the write
    /// asks for, because the layout read the password.
    #[test]
    fn a_password_written_from_outside_reaches_the_field() {
        let password = crate::reactive::create_password();
        let (mut tree, root, id) = field_in_container(password_input(password).mask_char('*'));

        password.set(Secret::from("abc".to_owned()));
        relayout(&mut tree, root);
        assert_eq!(drawn_strings(&mut tree, id), ["***"]);

        password.clear();
        relayout(&mut tree, root);
        assert!(
            drawn_strings(&mut tree, id).is_empty(),
            "the mask outlived the text"
        );
    }

    /// Enter takes the text with it, and the field says so.
    #[test]
    fn a_submit_empties_what_the_field_draws() {
        let password = crate::reactive::create_password();
        password.set(Secret::from("abc".to_owned()));
        let (mut tree, root, id) = field_in_container(
            password_input(password)
                .mask_char('*')
                .on_submit(|_: Secret| {}),
        );
        assert_eq!(drawn_strings(&mut tree, id), ["***"]);

        tree.with_widget_mut(id, |w, wid, t| {
            w.event(
                t,
                wid,
                &Event::KeyDown {
                    key: Key::Enter,
                    modifiers: Default::default(),
                },
            )
        });
        relayout(&mut tree, root);
        assert!(
            drawn_strings(&mut tree, id).is_empty(),
            "the mask outlived the text"
        );
    }

    /// A paste is copied into the field, and the `String` it arrived in is
    /// zeroed — every byte of its buffer, not only its length.
    #[test]
    fn a_paste_wipes_the_string_it_arrived_in() {
        let password = crate::reactive::create_password();
        let mut field = password_input(password);
        let mut pasted = String::from("hunter2");
        let (ptr, capacity) = (pasted.as_ptr(), pasted.capacity());

        field.insert_pasted(
            &mut pasted,
            Edit {
                width: 200.0,
                at: std::time::Instant::now().into(),
            },
        );

        assert_eq!(
            password.with_untracked(|s| s.expose().to_owned()),
            "hunter2"
        );
        assert_eq!(pasted.as_ptr(), ptr, "the buffer checked is the one pasted");
        // SAFETY: the same allocation, still owned by `pasted`, and every one
        // of its `capacity` bytes was written by the wipe.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, capacity) };
        assert!(
            bytes.iter().all(|&b| b == 0),
            "the paste was left in memory"
        );
    }

    /// What the mask is made of is a value the field can be told.
    #[test]
    fn the_mask_character_answers_to_a_signal() {
        let mask = create_signal('.');
        let password = crate::reactive::create_password();
        password.set(Secret::from("aaaaaaaa".to_owned()));
        let (mut tree, root, id) = field_in_container(password_input(password).mask_char(mask));

        caret_to_end(&mut tree, id);
        let narrow = drawn_rects(&mut tree, id);

        mask.set('W');
        relayout(&mut tree, root);
        let wide = drawn_rects(&mut tree, id);

        assert!(
            wide[0].x > narrow[0].x,
            "eight W's have to be wider than eight full stops: {narrow:?} then \
             {wide:?}"
        );
    }

    /// Whether a caret is drawn at all, likewise.
    #[test]
    fn the_caret_answers_to_a_signal() {
        let show = create_signal(true);
        let (mut tree, root, id) =
            field_in_container(text_input(create_signal("hi".to_owned())).caret(show));

        assert_eq!(drawn_rects(&mut tree, id).len(), 1, "a caret to begin with");

        show.set(false);
        relayout(&mut tree, root);

        assert!(
            drawn_rects(&mut tree, id).is_empty(),
            "asked for no caret and got one anyway"
        );
        assert!(has_focus(id), "and the focus is still here");
    }

    /// A field's declared colour eases exactly as a text's does.
    ///
    /// The two widgets take their style setters from one list
    /// (`declares_text_style`), so this is parity being *observed* rather than
    /// assumed: the macro guarantees the method exists, and this guarantees the
    /// storage behind it is wired — an input that accepted a transition and
    /// ignored it would be the silently-dropped value the whole rule exists to
    /// prevent.
    #[test]
    fn a_declared_transition_eases_an_input_colour() {
        use crate::animation::Animate;

        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let (mut tree, root, _id) = field_in_container(
            text_input(create_signal("abc".to_owned()))
                .color((move || hue.get()).transition(400.0)),
        );

        let t0 = std::time::Instant::now();
        drawn_text(&mut tree, root, t0);

        hue.set(Color::rgb(1.0, 1.0, 1.0));
        drawn_text(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let (midway, _) = drawn_text(&mut tree, root, t0 + std::time::Duration::from_millis(100))
            .expect("the field draws its text");

        assert!(
            midway.r > 0.01 && midway.r < 0.99,
            "an input's declared colour has to ease like a text's, got {midway:?}"
        );
    }

    /// A field's own font size is resolved by its own `refresh`, so the door
    /// `Text` passes through has to be on this path too.
    ///
    /// One more way to see the same poisoning: a bad size reaches the glyph
    /// positions the caret is placed from as well as the measurement, so a field
    /// that has seen one keeps the size it had and stops following its signal.
    #[test]
    fn a_fields_font_size_follows_its_signal_again_once_the_signal_recovers() {
        use crate::animation::Animate;

        let ms = std::time::Duration::from_millis;
        let size = create_signal(10.0f32);
        let (mut tree, root, _id) = field_in_container(
            text_input(create_signal("abc".to_owned()))
                .font_size((move || size.get()).transition(200.0)),
        );

        let t0 = std::time::Instant::now();
        drawn_text(&mut tree, root, t0);

        size.set(f32::NAN);
        drawn_text(&mut tree, root, t0 + ms(1));
        drawn_text(&mut tree, root, t0 + ms(50));

        size.set(30.0);
        drawn_text(&mut tree, root, t0 + ms(100));

        let painted = drawn_text(&mut tree, root, t0 + ms(1000)).map(|(_, size)| size);
        assert!(
            painted.is_some_and(|size| (size - 30.0).abs() < 0.5),
            "a field given one bad size never followed its signal again: the \
             glyphs are drawn at {painted:?} rather than 30"
        );
    }

    /// A field resolves its colour on two complementary paths, and both are the
    /// door's.
    ///
    /// `refresh` reads it only where a motion was declared, and paint only where
    /// one was not, so no single scenario reaches both — this is the second.
    /// Nothing downstream guards: the channels go to the shader as they are.
    #[test]
    fn a_fields_colour_with_no_motion_is_painted_through_the_door() {
        use crate::animation::Animatable;

        let hue = create_signal(Color::rgba(f32::NAN, 0.0, 0.0, 1.0));
        let (mut tree, root, _id) =
            field_in_container(text_input(create_signal("abc".to_owned())).color(hue));

        let (painted, _) = drawn_text(&mut tree, root, std::time::Instant::now())
            .expect("the field draws its text");
        assert!(
            painted.channels().iter().all(|c| c.is_finite()),
            "a colour nobody can compute reached the shader: {painted:?}"
        );
    }

    /// And the path `refresh` owns: a colour with a motion, poisoned and then
    /// finite again.
    #[test]
    fn a_fields_colour_follows_its_signal_again_once_the_signal_recovers() {
        use crate::animation::Animate;

        let ms = std::time::Duration::from_millis;
        let hue = create_signal(Color::rgb(0.0, 0.0, 0.0));
        let (mut tree, root, _id) = field_in_container(
            text_input(create_signal("abc".to_owned()))
                .color((move || hue.get()).transition(200.0)),
        );

        let t0 = std::time::Instant::now();
        drawn_text(&mut tree, root, t0);

        hue.set(Color::rgba(f32::NAN, 0.0, 0.0, 1.0));
        drawn_text(&mut tree, root, t0 + ms(1));
        drawn_text(&mut tree, root, t0 + ms(50));

        // Red, not white: white is what a colour nobody declared falls back to.
        hue.set(Color::rgb(1.0, 0.0, 0.0));
        drawn_text(&mut tree, root, t0 + ms(100));

        let painted = drawn_text(&mut tree, root, t0 + ms(1000)).map(|(color, _)| color);
        assert!(
            painted.is_some_and(|color| color.r > 0.99 && color.g < 0.01),
            "a field given one bad colour never followed its signal again: \
             got {painted:?}"
        );
    }

    /// The field walks its own chain, so the fall-through is its own to get
    /// right — see `an_override_that_is_not_a_number_falls_through_to_the_declaration`
    /// for the rule and why a fold could not state it.
    #[test]
    fn an_override_that_is_not_a_number_falls_through_to_the_fields_declaration() {
        let hot = create_signal(true);
        let (mut tree, root, _id) = field_in_container(
            text_input(create_signal("abc".to_owned()))
                .font_size(32.0)
                .color(Color::RED)
                .state(hot, |s: TextStyle| {
                    s.font_size(f32::NAN)
                        .color(Color::rgba(f32::NAN, 0.0, 0.0, 1.0))
                }),
        );

        let (color, size) = drawn_text(&mut tree, root, std::time::Instant::now())
            .expect("the field draws its text");
        assert!(
            (size - 32.0).abs() < 0.01,
            "a bad override falls through to the size the field declares, not to \
             the default: got {size}"
        );
        assert_eq!(color, Color::RED, "and to the colour it declares");
    }

    /// A field's font size appears after a measure too, under a box of exact
    /// size that hands it the same constraints in both passes.
    ///
    /// The field is never skipped itself; the box around it is, and nothing
    /// that box declares moves. What keeps it laid out for real is the size the
    /// measure placed in the field without letting it appear.
    /// Read off the glyphs rather than the box: the field's height only grows.
    #[test]
    fn a_font_size_under_an_exact_box_appears_after_a_measure() {
        use crate::prelude::*;

        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            container().width(200.0).height(100.0).child(
                container().width(200.0).height(60.0).child(
                    text_input(create_signal(String::from("entering"))).font_size(
                        40.0f32
                            .transition(Transition::new(100.0, TimingFunction::Linear))
                            .entering_from(10.0),
                    ),
                ),
            ),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let field = tree.get_children(tree.get_children(root)[0])[0];

        let t0 = std::time::Instant::now();
        // The loop's order: jobs, the layout roots they queued — the inner box
        // is one — at the constraints they last had, then the surface root.
        let frame = |tree: &mut Tree, ms: u64, constraints: Constraints| {
            tree.set_frame_instant(Some(t0 + std::time::Duration::from_millis(ms)));
            let roots = [root].into_iter().collect();
            crate::jobs::distribute_jobs(tree, &roots);
            let drained = crate::jobs::drain_surface_jobs(root);
            let mut layout_roots = Vec::new();
            crate::jobs::process_jobs(&drained, tree, &mut layout_roots);
            crate::jobs::recycle_job_buffer(drained);
            for boundary in layout_roots {
                let c = tree
                    .last_layout_constraints(boundary)
                    .unwrap_or(constraints);
                tree.layout_widget(boundary, c);
            }
            tree.layout_widget(root, constraints);
            let node = paint_once(tree, field);
            tree.set_frame_instant(None);
            node.commands.iter().find_map(|cmd| match &**cmd {
                crate::renderer::DrawCommand::Text { font_size, .. } => Some(*font_size),
                _ => None,
            })
        };
        // The popup path first: a measure of the natural size, before the
        // layouts that place it.
        let measure = |tree: &mut Tree, ms: u64, constraints: Constraints| {
            tree.set_frame_instant(Some(t0 + std::time::Duration::from_millis(ms)));
            let _ = crate::jobs::pump_and_measure(tree, root, constraints);
            tree.set_frame_instant(None);
        };
        measure(&mut tree, 0, Constraints::new(200.0, 0.0, 200.0, 300.0));
        let loose = Constraints::new(0.0, 0.0, 200.0, 100.0);
        frame(&mut tree, 0, loose);
        let midway = frame(&mut tree, 50, loose).expect("the field draws its text");
        assert!(
            midway > 10.0 && midway < 40.0,
            "the size has to grow rather than be grown, got {midway}"
        );
    }

    /// A laid-out input, focused unless told otherwise.
    fn field(input: TextInput, focused: bool) -> (Tree, WidgetId) {
        clear_pending_jobs();
        clear_scheduled_jobs();
        crate::reactive::focus::clear_focus();

        let mut tree = Tree::new();
        let id = tree.register(Box::new(input));
        tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
        tree.layout_widget(id, Constraints::new(0.0, 0.0, 200.0, 40.0));
        if focused {
            request_focus(&tree, id);
        }
        clear_pending_jobs();
        (tree, id)
    }

    fn advance(tree: &mut Tree, id: WidgetId) -> bool {
        tree.with_widget_mut(id, |w, id, t| w.advance_animations(t, id))
            .unwrap_or(false)
    }

    fn paint_once(tree: &mut Tree, id: WidgetId) -> crate::renderer::RenderNode {
        let mut node = crate::renderer::RenderNode::new(id.as_u64());
        tree.paint_widget(id, &mut node);
        node
    }

    #[test]
    fn painting_a_focused_caret_asks_for_the_wake_that_toggles_it() {
        // The regression: the toggle lives in `advance_animations`, which the loop
        // only calls for an Animation job, and *nothing* asked for one — focus from
        // a click, from `autofocus` or from `WidgetRef::focus()` all queue a plain
        // repaint. So the caret never blinked at all, while the tests that called
        // `advance_animations` by hand were happy: they skipped the broken part.
        let (mut tree, id) = field(text_input(create_signal(String::new())), true);
        assert_eq!(next_deadline(), None, "nothing has been drawn yet");

        paint_once(&mut tree, id);

        assert!(
            next_deadline().is_some(),
            "a caret on screen has to ask to be woken, or it stays as it is forever"
        );
    }

    #[test]
    fn the_blink_keeps_asking_after_each_toggle() {
        // One wake is a blink that stops after half a cycle.
        let (mut tree, id) = field(text_input(create_signal(String::new())), true);
        paint_once(&mut tree, id);

        advance(&mut tree, id);
        clear_pending_jobs();
        clear_scheduled_jobs();
        paint_once(&mut tree, id);

        assert!(next_deadline().is_some());
    }

    #[test]
    fn painting_a_field_without_a_caret_asks_for_nothing() {
        let (mut tree, id) = field(text_input(create_signal(String::new())).no_caret(), true);

        paint_once(&mut tree, id);

        assert_eq!(
            next_deadline(),
            None,
            "no caret, no wake — this is what a lock screen costs while nobody \
             touches it"
        );
    }

    #[test]
    fn painting_an_unfocused_field_asks_for_nothing() {
        let (mut tree, id) = field(text_input(create_signal(String::new())), false);

        paint_once(&mut tree, id);

        assert_eq!(next_deadline(), None);
    }

    #[test]
    fn advancing_a_blinking_caret_never_asks_for_a_frame() {
        // The division of labour: `advance_animations` applies the toggle, `paint`
        // asks for the next wake. What neither may do is request an animation job,
        // which means "advance me every frame" and is what pinned the loop at
        // 60 fps to redraw the same pixels 113 frames out of 114.
        let (mut tree, id) = field(text_input(create_signal(String::new())), true);

        let blinking = advance(&mut tree, id);

        assert!(blinking, "a focused caret is a blinking one");
        assert!(
            !queued_job_types(id).contains(&JobType::Animation),
            "queued: {:?}",
            queued_job_types(id)
        );
    }

    #[test]
    fn a_field_without_a_caret_asks_for_nothing() {
        let (mut tree, id) = field(text_input(create_signal(String::new())).no_caret(), true);

        let blinking = advance(&mut tree, id);

        assert!(!blinking);
        assert_eq!(
            next_deadline(),
            None,
            "no caret, no blink, nothing to wake the loop for — this is what a \
             lock screen costs while nobody touches it"
        );
    }

    #[test]
    fn an_unfocused_field_asks_for_nothing() {
        let (mut tree, id) = field(text_input(create_signal(String::new())), false);

        let blinking = advance(&mut tree, id);

        assert!(!blinking);
        assert_eq!(next_deadline(), None);
    }
}
