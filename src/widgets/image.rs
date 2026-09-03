//! Image widget for displaying raster and SVG images.
//!
//! Supports PNG, JPEG, GIF, WebP raster formats and SVG vector graphics.
//! Images compose with container transforms (rotate, scale, translate).

use std::path::PathBuf;
use std::sync::Arc;

use crate::jobs::JobType;
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoSignal, OptionSignalExt, Signal, with_signal_tracking};
use crate::renderer::PaintContext;
use crate::tree::{Tree, WidgetId};

use super::widget::{Rect, Widget};

/// Source for an image - can be a file path or in-memory bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    /// Raster image from a file path (PNG, JPEG, GIF, WebP)
    Path(PathBuf),
    /// Raster image from in-memory bytes
    Bytes(Arc<[u8]>),
    /// Raw pre-decoded RGBA8 pixels (row-major, `width * height * 4` bytes).
    ///
    /// Skips the decode step entirely — for pixel data that never existed in
    /// an encoded format, like tray icon pixmaps or album art from D-Bus.
    Rgba {
        width: u32,
        height: u32,
        pixels: Arc<[u8]>,
    },
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
///
/// The image carries the pixels and how they map into the box it is given;
/// the box itself comes from the enclosing container, like every other size in
/// guido:
///
/// ```ignore
/// container()
///     .width(fill())
///     .height(fill())
///     .child(image(source).content_fit(ContentFit::Cover))
/// ```
pub struct Image {
    source: Signal<ImageSource>,
    content_fit: Option<Signal<ContentFit>>,
    /// Read under layout tracking, and used again by paint on the same frame.
    cached_content_fit: ContentFit,
    /// Cached intrinsic size from the image source
    intrinsic_size: Option<(u32, u32)>,
    /// Cached source for change detection
    cached_source: Option<ImageSource>,
}

impl Image {
    /// Create a new image widget from a source.
    pub fn new<M>(source: impl IntoSignal<ImageSource, M>) -> Self {
        Self {
            source: source.into_signal(),
            content_fit: None,
            cached_content_fit: ContentFit::default(),
            intrinsic_size: None,
            cached_source: None,
        }
    }

    /// Set the content fit mode.
    pub fn content_fit<M>(mut self, fit: impl IntoSignal<ContentFit, M>) -> Self {
        self.content_fit = Some(fit.into_signal());
        self
    }

    /// Get the current intrinsic size if known.
    pub fn intrinsic_size(&self) -> Option<(u32, u32)> {
        self.intrinsic_size
    }

    /// The box this image occupies, from the constraints and the fit mode.
    ///
    /// The fit modes disagree only about what to do with room that is offered
    /// but not demanded:
    ///
    /// - `Fill` and `Cover` take all of it. They differ in how the pixels land
    ///   inside — stretched or cropped — which is the renderer's job, not this
    ///   one's. `Cover` sizing itself to the aspect ratio was the old bug: an
    ///   image asked to cover its box would letterbox instead, because it had
    ///   shrunk the box to the shape it was trying to cover.
    /// - `Contain` promises the whole image is visible, so with room to spare
    ///   it takes only the largest aspect-preserving rect that fits, leaving
    ///   the letterboxing to the parent's alignment rather than painting it.
    /// - `None` is the intrinsic pixels, clamped into whatever room there is.
    ///
    /// An axis whose constraint is unbounded has no room to take, so every
    /// mode falls back to the intrinsic size there.
    fn calculate_size(&self, constraints: &Constraints) -> Size {
        let (intrinsic_w, intrinsic_h) = self.intrinsic_size.unwrap_or((100, 100));
        let intrinsic_w = intrinsic_w as f32;
        let intrinsic_h = intrinsic_h as f32;
        let aspect = intrinsic_w / intrinsic_h;

        let offered_w = if constraints.max_width.is_finite() {
            constraints.max_width
        } else {
            intrinsic_w
        };
        let offered_h = if constraints.max_height.is_finite() {
            constraints.max_height
        } else {
            intrinsic_h
        };

        let size = match self.cached_content_fit {
            ContentFit::None => Size::new(intrinsic_w, intrinsic_h),
            ContentFit::Fill | ContentFit::Cover => Size::new(offered_w, offered_h),
            ContentFit::Contain => {
                // A tight axis is not room on offer, it is a decision already
                // made; the other axis follows from the aspect ratio.
                let tight_w = constraints.min_width == constraints.max_width;
                let tight_h = constraints.min_height == constraints.max_height;
                match (tight_w, tight_h) {
                    (true, true) => Size::new(offered_w, offered_h),
                    (true, false) => Size::new(offered_w, offered_w / aspect),
                    (false, true) => Size::new(offered_h * aspect, offered_h),
                    // Largest rect of this aspect that fits in what is offered.
                    (false, false) => {
                        if offered_w / offered_h > aspect {
                            Size::new(offered_h * aspect, offered_h)
                        } else {
                            Size::new(offered_w, offered_w / aspect)
                        }
                    }
                }
            }
        };

        constraints.constrain(size)
    }
}

impl Widget for Image {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        // Images are never relayout boundaries
        tree.set_relayout_boundary(id, false);

        // Read the source with signal tracking so a change triggers re-layout
        // Both under the same tracking: the fit decides the measured size, so a
        // write to it has to re-run layout exactly as a new source does.
        let (current_source, fit) = with_signal_tracking(id, JobType::Layout, || {
            (
                self.source.get(),
                self.content_fit.get_or(ContentFit::default()),
            )
        });
        self.cached_content_fit = fit;

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

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // Clear needs_layout flag since layout is complete
        tree.clear_needs_layout(id);

        size
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        if let Some(ref source) = self.cached_source {
            let size = tree.cached_size(id).unwrap_or_default();
            let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
            ctx.draw_image(source.clone(), local_bounds, self.cached_content_fit);
        }
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
pub fn image<M>(source: impl IntoSignal<ImageSource, M>) -> Image {
    Image::new(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs;
    use crate::reactive::create_signal;

    fn measured(tree: &mut Tree, root: WidgetId, c: Constraints) -> Size {
        jobs::pump_and_layout(tree, root, c).expect("the root is registered")
    }

    /// A square SVG, inline, so the test needs no asset on disk and no decoder.
    fn svg(side: u32) -> ImageSource {
        let src = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}"><rect width="{side}" height="{side}" fill="red"/></svg>"#
        );
        ImageSource::SvgBytes(src.into_bytes().into())
    }

    /// An image draws itself, at the box it was given and the fit it declares.
    ///
    /// `paint` had nothing watching it at all: emptying the whole method was
    /// invisible to every test, because the one that asks about `content_fit`
    /// asks the *measured size* and never looks at what was drawn.
    #[test]
    fn an_image_draws_its_source_into_its_own_box() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(Image::new(svg(20)).content_fit(ContentFit::Fill)));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        measured(&mut tree, root, Constraints::new(0.0, 0.0, 40.0, 40.0));

        let mut node = crate::renderer::RenderNode::new(root.as_u64());
        tree.with_widget_mut(root, |w, id, t| {
            let mut ctx = crate::renderer::PaintContext::new(&mut node);
            w.paint(t, id, &mut ctx);
        });

        let drawn = node
            .commands
            .iter()
            .find_map(|cmd| match &**cmd {
                crate::renderer::DrawCommand::Image {
                    rect, content_fit, ..
                } => Some((*rect, *content_fit)),
                _ => None,
            })
            .expect("an image command");

        assert_eq!(
            (drawn.0.width, drawn.0.height),
            (40.0, 40.0),
            "the image is drawn into the box layout gave it, got {:?}",
            drawn.0
        );
        assert_eq!(drawn.1, ContentFit::Fill, "and with the fit it declares");
    }

    /// How an image fills its box is a value it can be told.
    ///
    /// Read under the same tracking as the source, because the fit decides the
    /// measured size — `ContentFit::None` reports the intrinsic size whatever
    /// is on offer, `Fill` reports what is offered — so a write has to re-run
    /// layout rather than only repaint. Those two are the pair asserted here:
    /// they disagree by construction whenever the offer is not the intrinsic
    /// size, which needs no image file to be true.
    #[test]
    fn content_fit_answers_to_a_signal() {
        let fills = create_signal(true);
        let fit = move || {
            if fills.get() {
                ContentFit::Fill
            } else {
                ContentFit::None
            }
        };

        // Under a container, and measured from the container, because that is
        // what makes this a test of *reactivity* rather than of plumbing: a
        // container takes an unchanged-constraints early-out and never asks its
        // children again, so the second pass only reaches the image if the
        // write marked it. An image laid out directly re-measures on every call
        // and would pass with the fit read untracked.
        let mut tree = Tree::new();
        let root = tree.register(Box::new(crate::widgets::container().child(
            Image::new(ImageSource::Path("does-not-exist.png".into())).content_fit(fit),
        )));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let offered = Constraints::new(0.0, 0.0, 200.0, 120.0);
        let filled = measured(&mut tree, root, offered);

        fills.set(false);
        let intrinsic = measured(&mut tree, root, offered);

        assert_ne!(
            filled, intrinsic,
            "the fit was read once: both passes measured {filled:?}"
        );
        assert_eq!(
            filled,
            Size::new(200.0, 120.0),
            "Fill takes what is offered"
        );
    }
}
