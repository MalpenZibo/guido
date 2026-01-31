//! Image widget for displaying raster and SVG images.
//!
//! Supports PNG, JPEG, GIF, WebP raster formats and SVG vector graphics.
//! Images compose with container transforms (rotate, scale, translate).

use std::path::PathBuf;
use std::sync::Arc;

use crate::layout::{Constraints, Size};
use crate::reactive::{
    IntoMaybeDyn, MaybeDyn, WidgetId, finish_layout_tracking, register_relayout_boundary,
    start_layout_tracking,
};
use crate::renderer::PaintContext;
#[cfg(feature = "renderer_v2")]
use crate::renderer_v2::PaintContextV2;

use super::widget::{EventResponse, Rect, Widget};

/// Source for an image - can be a file path or in-memory bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    /// Raster image from a file path (PNG, JPEG, GIF, WebP)
    Path(PathBuf),
    /// Raster image from in-memory bytes
    Bytes(Arc<[u8]>),
    /// SVG from a file path
    SvgPath(PathBuf),
    /// SVG from in-memory bytes
    SvgBytes(Arc<[u8]>),
}

impl ImageSource {
    /// Check if this is an SVG source
    pub fn is_svg(&self) -> bool {
        matches!(self, ImageSource::SvgPath(_) | ImageSource::SvgBytes(_))
    }
}

impl From<&str> for ImageSource {
    fn from(path: &str) -> Self {
        let path = PathBuf::from(path);
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            ImageSource::SvgPath(path)
        } else {
            ImageSource::Path(path)
        }
    }
}

impl From<String> for ImageSource {
    fn from(path: String) -> Self {
        ImageSource::from(path.as_str())
    }
}

impl From<PathBuf> for ImageSource {
    fn from(path: PathBuf) -> Self {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
        {
            ImageSource::SvgPath(path)
        } else {
            ImageSource::Path(path)
        }
    }
}

impl IntoMaybeDyn<ImageSource> for ImageSource {
    fn into_maybe_dyn(self) -> MaybeDyn<ImageSource> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<ImageSource> for &str {
    fn into_maybe_dyn(self) -> MaybeDyn<ImageSource> {
        MaybeDyn::Static(ImageSource::from(self))
    }
}

impl IntoMaybeDyn<ImageSource> for String {
    fn into_maybe_dyn(self) -> MaybeDyn<ImageSource> {
        MaybeDyn::Static(ImageSource::from(self))
    }
}

impl IntoMaybeDyn<ImageSource> for PathBuf {
    fn into_maybe_dyn(self) -> MaybeDyn<ImageSource> {
        MaybeDyn::Static(ImageSource::from(self))
    }
}

/// How the image content should fit within its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ContentFit {
    /// Scale to fit within bounds while preserving aspect ratio.
    /// May leave empty space (letterboxing).
    #[default]
    Contain,
    /// Scale to cover bounds while preserving aspect ratio.
    /// May crop the image.
    Cover,
    /// Stretch to exactly fill bounds, ignoring aspect ratio.
    Fill,
    /// Use the image's intrinsic size, ignoring widget bounds.
    None,
}

/// Image widget for displaying raster and SVG images.
pub struct Image {
    widget_id: WidgetId,
    source: MaybeDyn<ImageSource>,
    width: Option<MaybeDyn<f32>>,
    height: Option<MaybeDyn<f32>>,
    content_fit: ContentFit,
    bounds: Rect,
    /// Cached intrinsic size from the image source
    intrinsic_size: Option<(u32, u32)>,
    /// Cached source for change detection
    cached_source: Option<ImageSource>,
}

impl Image {
    /// Create a new image widget from a source.
    pub fn new(source: impl IntoMaybeDyn<ImageSource>) -> Self {
        Self {
            widget_id: WidgetId::next(),
            source: source.into_maybe_dyn(),
            width: None,
            height: None,
            content_fit: ContentFit::default(),
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            intrinsic_size: None,
            cached_source: None,
        }
    }

    /// Set a fixed width for the image.
    pub fn width(mut self, width: impl IntoMaybeDyn<f32>) -> Self {
        self.width = Some(width.into_maybe_dyn());
        self
    }

    /// Set a fixed height for the image.
    pub fn height(mut self, height: impl IntoMaybeDyn<f32>) -> Self {
        self.height = Some(height.into_maybe_dyn());
        self
    }

    /// Set the content fit mode.
    pub fn content_fit(mut self, fit: ContentFit) -> Self {
        self.content_fit = fit;
        self
    }

    /// Get the current intrinsic size if known.
    pub fn intrinsic_size(&self) -> Option<(u32, u32)> {
        self.intrinsic_size
    }

    /// Calculate the display size based on intrinsic size, explicit dimensions, and fit mode.
    fn calculate_size(&self, constraints: &Constraints) -> Size {
        let explicit_width = self.width.as_ref().map(|w| w.get());
        let explicit_height = self.height.as_ref().map(|h| h.get());

        // If we have both explicit dimensions, use them
        if let (Some(w), Some(h)) = (explicit_width, explicit_height) {
            return Size::new(
                w.max(constraints.min_width).min(constraints.max_width),
                h.max(constraints.min_height).min(constraints.max_height),
            );
        }

        // Get intrinsic size or use a default
        let (intrinsic_w, intrinsic_h) = self.intrinsic_size.unwrap_or((100, 100));
        let intrinsic_w = intrinsic_w as f32;
        let intrinsic_h = intrinsic_h as f32;
        let aspect = intrinsic_w / intrinsic_h;

        match self.content_fit {
            ContentFit::None => {
                // Use intrinsic size directly
                Size::new(
                    intrinsic_w.max(constraints.min_width),
                    intrinsic_h.max(constraints.min_height),
                )
            }
            ContentFit::Fill => {
                // Use explicit dimensions or fill available space
                let width = explicit_width
                    .unwrap_or(constraints.max_width)
                    .max(constraints.min_width)
                    .min(constraints.max_width);
                let height = explicit_height
                    .unwrap_or(constraints.max_height)
                    .max(constraints.min_height)
                    .min(constraints.max_height);
                Size::new(width, height)
            }
            ContentFit::Contain | ContentFit::Cover => {
                // If one dimension is explicit, calculate the other from aspect ratio
                let (target_w, target_h) = match (explicit_width, explicit_height) {
                    (Some(w), None) => (w, w / aspect),
                    (None, Some(h)) => (h * aspect, h),
                    (None, None) => {
                        // Fit within constraints
                        let max_w = constraints.max_width;
                        let max_h = constraints.max_height;
                        if max_w / max_h > aspect {
                            // Height is the limiting factor
                            (max_h * aspect, max_h)
                        } else {
                            // Width is the limiting factor
                            (max_w, max_w / aspect)
                        }
                    }
                    (Some(w), Some(h)) => (w, h),
                };

                Size::new(
                    target_w
                        .max(constraints.min_width)
                        .min(constraints.max_width),
                    target_h
                        .max(constraints.min_height)
                        .min(constraints.max_height),
                )
            }
        }
    }
}

impl Widget for Image {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Start layout tracking for dependency registration
        start_layout_tracking(self.widget_id);

        // Images are never relayout boundaries
        register_relayout_boundary(self.widget_id, false);

        // Read source (registers layout dependencies if reactive)
        let current_source = self.source.get();

        // Load intrinsic size if not cached or source changed
        let source_changed = self
            .cached_source
            .as_ref()
            .map(|cached| cached != &current_source)
            .unwrap_or(true);

        if source_changed || self.intrinsic_size.is_none() {
            self.intrinsic_size = crate::image_metadata::get_intrinsic_size(&current_source);
        }

        // Update cached source
        self.cached_source = Some(current_source);

        let size = self.calculate_size(&constraints);
        self.bounds.width = size.width;
        self.bounds.height = size.height;

        // Finish layout tracking
        finish_layout_tracking();

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        if let Some(ref source) = self.cached_source {
            ctx.draw_image(source.clone(), self.bounds, self.content_fit);
        }
    }

    #[cfg(feature = "renderer_v2")]
    fn paint_v2(&self, ctx: &mut PaintContextV2) {
        if let Some(ref source) = self.cached_source {
            ctx.draw_image(source.clone(), self.bounds, self.content_fit);
        }
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
}

/// Create an image widget from a source.
///
/// # Examples
///
/// ```ignore
/// // From file path (auto-detects SVG)
/// image("./icon.png")
/// image("./logo.svg")
///
/// // With explicit dimensions
/// image("./icon.png")
///     .width(32.0)
///     .height(32.0)
///
/// // With content fit mode
/// image("./photo.jpg")
///     .width(200.0)
///     .height(150.0)
///     .content_fit(ContentFit::Cover)
///
/// // From ImageSource
/// image(ImageSource::SvgBytes(svg_data.into()))
/// ```
pub fn image(source: impl IntoMaybeDyn<ImageSource>) -> Image {
    Image::new(source)
}
