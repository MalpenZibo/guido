//! TextInput widget for single-line text editing.
//!
//! The TextInput widget handles:
//! - Text display and editing
//! - Cursor blinking and positioning
//! - Text selection with mouse and keyboard
//! - Password masking mode
//!
//! Styling (background, borders, etc.) should be handled by wrapping in a Container.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::default_font_family;
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job, request_job_at};
use crate::layout::{Constraints, Size};
use crate::reactive::focus::focused_widget;
use crate::reactive::{
    CursorIcon, IntoSignal, OptionSignalExt, RwSignal, Signal, clipboard_copy, clipboard_paste,
    has_focus, primary_copy, primary_paste, release_focus, request_focus, set_cursor,
    with_signal_tracking,
};
use crate::renderer::{PaintContext, char_index_from_x_styled};
use crate::tree::{Tree, WidgetId};
use crate::widget_ref::{WidgetRef, register_widget_ref};

use super::control::Control;
use super::font::{FontFamily, FontWeight};
use super::input_style::{InputStyle, InputStyled};
use super::state_layer::{StateWhen, Stateful};
use super::text_style::{TextStyle, TextStyled};
use super::widget::{Color, Event, EventResponse, Key, MouseButton, Rect, Widget};

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
    at: Instant,
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
    /// Stack of past states (most recent edit.at end)
    undo_stack: VecDeque<HistoryEntry>,
    /// Stack of undone states for redo
    redo_stack: VecDeque<HistoryEntry>,
    /// Time of last edit (for coalescing)
    last_edit_time: Instant,
    /// Type of last edit (for coalescing)
    last_edit_type: Option<EditType>,
}

impl History {
    fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            last_edit_time: Instant::now(),
            last_edit_type: None,
        }
    }

    /// Push a new state to history (clears redo stack)
    /// Uses coalescing to merge similar edits within a time window
    fn push(&mut self, entry: HistoryEntry, edit_type: EditType, at: Instant) {
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
    /// Create a new selection with cursor edit.at given position (no selection)
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

pub struct TextInput {
    // Content (actual value, never masked)
    /// Signal for two-way binding
    value: RwSignal<String>,
    cached_value: String,
    cached_char_count: usize,
    cached_display_text: String,
    display_text_dirty: bool,

    // Measurement cache (avoid repeated text shaping in paint)
    /// Total width of display text
    cached_text_width: f32,
    /// Cumulative width edit.at each character index (length = char_count + 1)
    /// cached_glyph_positions[i] = width of text[0..i]
    cached_glyph_positions: Vec<f32>,
    /// Whether measurements need to be recalculated
    measurements_dirty: bool,

    // Metrics resolved from the enclosing container's style. Cached because a
    // change to any of them invalidates the glyph measurements below.
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,

    // Password mode
    is_password: bool,
    mask_char: char,

    /// Whether a caret is drawn edit.at all. Off costs nothing: no caret, no blink,
    /// nothing to wake the loop for.
    caret: bool,

    /// Handle for application code to reach this input — to focus it, mostly.
    widget_ref: Option<WidgetRef>,
    /// Shown while the value is empty. Reactive: a prompt that changes — PAM
    /// asking a different question — changes what the empty field says.
    placeholder: Option<Signal<String>>,

    /// An unmade offer of the initial focus. Cleared once made, so autofocus is
    /// a *first layout* behaviour rather than something that fights the user on
    /// every relayout.
    autofocus_pending: bool,

    // Selection state
    selection: Selection,

    // Cursor blinking
    cursor_visible: bool,
    last_cursor_toggle: Instant,

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

    // Callbacks
    on_change: Option<TextCallback>,
    on_submit: Option<TextCallback>,
    /// What this input declares about its own text, and about the furniture
    /// only it draws. Boxed and absent by default.
    text_style: Option<Box<TextStyle>>,
    input_style: Option<Box<InputStyle>>,
    /// Overrides that apply while this field's control is in a state, in
    /// declaration order.
    states: Vec<(StateWhen, TextStyle)>,
}

impl TextInput {
    /// Create a TextInput with a Signal for two-way binding.
    /// Changes made in the TextInput will be written back to the signal.
    pub fn new(signal: RwSignal<String>) -> Self {
        // Use get_untracked() to avoid registering layout dependencies during widget creation.
        // Layout dependencies should only be registered during the widget's own layout phase.
        let cached_value = signal.get_untracked();
        let cached_char_count = cached_value.chars().count();
        let default_family = default_font_family();
        Self {
            value: signal,
            cached_value,
            cached_char_count,
            cached_display_text: String::new(),
            display_text_dirty: true,
            cached_text_width: 0.0,
            cached_glyph_positions: Vec::new(),
            measurements_dirty: true,
            cached_font_size: 14.0,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
            is_password: false,
            mask_char: '•',
            caret: true,
            widget_ref: None,
            placeholder: None,
            autofocus_pending: false,
            selection: Selection::new(0),
            cursor_visible: true,
            last_cursor_toggle: Instant::now(),
            is_dragging: false,
            is_hovered: false,
            hover: crate::reactive::signal::create_signal(false),
            history: History::new(),
            scroll_offset: 0.0,
            on_change: None,
            on_submit: None,
            text_style: None,
            input_style: None,
            states: Vec::new(),
        }
    }

    /// This input's own declarations, completed by the ancestors' for whatever
    /// they leave out. Each walk is skipped when nothing is left to find.
    fn resolved_text_style(&self, tree: &Tree, id: WidgetId) -> TextStyle {
        let mut style = TextStyle::default();
        // Active overrides first and last declared first, so they outrank this
        // field's own declaration; `inherit_from` takes only what is missing,
        // which resolves the chain per property.
        if !self.states.is_empty() {
            let control = tree.nearest_control(id);
            for (when, override_) in self.states.iter().rev() {
                if self.is_state_active(id, control.as_ref(), when) {
                    style.inherit_from(override_);
                }
            }
        }
        if let Some(own) = self.text_style.as_deref() {
            style.inherit_from(own);
        }
        style
    }

    /// Whether an override applies. Reading the answer subscribes the field to
    /// its control, so it is asked only for a state it declares.
    fn is_state_active(&self, id: WidgetId, control: Option<&Control>, when: &StateWhen) -> bool {
        match (when, control) {
            (StateWhen::When(condition), _) => condition.get(),
            (StateWhen::Hovered, Some(control)) => control.is_hovered(),
            (StateWhen::Pressed, Some(control)) => control.is_pressed(),
            (StateWhen::Focused, Some(control)) => control.has_focus(),
            // No control above: the field is its own unit. It already tracks
            // the pointer for its cursor, and it is the thing that holds the
            // focus, so both answers are its own.
            (StateWhen::Hovered, None) => self.hover.get(),
            (StateWhen::Focused, None) => crate::reactive::focus::focus_path().contains(id),
            (StateWhen::Pressed, None) => false,
        }
    }

    fn resolved_input_style(&self) -> InputStyle {
        self.input_style.as_deref().copied().unwrap_or_default()
    }

    /// Enable password mode (masks text with bullet characters)
    pub fn password(mut self, enabled: bool) -> Self {
        self.is_password = enabled;
        self
    }

    /// Set custom mask character for password mode (default: '•')
    pub fn mask_char(mut self, c: char) -> Self {
        self.mask_char = c;
        self
    }

    /// Attach a handle, so application code can move the keyboard here.
    ///
    /// The container has the same builder; put the ref on the *input* when what
    /// you mean is "focus this field", since a container cannot take focus.
    ///
    /// ```ignore
    /// let field = create_widget_ref();
    /// // ...
    /// container().on_click(move || field.focus())
    /// ```
    pub fn widget_ref(mut self, widget_ref: WidgetRef) -> Self {
        self.widget_ref = Some(widget_ref);
        self
    }

    /// Text to show while the field is empty.
    ///
    /// Drawn in the placeholder colour — this field's text colour edit.at reduced
    /// alpha unless it declares
    /// [`placeholder_color`](crate::widgets::InputStyled::placeholder_color) — and
    /// never masked, since it is a label rather than a value: a password field
    /// with a placeholder shows the word, not bullets.
    ///
    /// Reactive, so a prompt that changes changes the empty field with it.
    pub fn placeholder<M>(mut self, text: impl IntoSignal<String, M>) -> Self {
        self.placeholder = Some(text.into_signal());
        self
    }

    /// Draw no caret, keeping the focus and everything it carries — click to
    /// position, drag to select, the keyboard.
    ///
    /// For a field where the caret says nothing: a masked one, where every
    /// character looks the same and you are always edit.at the end. swaylock draws no
    /// caret for that reason.
    ///
    /// It is also the cheapest field there is. A blinking caret is the one thing
    /// a still screen redraws on its own, twice a second, forever; without it an
    /// idle surface wakes the loop for nothing edit.at all.
    pub fn no_caret(mut self) -> Self {
        self.caret = false;
        self
    }

    /// Take keyboard focus when this input first appears, if nothing else has it.
    ///
    /// For a screen that exists to be typed into — a lock screen, a search
    /// overlay, a dialog with one field — where making the user click first is
    /// the wrong answer, and where there is no cursor to click *with* on a
    /// surface that has no pointer.
    ///
    /// The offer is made once, edit.at the input's first layout, and only when no
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

    /// Set callback for text changes
    pub fn on_change<F: Fn(&str) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Box::new(callback));
        self
    }

    /// Set callback for submit (Enter key)
    pub fn on_submit<F: Fn(&str) + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Box::new(callback));
        self
    }

    /// Get the display text (masked if password mode), using cache when clean
    fn display_text(&mut self) -> &str {
        if self.display_text_dirty {
            self.cached_display_text = if self.is_password {
                self.mask_char.to_string().repeat(self.cached_char_count)
            } else {
                self.cached_value.clone()
            };
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
            font_family,
            font_weight,
        );
        self.cached_text_width = self
            .cached_glyph_positions
            .last()
            .copied()
            .unwrap_or_default();

        self.measurements_dirty = false;
    }

    /// Get cached width edit.at a character index (0 to char_count inclusive)
    fn cached_width_at_char(&self, char_index: usize) -> f32 {
        self.cached_glyph_positions
            .get(char_index)
            .copied()
            .unwrap_or(self.cached_text_width)
    }

    /// Convert a character index to a byte index in the cached value
    fn char_to_byte_index(&self, char_index: usize) -> usize {
        self.cached_value
            .char_indices()
            .nth(char_index)
            .map(|(i, _)| i)
            .unwrap_or(self.cached_value.len())
    }

    /// Convert a character range to a byte range in the cached value
    fn char_range_to_byte_range(&self, start: usize, end: usize) -> (usize, usize) {
        let byte_start = self.char_to_byte_index(start);
        let byte_end = self.char_to_byte_index(end);
        (byte_start, byte_end)
    }

    /// Refresh cached values from the bound signal and this field's style.
    ///
    /// The reads happen in this widget's tracking scope, so a change to a
    /// declared metric re-lays-out this input and nothing else.
    fn refresh(&mut self, tree: &Tree, id: WidgetId) -> f32 {
        let (new_value, new_font_size, new_font_family, new_font_weight, overflow) =
            with_signal_tracking(id, JobType::Layout, || {
                let style = self.resolved_text_style(tree, id);
                (
                    self.value.get(),
                    style.font_size.get_or(14.0),
                    style.font_family.get_or_else(default_font_family),
                    style.font_weight.get_or(FontWeight::NORMAL),
                    crate::widgets::text::decoration_overflow(
                        style.stroke.map(|s| s.get()),
                        style.shadow.map(|s| s.get()),
                    ),
                )
            });

        // Check if value changed (need to update char count and selection)
        if new_value != self.cached_value {
            self.cached_value = new_value;
            self.cached_char_count = self.cached_value.chars().count();
            self.display_text_dirty = true;
            self.measurements_dirty = true;
            // Clamp selection to valid range
            self.selection.cursor = self.selection.cursor.min(self.cached_char_count);
            self.selection.anchor = self.selection.anchor.min(self.cached_char_count);
        }

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
    /// Returns whether the caret is drawn edit.at all.
    ///
    /// The wake is *scheduled*, not animated. Asking for an animation frame —
    /// which is what this did — pins the loop edit.at 60 fps for a square wave that
    /// changes twice a second, so 113 frames out of 114 repaint the same pixels.
    /// A focused field is the normal state of a lock screen, and that ran all
    /// night.
    fn update_cursor_blink(&mut self, id: WidgetId, now: Instant) -> bool {
        if !self.caret || !has_focus(id) {
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
    fn reset_cursor_blink(&mut self, at: Instant) {
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
                &self.cached_font_family,
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

    /// Insert text edit.at cursor, replacing any selection
    fn insert_text(&mut self, text: &str, edit: Edit) {
        // Save state before modification
        self.save_to_history(EditType::Insert, edit.at);

        let (start, end) = self.selection.range();
        let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);
        let inserted_char_count = text.chars().count();

        // Replace selection with new text
        let mut new_value = String::with_capacity(self.cached_value.len() + text.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(text);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
        // Update cached char count: old - deleted + inserted
        self.cached_char_count = self.cached_char_count - (end - start) + inserted_char_count;
        self.display_text_dirty = true;
        self.measurements_dirty = true;
        self.selection = Selection::new(start + inserted_char_count);

        self.notify_change();
        self.reset_cursor_blink(edit.at);
        self.ensure_cursor_visible(edit.width);
    }

    /// Delete selected text or character before/after cursor
    fn delete(&mut self, forward: bool, edit: Edit) {
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

        let mut new_value = String::with_capacity(self.cached_value.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
        self.cached_char_count -= end - start;
        self.display_text_dirty = true;
        self.measurements_dirty = true;
        self.notify_change();
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

        if direction < 0 {
            // Move left - collect only the prefix up to cursor (not entire string)
            if start == 0 {
                return 0;
            }

            // Collect characters before cursor position
            let prefix: Vec<char> = self.cached_value.chars().take(start).collect();
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

            let mut pos = start;
            let mut chars = self.cached_value.chars().skip(start).peekable();

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
            Some(self.cached_value[byte_start..byte_end].to_string())
        } else {
            None
        }
    }

    /// The selection, when it is allowed to leave the widget.
    ///
    /// `None` in password mode. What a masked field holds must not reach the
    /// clipboard or the primary selection, and the leak that matters is not
    /// Ctrl+C — it is the primary selection, which an ordinary mouse drag fills
    /// with no keystroke edit.at all, ready for a middle-click anywhere else. GTK4's
    /// `GtkPasswordEntry` refuses to export for the same reason; GTK3 exported
    /// the mask instead, which is a row of bullets that is no use to paste.
    ///
    /// Pasting *into* the field stays allowed. Blocking that is the security
    /// theatre banking sites are mocked for: it stops password managers, not
    /// attackers, and pushes people towards passwords they can type.
    fn exportable_selection(&self) -> Option<String> {
        if self.is_password {
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
        if self.is_password {
            return;
        }
        if self.selection.has_selection() {
            self.copy_selection();
            self.delete(false, edit); // Delete the selection
        }
    }

    /// Paste text from clipboard
    fn paste(&mut self, edit: Edit) {
        if let Some(text) = clipboard_paste() {
            self.insert_text(&text, edit);
        }
    }

    /// Save current state to history (call before making changes)
    fn save_to_history(&mut self, edit_type: EditType, at: Instant) {
        self.history.push(
            HistoryEntry {
                text: self.cached_value.clone(),
                cursor: self.selection.cursor,
                anchor: self.selection.anchor,
            },
            edit_type,
            at,
        );
    }

    /// Get current state as a history entry
    fn current_history_entry(&self) -> HistoryEntry {
        HistoryEntry {
            text: self.cached_value.clone(),
            cursor: self.selection.cursor,
            anchor: self.selection.anchor,
        }
    }

    /// Undo the last change
    fn undo(&mut self, edit: Edit) {
        let current = self.current_history_entry();
        if let Some(previous) = self.history.undo(current) {
            self.cached_value = previous.text;
            self.cached_char_count = self.cached_value.chars().count();
            self.display_text_dirty = true;
            self.measurements_dirty = true;
            self.selection.cursor = previous.cursor;
            self.selection.anchor = previous.anchor;
            self.history.reset_coalescing();
            self.notify_change();
            self.reset_cursor_blink(edit.at);
            self.ensure_cursor_visible(edit.width);
        }
    }

    /// Redo the last undone change
    fn redo(&mut self, edit: Edit) {
        let current = self.current_history_entry();
        if let Some(next) = self.history.redo(current) {
            self.cached_value = next.text;
            self.cached_char_count = self.cached_value.chars().count();
            self.display_text_dirty = true;
            self.measurements_dirty = true;
            self.selection.cursor = next.cursor;
            self.selection.anchor = next.anchor;
            self.history.reset_coalescing();
            self.notify_change();
            self.reset_cursor_blink(edit.at);
            self.ensure_cursor_visible(edit.width);
        }
    }

    /// Notify change callback and sync to signal
    fn notify_change(&self) {
        // Update the signal for two-way binding
        self.value.set(self.cached_value.clone());
        // Call the on_change callback
        if let Some(ref callback) = self.on_change {
            callback(&self.cached_value);
        }
    }

    /// Handle key down event
    fn handle_key(&mut self, key: &Key, ctrl: bool, shift: bool, edit: Edit) -> EventResponse {
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
                if let Some(ref callback) = self.on_submit {
                    callback(&self.cached_value);
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
                    self.insert_text(&c.to_string(), edit);
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }
}

impl Stateful for TextInput {
    type Style = TextStyle;

    fn push_state_style(&mut self, when: StateWhen, style: TextStyle) {
        self.states.push((when, style));
    }
}

impl TextStyled for TextInput {
    fn text_style_mut(&mut self) -> &mut TextStyle {
        self.text_style.get_or_insert_with(Box::default)
    }
}

impl InputStyled for TextInput {
    fn input_style_mut(&mut self) -> &mut InputStyle {
        self.input_style.get_or_insert_with(Box::default)
    }
}

impl Widget for TextInput {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        self.update_cursor_blink(id, tree.frame_instant())
    }

    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        // Text inputs are never relayout boundaries
        tree.set_relayout_boundary(id, false);

        // Refresh cached values from reactive properties
        // This reads signals and registers layout dependencies
        let overflow = self.refresh(tree, id);
        tree.set_paint_overflow(id, overflow);

        // Update measurement cache (has internal dirty check)
        self.update_measurements();

        // Use cached text width for sizing (TextMeasurer caches the actual measurement)
        // Use previous height from tree to maintain stable sizing
        let prev_height = tree.cached_size(id).map(|s| s.height).unwrap_or(0.0);
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

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // Clear needs_layout flag since layout is complete
        tree.clear_needs_layout(id);

        if let Some(widget_ref) = self.widget_ref {
            register_widget_ref(id, widget_ref);
        }

        // Here rather than edit.at construction because focus needs the tree, and
        // this is the first moment the input is in one — the same reason Flutter
        // makes you wait for a post-frame callback to request focus by hand.
        if self.autofocus_pending {
            self.autofocus_pending = false;
            if focused_widget().is_none() {
                request_focus(tree, id);
            }
        }

        size
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let bounds = tree.get_bounds(id).unwrap_or_default();
        let display = self.display_text_cached();
        let is_focused = has_focus(id);

        // Read the declared colours with tracking, so a change to one repaints
        // this input and nothing else.
        let (text_color, selection_color, cursor_color, stroke, shadow, placeholder) =
            with_signal_tracking(id, JobType::Paint, || {
                let style = self.resolved_text_style(tree, id);
                let input = self.resolved_input_style();
                let text_color = style.color.get_or(Color::WHITE);
                // Only when there is nothing to show instead. Read inside the
                // tracking scope like every other paint input, so a prompt that
                // changes repaints the field.
                let placeholder = self
                    .placeholder
                    .filter(|_| self.cached_value.is_empty())
                    .map(|signal| {
                        let color = input.placeholder_color.get_or(Color::rgba(
                            text_color.r,
                            text_color.g,
                            text_color.b,
                            text_color.a * PLACEHOLDER_ALPHA,
                        ));
                        (signal.get(), color)
                    });
                (
                    text_color,
                    input
                        .selection_color
                        .get_or(Color::rgba(0.4, 0.6, 1.0, 0.4)),
                    // The caret defaults to the text colour: an input that
                    // only sets `text_color` should not sprout a blue cursor.
                    input.cursor_color.get_or(text_color),
                    style.stroke.map(|s| s.get()),
                    style.shadow.map(|s| s.get()),
                    placeholder,
                )
            });

        // The text scrolls horizontally under a fixed viewport, so whatever
        // slid past either edge has to be cut edit.at the widget's bounds.
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
            self.cached_font_family.clone(),
            self.cached_font_weight,
            stroke,
            shadow,
        );

        // The caret, and the wake that keeps it blinking.
        //
        // `self.caret` gates the *drawing*, not only the blink: stopping the blink
        // leaves `cursor_visible` edit.at whatever it last was — true, from the
        // constructor — and a field asked for no caret got a permanent one.
        if self.caret && is_focused {
            // The toggle happens in `advance_animations`, and this is what asks
            // for the wake that runs it. It has to be here, not edit.at the moment
            // focus arrives: focus comes from a click, from `autofocus`, or from
            // `WidgetRef::focus()`, and all three only queue a repaint. Asking
            // from the repaint covers every one of them, and covers the half of
            // the cycle where the caret is hidden and there is nothing to draw.
            request_job_at(
                id,
                JobRequest::Animation(RequiredJob::Paint),
                self.last_cursor_toggle + Duration::from_millis(CURSOR_BLINK_MS),
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

        match event {
            Event::MouseDown { x, y, button }
                if bounds.contains(*x, *y) && *button == MouseButton::Left =>
            {
                // Focus, then repaint to show the caret where the click landed.
                // The blink schedules its own next wake from `paint`.
                request_focus(tree, id);
                request_job(id, JobRequest::Paint);

                // Set cursor position
                let char_index = self.char_index_at_x(*x, bounds);
                self.selection = Selection::new(char_index);
                self.is_dragging = true;
                self.reset_cursor_blink(edit.at);
                self.ensure_cursor_visible(bounds.width);

                return EventResponse::Handled;
            }
            Event::MouseMove { x, y, .. } => {
                let in_bounds = bounds.contains(*x, *y);

                // Update hover state and cursor
                if in_bounds && !self.is_hovered {
                    self.is_hovered = true;
                    self.hover.set(true);
                    set_cursor(CursorIcon::Text);
                } else if !in_bounds && self.is_hovered {
                    self.is_hovered = false;
                    self.hover.set(false);
                    set_cursor(CursorIcon::Default);
                }

                if self.is_dragging {
                    // Extend selection while dragging
                    let char_index = self.char_index_at_x(*x, bounds);
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
            Event::MouseDown { x, y, button }
                if bounds.contains(*x, *y) && *button == MouseButton::Middle =>
            {
                // Middle-click paste from the primary selection
                request_focus(tree, id);
                let char_index = self.char_index_at_x(*x, bounds);
                self.selection = Selection::new(char_index);
                if let Some(text) = primary_paste() {
                    self.insert_text(&text, edit);
                }
                self.reset_cursor_blink(edit.at);
                request_job(id, JobRequest::Paint);
                return EventResponse::Handled;
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
            Event::MouseLeave if self.is_hovered => {
                self.is_hovered = false;
                self.hover.set(false);
                set_cursor(CursorIcon::Default);
            }
            _ => {}
        }

        EventResponse::Ignored
    }
}

/// Create a text input widget with two-way signal binding.
///
/// Changes made in the text input will be written back to the signal.
/// ```ignore
/// let username = create_signal(String::new());
/// text_input(username)
/// ```
pub fn text_input(signal: RwSignal<String>) -> TextInput {
    TextInput::new(signal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{clear_pending_jobs, clear_scheduled_jobs, next_deadline, queued_job_types};
    use crate::layout::Constraints;
    use crate::reactive::create_signal;

    /// A laid-out input, focused unless told otherwise.
    fn field(input: TextInput, focused: bool) -> (Tree, WidgetId) {
        clear_pending_jobs();
        clear_scheduled_jobs();
        crate::reactive::focus::clear_focus();

        let mut tree = Tree::new();
        let id = tree.register(Box::new(input));
        tree.with_widget_mut(id, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(id, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 200.0, 40.0))
        });
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

    fn paint_once(tree: &mut Tree, id: WidgetId) {
        let mut node = crate::renderer::RenderNode::new(id.as_u64());
        tree.with_widget_mut(id, |w, id, t| {
            let mut ctx = crate::renderer::PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });
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
