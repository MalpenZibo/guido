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

use crate::layout::{Constraints, Size};
use crate::reactive::{
    clipboard_copy, clipboard_paste, has_focus, release_focus, request_animation_frame,
    request_focus, ChangeFlags, IntoMaybeDyn, MaybeDyn, Signal, WidgetId,
};
use crate::renderer::{char_index_from_x, measure_text, measure_text_to_char, PaintContext};

use super::impl_dirty_flags;
use super::widget::{Color, Event, EventResponse, Key, Modifiers, MouseButton, Rect, Widget};

/// Cursor blink interval in milliseconds
const CURSOR_BLINK_MS: u64 = 530;

/// Key repeat delay (time before repeat starts) in milliseconds
const KEY_REPEAT_DELAY_MS: u64 = 400;

/// Key repeat interval (time between repeats) in milliseconds
const KEY_REPEAT_INTERVAL_MS: u64 = 35;

/// Maximum number of undo history entries
const MAX_HISTORY_SIZE: usize = 100;

/// Padding from edges before scrolling starts
const SCROLL_PADDING: f32 = 2.0;

/// Time window for coalescing similar edits (in milliseconds)
const HISTORY_COALESCE_MS: u64 = 500;

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
    fn push(&mut self, entry: HistoryEntry, edit_type: EditType) {
        let now = Instant::now();
        let since_last = now.duration_since(self.last_edit_time);

        // Don't push if it's the same as the last entry
        if let Some(last) = self.undo_stack.back() {
            if last.text == entry.text {
                return;
            }
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

        self.last_edit_time = now;
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

    /// Check if undo is available
    #[allow(dead_code)]
    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Check if redo is available
    #[allow(dead_code)]
    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
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

pub struct TextInput {
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,

    // Content (actual value, never masked)
    value: MaybeDyn<String>,
    /// Signal for two-way binding (write-back when text changes)
    value_signal: Option<Signal<String>>,
    cached_value: String,
    cached_display_text: String,
    display_text_dirty: bool,

    // Styling
    text_color: MaybeDyn<Color>,
    cursor_color: MaybeDyn<Color>,
    selection_color: MaybeDyn<Color>,
    font_size: MaybeDyn<f32>,
    cached_font_size: f32,

    // Password mode
    is_password: bool,
    mask_char: char,

    // Selection state
    selection: Selection,

    // Cursor blinking
    cursor_visible: bool,
    last_cursor_toggle: Instant,

    // Key repeat state
    pressed_key: Option<(Key, Modifiers)>,
    key_press_time: Instant,
    last_repeat_time: Instant,

    // Mouse drag selection
    is_dragging: bool,

    // Undo/redo history
    history: History,

    // Horizontal scroll offset for text overflow
    scroll_offset: f32,

    // Layout
    bounds: Rect,

    // Callbacks
    on_change: Option<TextCallback>,
    on_submit: Option<TextCallback>,
}

impl TextInput {
    pub fn new(value: impl IntoMaybeDyn<String>) -> Self {
        let value = value.into_maybe_dyn();
        let cached_value = value.get();
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            value,
            value_signal: None,
            cached_value,
            cached_display_text: String::new(),
            display_text_dirty: true,
            text_color: MaybeDyn::Static(Color::WHITE),
            cursor_color: MaybeDyn::Static(Color::rgb(0.4, 0.8, 1.0)),
            selection_color: MaybeDyn::Static(Color::rgba(0.4, 0.6, 1.0, 0.4)),
            font_size: MaybeDyn::Static(14.0),
            cached_font_size: 14.0,
            is_password: false,
            mask_char: '•',
            selection: Selection::new(0),
            cursor_visible: true,
            last_cursor_toggle: Instant::now(),
            pressed_key: None,
            key_press_time: Instant::now(),
            last_repeat_time: Instant::now(),
            is_dragging: false,
            history: History::new(),
            scroll_offset: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            on_change: None,
            on_submit: None,
        }
    }

    /// Create a TextInput with a Signal for two-way binding.
    /// Changes made in the TextInput will be written back to the signal.
    pub fn from_signal(signal: Signal<String>) -> Self {
        let cached_value = signal.get();
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            value: MaybeDyn::Dynamic(std::sync::Arc::new(move || signal.get())),
            value_signal: Some(signal),
            cached_value,
            cached_display_text: String::new(),
            display_text_dirty: true,
            text_color: MaybeDyn::Static(Color::WHITE),
            cursor_color: MaybeDyn::Static(Color::rgb(0.4, 0.8, 1.0)),
            selection_color: MaybeDyn::Static(Color::rgba(0.4, 0.6, 1.0, 0.4)),
            font_size: MaybeDyn::Static(14.0),
            cached_font_size: 14.0,
            is_password: false,
            mask_char: '•',
            selection: Selection::new(0),
            cursor_visible: true,
            last_cursor_toggle: Instant::now(),
            pressed_key: None,
            key_press_time: Instant::now(),
            last_repeat_time: Instant::now(),
            is_dragging: false,
            history: History::new(),
            scroll_offset: 0.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            on_change: None,
            on_submit: None,
        }
    }

    /// Set the text color
    pub fn text_color(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.text_color = color.into_maybe_dyn();
        self
    }

    /// Set the cursor color
    pub fn cursor_color(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.cursor_color = color.into_maybe_dyn();
        self
    }

    /// Set the selection highlight color
    pub fn selection_color(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.selection_color = color.into_maybe_dyn();
        self
    }

    /// Set the font size
    pub fn font_size(mut self, size: impl IntoMaybeDyn<f32>) -> Self {
        self.font_size = size.into_maybe_dyn();
        self
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
                self.mask_char
                    .to_string()
                    .repeat(self.cached_value.chars().count())
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

    /// Refresh cached values and return true if changed
    fn refresh(&mut self) -> bool {
        let new_value = self.value.get();
        let new_font_size = self.font_size.get();

        let value_changed = new_value != self.cached_value;
        let font_changed = (new_font_size - self.cached_font_size).abs() > f32::EPSILON;

        if value_changed {
            self.cached_value = new_value;
            self.display_text_dirty = true;
            // Clamp selection to valid range
            let char_count = self.cached_value.chars().count();
            self.selection.cursor = self.selection.cursor.min(char_count);
            self.selection.anchor = self.selection.anchor.min(char_count);
        }
        if font_changed {
            self.cached_font_size = new_font_size;
        }

        value_changed || font_changed
    }

    /// Update cursor blink state
    fn update_cursor_blink(&mut self) {
        if has_focus(self.widget_id) {
            let now = Instant::now();
            if now.duration_since(self.last_cursor_toggle) >= Duration::from_millis(CURSOR_BLINK_MS)
            {
                self.cursor_visible = !self.cursor_visible;
                self.last_cursor_toggle = now;
            }
            // Keep requesting frames for blinking
            request_animation_frame();
        }
    }

    /// Reset cursor to visible (called on input)
    fn reset_cursor_blink(&mut self) {
        self.cursor_visible = true;
        self.last_cursor_toggle = Instant::now();
    }

    /// Handle key repeat for held keys
    fn handle_key_repeat(&mut self) {
        if !has_focus(self.widget_id) {
            self.pressed_key = None;
            return;
        }

        if let Some((key, modifiers)) = self.pressed_key {
            let now = Instant::now();
            let since_press = now.duration_since(self.key_press_time);
            let since_repeat = now.duration_since(self.last_repeat_time);

            // Check if we're past the initial delay
            if since_press >= Duration::from_millis(KEY_REPEAT_DELAY_MS) {
                // Check if it's time for another repeat
                if since_repeat >= Duration::from_millis(KEY_REPEAT_INTERVAL_MS) {
                    self.handle_key(&key, modifiers.ctrl, modifiers.shift);
                    self.last_repeat_time = now;
                }
            }

            // Keep requesting frames while a key is held
            request_animation_frame();
        }
    }

    /// Get character index from x coordinate relative to text start
    fn char_index_at_x(&self, x: f32) -> usize {
        let display = self.display_text_cached();
        let text_x = self.bounds.x;
        // Account for scroll offset
        let relative_x = x - text_x + self.scroll_offset;
        char_index_from_x(display, self.cached_font_size, relative_x)
    }

    /// Ensure the cursor is visible by adjusting scroll offset
    fn ensure_cursor_visible(&mut self) {
        let cursor_x = measure_text_to_char(
            self.display_text_cached(),
            self.cached_font_size,
            self.selection.cursor,
        );

        let visible_width = self.bounds.width - SCROLL_PADDING * 2.0;

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
    fn insert_text(&mut self, text: &str) {
        // Save state before modification
        self.save_to_history(EditType::Insert);

        let (start, end) = self.selection.range();
        let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);

        // Replace selection with new text
        let mut new_value = String::with_capacity(self.cached_value.len() + text.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(text);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
        self.display_text_dirty = true;
        self.selection = Selection::new(start + text.chars().count());

        self.notify_change();
        self.reset_cursor_blink();
        self.ensure_cursor_visible();
    }

    /// Delete selected text or character before/after cursor
    fn delete(&mut self, forward: bool) {
        // Check if there's anything to delete
        let has_content_to_delete = if self.selection.has_selection() {
            true
        } else if forward {
            self.selection.cursor < self.cached_value.chars().count()
        } else {
            self.selection.cursor > 0
        };

        // Save state before modification (only if we'll actually delete something)
        if has_content_to_delete {
            self.save_to_history(EditType::Delete);
        }

        if self.selection.has_selection() {
            // Delete selection
            let (start, end) = self.selection.range();
            self.delete_range(start, end);
            self.selection = Selection::new(start);
        } else if forward {
            // Delete character after cursor
            let char_count = self.cached_value.chars().count();
            if self.selection.cursor < char_count {
                self.delete_range(self.selection.cursor, self.selection.cursor + 1);
            }
        } else {
            // Delete character before cursor (backspace)
            if self.selection.cursor > 0 {
                self.delete_range(self.selection.cursor - 1, self.selection.cursor);
                self.selection = Selection::new(self.selection.cursor - 1);
            }
        }
        self.reset_cursor_blink();
        self.ensure_cursor_visible();
    }

    /// Delete a range of characters
    fn delete_range(&mut self, start: usize, end: usize) {
        let (byte_start, byte_end) = self.char_range_to_byte_range(start, end);

        let mut new_value = String::with_capacity(self.cached_value.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
        self.display_text_dirty = true;
        self.notify_change();
    }

    /// Move cursor left/right, optionally extending selection
    fn move_cursor(&mut self, direction: i32, extend_selection: bool, word: bool) {
        let char_count = self.cached_value.chars().count();
        let new_pos = if word {
            self.find_word_boundary(self.selection.cursor, direction)
        } else if direction < 0 {
            self.selection.cursor.saturating_sub(1)
        } else {
            (self.selection.cursor + 1).min(char_count)
        };

        self.selection.cursor = new_pos;
        if !extend_selection {
            self.selection.collapse();
        }
        self.reset_cursor_blink();
        self.ensure_cursor_visible();
    }

    /// Find word boundary in given direction
    fn find_word_boundary(&self, start: usize, direction: i32) -> usize {
        let chars: Vec<char> = self.cached_value.chars().collect();
        let len = chars.len();

        if direction < 0 {
            // Move left
            if start == 0 {
                return 0;
            }
            let mut pos = start - 1;
            // Skip whitespace
            while pos > 0 && chars[pos].is_whitespace() {
                pos -= 1;
            }
            // Skip word characters
            while pos > 0 && !chars[pos - 1].is_whitespace() {
                pos -= 1;
            }
            pos
        } else {
            // Move right
            if start >= len {
                return len;
            }
            let mut pos = start;
            // Skip word characters
            while pos < len && !chars[pos].is_whitespace() {
                pos += 1;
            }
            // Skip whitespace
            while pos < len && chars[pos].is_whitespace() {
                pos += 1;
            }
            pos
        }
    }

    /// Move cursor to start/end
    fn move_to_edge(&mut self, to_start: bool, extend_selection: bool) {
        self.selection.cursor = if to_start {
            0
        } else {
            self.cached_value.chars().count()
        };
        if !extend_selection {
            self.selection.collapse();
        }
        self.reset_cursor_blink();
        self.ensure_cursor_visible();
    }

    /// Select all text
    fn select_all(&mut self) {
        self.selection.anchor = 0;
        self.selection.cursor = self.cached_value.chars().count();
        self.reset_cursor_blink();
        self.ensure_cursor_visible();
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

    /// Copy selected text to clipboard
    fn copy_selection(&self) {
        if let Some(text) = self.get_selected_text() {
            clipboard_copy(&text);
        }
    }

    /// Cut selected text (copy and delete)
    fn cut_selection(&mut self) {
        if self.selection.has_selection() {
            self.copy_selection();
            self.delete(false); // Delete the selection
        }
    }

    /// Paste text from clipboard
    fn paste(&mut self) {
        if let Some(text) = clipboard_paste() {
            self.insert_text(&text);
        }
    }

    /// Save current state to history (call before making changes)
    fn save_to_history(&mut self, edit_type: EditType) {
        self.history.push(
            HistoryEntry {
                text: self.cached_value.clone(),
                cursor: self.selection.cursor,
                anchor: self.selection.anchor,
            },
            edit_type,
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
    fn undo(&mut self) {
        let current = self.current_history_entry();
        if let Some(previous) = self.history.undo(current) {
            self.cached_value = previous.text;
            self.display_text_dirty = true;
            self.selection.cursor = previous.cursor;
            self.selection.anchor = previous.anchor;
            self.history.reset_coalescing();
            self.notify_change();
            self.reset_cursor_blink();
            self.ensure_cursor_visible();
        }
    }

    /// Redo the last undone change
    fn redo(&mut self) {
        let current = self.current_history_entry();
        if let Some(next) = self.history.redo(current) {
            self.cached_value = next.text;
            self.display_text_dirty = true;
            self.selection.cursor = next.cursor;
            self.selection.anchor = next.anchor;
            self.history.reset_coalescing();
            self.notify_change();
            self.reset_cursor_blink();
            self.ensure_cursor_visible();
        }
    }

    /// Notify change callback and sync to signal
    fn notify_change(&self) {
        // Update the signal for two-way binding
        if let Some(ref signal) = self.value_signal {
            signal.set(self.cached_value.clone());
        }
        // Call the on_change callback
        if let Some(ref callback) = self.on_change {
            callback(&self.cached_value);
        }
    }

    /// Handle key down event
    fn handle_key(&mut self, key: &Key, ctrl: bool, shift: bool) -> EventResponse {
        match key {
            Key::Backspace => {
                self.delete(false);
                EventResponse::Handled
            }
            Key::Delete => {
                self.delete(true);
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
                    self.reset_cursor_blink();
                } else {
                    self.move_cursor(-1, shift, ctrl);
                }
                EventResponse::Handled
            }
            Key::Right => {
                if !shift && self.selection.has_selection() {
                    // Collapse to end of selection
                    let (_, end) = self.selection.range();
                    self.selection = Selection::new(end);
                    self.reset_cursor_blink();
                } else {
                    self.move_cursor(1, shift, ctrl);
                }
                EventResponse::Handled
            }
            Key::Home => {
                self.move_to_edge(true, shift);
                EventResponse::Handled
            }
            Key::End => {
                self.move_to_edge(false, shift);
                EventResponse::Handled
            }
            Key::Char(c) => {
                if ctrl {
                    match c.to_ascii_lowercase() {
                        'a' => {
                            self.select_all();
                            EventResponse::Handled
                        }
                        'c' => {
                            self.copy_selection();
                            EventResponse::Handled
                        }
                        'x' => {
                            self.cut_selection();
                            EventResponse::Handled
                        }
                        'v' => {
                            self.paste();
                            EventResponse::Handled
                        }
                        'z' => {
                            // Ctrl+Shift+Z = redo, Ctrl+Z = undo
                            if shift {
                                self.redo();
                            } else {
                                self.undo();
                            }
                            EventResponse::Handled
                        }
                        'y' => {
                            // Ctrl+Y = redo
                            self.redo();
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                } else if !c.is_control() {
                    self.insert_text(&c.to_string());
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            }
            _ => EventResponse::Ignored,
        }
    }
}

impl Widget for TextInput {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let content_changed = self.refresh();

        // Update cursor blink if focused
        self.update_cursor_blink();

        // Handle key repeat for held keys
        self.handle_key_repeat();

        // Skip re-measurement if nothing changed and we don't need layout
        if !content_changed && !self.needs_layout() && self.bounds.width > 0.0 {
            return Size::new(self.bounds.width, self.bounds.height);
        }

        // Refresh display text cache if dirty
        let _ = self.display_text();
        let measured = measure_text(self.display_text_cached(), self.cached_font_size, None);

        // Ensure minimum height for empty text
        let height = measured.height.max(self.cached_font_size * 1.2);

        // Text inputs should fill available width (like HTML input elements)
        // Use max_width if available, otherwise fall back to measured width
        let width = if constraints.max_width.is_finite() && constraints.max_width > 0.0 {
            constraints.max_width
        } else {
            measured.width.max(100.0) // Minimum 100px if unconstrained
        };

        let size = Size::new(
            width.max(constraints.min_width).min(constraints.max_width),
            height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let display = self.display_text_cached();
        let text_color = self.text_color.get();
        let is_focused = has_focus(self.widget_id);

        // Clip to bounds (prevents text overflow)
        ctx.push_clip(self.bounds, 0.0, 1.0);

        // Draw selection highlight if focused and has selection
        if is_focused && self.selection.has_selection() {
            let (start, end) = self.selection.range();
            let start_x =
                measure_text_to_char(display, self.cached_font_size, start) - self.scroll_offset;
            let end_x =
                measure_text_to_char(display, self.cached_font_size, end) - self.scroll_offset;

            let selection_rect = Rect::new(
                self.bounds.x + start_x,
                self.bounds.y,
                end_x - start_x,
                self.bounds.height,
            );
            ctx.draw_rect(selection_rect, self.selection_color.get());
        }

        // Draw text with scroll offset
        // Use a very large width to prevent word wrapping - the clip rect handles clipping
        let text_width = measure_text(display, self.cached_font_size, None).width;
        let text_bounds = Rect::new(
            self.bounds.x - self.scroll_offset,
            self.bounds.y,
            text_width.max(self.bounds.width), // Use actual text width to prevent wrapping
            self.bounds.height,
        );
        ctx.draw_text(display, text_bounds, text_color, self.cached_font_size);

        // Draw cursor if focused and visible
        if is_focused && self.cursor_visible {
            let cursor_x =
                measure_text_to_char(display, self.cached_font_size, self.selection.cursor)
                    - self.scroll_offset;
            let cursor_rect = Rect::new(
                self.bounds.x + cursor_x,
                self.bounds.y,
                1.5, // cursor width
                self.bounds.height,
            );
            ctx.draw_rect(cursor_rect, self.cursor_color.get());
        }

        ctx.pop_clip();
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        match event {
            Event::MouseDown { x, y, button } => {
                if self.bounds.contains(*x, *y) && *button == MouseButton::Left {
                    // Request focus
                    request_focus(self.widget_id);
                    request_animation_frame();

                    // Set cursor position
                    let char_index = self.char_index_at_x(*x);
                    self.selection = Selection::new(char_index);
                    self.is_dragging = true;
                    self.reset_cursor_blink();
                    self.ensure_cursor_visible();

                    return EventResponse::Handled;
                }
            }
            Event::MouseMove { x, .. } => {
                if self.is_dragging {
                    // Extend selection while dragging
                    let char_index = self.char_index_at_x(*x);
                    self.selection.cursor = char_index;
                    self.ensure_cursor_visible();
                    request_animation_frame();
                    return EventResponse::Handled;
                }
            }
            Event::MouseUp { button, .. } => {
                if *button == MouseButton::Left && self.is_dragging {
                    self.is_dragging = false;
                    return EventResponse::Handled;
                }
            }
            Event::KeyDown { key, modifiers } => {
                if has_focus(self.widget_id) {
                    // Track key for repeat
                    let now = Instant::now();
                    self.pressed_key = Some((*key, *modifiers));
                    self.key_press_time = now;
                    self.last_repeat_time = now;

                    let response = self.handle_key(key, modifiers.ctrl, modifiers.shift);
                    if response == EventResponse::Handled {
                        request_animation_frame();
                    }
                    return response;
                }
            }
            Event::KeyUp { key, .. } => {
                // Stop repeating when key is released
                if let Some((pressed_key, _)) = self.pressed_key {
                    if pressed_key == *key {
                        self.pressed_key = None;
                    }
                }
            }
            Event::FocusOut => {
                if has_focus(self.widget_id) {
                    release_focus(self.widget_id);
                    self.cursor_visible = false;
                    self.is_dragging = false;
                    request_animation_frame();
                }
            }
            _ => {}
        }

        EventResponse::Ignored
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.bounds.x = x;
        self.bounds.y = y;
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn id(&self) -> WidgetId {
        self.widget_id
    }

    impl_dirty_flags!();
}

/// Create a text input widget (one-way binding)
///
/// Accepts static strings, closures, or signals:
/// ```ignore
/// text_input("default value")  // static initial value
/// text_input(move || some_signal.get())  // reactive closure
/// ```
///
/// Note: For two-way binding with a signal, use [`text_input_signal`] instead.
pub fn text_input(value: impl IntoMaybeDyn<String>) -> TextInput {
    TextInput::new(value)
}

/// Create a text input widget with two-way signal binding
///
/// Changes made in the text input will be written back to the signal.
/// ```ignore
/// let username = create_signal(String::new());
/// text_input_signal(username)  // two-way binding
/// ```
pub fn text_input_signal(signal: Signal<String>) -> TextInput {
    TextInput::from_signal(signal)
}
