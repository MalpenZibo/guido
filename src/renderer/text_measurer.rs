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
}

thread_local! {
    static TEXT_MEASURER: RefCell<TextMeasurer> = RefCell::new(TextMeasurer::new());
}

/// Measure text dimensions using the font system
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> Size {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure(text, font_size, max_width))
}
