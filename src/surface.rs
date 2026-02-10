//! Multi-surface support for Guido applications.
//!
//! This module provides types for creating and managing multiple Wayland layer shell
//! surfaces within a single Guido application. Each surface has its own widget tree
//! but all surfaces share the same reactive signals and app state.
//!
//! # Static Surface Definition (at startup)
//!
//! ```ignore
//! App::new()
//!     .add_surface(
//!         SurfaceConfig::new()
//!             .height(32)
//!             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
//!             .layer(Layer::Top)
//!             .namespace("status-bar"),
//!         move || status_bar_widget()
//!     )
//!     .run();
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

use crate::platform::{Anchor, KeyboardInteractivity, Layer};
use crate::widgets::{Color, Widget};

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
}

// Thread-local storage for the surface command queue.
// Both sender and receiver are on the main thread — this is just a deferred command queue.
thread_local! {
    static SURFACE_COMMANDS: RefCell<Vec<SurfaceCommand>> = const { RefCell::new(Vec::new()) };
}

/// Push a surface command to the thread-local queue.
fn push_surface_command(cmd: SurfaceCommand) {
    SURFACE_COMMANDS.with(|cmds| {
        cmds.borrow_mut().push(cmd);
    });
    crate::jobs::request_frame();
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
/// let (app, status_bar_id) = App::new()
///     .add_surface(config, move || {
///         container()
///             .on_click(move || {
///                 // Get handle and modify properties
///                 let handle = surface_handle(status_bar_id);
///                 handle.set_layer(Layer::Overlay);
///             })
///             .child(text("Click to promote to overlay"))
///     });
/// app.run();
/// ```
pub fn surface_handle(id: SurfaceId) -> SurfaceHandle {
    SurfaceHandle { id }
}
