use crate::layout::{Constraints, Size};
use crate::reactive::{ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::{PaintContext, measure_text};

use super::impl_dirty_flags;
use super::widget::{Color, EventResponse, Rect, Widget};

pub struct Text {
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,
    content: MaybeDyn<String>,
    color: MaybeDyn<Color>,
    font_size: MaybeDyn<f32>,
    /// If true, text won't wrap and will be clipped by parent container
    nowrap: bool,
    cached_text: String,
    cached_font_size: f32,
    bounds: Rect,
}

impl Text {
    pub fn new(content: impl IntoMaybeDyn<String>) -> Self {
        let content = content.into_maybe_dyn();
        let cached_text = content.get();
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            content,
            color: MaybeDyn::Static(Color::WHITE),
            font_size: MaybeDyn::Static(14.0),
            nowrap: false,
            cached_text,
            cached_font_size: 14.0,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
        }
    }

    pub fn color(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.color = color.into_maybe_dyn();
        self
    }

    pub fn font_size(mut self, size: impl IntoMaybeDyn<f32>) -> Self {
        self.font_size = size.into_maybe_dyn();
        self
    }

    /// Prevent text from wrapping. Text will be clipped by parent container.
    /// Use this for text inside animated containers to prevent re-wrapping during animation.
    pub fn nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Refresh cached values only when they've changed.
    /// Returns true if any value changed (requiring re-measurement).
    fn refresh(&mut self) -> bool {
        let new_text = self.content.get();
        let new_font_size = self.font_size.get();

        let text_changed = new_text != self.cached_text;
        let font_changed = (new_font_size - self.cached_font_size).abs() > f32::EPSILON;

        if text_changed {
            self.cached_text = new_text;
        }
        if font_changed {
            self.cached_font_size = new_font_size;
        }

        text_changed || font_changed
    }
}

impl Widget for Text {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let content_changed = self.refresh();

        // Skip re-measurement if nothing changed and we don't need layout
        if !content_changed && !self.needs_layout() && self.bounds.width > 0.0 {
            return Size::new(self.bounds.width, self.bounds.height);
        }

        // Use actual font metrics for accurate measurement
        // If nowrap is true, don't pass max_width so text won't wrap
        let max_width = if self.nowrap {
            None
        } else {
            Some(constraints.max_width)
        };
        let measured = measure_text(&self.cached_text, self.cached_font_size, max_width);

        let size = Size::new(
            measured
                .width
                .max(constraints.min_width)
                .min(constraints.max_width),
            measured
                .height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let color = self.color.get();
        ctx.draw_text(&self.cached_text, self.bounds, color, self.cached_font_size);
    }

    fn event(&mut self, _event: &super::widget::Event) -> EventResponse {
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

/// Create a text widget
///
/// Accepts static strings, closures, or signals:
/// ```ignore
/// text("Hello")  // static string
/// text(move || format!("Count: {}", count.get()))  // reactive closure
/// text(my_signal)  // reactive signal
/// ```
pub fn text(content: impl IntoMaybeDyn<String>) -> Text {
    Text::new(content)
}
