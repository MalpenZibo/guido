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
    /// Width of the surface in logical pixels.
    pub width: u32,
    /// Height of the surface in logical pixels.
    pub height: u32,
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
    /// Exclusive zone (reserves screen space). None means use height.
    pub exclusive_zone: Option<i32>,
    /// Margins from the anchored screen edges (top, right, bottom, left).
    pub margin: (i32, i32, i32, i32),
    /// Output (monitor) to show the surface on. None lets the compositor choose.
    pub output: Option<OutputId>,
    /// Input region in logical surface coordinates. `None` means the whole
    /// surface accepts input; `Some(rects)` limits input to those rectangles
    /// (an empty list makes the surface fully click-through).
    pub input_region: Option<Vec<Rect>>,
}

impl Default for SurfaceConfig {
    fn default() -> Self {
        Self {
            width: 400,
            height: 300,
            anchor: Anchor::empty(),
            layer: Layer::Top,
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: "guido-surface".to_string(),
            background_color: Color::rgb(0.1, 0.1, 0.15),
            exclusive_zone: None,
            margin: (0, 0, 0, 0),
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
    pub fn width(mut self, width: u32) -> Self {
        self.width = width;
        self
    }

    /// Set the height of the surface.
    pub fn height(mut self, height: u32) -> Self {
        self.height = height;
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
    pub fn exclusive_zone(mut self, zone: Option<i32>) -> Self {
        self.exclusive_zone = zone;
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
    /// Use `SurfaceHandle::set_margin` to change them at runtime.
    pub fn margin(mut self, top: i32, right: i32, bottom: i32, left: i32) -> Self {
        self.margin = (top, right, bottom, left);
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
///     PopupConfig::new(250, 300)
///         .anchor_rect(button_rect)
///         .anchor(PopupAnchor::Bottom)
///         .gravity(PopupGravity::Bottom)
///         .grab(),
///     move || menu_widget(),
/// );
/// ```
#[derive(Clone)]
pub struct PopupConfig {
    /// Popup size in logical pixels (required by xdg_positioner).
    pub width: u32,
    pub height: u32,
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
    /// Create a popup configuration with the given size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            anchor_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            anchor: PopupAnchor::Bottom,
            gravity: PopupGravity::Bottom,
            offset: (0, 0),
            grab: false,
            background_color: Color::TRANSPARENT,
        }
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

    /// Set the size of this surface in logical pixels.
    ///
    /// Note: When anchored to both edges on an axis (e.g., LEFT and RIGHT),
    /// the compositor may override that dimension.
    pub fn set_size(&self, width: u32, height: u32) {
        push_surface_command(SurfaceCommand::SetSize {
            id: self.id,
            width,
            height,
        });
    }

    /// Set the exclusive zone for this surface.
    ///
    /// The exclusive zone reserves screen space so other windows don't
    /// overlap. Pass 0 for no exclusive zone, or a positive value for
    /// the number of pixels to reserve.
    pub fn set_exclusive_zone(&self, zone: i32) {
        push_surface_command(SurfaceCommand::SetExclusiveZone { id: self.id, zone });
    }

    /// Set the margin for this surface.
    ///
    /// Margins add space between the surface and the screen edge it's
    /// anchored to.
    pub fn set_margin(&self, top: i32, right: i32, bottom: i32, left: i32) {
        push_surface_command(SurfaceCommand::SetMargin {
            id: self.id,
            top,
            right,
            bottom,
            left,
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
        width: u32,
        height: u32,
    },
    /// Set the exclusive zone for a surface.
    SetExclusiveZone { id: SurfaceId, zone: i32 },
    /// Set the margin for a surface.
    SetMargin {
        id: SurfaceId,
        top: i32,
        right: i32,
        bottom: i32,
        left: i32,
    },
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
    crate::jobs::request_frame();
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
///     PopupConfig::new(250, 300)
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
