use crate::default_font_family;
use crate::jobs::JobType;
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoSignal, OptionSignalExt, Signal, with_signal_tracking};
use crate::renderer::{PaintContext, measure_text_styled};
use crate::tree::{Tree, WidgetId};

use super::font::{FontFamily, FontWeight};
use super::widget::{Color, EventResponse, Rect, Widget};

pub struct Text {
    content: Signal<String>,
    color: Option<Signal<Color>>,
    font_size: Option<Signal<f32>>,
    font_family: Option<Signal<FontFamily>>,
    font_weight: Option<Signal<FontWeight>>,
    /// If true, text won't wrap and will be clipped by parent container
    nowrap: bool,
    /// Cached values for painting (avoid re-reading signals)
    cached_text: String,
    cached_font_size: f32,
    cached_font_family: FontFamily,
    cached_font_weight: FontWeight,
}

impl Text {
    pub fn new<M>(content: impl IntoSignal<String, M>) -> Self {
        let content = content.into_signal();
        // Don't read content during widget creation - this would register layout dependencies
        // with the wrong widget (the parent container that's currently being laid out).
        // The cached_text will be populated during the first layout via refresh().
        let default_family = default_font_family();
        Self {
            content,
            color: None,
            font_size: None,
            font_family: None,
            font_weight: None,
            nowrap: false,
            cached_text: String::new(), // Will be set during first layout
            cached_font_size: 14.0,
            cached_font_family: default_family,
            cached_font_weight: FontWeight::NORMAL,
        }
    }

    pub fn color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.color = Some(color.into_signal());
        self
    }

    pub fn font_size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.font_size = Some(size.into_signal());
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
    pub fn font_family<M>(mut self, family: impl IntoSignal<FontFamily, M>) -> Self {
        self.font_family = Some(family.into_signal());
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
    pub fn font_weight<M>(mut self, weight: impl IntoSignal<FontWeight, M>) -> Self {
        self.font_weight = Some(weight.into_signal());
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
    fn refresh(&mut self, id: WidgetId) {
        with_signal_tracking(id, JobType::Layout, || {
            self.cached_text = self.content.get();
            self.cached_font_size = self.font_size.get_or(14.0);
            self.cached_font_family = self.font_family.get_or_else(default_font_family);
            self.cached_font_weight = self.font_weight.get_or(FontWeight::NORMAL);
        });
    }
}

impl Widget for Text {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        // Text widgets are never relayout boundaries
        tree.set_relayout_boundary(id, false);

        // Refresh cached values from reactive properties
        // This reads signals and registers layout dependencies
        self.refresh(id);

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

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // Clear needs_layout flag since layout is complete
        tree.clear_needs_layout(id);

        size
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        let size = tree.cached_size(id).unwrap_or_default();
        let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
        // Read color with tracking so signal changes trigger repaint
        let color = with_signal_tracking(id, JobType::Paint, || self.color.get_or(Color::WHITE));
        ctx.draw_text_styled(
            &self.cached_text,
            local_bounds,
            color,
            self.cached_font_size,
            self.cached_font_family.clone(),
            self.cached_font_weight,
        );
    }

    fn event(
        &mut self,
        _tree: &mut Tree,
        _id: WidgetId,
        _event: &super::widget::Event,
    ) -> EventResponse {
        EventResponse::Ignored
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
pub fn text<M>(content: impl IntoSignal<String, M>) -> Text {
    Text::new(content)
}
