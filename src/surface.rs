//! Multi-surface support for Guido applications.
//!
//! This module provides types for creating and managing multiple Wayland layer shell
//! surfaces within a single Guido application. Each surface has its own widget tree
//! but all surfaces share the same reactive signals and app state.
//!
//! # Static Surface Definition (at startup)
//!
//! ```ignore
//! App::new().run(|app| {
//!     app.add_surface(
//!         SurfaceConfig::new()
//!             .height(32)
//!             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
//!             .layer(Layer::Top)
//!             .namespace("status-bar"),
//!         move || status_bar_widget()
//!     );
//! });
//! ```
//!
//! # Dynamic Surface Creation (at runtime)
//!
//! ```ignore
//! // In an event handler or anywhere in widget code:
//! let handle = spawn_surface(
//!     SurfaceConfig::new()
//!         .width(300)
//!         .height(200)
//!         .layer(Layer::Overlay),
//!     move || popup_widget()
//! );
//!
//! // Later, to close the surface:
//! handle.close();
//! ```

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::outputs::OutputId;
use crate::platform::{Anchor, KeyboardInteractivity, Layer};
use crate::widgets::{Color, Rect, Widget};

/// Unique identifier for each surface in the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);

impl SurfaceId {
    /// Create a new unique surface ID.
    pub fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        SurfaceId(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value (for debugging/logging).
    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// Configuration for a layer shell surface.
///
/// Use the builder pattern to configure surface properties:
///
/// ```ignore
/// SurfaceConfig::new()
///     .width(300)
///     .height(200)
///     .anchor(Anchor::TOP | Anchor::RIGHT)
///     .layer(Layer::Overlay)
///     .keyboard_interactivity(KeyboardInteractivity::Exclusive)
///     .namespace("my-popup")
///     .background_color(Color::rgb(0.2, 0.2, 0.3))
/// ```
#[derive(Clone)]
pub struct SurfaceConfig {
    /// Width of the surface (fixed pixels or content-following).
    pub width: SurfaceExtent,
    /// Height of the surface (fixed pixels or content-following).
    pub height: SurfaceExtent,
    /// Anchor edges for the surface position.
    pub anchor: Anchor,
    /// Layer shell layer (background, bottom, top, overlay).
    pub layer: Layer,
    /// Keyboard interactivity mode.
    pub keyboard_interactivity: KeyboardInteractivity,
    /// Namespace identifier for the surface.
    pub namespace: String,
    /// Background color for the surface.
    pub background_color: Color,
    /// Screen-space reservation policy (see [`ExclusiveZone`]).
    pub exclusive_zone: ExclusiveZone,
    /// Margins from the anchored screen edges.
    pub margin: Margin,
    /// Output (monitor) to show the surface on. None lets the compositor choose.
    pub output: Option<OutputId>,
    /// Input region in logical surface coordinates. `None` means the whole
    /// surface accepts input; `Some(rects)` limits input to those rectangles
    /// (an empty list makes the surface fully click-through).
    pub input_region: Option<Vec<Rect>>,
}

/// Margins from the anchored screen edges, in logical pixels.
///
/// The same shorthands a [`Padding`](crate::widgets::Padding) takes, in the
/// same order — one value for every edge, `[vertical, horizontal]`, or the
/// full `[top, right, bottom, left]`:
///
/// ```ignore
/// SurfaceConfig::new().margin(8)                   // all four edges
/// SurfaceConfig::new().margin([0, 12])             // none top/bottom, 12 aside
/// SurfaceConfig::new().margin([8, 12, 0, 12])      // top, right, bottom, left
/// ```
///
/// Negative values are allowed: layer-shell reads them as pushing the surface
/// past its anchored edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Margin {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

impl Margin {
    /// The same margin on every edge.
    pub fn all(value: i32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub(crate) fn is_zero(self) -> bool {
        self == Self::default()
    }
}

impl From<i32> for Margin {
    fn from(v: i32) -> Self {
        Margin::all(v)
    }
}

impl From<u32> for Margin {
    fn from(v: u32) -> Self {
        Margin::all(v as i32)
    }
}

/// `[vertical, horizontal]` — CSS-style 2-value shorthand.
impl From<[i32; 2]> for Margin {
    fn from(v: [i32; 2]) -> Self {
        Margin {
            top: v[0],
            right: v[1],
            bottom: v[0],
            left: v[1],
        }
    }
}

/// `[top, right, bottom, left]` — CSS-style 4-value shorthand.
impl From<[i32; 4]> for Margin {
    fn from(v: [i32; 4]) -> Self {
        Margin {
            top: v[0],
            right: v[1],
            bottom: v[2],
            left: v[3],
        }
    }
}

/// Per-axis sizing for layer surfaces.
///
/// `Fixed` is a size in logical pixels (plain integers convert into it).
/// `Content` follows the content's natural size on that axis — the
/// [`content()`] constructor reads like [`fill()`](crate::layout::fill)
/// and friends:
///
/// ```ignore
/// SurfaceConfig::new()
///     .width(360)             // fixed
///     .height(content())      // follows the toast stack
/// ```
///
/// Content semantics, designed to stay footgun-free:
/// - Resizing is asynchronous (one compositor round trip per change) and
///   happens on CONTENT changes, not per animation frame: the natural
///   size is measured against animation **targets**, so an animated
///   growth resizes once up front and the animation plays inside the
///   final-size surface.
/// - On an axis anchored to both screen edges the compositor owns the
///   size; `Content` there is ignored with a warning at creation.
/// - An exclusive zone of [`ExclusiveZone::Auto`] follows content
///   resizes; every other policy never moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceExtent {
    /// Fixed size in logical pixels.
    Fixed(u32),
    /// Follow the content's natural size on this axis.
    Content,
}

/// Which axes the compositor owns, from the anchor: an axis pinned to both of
/// its screen edges is stretched to fit and the surface's request for it is
/// ignored.
///
/// Returns `(width, height)`.
pub(crate) fn compositor_owned_axes(anchor: Anchor) -> (bool, bool) {
    (
        anchor.contains(Anchor::LEFT) && anchor.contains(Anchor::RIGHT),
        anchor.contains(Anchor::TOP) && anchor.contains(Anchor::BOTTOM),
    )
}

/// Warn about a `content()` axis the compositor owns, which can never take
/// effect. For the creation path, where the *other* axis is usually just the
/// default nobody chose — a bar declares its height and leaves the width alone —
/// so a warning about a `Fixed` one there would be noise.
pub(crate) fn warn_content_on_stretched_axis(id: SurfaceId, config: &SurfaceConfig) {
    let (stretch_w, stretch_h) = compositor_owned_axes(config.anchor);
    if (stretch_w && config.width.is_content()) || (stretch_h && config.height.is_content()) {
        log::warn!(
            "Surface {id:?}: content() sizing on an axis anchored to both screen \
             edges is compositor-owned and will be ignored"
        );
    }
}

/// The same for a size asked for at runtime, where every value is one the caller
/// chose — so a `Fixed` one being discarded is worth saying too. Silently
/// dropping a number somebody passed is the failure mode this exists for.
pub(crate) fn warn_size_request_on_stretched_axis(id: SurfaceId, config: &SurfaceConfig) {
    let (stretch_w, stretch_h) = compositor_owned_axes(config.anchor);
    for (stretched, extent, axis) in [
        (stretch_w, config.width, "width"),
        (stretch_h, config.height, "height"),
    ] {
        if !stretched {
            continue;
        }
        match extent {
            SurfaceExtent::Content => log::warn!(
                "Surface {id:?}: content() {axis} on an axis anchored to both \
                 screen edges is compositor-owned and will be ignored"
            ),
            SurfaceExtent::Fixed(v) => log::warn!(
                "Surface {id:?}: {axis} of {v} on an axis anchored to both screen \
                 edges is compositor-owned and will be ignored"
            ),
        }
    }
}

/// The size an axis is currently asking for: its own, if it has one, and the
/// size the compositor has confirmed while a content axis waits to be measured.
///
/// Before the first configure there is no confirmed size, and the placeholder is
/// what creation asks for too — the content-measure pass runs on the first
/// frames of a surface either way, so 1px is a value it is leaving, not one it
/// can be stuck at.
pub(crate) fn requested_extent(extent: SurfaceExtent, live: Option<u32>) -> u32 {
    match extent {
        SurfaceExtent::Fixed(v) => v,
        SurfaceExtent::Content => live.unwrap_or_else(|| extent.initial()),
    }
}

/// What a resize asks the compositor for, and whether the surface has to be
/// re-measured before that answer is right.
///
/// A content axis has no size of its own yet — `SurfaceExtent::initial()` is
/// 1px — so asking for it directly collapses the surface, and nothing brings it
/// back: the content-measure pass runs only for a surface that had layout
/// activity, and a bare `set_size` produces none. So a content axis holds the
/// confirmed size and asks for a layout, which is what brings the measure round.
pub(crate) fn resize_request(config: &SurfaceConfig, live: Option<(u32, u32)>) -> (u32, u32, bool) {
    let (owns_w, owns_h) = compositor_owned_axes(config.anchor);
    let (width, height) = honour_owned_axes(
        config.anchor,
        requested_extent(config.width, live.map(|(w, _)| w)),
        requested_extent(config.height, live.map(|(_, h)| h)),
    );
    // Only for a content axis that is *ours*. Stretched, the measure runs and
    // `honour_owned_axes` throws the answer away — a full re-layout of the
    // subtree for an axis that was never going to be ours. A bar declared
    // `width(content()).anchor(LEFT | RIGHT)` did exactly that on every resize
    // and every re-anchoring.
    let measure = (config.width.is_content() && !owns_w) || (config.height.is_content() && !owns_h);
    (width, height, measure)
}

/// Zero the axes the compositor owns, whatever size was going to be asked for.
///
/// Zero on such an axis is how layer-shell says "you decide", and it is not
/// optional: `zwlr_layer_surface_v1::set_size` makes omitting a dimension
/// *without* opposite-edge anchoring a protocol error, and sending a number
/// *with* it hands back an axis that is not ours. The anchor decides, so every
/// path that sends a size comes through here — creation, a runtime resize, a
/// re-anchoring, and the content-measure pass.
pub(crate) fn honour_owned_axes(anchor: Anchor, width: u32, height: u32) -> (u32, u32) {
    let (stretch_w, stretch_h) = compositor_owned_axes(anchor);
    (
        if stretch_w { 0 } else { width },
        if stretch_h { 0 } else { height },
    )
}

impl SurfaceExtent {
    /// The initial protocol size (`Content` starts at 1px until the first
    /// measure lands).
    pub(crate) fn initial(self) -> u32 {
        match self {
            SurfaceExtent::Fixed(v) => v,
            SurfaceExtent::Content => 1,
        }
    }

    pub(crate) fn is_content(self) -> bool {
        matches!(self, SurfaceExtent::Content)
    }
}

impl From<u32> for SurfaceExtent {
    fn from(v: u32) -> Self {
        SurfaceExtent::Fixed(v)
    }
}

impl From<i32> for SurfaceExtent {
    fn from(v: i32) -> Self {
        SurfaceExtent::Fixed(v.max(0) as u32)
    }
}

/// A [`SurfaceExtent`] that follows the content's natural size.
pub fn content() -> SurfaceExtent {
    SurfaceExtent::Content
}

/// Screen-space reservation for layer surfaces, mapping the layer-shell
/// exclusive-zone semantics to intent. Reserving is **opt-in**: the
/// default is [`ExclusiveZone::None`] — a bar declares its reservation
/// explicitly:
///
/// ```ignore
/// .exclusive_zone(ExclusiveZone::Auto)    // a bar reserving itself
/// .exclusive_zone(34)                     // fixed reservation
/// .exclusive_zone(ExclusiveZone::None)    // reserve nothing (toasts, OSD)
/// .exclusive_zone(ExclusiveZone::Ignore)  // overlap panels too
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusiveZone {
    /// Reserve the surface's own extent on the anchored axis, plus the
    /// margin on that edge (gtk-layer-shell's "auto exclusive zone"): the
    /// height of a top/bottom bar, the width of a left/right dock — the
    /// axis follows from the anchor, it is never a choice. Content-sized
    /// surfaces keep the reservation in sync when they resize. Per the
    /// layer-shell spec a zone is only meaningful anchored to one edge
    /// (plus optionally both perpendicular ones); on other anchors this
    /// resolves to no reservation.
    Auto,
    /// Reserve exactly this many logical pixels.
    Fixed(u32),
    /// Reserve nothing; the surface is moved by other surfaces' zones.
    None,
    /// Reserve nothing and ignore other surfaces' zones (the surface may
    /// overlap panels; protocol value -1).
    Ignore,
}

impl ExclusiveZone {
    /// Protocol value. [`Auto`](ExclusiveZone::Auto) resolves against the
    /// surface extent on the anchored axis plus that edge's margin.
    pub(crate) fn resolve(self, anchor: Anchor, margin: Margin, width: u32, height: u32) -> i32 {
        match self {
            ExclusiveZone::Auto => match Self::follow_axis(anchor) {
                Some(FollowAxis::Height) => {
                    let edge_margin = if anchor.contains(Anchor::TOP) {
                        margin.top
                    } else {
                        margin.bottom
                    };
                    height as i32 + edge_margin
                }
                Some(FollowAxis::Width) => {
                    let edge_margin = if anchor.contains(Anchor::LEFT) {
                        margin.left
                    } else {
                        margin.right
                    };
                    width as i32 + edge_margin
                }
                None => {
                    log::warn!(
                        "ExclusiveZone::Auto on a corner/full anchor has no \
                         meaningful axis (layer-shell spec); reserving nothing"
                    );
                    0
                }
            },
            ExclusiveZone::Fixed(z) => z as i32,
            ExclusiveZone::None => 0,
            ExclusiveZone::Ignore => -1,
        }
    }

    /// The axis an `Auto` reservation tracks, from the anchor — the
    /// layer-shell rule: a zone is meaningful anchored to one edge, or
    /// one edge plus both perpendicular ones.
    pub(crate) fn follow_axis(anchor: Anchor) -> Option<FollowAxis> {
        let vertical_edge = anchor.contains(Anchor::TOP) != anchor.contains(Anchor::BOTTOM);
        let horizontal_edge = anchor.contains(Anchor::LEFT) != anchor.contains(Anchor::RIGHT);
        match (vertical_edge, horizontal_edge) {
            (true, false) => Some(FollowAxis::Height),
            (false, true) => Some(FollowAxis::Width),
            // Corner (one edge of each axis) or no meaningful anchor
            _ => None,
        }
    }
}

/// The axis an [`ExclusiveZone::Auto`] reservation tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowAxis {
    Width,
    Height,
}

impl From<u32> for ExclusiveZone {
    fn from(z: u32) -> Self {
        ExclusiveZone::Fixed(z)
    }
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            width: SurfaceExtent::Fixed(400),
            height: SurfaceExtent::Fixed(300),
            anchor: Anchor::empty(),
            layer: Layer::Top,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: "guido-surface".to_string(),
            background_color: Color::rgb(0.1, 0.1, 0.15),
            exclusive_zone: ExclusiveZone::None,
            margin: Margin::default(),
            output: None,
            input_region: None,
        }
    }
}

impl SurfaceConfig {
    /// Create a new surface configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the width of the surface.
    pub fn width(mut self, width: impl Into<SurfaceExtent>) -> Self {
        self.width = width.into();
        self
    }

    /// Set the height of the surface.
    pub fn height(mut self, height: impl Into<SurfaceExtent>) -> Self {
        self.height = height.into();
        self
    }

    /// Set the anchor edges for the surface.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set the layer shell layer.
    pub fn layer(mut self, layer: Layer) -> Self {
        self.layer = layer;
        self
    }

    /// Set the namespace identifier for the surface.
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Set the background color for the surface.
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = color;
        self
    }

    /// Set the exclusive zone (reserves screen space).
    /// Pass Some(0) for no exclusive zone, None to use the surface height.
    pub fn exclusive_zone(mut self, zone: impl Into<ExclusiveZone>) -> Self {
        self.exclusive_zone = zone.into();
        self
    }

    /// Set the keyboard interactivity mode.
    ///
    /// - `KeyboardInteractivity::None`: Surface never receives keyboard focus.
    /// - `KeyboardInteractivity::OnDemand`: Surface receives focus when clicked (default).
    /// - `KeyboardInteractivity::Exclusive`: Surface grabs keyboard focus exclusively.
    pub fn keyboard_interactivity(mut self, mode: KeyboardInteractivity) -> Self {
        self.keyboard_interactivity = mode;
        self
    }

    /// Set the margins from the anchored screen edges, applied at creation.
    ///
    /// Use [`SurfaceHandle::set_margin`] to change them at runtime.
    pub fn margin(mut self, margin: impl Into<Margin>) -> Self {
        self.margin = margin.into();
        self
    }

    /// Pin the surface to a specific output (monitor).
    ///
    /// Get output ids from the reactive [`crate::outputs::outputs`] list. If
    /// the output is disconnected before the surface is created, the
    /// compositor chooses one instead. The output cannot be changed after
    /// creation (a layer surface is bound to its output for its lifetime).
    pub fn output(mut self, output: OutputId) -> Self {
        self.output = Some(output);
        self
    }

    /// Limit pointer/touch input to the given rectangles (logical surface
    /// coordinates). Everything outside them lets clicks pass through to
    /// whatever is below. An empty list makes the surface fully
    /// click-through.
    ///
    /// Use `SurfaceHandle::set_input_region` to change it at runtime.
    pub fn input_region(mut self, rects: impl Into<Vec<Rect>>) -> Self {
        self.input_region = Some(rects.into());
        self
    }

    /// Make the whole surface click-through: pointer and touch input passes
    /// to whatever is below. Shorthand for `input_region([])`.
    pub fn click_through(mut self) -> Self {
        self.input_region = Some(Vec::new());
        self
    }
}

/// Which point of the anchor rectangle a popup attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PopupAnchor {
    /// Center of the anchor rect.
    #[default]
    None,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    BottomLeft,
    TopRight,
    BottomRight,
}

/// Which direction a popup grows from its anchor point.
///
/// `Bottom` means "downwards" (typical menu under a bar button).
pub type PopupGravity = PopupAnchor;

/// Configuration for an xdg popup anchored to a parent surface.
///
/// The compositor positions the popup relative to `anchor_rect` (parent
/// surface coordinates) and adjusts it to stay on screen (flip/slide).
///
/// ```ignore
/// spawn_popup(
///     bar_surface_id,
///     PopupConfig::new(250)
///         .anchor_rect(button_rect)
///         .anchor(PopupAnchor::Bottom)
///         .gravity(PopupGravity::Bottom)
///         .grab(),
///     move || menu_widget(),
/// );
/// ```
#[derive(Clone)]
pub struct PopupConfig {
    /// Popup width in logical pixels.
    pub width: u32,
    /// Popup height in logical pixels. `None` (the default) sizes the popup
    /// to its content: the widget is laid out at the given width before the
    /// popup is created, and repositioned when its content height changes
    /// (submenus expanding, lists growing). Content using `height(fill())`
    /// resolves to the screen-height cap — give such popups an explicit
    /// height instead.
    pub height: Option<u32>,
    /// Rectangle the popup anchors to, in parent surface coordinates.
    pub anchor_rect: Rect,
    /// Which point of the anchor rect to attach to.
    pub anchor: PopupAnchor,
    /// Which direction the popup grows.
    pub gravity: PopupGravity,
    /// Extra offset from the anchor point.
    pub offset: (i32, i32),
    /// Take an input grab: clicking outside dismisses the popup (menu
    /// semantics). The compositor reports the dismissal reactively — see
    /// [`PopupHandle::dismissed`].
    pub grab: bool,
    /// Background color of the popup surface.
    pub background_color: Color,
}

impl PopupConfig {
    /// Create a popup configuration with the given width; the height sizes
    /// to the content (see [`PopupConfig::height`] to fix it instead).
    pub fn new(width: u32) -> Self {
        Self {
            width,
            height: None,
            anchor_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            anchor: PopupAnchor::Bottom,
            gravity: PopupGravity::Bottom,
            offset: (0, 0),
            grab: false,
            background_color: Color::TRANSPARENT,
        }
    }

    /// Fix the popup height instead of sizing it to the content.
    pub fn height(mut self, height: u32) -> Self {
        self.height = Some(height);
        self
    }

    /// Set the rectangle the popup anchors to (parent surface coordinates).
    /// Typically a widget's bounds from a [`crate::widget_ref::WidgetRef`].
    pub fn anchor_rect(mut self, rect: Rect) -> Self {
        self.anchor_rect = rect;
        self
    }

    /// Set which point of the anchor rect the popup attaches to.
    pub fn anchor(mut self, anchor: PopupAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set which direction the popup grows.
    pub fn gravity(mut self, gravity: PopupGravity) -> Self {
        self.gravity = gravity;
        self
    }

    /// Set an extra offset from the anchor point.
    pub fn offset(mut self, x: i32, y: i32) -> Self {
        self.offset = (x, y);
        self
    }

    /// Take an input grab: clicking outside dismisses the popup.
    pub fn grab(mut self) -> Self {
        self.grab = true;
        self
    }

    /// Set the background color of the popup surface.
    pub fn background_color(mut self, color: Color) -> Self {
        self.background_color = color;
        self
    }
}

/// Handle to a spawned surface for controlling it from widget code.
///
/// The handle can be cloned and shared between callbacks. It allows
/// closing the surface and checking if it's still open.
#[derive(Clone)]
pub struct SurfaceHandle {
    id: SurfaceId,
}

impl SurfaceHandle {
    /// Close this surface (removes from screen, destroys widget tree).
    pub fn close(&self) {
        push_surface_command(SurfaceCommand::Close(self.id));
    }

    /// Get the surface ID.
    pub fn id(&self) -> SurfaceId {
        self.id
    }

    /// Set the layer shell layer for this surface.
    ///
    /// Changes take effect immediately. Use `Layer::Overlay` to appear above
    /// other windows, `Layer::Top` for normal status bars, etc.
    pub fn set_layer(&self, layer: Layer) {
        push_surface_command(SurfaceCommand::SetLayer { id: self.id, layer });
    }

    /// Set the keyboard interactivity mode for this surface.
    ///
    /// - `KeyboardInteractivity::None`: Surface never receives keyboard focus.
    /// - `KeyboardInteractivity::OnDemand`: Surface receives focus when clicked.
    /// - `KeyboardInteractivity::Exclusive`: Surface grabs keyboard focus exclusively.
    pub fn set_keyboard_interactivity(&self, mode: KeyboardInteractivity) {
        push_surface_command(SurfaceCommand::SetKeyboardInteractivity { id: self.id, mode });
    }

    /// Set the anchor edges for this surface.
    ///
    /// Anchor determines which screen edges the surface attaches to.
    /// For example, `Anchor::TOP | Anchor::LEFT | Anchor::RIGHT` creates a
    /// top bar that spans the width of the screen.
    pub fn set_anchor(&self, anchor: Anchor) {
        push_surface_command(SurfaceCommand::SetAnchor {
            id: self.id,
            anchor,
        });
    }

    /// Set the size of this surface, in the same vocabulary
    /// [`SurfaceConfig::width`] takes — so an axis can be handed back to
    /// [`content()`] at runtime, not only pinned to a number.
    ///
    /// Note: when anchored to both edges on an axis (e.g. LEFT and RIGHT),
    /// the compositor may override that dimension.
    pub fn set_size(&self, width: impl Into<SurfaceExtent>, height: impl Into<SurfaceExtent>) {
        push_surface_command(SurfaceCommand::SetSize {
            id: self.id,
            width: width.into(),
            height: height.into(),
        });
    }

    /// Set the screen-space reservation for this surface, in the same
    /// vocabulary [`SurfaceConfig::exclusive_zone`] takes.
    ///
    /// [`ExclusiveZone::Auto`] keeps following the surface's extent from here
    /// on, exactly as it would had it been declared at creation.
    pub fn set_exclusive_zone(&self, zone: impl Into<ExclusiveZone>) {
        push_surface_command(SurfaceCommand::SetExclusiveZone {
            id: self.id,
            zone: zone.into(),
        });
    }

    /// Set the margins from the anchored screen edges, in the same vocabulary
    /// [`SurfaceConfig::margin`] takes.
    pub fn set_margin(&self, margin: impl Into<Margin>) {
        push_surface_command(SurfaceCommand::SetMargin {
            id: self.id,
            margin: margin.into(),
        });
    }

    /// Set the input region for this surface.
    ///
    /// `None` restores the default (the whole surface accepts input).
    /// `Some(rects)` limits pointer/touch input to those rectangles in
    /// logical surface coordinates — everything outside them lets clicks
    /// pass through to whatever is below. `Some(vec![])` makes the surface
    /// fully click-through.
    pub fn set_input_region(&self, rects: Option<Vec<Rect>>) {
        push_surface_command(SurfaceCommand::SetInputRegion { id: self.id, rects });
    }
}

/// Commands for dynamic surface creation/destruction and property modification.
#[allow(clippy::type_complexity)]
pub(crate) enum SurfaceCommand {
    /// Create a new surface with the given configuration and widget factory.
    Create {
        id: SurfaceId,
        config: SurfaceConfig,
        widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
    },
    /// Close and destroy a surface by ID.
    Close(SurfaceId),
    /// Set the layer shell layer for a surface.
    SetLayer { id: SurfaceId, layer: Layer },
    /// Set the keyboard interactivity mode for a surface.
    SetKeyboardInteractivity {
        id: SurfaceId,
        mode: KeyboardInteractivity,
    },
    /// Set the anchor edges for a surface.
    SetAnchor { id: SurfaceId, anchor: Anchor },
    /// Set the size of a surface.
    SetSize {
        id: SurfaceId,
        width: SurfaceExtent,
        height: SurfaceExtent,
    },
    /// Set the screen-space reservation for a surface. Resolved to a protocol
    /// value by the loop, which is where the anchor and the current extent are.
    SetExclusiveZone { id: SurfaceId, zone: ExclusiveZone },
    /// Set the margin for a surface.
    SetMargin { id: SurfaceId, margin: Margin },
    /// Set the input region for a surface.
    SetInputRegion {
        id: SurfaceId,
        rects: Option<Vec<Rect>>,
    },
    /// Create an xdg popup anchored to a parent surface.
    CreatePopup {
        id: SurfaceId,
        parent: SurfaceId,
        config: PopupConfig,
        widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
    },
}

// Thread-local storage for the surface command queue.
// Both sender and receiver are on the main thread — this is just a deferred command queue.
thread_local! {
    static SURFACE_COMMANDS: RefCell<Vec<SurfaceCommand>> = const { RefCell::new(Vec::new()) };
}

/// Push a surface command to the thread-local queue.
pub(crate) fn push_surface_command(cmd: SurfaceCommand) {
    SURFACE_COMMANDS.with(|cmds| {
        cmds.borrow_mut().push(cmd);
    });
    crate::jobs::wake_loop();
}

/// Reset the surface command queue.
///
/// Called during `App::drop()` to clear stale surface commands.
pub(crate) fn reset_surface_commands() {
    SURFACE_COMMANDS.with(|cmds| cmds.borrow_mut().clear());
}

/// Drain all pending surface commands. Called by the main event loop.
pub(crate) fn drain_surface_commands() -> Vec<SurfaceCommand> {
    SURFACE_COMMANDS.with(|cmds| cmds.borrow_mut().drain(..).collect())
}

/// Spawn a new surface at runtime.
///
/// This function can be called from anywhere in widget code (e.g., event handlers)
/// to create a new layer shell surface dynamically.
///
/// The widget factory closure creates the root widget for the surface.
///
/// # Arguments
///
/// * `config` - Configuration for the new surface
/// * `widget_fn` - Factory function that creates the root widget for the surface
///
/// # Returns
///
/// A `SurfaceHandle` that can be used to close the surface later.
///
/// # Example
///
/// ```ignore
/// let handle = spawn_surface(
///     SurfaceConfig::new()
///         .width(300)
///         .height(200)
///         .layer(Layer::Overlay),
///     || {
///         container()
///             .background(Color::rgb(0.2, 0.2, 0.3))
///             .child(text("Popup content"))
///     }
/// );
///
/// // Later, to close:
/// handle.close();
/// ```
pub fn spawn_surface<W, F>(config: SurfaceConfig, widget_fn: F) -> SurfaceHandle
where
    W: Widget + 'static,
    F: FnOnce() -> W + 'static,
{
    let id = SurfaceId::next();

    push_surface_command(SurfaceCommand::Create {
        id,
        config,
        widget_fn: Box::new(move || Box::new(widget_fn())),
    });

    SurfaceHandle { id }
}

/// Handle to a spawned popup.
///
/// Popups close either programmatically ([`PopupHandle::close`]) or by the
/// compositor (grab + outside click, parent gone): watch that reactively
/// via [`PopupHandle::dismissed`].
#[derive(Clone, Copy)]
pub struct PopupHandle {
    id: SurfaceId,
    dismissed: crate::reactive::RwSignal<bool>,
}

impl PopupHandle {
    /// The popup's surface ID.
    pub fn id(&self) -> SurfaceId {
        self.id
    }

    /// Close the popup programmatically.
    pub fn close(&self) {
        self.dismissed.set(true);
        push_surface_command(SurfaceCommand::Close(self.id));
    }

    /// Whether the popup has been dismissed (tracked read — reactive inside
    /// tracked closures). True after `close()` or a compositor dismissal
    /// (click outside a grabbed popup, parent closed).
    pub fn dismissed(&self) -> bool {
        self.dismissed.get()
    }
}

// Registry of popup dismissal signals so the platform layer can flip them
// when the compositor dismisses a popup (xdg_popup.popup_done).
thread_local! {
    static POPUP_DISMISSED: RefCell<std::collections::HashMap<SurfaceId, crate::reactive::RwSignal<bool>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Mark a popup dismissed (called by the platform layer on popup_done, and
/// on close). Removes the registry entry.
pub(crate) fn mark_popup_dismissed(id: SurfaceId) {
    let signal = POPUP_DISMISSED.with(|reg| reg.borrow_mut().remove(&id));
    if let Some(signal) = signal {
        signal.set(true);
    }
}

/// Reset popup registry state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_popups() {
    POPUP_DISMISSED.with(|reg| reg.borrow_mut().clear());
}

/// Spawn an xdg popup anchored to a parent surface.
///
/// The compositor positions the popup relative to `config.anchor_rect`
/// (parent surface coordinates), keeps it on screen (flip/slide at screen
/// edges), and — with [`PopupConfig::grab`] — dismisses it when the user
/// clicks outside: real menu semantics, no fullscreen overlay needed.
///
/// The popup renders its own widget tree and shares the reactive state
/// with the rest of the app, like any surface.
///
/// # Example
///
/// ```ignore
/// let button_rect = button_ref.rect().get();
/// let popup = spawn_popup(
///     bar_id,
///     PopupConfig::new(250)
///         .anchor_rect(button_rect)
///         .grab(),
///     move || menu_widget(),
/// );
/// // Reactively observe dismissal (e.g. reset the open/closed state):
/// create_effect(move || {
///     if popup.dismissed() {
///         menu_open.set(false);
///     }
/// });
/// ```
pub fn spawn_popup<W, F>(parent: SurfaceId, config: PopupConfig, widget_fn: F) -> PopupHandle
where
    W: Widget + 'static,
    F: FnOnce() -> W + 'static,
{
    let id = SurfaceId::next();
    let dismissed = crate::reactive::create_signal(false);
    POPUP_DISMISSED.with(|reg| {
        reg.borrow_mut().insert(id, dismissed);
    });

    push_surface_command(SurfaceCommand::CreatePopup {
        id,
        parent,
        config,
        widget_fn: Box::new(move || Box::new(widget_fn())),
    });

    PopupHandle { id, dismissed }
}

/// Get a handle to control an existing surface.
///
/// This can be used to modify surfaces added via `add_surface()` or `spawn_surface()`.
/// The handle allows changing surface properties like layer, keyboard interactivity,
/// anchor, size, exclusive zone, and margin.
///
/// # Example
///
/// ```ignore
/// // Store the ID when adding the surface
/// App::new().run(|app| {
///     let status_bar_id = app.add_surface(config, move || {
///         container()
///             .on_click(move || {
///                 // Get handle and modify properties
///                 let handle = surface_handle(status_bar_id);
///                 handle.set_layer(Layer::Overlay);
///             })
///             .child(text("Click to promote to overlay"))
///     });
/// });
/// ```
pub fn surface_handle(id: SurfaceId) -> SurfaceHandle {
    SurfaceHandle { id }
}

/// Whether any surface command (spawn, close, property change) is queued.
///
/// Part of the loop's wakeup check — see `queued_but_unwoken` in `lib.rs`.
pub(crate) fn surface_commands_pending() -> bool {
    SURFACE_COMMANDS.with(|cmds| !cmds.borrow().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The doc promises "the same shorthands a `Padding` takes, in the same
    /// order". The two types are written out separately — one holds i32 for the
    /// protocol, the other f32 for layout — so the promise is only worth
    /// anything if something checks it. This does, edge by edge, over every
    /// shorthand and a set of inputs where confusing two edges shows up:
    /// asymmetric, and non-zero everywhere.
    #[test]
    fn a_margin_converts_exactly_as_a_padding_does() {
        let same = |m: Margin, p: crate::widgets::Padding, what: &str| {
            assert_eq!(m.top as f32, p.top, "{what}: top");
            assert_eq!(m.right as f32, p.right, "{what}: right");
            assert_eq!(m.bottom as f32, p.bottom, "{what}: bottom");
            assert_eq!(m.left as f32, p.left, "{what}: left");
        };

        same(
            Margin::from(8),
            crate::widgets::Padding::from(8),
            "one value",
        );
        same(
            Margin::all(8),
            crate::widgets::Padding::all(8.0),
            "all(), the named form",
        );
        same(
            Margin::from([4, 12]),
            crate::widgets::Padding::from([4, 12]),
            "[vertical, horizontal]",
        );
        same(
            Margin::from([1, 2, 3, 4]),
            crate::widgets::Padding::from([1, 2, 3, 4]),
            "[top, right, bottom, left]",
        );

        // And spelled out once, so a shared mistake in both types would still
        // be caught: CSS order, clockwise from the top.
        assert_eq!(
            Margin::from([1, 2, 3, 4]),
            Margin {
                top: 1,
                right: 2,
                bottom: 3,
                left: 4
            }
        );
        assert_eq!(
            Margin::from([4, 12]),
            Margin {
                top: 4,
                right: 12,
                bottom: 4,
                left: 12
            }
        );
    }

    /// An `Auto` reservation counts the margin on the edge it is anchored to,
    /// and only that one.
    #[test]
    fn an_auto_zone_adds_the_anchored_edges_margin() {
        let margin = Margin::from([6, 20, 9, 20]);
        let top = ExclusiveZone::Auto.resolve(Anchor::TOP, margin, 800, 32);
        assert_eq!(top, 32 + 6);

        let bottom = ExclusiveZone::Auto.resolve(Anchor::BOTTOM, margin, 800, 32);
        assert_eq!(bottom, 32 + 9);

        let left = ExclusiveZone::Auto.resolve(Anchor::LEFT, margin, 48, 600);
        assert_eq!(left, 48 + 20);
    }

    /// The other policies are numbers, and never consult anything.
    #[test]
    fn the_other_zone_policies_ignore_the_surface() {
        let m = Margin::all(10);
        assert_eq!(ExclusiveZone::None.resolve(Anchor::TOP, m, 800, 32), 0);
        assert_eq!(ExclusiveZone::Ignore.resolve(Anchor::TOP, m, 800, 32), -1);
        assert_eq!(
            ExclusiveZone::from(34u32).resolve(Anchor::TOP, m, 800, 32),
            34
        );
    }
}
