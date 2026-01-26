use crate::layout::Size;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping};
use std::cell::RefCell;

pub struct TextMeasurer {
    font_system: FontSystem,
}

impl TextMeasurer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
        }
    }

    pub fn measure(&mut self, text: &str, font_size: f32, max_width: Option<f32>) -> Size {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, true);

        let mut width = 0.0f32;
        let mut height = 0.0f32;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            height += run.line_height;
        }

        // Ensure minimum height for empty text
        if height == 0.0 {
            height = font_size * 1.2;
        }

        Size::new(width, height)
    }

    /// Measure text width up to a specific character index.
    /// This is useful for cursor positioning in text input widgets.
    pub fn measure_to_char(&mut self, text: &str, font_size: f32, char_index: usize) -> f32 {
        if char_index == 0 || text.is_empty() {
            return 0.0;
        }

        // Get the byte position for the character index
        let byte_pos = text
            .char_indices()
            .nth(char_index)
            .map(|(i, _)| i)
            .unwrap_or(text.len());

        let prefix = &text[..byte_pos];
        self.measure(prefix, font_size, None).width
    }

    /// Find the character index from an x-coordinate using binary search.
    /// This is useful for click-to-position in text input widgets.
    pub fn char_from_x(&mut self, text: &str, font_size: f32, x: f32) -> usize {
        if text.is_empty() || x <= 0.0 {
            return 0;
        }

        let char_count = text.chars().count();
        let total_width = self.measure(text, font_size, None).width;
        if x >= total_width {
            return char_count;
        }

        // Binary search for the character position
        let mut left = 0;
        let mut right = char_count;

        while left < right {
            let mid = (left + right) / 2;
            let width = self.measure_to_char(text, font_size, mid);
            if width < x {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // Check if click is closer to previous character
        if left > 0 {
            let prev_width = self.measure_to_char(text, font_size, left - 1);
            let curr_width = self.measure_to_char(text, font_size, left);
            if (x - prev_width) < (curr_width - x) {
                return left - 1;
            }
        }

        left.min(char_count)
    }
}

thread_local! {
    static TEXT_MEASURER: RefCell<TextMeasurer> = RefCell::new(TextMeasurer::new());
}

/// Measure text dimensions using the font system
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> Size {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure(text, font_size, max_width))
}

/// Measure text width up to a specific character index (for cursor positioning)
pub fn measure_text_to_char(text: &str, font_size: f32, char_index: usize) -> f32 {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure_to_char(text, font_size, char_index))
}

/// Find the character index from an x-coordinate (for click-to-position)
pub fn char_index_from_x(text: &str, font_size: f32, x: f32) -> usize {
    TEXT_MEASURER.with_borrow_mut(|m| m.char_from_x(text, font_size, x))
}
