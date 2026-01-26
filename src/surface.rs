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
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::platform::{Anchor, Layer};
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
}

/// Handle to a spawned surface for controlling it from widget code.
///
/// The handle can be cloned and shared between callbacks. It allows
/// closing the surface and checking if it's still open.
#[derive(Clone)]
pub struct SurfaceHandle {
    id: SurfaceId,
    command_sender: Sender<SurfaceCommand>,
}

impl SurfaceHandle {
    /// Create a new surface handle.
    pub(crate) fn new(id: SurfaceId, command_sender: Sender<SurfaceCommand>) -> Self {
        Self { id, command_sender }
    }

    /// Close this surface (removes from screen, destroys widget tree).
    pub fn close(&self) {
        let _ = self.command_sender.send(SurfaceCommand::Close(self.id));
    }

    /// Get the surface ID.
    pub fn id(&self) -> SurfaceId {
        self.id
    }
}

/// Commands for dynamic surface creation/destruction.
pub(crate) enum SurfaceCommand {
    /// Create a new surface with the given configuration and widget factory.
    Create {
        id: SurfaceId,
        config: SurfaceConfig,
        widget_fn: Box<dyn FnOnce() -> Box<dyn Widget> + Send>,
    },
    /// Close and destroy a surface by ID.
    Close(SurfaceId),
}

// Thread-local storage for the surface command channel.
// This allows spawn_surface to be called from anywhere in widget code.
thread_local! {
    static SURFACE_COMMAND_TX: RefCell<Option<Sender<SurfaceCommand>>> = const { RefCell::new(None) };
}

/// Initialize the surface command channel. Called by App::run().
pub(crate) fn init_surface_commands() -> Receiver<SurfaceCommand> {
    let (tx, rx) = channel();
    SURFACE_COMMAND_TX.with(|cell| {
        *cell.borrow_mut() = Some(tx);
    });
    rx
}

/// Spawn a new surface at runtime.
///
/// This function can be called from anywhere in widget code (e.g., event handlers)
/// to create a new layer shell surface dynamically.
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
/// # Panics
///
/// Panics if called before `App::run()` has initialized the command channel.
///
/// # Example
///
/// ```ignore
/// let handle = spawn_surface(
///     SurfaceConfig::new()
///         .width(300)
///         .height(200)
///         .layer(Layer::Overlay),
///     move || {
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
    F: FnOnce() -> W + Send + 'static,
{
    let id = SurfaceId::next();
    let tx = SURFACE_COMMAND_TX.with(|cell| {
        cell.borrow()
            .clone()
            .expect("spawn_surface called before App::run()")
    });

    let cmd = SurfaceCommand::Create {
        id,
        config,
        widget_fn: Box::new(move || Box::new(widget_fn())),
    };
    tx.send(cmd).expect("Event loop closed");

    SurfaceHandle::new(id, tx)
}
