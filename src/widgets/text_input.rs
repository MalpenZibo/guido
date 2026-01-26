//! TextInput widget for single-line text editing.
//!
//! The TextInput widget handles:
//! - Text display and editing
//! - Cursor blinking and positioning
//! - Text selection with mouse and keyboard
//! - Password masking mode
//!
//! Styling (background, borders, etc.) should be handled by wrapping in a Container.

use std::time::{Duration, Instant};

use crate::layout::{Constraints, Size};
use crate::reactive::{
    has_focus, release_focus, request_animation_frame, request_focus, ChangeFlags, IntoMaybeDyn,
    MaybeDyn, WidgetId,
};
use crate::renderer::{char_index_from_x, measure_text, measure_text_to_char, PaintContext};

use super::impl_dirty_flags;
use super::widget::{Color, Event, EventResponse, Key, MouseButton, Rect, Widget};

/// Cursor blink interval in milliseconds
const CURSOR_BLINK_MS: u64 = 530;

/// Type alias for text input callbacks
type TextCallback = Box<dyn Fn(&str)>;

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
    cached_value: String,

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

    // Mouse drag selection
    is_dragging: bool,

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
            cached_value,
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
            is_dragging: false,
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

    /// Get the display text (masked if password mode)
    fn display_text(&self) -> String {
        if self.is_password {
            self.mask_char
                .to_string()
                .repeat(self.cached_value.chars().count())
        } else {
            self.cached_value.clone()
        }
    }

    /// Refresh cached values and return true if changed
    fn refresh(&mut self) -> bool {
        let new_value = self.value.get();
        let new_font_size = self.font_size.get();

        let value_changed = new_value != self.cached_value;
        let font_changed = (new_font_size - self.cached_font_size).abs() > f32::EPSILON;

        if value_changed {
            self.cached_value = new_value;
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

    /// Get character index from x coordinate relative to text start
    fn char_index_at_x(&self, x: f32) -> usize {
        let display = self.display_text();
        let text_x = self.bounds.x;
        let relative_x = x - text_x;
        char_index_from_x(&display, self.cached_font_size, relative_x)
    }

    /// Insert text at cursor, replacing any selection
    fn insert_text(&mut self, text: &str) {
        let (start, end) = self.selection.range();

        // Convert character indices to byte indices
        let byte_start = self
            .cached_value
            .char_indices()
            .nth(start)
            .map(|(i, _)| i)
            .unwrap_or(self.cached_value.len());
        let byte_end = self
            .cached_value
            .char_indices()
            .nth(end)
            .map(|(i, _)| i)
            .unwrap_or(self.cached_value.len());

        // Replace selection with new text
        let mut new_value = String::with_capacity(self.cached_value.len() + text.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(text);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
        self.selection = Selection::new(start + text.chars().count());

        self.notify_change();
        self.reset_cursor_blink();
    }

    /// Delete selected text or character before/after cursor
    fn delete(&mut self, forward: bool) {
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
    }

    /// Delete a range of characters
    fn delete_range(&mut self, start: usize, end: usize) {
        let byte_start = self
            .cached_value
            .char_indices()
            .nth(start)
            .map(|(i, _)| i)
            .unwrap_or(self.cached_value.len());
        let byte_end = self
            .cached_value
            .char_indices()
            .nth(end)
            .map(|(i, _)| i)
            .unwrap_or(self.cached_value.len());

        let mut new_value = String::with_capacity(self.cached_value.len());
        new_value.push_str(&self.cached_value[..byte_start]);
        new_value.push_str(&self.cached_value[byte_end..]);

        self.cached_value = new_value;
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
    }

    /// Select all text
    fn select_all(&mut self) {
        self.selection.anchor = 0;
        self.selection.cursor = self.cached_value.chars().count();
        self.reset_cursor_blink();
    }

    /// Notify change callback
    fn notify_change(&self) {
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
                        // Note: Clipboard operations (Ctrl+C/V/X) require Wayland data device
                        // protocol which is not implemented yet
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

        // Skip re-measurement if nothing changed and we don't need layout
        if !content_changed && !self.needs_layout() && self.bounds.width > 0.0 {
            return Size::new(self.bounds.width, self.bounds.height);
        }

        let display = self.display_text();
        let measured = measure_text(&display, self.cached_font_size, None);

        // Use measured width but ensure minimum height for empty text
        let height = measured.height.max(self.cached_font_size * 1.2);

        let size = Size::new(
            measured
                .width
                .max(constraints.min_width)
                .min(constraints.max_width),
            height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let display = self.display_text();
        let text_color = self.text_color.get();
        let is_focused = has_focus(self.widget_id);

        // Draw selection highlight if focused and has selection
        if is_focused && self.selection.has_selection() {
            let (start, end) = self.selection.range();
            let start_x = measure_text_to_char(&display, self.cached_font_size, start);
            let end_x = measure_text_to_char(&display, self.cached_font_size, end);

            let selection_rect = Rect::new(
                self.bounds.x + start_x,
                self.bounds.y,
                end_x - start_x,
                self.bounds.height,
            );
            ctx.draw_rect(selection_rect, self.selection_color.get());
        }

        // Draw text
        ctx.draw_text(&display, self.bounds, text_color, self.cached_font_size);

        // Draw cursor if focused and visible
        if is_focused && self.cursor_visible {
            let cursor_x =
                measure_text_to_char(&display, self.cached_font_size, self.selection.cursor);
            let cursor_rect = Rect::new(
                self.bounds.x + cursor_x,
                self.bounds.y,
                1.5, // cursor width
                self.bounds.height,
            );
            ctx.draw_rect(cursor_rect, self.cursor_color.get());
        }
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

                    return EventResponse::Handled;
                }
            }
            Event::MouseMove { x, y } => {
                if self.is_dragging {
                    // Extend selection while dragging
                    let char_index = self.char_index_at_x(*x);
                    self.selection.cursor = char_index;
                    request_animation_frame();
                    return EventResponse::Handled;
                }
                // Check if we're over the text input for cursor styling (future)
                if self.bounds.contains(*x, *y) {
                    // Could set cursor to text cursor here
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
                    let response = self.handle_key(key, modifiers.ctrl, modifiers.shift);
                    if response == EventResponse::Handled {
                        request_animation_frame();
                    }
                    return response;
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

/// Create a text input widget
///
/// Accepts static strings, closures, or signals:
/// ```ignore
/// text_input(username)  // reactive signal
/// text_input("default value")  // static initial value
/// ```
pub fn text_input(value: impl IntoMaybeDyn<String>) -> TextInput {
    TextInput::new(value)
}
