use crate::layout::Size;
use crate::widgets::font::{FontFamily, FontWeight};
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use std::cell::RefCell;
use std::collections::HashMap;

/// Cache key for measurement results.
/// Uses f32::to_bits() for hashable floats.
#[derive(Hash, Eq, PartialEq, Clone)]
struct MeasureCacheKey {
    text: String,
    font_size_bits: u32,
    font_family: FontFamily,
    font_weight: FontWeight,
    max_width_bits: Option<u32>,
}

pub struct TextMeasurer {
    font_system: FontSystem,
    measure_cache: HashMap<MeasureCacheKey, Size>,
}

impl TextMeasurer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            measure_cache: HashMap::new(),
        }
    }

    pub fn measure(&mut self, text: &str, font_size: f32, max_width: Option<f32>) -> Size {
        self.measure_styled(
            text,
            font_size,
            max_width,
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    pub fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> Size {
        // Build cache key
        let cache_key = MeasureCacheKey {
            text: text.to_string(),
            font_size_bits: font_size.to_bits(),
            font_family: font_family.clone(),
            font_weight,
            max_width_bits: max_width.map(|w| w.to_bits()),
        };

        // Check cache first
        if let Some(&cached_size) = self.measure_cache.get(&cache_key) {
            return cached_size;
        }

        // Measure text
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new()
                .family(font_family.to_cosmic())
                .weight(font_weight.to_cosmic()),
            Shaping::Basic,
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

        let size = Size::new(width, height);

        // Cache the result
        self.measure_cache.insert(cache_key, size);

        size
    }

    /// Measure text width up to a specific character index.
    /// This is useful for cursor positioning in text input widgets.
    pub fn measure_to_char(&mut self, text: &str, font_size: f32, char_index: usize) -> f32 {
        self.measure_to_char_styled(
            text,
            font_size,
            char_index,
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Measure text width up to a specific character index with font styling.
    pub fn measure_to_char_styled(
        &mut self,
        text: &str,
        font_size: f32,
        char_index: usize,
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> f32 {
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
        self.measure_styled(prefix, font_size, None, font_family, font_weight)
            .width
    }

    /// Find the character index from an x-coordinate using binary search.
    /// This is useful for click-to-position in text input widgets.
    pub fn char_from_x(&mut self, text: &str, font_size: f32, x: f32) -> usize {
        self.char_from_x_styled(
            text,
            font_size,
            x,
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Find the character index from an x-coordinate with font styling.
    pub fn char_from_x_styled(
        &mut self,
        text: &str,
        font_size: f32,
        x: f32,
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> usize {
        if text.is_empty() || x <= 0.0 {
            return 0;
        }

        let char_count = text.chars().count();
        let total_width = self
            .measure_styled(text, font_size, None, font_family, font_weight)
            .width;
        if x >= total_width {
            return char_count;
        }

        // Binary search for the character position
        let mut left = 0;
        let mut right = char_count;

        while left < right {
            let mid = (left + right) / 2;
            let width = self.measure_to_char_styled(text, font_size, mid, font_family, font_weight);
            if width < x {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // Check if click is closer to previous character
        if left > 0 {
            let prev_width =
                self.measure_to_char_styled(text, font_size, left - 1, font_family, font_weight);
            let curr_width =
                self.measure_to_char_styled(text, font_size, left, font_family, font_weight);
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

/// Measure text dimensions with specified font family and weight
pub fn measure_text_styled(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> Size {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.measure_styled(text, font_size, max_width, font_family, font_weight))
}

/// Measure text width up to a specific character index (for cursor positioning)
pub fn measure_text_to_char(text: &str, font_size: f32, char_index: usize) -> f32 {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure_to_char(text, font_size, char_index))
}

/// Measure text width up to a character index with font styling
pub fn measure_text_to_char_styled(
    text: &str,
    font_size: f32,
    char_index: usize,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> f32 {
    TEXT_MEASURER.with_borrow_mut(|m| {
        m.measure_to_char_styled(text, font_size, char_index, font_family, font_weight)
    })
}

/// Find the character index from an x-coordinate (for click-to-position)
pub fn char_index_from_x(text: &str, font_size: f32, x: f32) -> usize {
    TEXT_MEASURER.with_borrow_mut(|m| m.char_from_x(text, font_size, x))
}

/// Find character index from x-coordinate with font styling
pub fn char_index_from_x_styled(
    text: &str,
    font_size: f32,
    x: f32,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> usize {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.char_from_x_styled(text, font_size, x, font_family, font_weight))
}
