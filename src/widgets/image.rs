//! Image widget for displaying raster and SVG images.
//!
//! Supports PNG, JPEG, GIF, WebP raster formats and SVG vector graphics.
//! Images compose with container transforms (rotate, scale, translate).

use std::path::PathBuf;
use std::sync::Arc;

use crate::animation::IntoAnimated;
use crate::finite::AllFinite;
use crate::image_decode::DecodeHandle;
use crate::jobs::{JobRequest, RequiredJob, request_animation_frame, request_job};
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoSignal, Prop, RwSignal, Signal, create_memo, create_signal};
use crate::renderer::PaintContext;
use crate::tree::{LayoutCtx, Tree, WidgetId};

use super::container::{AnimationState, declare};
use super::state_layer::{StateWhen, Stateful, notice_own_hover, state_is_active};
use super::widget::{Color, Event, EventResponse, Rect, Widget};

/// Source for an image - can be a file path or in-memory bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum ImageSource {
    /// Raster image from a file path (PNG, JPEG, GIF, WebP)
    Path(PathBuf),
    /// Raster image from in-memory bytes
    Bytes(Arc<[u8]>),
    /// Raw pre-decoded RGBA8 pixels (row-major, `width * height * 4` bytes),
    /// with straight alpha — not premultiplied, as an image decoder gives
    /// them. Premultiplied pixels would have their edges darkened twice.
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

/// What an image looks like while its control is in a state — see
/// [`Stateful`].
///
/// ```no_run
/// # use guido::prelude::*;
/// # struct Theme { text: Color, accent: Color, dim: Color }
/// # let theme = Theme { text: Color::WHITE, accent: Color::rgb(0.4, 0.6, 1.0), dim: Color::rgb(0.4, 0.4, 0.4) };
/// image("./power.svg")
///     .tint(theme.text)
///     .when_hovered(|s| s.tint(theme.accent))
///     .when_disabled(|s| s.tint(theme.dim));
/// ```
#[derive(Clone, Copy, Default)]
pub struct ImageStyle {
    /// The tint to draw in while the state holds.
    pub tint: Prop<Color>,
}

impl ImageStyle {
    /// Draw the image in `color` while the state holds, as
    /// [`Image::tint`] does.
    ///
    /// A value, never a motion: how the tint moves belongs to the declaration
    /// on the image, which this supplies a value for.
    pub fn tint<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.tint = color.into_prop();
        self
    }
}

/// What an image declares beyond what it is: the states it answers and how
/// its tint moves. Boxed and absent together, so an image that declares
/// neither pays one pointer for both.
#[derive(Default)]
struct ImageStates {
    /// Overrides that apply while this image's control is in a state, in
    /// declaration order.
    states: Vec<(StateWhen, ImageStyle)>,
    /// Set only once a pointer state is declared, and read only when no
    /// control encloses this image — the case where it is its own unit.
    own_hover: Option<RwSignal<bool>>,
    /// The tint's motion, when it was declared with one.
    tint: Option<AnimationState<Color>>,
}

/// Image widget for displaying raster and SVG images.
///
/// The image carries the pixels and how they map into the box it is given;
/// the box itself comes from the enclosing container, like every other size in
/// guido:
///
/// ```no_run
/// # use guido::prelude::*;
/// # let source = "./photo.jpg";
/// container()
///     .width(fill())
///     .height(fill())
///     .child(image(source).content_fit(ContentFit::Cover));
/// ```
pub struct Image {
    source: Signal<ImageSource>,
    content_fit: Prop<ContentFit>,
    /// Read by paint alone: a new tint repaints, and lays nothing out again —
    /// unless it moves, when layout reads it too, to point the motion at it.
    tint: Prop<Color>,
    /// Absent for every image that declares no state and no motion, which is
    /// almost every one.
    extras: Option<Box<ImageStates>>,
    /// Read under layout tracking, and used again by paint on the same frame.
    cached_content_fit: ContentFit,
    /// Cached intrinsic size from the image source
    intrinsic_size: Option<(f32, f32)>,
    /// Cached source for change detection
    cached_source: Option<ImageSource>,
    /// The decode cache's entry for the source, held while it is shown so the
    /// decoded pixels go when the last image showing them does. `None` for a
    /// source that needs no decode.
    decode: Option<DecodeHandle>,
}

impl Image {
    /// Create a new image widget from a source.
    pub fn new<M>(source: impl IntoSignal<ImageSource, M>) -> Self {
        Self {
            source: source.into_signal(),
            content_fit: Prop::Unset,
            tint: Prop::Unset,
            extras: None,
            cached_content_fit: ContentFit::default(),
            intrinsic_size: None,
            cached_source: None,
            decode: None,
        }
    }

    /// Set the content fit mode.
    pub fn content_fit<M>(mut self, fit: impl IntoSignal<ContentFit, M>) -> Self {
        self.content_fit = fit.into_prop();
        self
    }

    /// Draw every pixel in `color`, keeping the image's own alpha — a
    /// monochrome icon in whatever colour its surroundings ask for.
    ///
    /// The colour is applied as the image is drawn, from the texture it
    /// already has: a new tint is a repaint, not a new raster, so a hover or a
    /// theme change costs what a font icon's colour change does. The tint's
    /// own alpha fades the image as an opacity would.
    ///
    /// It carries how it moves, as a text's colour does, so a state that
    /// overrides it — `when_hovered(|s| s.tint(..))` — eases under a
    /// transition declared here, and switches without one.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// let hovered = create_signal(false);
    /// container().width(16.0).height(16.0).child(
    ///     image("./battery.svg").tint(move || {
    ///         if hovered.get() { Color::WHITE } else { Color::rgb(0.6, 0.6, 0.6) }
    ///     }),
    /// );
    /// ```
    pub fn tint<M>(mut self, color: impl IntoAnimated<Color, M>) -> Self {
        self.tint = declare(&mut self.extras, color, |e: &mut ImageStates| &mut e.tint);
        self
    }

    /// The tint the declaration and the active states resolve to, nearest
    /// override first, and `None` for an image drawn in its own colours.
    ///
    /// Reads the conditions of the states that declare a tint, so the pass it
    /// is called in follows them: paint, to draw the new tint; layout too, where
    /// the tint moves, to point the motion at it.
    fn resolved_tint(&self, tree: &Tree, id: WidgetId) -> Option<Color> {
        if let Some(extras) = self.extras.as_deref()
            && !extras.states.is_empty()
        {
            let control = tree.nearest_control(id);
            for (when, style) in extras.states.iter().rev() {
                if style.tint.is_set()
                    && state_is_active(id, control.as_ref(), when, extras.own_hover)
                    && let Some(tint) = style.tint.get().filter(|t| t.all_finite())
                {
                    return Some(tint);
                }
            }
        }
        self.tint.get()
    }

    /// Whether the image can be drawn: true once its source is decoded or
    /// rasterized, and from the start for `Rgba`, which needs neither.
    ///
    /// A raster `Path` or `Bytes` source is decoded off the frame, and an SVG
    /// rasterized there, so the first frame lays the box out at the image's
    /// size and draws nothing in it. This is what an application fades it in
    /// with — the image itself does not:
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// let wallpaper = image("./wallpaper.png").content_fit(ContentFit::Cover);
    /// let ready = wallpaper.ready();
    /// container()
    ///     .opacity((move || if ready.get() { 1.0 } else { 0.0 }).transition(200.0))
    ///     .child(wallpaper);
    /// ```
    ///
    /// It follows the source: a new source is not ready until it is decoded.
    /// A source whose decode failed is never ready.
    pub fn ready(&self) -> Signal<bool> {
        let source = self.source;
        // A memo, so the lookup runs when the source or its decode moves and
        // not on every read of whatever fades with it.
        create_memo(move || crate::image_decode::is_ready(&source.get())).into_signal()
    }

    /// The size the source lays out at, once a layout has read it — see
    /// [`get_intrinsic_size`](crate::image_metadata::get_intrinsic_size).
    pub fn intrinsic_size(&self) -> Option<(f32, f32)> {
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
        let (intrinsic_w, intrinsic_h) = self.intrinsic_size.unwrap_or((100.0, 100.0));
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

impl Stateful for Image {
    type Style = ImageStyle;

    fn push_state_style(&mut self, when: StateWhen, style: ImageStyle) {
        let extras = self.extras.get_or_insert_with(Box::default);
        if matches!(when, StateWhen::Hovered) && extras.own_hover.is_none() {
            extras.own_hover = Some(create_signal(false));
        }
        extras.states.push((when, style));
    }
}

impl Widget for Image {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let Some(anim) = self.extras.as_mut().and_then(|e| e.tint.as_mut()) else {
            return false;
        };
        let moved = anim.advance(tree.frame_instant()).is_changed();
        let running = anim.is_animating();
        request_animation_frame(id, moved.then_some(RequiredJob::Paint), running);
        running
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        let id = ctx.id();
        // Images are never relayout boundaries
        ctx.tree().set_relayout_boundary(id, false);

        // Only where the tint moves: reading it here subscribes this layout to
        // it, which an image that merely paints a tint has no reason to pay.
        if self.extras.as_ref().is_some_and(|e| e.tint.is_some())
            && let Some(target) = self.resolved_tint(ctx.tree(), id)
        {
            let now = ctx.tree_ref().frame_instant();
            if let Some(anim) = self.extras.as_mut().and_then(|e| e.tint.as_mut()) {
                anim.retarget(ctx, target, now);
                if anim.is_animating() {
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }
            }
        }

        // Both reads belong to this layout: the fit decides the measured size,
        // so a write to it re-runs the layout exactly as a new source does.
        let (current_source, fit) = (
            self.source.get(),
            self.content_fit.get_or(ContentFit::default()),
        );
        self.cached_content_fit = fit;

        // Load intrinsic size if not cached or source changed
        let source_changed = self
            .cached_source
            .as_ref()
            .map(|cached| cached != &current_source)
            .unwrap_or(true);

        if source_changed {
            // Taken before the old one is let go, so a source that did not
            // really change keeps its entry rather than dropping and remaking
            // it. The header came with the entry; only the pixels are pending.
            self.decode = crate::image_decode::acquire(&current_source);
        }
        if source_changed || self.intrinsic_size.is_none() {
            self.intrinsic_size = match &self.decode {
                Some(decode) => decode.size(),
                None => crate::image_metadata::get_intrinsic_size(&current_source),
            };
        }

        // Update cached source
        self.cached_source = Some(current_source);

        self.calculate_size(&constraints)
    }

    /// Notice the pointer, for an image that is its own unit — see
    /// `notice_own_hover`.
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        let own_hover = self.extras.as_ref().and_then(|e| e.own_hover);
        notice_own_hover(own_hover, tree, id, event)
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let (tree, id) = (ctx.tree(), ctx.id());
        // Draw in LOCAL coordinates (0,0 is widget origin)
        // Parent Container sets position transform
        if let Some(ref source) = self.cached_source {
            let size = tree.cached_size(id).unwrap_or_default();
            let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);
            let tint = match self.extras.as_ref().and_then(|e| e.tint.as_ref()) {
                Some(anim) => Some(anim.displayed()),
                None => self.resolved_tint(tree, id),
            };
            match &self.decode {
                // The held entry's own signal: a paint does no lookup.
                Some(decode) => {
                    let (state, decoded) = decode.state();
                    if crate::image_decode::paints(state, &decoded) {
                        ctx.push_image(
                            source.clone(),
                            Some(decoded),
                            local_bounds,
                            self.cached_content_fit,
                            tint,
                        );
                    }
                }
                None => ctx.draw_image(source.clone(), local_bounds, self.cached_content_fit, tint),
            }
        }
    }
}

/// Create an image widget from a source.
///
/// # Examples
///
/// ```no_run
/// # use guido::prelude::*;
/// # let svg_data: Vec<u8> = Vec::new();
/// // From file path (auto-detects SVG)
/// image("./icon.png");
/// image("./logo.svg");
///
/// // Sized by the container it sits in: an image has no width of its own
/// container().width(32.0).height(32.0).child(image("./icon.png"));
///
/// // With content fit mode, in a box that gives it its size
/// container()
///     .width(200.0)
///     .height(150.0)
///     .child(image("./photo.jpg").content_fit(ContentFit::Cover));
///
/// // From ImageSource
/// image(ImageSource::SvgBytes(svg_data.into()));
/// ```
pub fn image<M>(source: impl IntoSignal<ImageSource, M>) -> Image {
    Image::new(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs;
    use crate::reactive::create_signal;
    use crate::tree::{Tree, WidgetId};

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
        tree.paint_widget(root, &mut node, 1.0);

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

    /// Every image command a frame drew, as its tint, at a named instant.
    fn tints_at(tree: &mut Tree, root: WidgetId, at: std::time::Instant) -> Vec<Option<Color>> {
        tree.set_frame_instant(Some(at));
        measured(tree, root, Constraints::new(0.0, 0.0, 200.0, 100.0));
        let mut node = crate::renderer::RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node, 1.0);
        tree.set_frame_instant(None);

        fn walk(node: &crate::renderer::RenderNode, out: &mut Vec<Option<Color>>) {
            for cmd in &node.commands {
                if let crate::renderer::DrawCommand::Image { tint, .. } = &**cmd {
                    out.push(*tint);
                }
            }
            for child in &node.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        walk(&node, &mut out);
        out
    }

    /// A transition declared on the tint eases a state change, where a tint
    /// declared without one switches (#780).
    ///
    /// The pair differs in nothing but the transition and follows the same
    /// condition, so the eased one being between the two colours a frame later
    /// is the claim, and the plain one having arrived is what says the test is
    /// not watching a slow machine.
    #[test]
    fn a_declared_transition_eases_the_tint_into_a_state() {
        use crate::animation::Animate;
        use crate::widgets::Stateful;

        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);
        let hot = create_signal(false);
        let pixel = || ImageSource::Rgba {
            width: 1,
            height: 1,
            pixels: vec![0, 0, 0, 255].into(),
        };

        // One tree, so the two images do not share ids and the job queue.
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            crate::widgets::container()
                .child(
                    Image::new(pixel())
                        .tint(FROM.transition(400.0))
                        .state(hot, |s| s.tint(TO)),
                )
                .child(Image::new(pixel()).tint(FROM).state(hot, |s| s.tint(TO))),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        tints_at(&mut tree, root, t0);

        hot.set(true);

        // The frame the write lands on is where the ease begins.
        tints_at(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let drawn = tints_at(&mut tree, root, t0 + std::time::Duration::from_millis(100));
        let (eased, plain) = (drawn[0].expect("tinted"), drawn[1].expect("tinted"));
        assert!(
            plain.r > 0.99,
            "a tint declared without a transition switches, got {plain:?}"
        );
        assert!(
            eased.r > 0.01 && eased.r < 0.99,
            "a quarter of the way through a 400ms ease the tint has to be \
             between the two, got {eased:?}"
        );

        let settled = tints_at(&mut tree, root, t0 + std::time::Duration::from_millis(600));
        assert!(
            settled[0].is_some_and(|tint| tint.r > 0.99),
            "the ease never finished, got {:?}",
            settled[0]
        );
    }
}
