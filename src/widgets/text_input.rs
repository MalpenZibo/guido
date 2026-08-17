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
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job};
use crate::layout::{Constraints, Size};
use crate::reactive::{
    CursorIcon, OptionSignalExt, RwSignal, clipboard_copy, clipboard_paste, has_focus,
    primary_copy, primary_paste, release_focus, request_focus, set_cursor, with_signal_tracking,
};
use crate::renderer::{PaintContext, char_index_from_x_styled};
use crate::tree::{Tree, WidgetId};

use super::font::{FontFamily, FontWeight};
use super::widget::{Color, Event, EventResponse, Key, MouseButton, Rect, Widget};

/// Cursor blink interval in milliseconds
const CURSOR_BLINK_MS: u64 = 530;

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

    // Password mode
    is_password: bool,
    mask_char: char,

    // Selection state
    selection: Selection,

    // Cursor blinking
    cursor_visible: bool,
    last_cursor_toggle: Instant,

    // Mouse drag selection
    is_dragging: bool,

    // Mouse hover state (for cursor icon)
    is_hovered: bool,

    // Undo/redo history
    history: History,

    // Horizontal scroll offset for text overflow
    scroll_offset: f32,

    // Callbacks
    on_change: Option<TextCallback>,
    on_submit: Option<TextCallback>,
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
            selection: Selection::new(0),
            cursor_visible: true,
            last_cursor_toggle: Instant::now(),
            is_dragging: false,
            is_hovered: false,
            history: History::new(),
            scroll_offset: 0.0,
            on_change: None,
            on_submit: None,
        }
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

    /// Get cached width at a character index (0 to char_count inclusive)
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

    /// Refresh cached values from the bound signal and the inherited style.
    ///
    /// The reads happen in this widget's tracking scope, so whichever ancestor
    /// supplied a metric is the one whose change re-lays-out this input.
    fn refresh(&mut self, tree: &Tree, id: WidgetId) -> f32 {
        let (new_value, new_font_size, new_font_family, new_font_weight, overflow) =
            with_signal_tracking(id, JobType::Layout, || {
                let style = tree.inherited_text_style(id);
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

    /// Update cursor blink state.
    /// Returns true if the cursor is actively blinking (widget is focused).
    fn update_cursor_blink(&mut self, id: WidgetId) -> bool {
        if has_focus(id) {
            let now = Instant::now();
            if now.duration_since(self.last_cursor_toggle) >= Duration::from_millis(CURSOR_BLINK_MS)
            {
                self.cursor_visible = !self.cursor_visible;
                self.last_cursor_toggle = now;
            }
            // Keep requesting animation frames for blinking
            request_job(id, JobRequest::Animation(RequiredJob::Paint));
            true
        } else {
            false
        }
    }

    /// Reset cursor to visible (called on input)
    fn reset_cursor_blink(&mut self) {
        self.cursor_visible = true;
        self.last_cursor_toggle = Instant::now();
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
    fn ensure_cursor_visible(&mut self, bounds_width: f32) {
        // Ensure measurements are up to date
        self.update_measurements();

        let cursor_x = self.cached_width_at_char(self.selection.cursor);
        let visible_width = bounds_width - SCROLL_PADDING * 2.0;

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
    fn insert_text(&mut self, text: &str, bounds_width: f32) {
        // Save state before modification
        self.save_to_history(EditType::Insert);

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
        self.reset_cursor_blink();
        self.ensure_cursor_visible(bounds_width);
    }

    /// Delete selected text or character before/after cursor
    fn delete(&mut self, forward: bool, bounds_width: f32) {
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
            self.save_to_history(EditType::Delete);
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
        self.reset_cursor_blink();
        self.ensure_cursor_visible(bounds_width);
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
    fn move_cursor(
        &mut self,
        direction: i32,
        extend_selection: bool,
        word: bool,
        bounds_width: f32,
    ) {
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
        self.reset_cursor_blink();
        self.ensure_cursor_visible(bounds_width);
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
    fn move_to_edge(&mut self, to_start: bool, extend_selection: bool, bounds_width: f32) {
        self.selection.cursor = if to_start { 0 } else { self.cached_char_count };
        if !extend_selection {
            self.selection.collapse();
        }
        self.reset_cursor_blink();
        self.ensure_cursor_visible(bounds_width);
    }

    /// Select all text
    fn select_all(&mut self, bounds_width: f32) {
        self.selection.anchor = 0;
        self.selection.cursor = self.cached_char_count;
        self.reset_cursor_blink();
        self.ensure_cursor_visible(bounds_width);
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
    /// with no keystroke at all, ready for a middle-click anywhere else. GTK4's
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
    fn cut_selection(&mut self, bounds_width: f32) {
        // A cut that cannot copy is not a cut. Refusing the gesture outright is
        // what GtkPasswordEntry does, and it keeps Ctrl+X from quietly becoming
        // a delete while the user believes the clipboard was filled.
        if self.is_password {
            return;
        }
        if self.selection.has_selection() {
            self.copy_selection();
            self.delete(false, bounds_width); // Delete the selection
        }
    }

    /// Paste text from clipboard
    fn paste(&mut self, bounds_width: f32) {
        if let Some(text) = clipboard_paste() {
            self.insert_text(&text, bounds_width);
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
    fn undo(&mut self, bounds_width: f32) {
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
            self.reset_cursor_blink();
            self.ensure_cursor_visible(bounds_width);
        }
    }

    /// Redo the last undone change
    fn redo(&mut self, bounds_width: f32) {
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
            self.reset_cursor_blink();
            self.ensure_cursor_visible(bounds_width);
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
    fn handle_key(
        &mut self,
        key: &Key,
        ctrl: bool,
        shift: bool,
        bounds_width: f32,
    ) -> EventResponse {
        match key {
            Key::Backspace => {
                self.delete(false, bounds_width);
                EventResponse::Handled
            }
            Key::Delete => {
                self.delete(true, bounds_width);
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
                    self.move_cursor(-1, shift, ctrl, bounds_width);
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
                    self.move_cursor(1, shift, ctrl, bounds_width);
                }
                EventResponse::Handled
            }
            Key::Home => {
                self.move_to_edge(true, shift, bounds_width);
                EventResponse::Handled
            }
            Key::End => {
                self.move_to_edge(false, shift, bounds_width);
                EventResponse::Handled
            }
            Key::Char(c) => {
                if ctrl {
                    match c.to_ascii_lowercase() {
                        'a' => {
                            self.select_all(bounds_width);
                            EventResponse::Handled
                        }
                        'c' => {
                            self.copy_selection();
                            EventResponse::Handled
                        }
                        'x' => {
                            self.cut_selection(bounds_width);
                            EventResponse::Handled
                        }
                        'v' => {
                            self.paste(bounds_width);
                            EventResponse::Handled
                        }
                        'z' => {
                            // Ctrl+Shift+Z = redo, Ctrl+Z = undo
                            if shift {
                                self.redo(bounds_width);
                            } else {
                                self.undo(bounds_width);
                            }
                            EventResponse::Handled
                        }
                        'y' => {
                            // Ctrl+Y = redo
                            self.redo(bounds_width);
                            EventResponse::Handled
                        }
                        _ => EventResponse::Ignored,
                    }
                } else if !c.is_control() {
                    self.insert_text(&c.to_string(), bounds_width);
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
    fn advance_animations(&mut self, _tree: &mut Tree, id: WidgetId) -> bool {
        self.update_cursor_blink(id)
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

        size
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let bounds = tree.get_bounds(id).unwrap_or_default();
        let display = self.display_text_cached();
        let is_focused = has_focus(id);

        // Read the inherited colours with tracking so a change on whichever
        // ancestor supplied them repaints this input and nothing else.
        let (text_color, selection_color, cursor_color, stroke, shadow) =
            with_signal_tracking(id, JobType::Paint, || {
                let style = tree.inherited_text_style(id);
                let text_color = style.color.get_or(Color::WHITE);
                (
                    text_color,
                    style
                        .selection_color
                        .get_or(Color::rgba(0.4, 0.6, 1.0, 0.4)),
                    // The caret defaults to the text colour: an input that
                    // only sets `text_color` should not sprout a blue cursor.
                    style.cursor_color.get_or(text_color),
                    style.stroke.map(|s| s.get()),
                    style.shadow.map(|s| s.get()),
                )
            });

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
        ctx.draw_text_decorated(
            display,
            text_bounds,
            text_color,
            self.cached_font_size,
            self.cached_font_family.clone(),
            self.cached_font_weight,
            stroke,
            shadow,
        );

        // Draw cursor if focused and visible (LOCAL coords)
        if is_focused && self.cursor_visible {
            let cursor_x = self.cached_width_at_char(self.selection.cursor) - self.scroll_offset;
            let cursor_rect = Rect::new(
                cursor_x,
                0.0,
                1.5, // cursor width
                bounds.height,
            );
            ctx.draw_rounded_rect(cursor_rect, cursor_color, 0.0);
        }
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        // Get bounds from Tree for hit testing
        let bounds = tree.get_bounds(id).unwrap_or_default();

        match event {
            Event::MouseDown { x, y, button }
                if bounds.contains(*x, *y) && *button == MouseButton::Left =>
            {
                // Request focus and start cursor blink animation
                request_focus(tree, id);
                request_job(id, JobRequest::Animation(RequiredJob::Paint));

                // Set cursor position
                let char_index = self.char_index_at_x(*x, bounds);
                self.selection = Selection::new(char_index);
                self.is_dragging = true;
                self.reset_cursor_blink();
                self.ensure_cursor_visible(bounds.width);

                return EventResponse::Handled;
            }
            Event::MouseMove { x, y, .. } => {
                let in_bounds = bounds.contains(*x, *y);

                // Update hover state and cursor
                if in_bounds && !self.is_hovered {
                    self.is_hovered = true;
                    set_cursor(CursorIcon::Text);
                } else if !in_bounds && self.is_hovered {
                    self.is_hovered = false;
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
                    self.insert_text(&text, bounds.width);
                }
                self.reset_cursor_blink();
                request_job(id, JobRequest::Paint);
                return EventResponse::Handled;
            }
            Event::KeyDown { key, modifiers } if has_focus(id) => {
                let response = self.handle_key(key, modifiers.ctrl, modifiers.shift, bounds.width);
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
