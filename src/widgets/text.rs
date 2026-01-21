use crate::layout::{Constraints, Size};
use crate::reactive::{ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::{measure_text, PaintContext};

use super::widget::{Color, EventResponse, Rect, Widget};

pub struct Text {
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,
    content: MaybeDyn<String>,
    color: MaybeDyn<Color>,
    font_size: MaybeDyn<f32>,
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

    fn refresh(&mut self) {
        self.cached_text = self.content.get();
        self.cached_font_size = self.font_size.get();
    }
}

impl Widget for Text {
    fn layout(&mut self, constraints: Constraints) -> Size {
        self.refresh();

        // Use actual font metrics for accurate measurement
        let measured = measure_text(
            &self.cached_text,
            self.cached_font_size,
            Some(constraints.max_width),
        );

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

    fn mark_dirty(&mut self, flags: ChangeFlags) {
        self.dirty_flags |= flags;
    }

    fn needs_layout(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_LAYOUT)
    }

    fn needs_paint(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_PAINT)
    }

    fn clear_dirty(&mut self) {
        self.dirty_flags = ChangeFlags::empty();
    }
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
