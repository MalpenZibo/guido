use crate::default_font_family;
use crate::jobs::JobType;
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoMaybeDyn, MaybeDyn, with_signal_tracking};
use crate::renderer::{PaintContext, measure_text_styled};
use crate::tree::{Tree, WidgetId};

use super::font::{FontFamily, FontWeight};
use super::widget::{Color, EventResponse, Rect, Widget};

pub struct Text {
    widget_id: WidgetId,
    content: MaybeDyn<String>,
    color: MaybeDyn<Color>,
    font_size: MaybeDyn<f32>,
    font_family: MaybeDyn<FontFamily>,
    font_weight: MaybeDyn<FontWeight>,
    /// If true, text won't wrap and will be clipped by parent container
    nowrap: bool,
    /// Cached values for painting (avoid re-reading signals)
    cached_text: String,
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,
    bounds: Rect,
}

impl Text {
    pub fn new(content: impl IntoMaybeDyn<String>) -> Self {
        let content = content.into_maybe_dyn();
        // Don't read content during widget creation - this would register layout dependencies
        // with the wrong widget (the parent container that's currently being laid out).
        // The cached_text will be populated during the first layout via refresh().
        let default_family = default_font_family();
        Self {
            // widget_id will be assigned by Tree::register()
            widget_id: WidgetId::placeholder(),
            content,
            color: MaybeDyn::Static(Color::WHITE),
            font_size: MaybeDyn::Static(14.0),
            font_family: MaybeDyn::Static(default_family.clone()),
            font_weight: MaybeDyn::Static(FontWeight::NORMAL),
            nowrap: false,
            cached_text: String::new(), // Will be set during first layout
            cached_font_size: 14.0,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
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

    /// Set the font family.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text("Hello").font_family(FontFamily::Monospace)
    /// text("Hello").font_family(FontFamily::Name("Inter".into()))
    /// ```
    pub fn font_family(mut self, family: impl IntoMaybeDyn<FontFamily>) -> Self {
        self.font_family = family.into_maybe_dyn();
        self
    }

    /// Set the font weight.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text("Hello").font_weight(FontWeight::BOLD)
    /// text("Hello").font_weight(FontWeight(600))
    /// ```
    pub fn font_weight(mut self, weight: impl IntoMaybeDyn<FontWeight>) -> Self {
        self.font_weight = weight.into_maybe_dyn();
        self
    }

    /// Shorthand for bold text (FontWeight::BOLD).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text("Hello").bold()
    /// ```
    pub fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    /// Shorthand for monospace font (FontFamily::Monospace).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// text("Hello").mono()
    /// ```
    pub fn mono(self) -> Self {
        self.font_family(FontFamily::Monospace)
    }

    /// Prevent text from wrapping. Text will be clipped by parent container.
    /// Use this for text inside animated containers to prevent re-wrapping during animation.
    pub fn nowrap(mut self) -> Self {
        self.nowrap = true;
        self
    }

    /// Refresh cached values from reactive properties.
    /// Uses signal tracking to register layout dependencies so the widget
    /// is re-laid out when any of these signals change.
    fn refresh(&mut self) {
        with_signal_tracking(self.widget_id, JobType::Layout, || {
            self.cached_text = self.content.get();
            self.cached_font_size = self.font_size.get();
            self.cached_font_family = self.font_family.get();
            self.cached_font_weight = self.font_weight.get();
        });
    }
}

impl Widget for Text {
    fn layout(&mut self, tree: &mut Tree, constraints: Constraints) -> Size {
        // Text widgets are never relayout boundaries
        tree.set_relayout_boundary(self.widget_id, false);

        // Refresh cached values from reactive properties
        // This reads signals and registers layout dependencies
        self.refresh();

        // Determine the effective max_width for measurement
        // If nowrap is true, don't pass max_width so text won't wrap
        let max_width = if self.nowrap {
            None
        } else if constraints.max_width.is_finite() {
            Some(constraints.max_width)
        } else {
            None
        };

        // Measure text (TextMeasurer caches results internally)
        let measured = measure_text_styled(
            &self.cached_text,
            self.cached_font_size,
            max_width,
            &self.cached_font_family,
            self.cached_font_weight,
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

        // Cache constraints and size for partial layout
        tree.cache_layout(self.widget_id, constraints, size);

        // Clear dirty flag since layout is complete
        tree.clear_dirty(self.widget_id);

        size
    }

    fn paint(&self, _tree: &Tree, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let local_bounds = Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height);
        let color = self.color.get();
        ctx.draw_text_styled(
            &self.cached_text,
            local_bounds,
            color,
            self.cached_font_size,
            self.cached_font_family.clone(),
            self.cached_font_weight,
        );
    }

    fn event(&mut self, _tree: &mut Tree, _event: &super::widget::Event) -> EventResponse {
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

    fn set_id(&mut self, id: WidgetId) {
        self.widget_id = id;
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
