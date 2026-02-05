pub mod animation;
pub mod image_metadata;
mod jobs;
pub mod layout;
pub mod layout_stats;
pub mod reactive;
pub mod surface;
mod surface_manager;
pub mod transform;
pub mod transform_origin;
pub mod tree;
pub mod widgets;

// These modules are public for advanced use cases
pub mod platform;
pub mod renderer;

// Re-export macros
pub use guido_macros::component;

use std::cell::RefCell;
use std::collections::HashSet;

use layout::Constraints;
use platform::create_wayland_app;
use reactive::{set_system_clipboard, take_clipboard_change, take_cursor_change};
use renderer::{GpuContext, PaintContext, Renderer, flatten_tree_into};
use surface::{SurfaceCommand, SurfaceConfig, SurfaceId, init_surface_commands};
use surface_manager::{ManagedSurface, SurfaceManager};
use widgets::Widget;
use widgets::font::FontFamily;

// Calloop imports for event-driven main loop (via smithay-client-toolkit re-exports)
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::ping::make_ping;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;

// Thread-local storage for the default font family
thread_local! {
    static DEFAULT_FONT_FAMILY: RefCell<FontFamily> = const { RefCell::new(FontFamily::SansSerif) };
}

/// Set the application-wide default font family.
///
/// This should be called before creating any widgets. Widgets created after this
/// call will use the specified font family as their default.
///
/// # Example
///
/// ```ignore
/// set_default_font_family(FontFamily::Name("Inter".into()));
/// ```
pub fn set_default_font_family(family: FontFamily) {
    DEFAULT_FONT_FAMILY.with(|f| {
        *f.borrow_mut() = family;
    });
}

/// Get the current application-wide default font family.
pub fn default_font_family() -> FontFamily {
    DEFAULT_FONT_FAMILY.with(|f| f.borrow().clone())
}

pub mod prelude {
    pub use crate::animation::{SpringConfig, TimingFunction, Transition};
    pub use crate::layout::{
        Axis, Constraints, CrossAxisAlignment, Flex, Length, MainAxisAlignment, Overlay, Size,
        at_least, at_most, fill,
    };
    pub use crate::platform::{Anchor, KeyboardInteractivity, Layer};
    pub use crate::reactive::{
        Computed, CursorIcon, Effect, IntoMaybeDyn, MaybeDyn, ReadSignal, Service, ServiceContext,
        Signal, WriteSignal, batch, create_computed, create_effect, create_service, create_signal,
        on_cleanup, set_cursor,
    };
    pub use crate::renderer::{PaintContext, Shadow, measure_text};
    pub use crate::surface::{
        SurfaceConfig, SurfaceHandle, SurfaceId, spawn_surface, surface_handle,
    };
    pub use crate::transform::Transform;
    pub use crate::transform_origin::{HorizontalAnchor, TransformOrigin, VerticalAnchor};
    pub use crate::widgets::{
        Border, Color, Container, ContentFit, Event, EventResponse, FontFamily, FontWeight,
        GradientDirection, Image, ImageSource, IntoChildren, Key, LinearGradient, Modifiers,
        MouseButton, Overflow, Padding, Rect, ScrollAxis, ScrollSource, ScrollbarBuilder,
        ScrollbarVisibility, Selection, StateStyle, Text, TextInput, Widget, container, image,
        text, text_input,
    };
    pub use crate::{App, component, default_font_family, set_default_font_family};
}

use std::sync::mpsc::Receiver;

use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};

use crate::{
    jobs::{
        drain_pending_jobs, handle_animation_jobs, handle_layout_jobs, handle_paint_jobs,
        handle_reconcile_jobs, handle_unregister_jobs, has_pending_jobs, init_wakeup,
        take_frame_request,
    },
    tree::{Tree, WidgetId},
};

/// A surface definition that stores configuration and widget factory.
#[allow(clippy::type_complexity)]
struct SurfaceDefinition {
    id: SurfaceId,
    config: SurfaceConfig,
    widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
}

/// Process dynamic surface commands (create, close, property changes).
/// Returns false if all surfaces have been closed and the app should exit.
fn process_surface_commands(
    surface_rx: &Receiver<SurfaceCommand>,
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut platform::WaylandState,
    qh: &QueueHandle<platform::WaylandState>,
    tree: &mut Tree,
) -> bool {
    while let Ok(cmd) = surface_rx.try_recv() {
        match cmd {
            SurfaceCommand::Create {
                id,
                config,
                widget_fn,
            } => {
                log::info!("Creating dynamic surface {:?}", id);

                // Create Wayland surface
                wayland_state.create_surface_with_id(qh, id, &config);

                // Create the widget and managed surface (GPU init happens later)
                let widget = widget_fn();
                let managed = ManagedSurface::new(id, config, widget, tree);
                surface_manager.add(managed);
            }
            SurfaceCommand::Close(id) => {
                log::info!("Closing dynamic surface {:?}", id);
                wayland_state.destroy_surface(id);
                surface_manager.remove(id);

                // If no surfaces left, exit
                if surface_manager.is_empty() {
                    wayland_state.exit = true;
                    return false;
                }
            }
            SurfaceCommand::SetLayer { id, layer } => {
                wayland_state.set_surface_layer(id, layer);
            }
            SurfaceCommand::SetKeyboardInteractivity { id, mode } => {
                wayland_state.set_surface_keyboard_interactivity(id, mode);
            }
            SurfaceCommand::SetAnchor { id, anchor } => {
                wayland_state.set_surface_anchor(id, anchor);
            }
            SurfaceCommand::SetSize { id, width, height } => {
                wayland_state.set_surface_size(id, width, height);
            }
            SurfaceCommand::SetExclusiveZone { id, zone } => {
                wayland_state.set_surface_exclusive_zone(id, zone);
            }
            SurfaceCommand::SetMargin {
                id,
                top,
                right,
                bottom,
                left,
            } => {
                wayland_state.set_surface_margin(id, top, right, bottom, left);
            }
        }
    }
    true
}

/// Walk the render tree after painting and cache each node's output.
/// Also clears needs_paint flags for cached widgets.
fn cache_paint_results(tree: &mut Tree, node: &renderer::RenderNode) {
    let widget_id = WidgetId::from_u64(node.id);
    if tree.contains(widget_id) {
        tree.cache_paint(widget_id, node.clone());
        tree.clear_needs_paint(widget_id);
    }
    for child in &node.children {
        cache_paint_results(tree, child);
    }
}

/// Render a single surface using the hierarchical renderer.
#[allow(clippy::too_many_arguments)]
fn render_surface(
    id: SurfaceId,
    surface: &mut surface_manager::ManagedSurface,
    wayland_state: &mut platform::WaylandState,
    renderer: &mut Renderer,
    connection: &Connection,
    qh: &QueueHandle<platform::WaylandState>,
    tree: &mut Tree,
    layout_roots: &mut HashSet<WidgetId>,
) {
    // Get wayland surface state
    let Some(wayland_surface) = wayland_state.get_surface_mut(id) else {
        return;
    };

    // Skip if not configured yet
    if !wayland_surface.configured {
        return;
    }

    // Get pending events for this surface
    let events = wayland_surface.take_events();
    let scale_factor = wayland_surface.scale_factor;
    let width = wayland_surface.width;
    let height = wayland_surface.height;
    let first_frame_presented = wayland_surface.first_frame_presented;
    let scale_factor_received = wayland_surface.scale_factor_received;
    let wl_surface = wayland_surface.wl_surface.clone();

    // Skip if GPU not ready (will be initialized next frame)
    if !surface.is_gpu_ready() {
        return;
    }

    // Check for paste events
    let has_paste_event = events.iter().any(|e| {
        matches!(
            e,
            widgets::Event::KeyDown {
                key: widgets::Key::Char('v'),
                modifiers: widgets::Modifiers { ctrl: true, .. },
                ..
            }
        )
    });
    if has_paste_event && let Some(text) = wayland_state.read_external_clipboard(connection) {
        set_system_clipboard(text);
    }

    // Dispatch events to widget
    for event in &events {
        tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
            widget.event(tree, id, event);
        });
    }

    // Sync clipboard to Wayland if it changed (copy operations)
    if let Some(text) = take_clipboard_change() {
        wayland_state.set_clipboard(text, qh);
    }

    // Sync cursor to Wayland if it changed
    if let Some(cursor) = take_cursor_change() {
        wayland_state.set_cursor(cursor, qh);
    }

    // Calculate physical pixel dimensions (for HiDPI)
    let scale = scale_factor as u32;
    let physical_width = width * scale;
    let physical_height = height * scale;

    let wgpu_surface = surface.wgpu_surface.as_mut().unwrap();

    // Check for resize or scale change
    let needs_resize =
        wgpu_surface.width() != physical_width || wgpu_surface.height() != physical_height;
    let scale_changed = scale_factor != surface.previous_scale_factor;

    if needs_resize {
        log::info!(
            "Resizing surface {:?} to {}x{} (physical), scale {}",
            id,
            physical_width,
            physical_height,
            scale
        );
        wgpu_surface.resize(physical_width, physical_height);
    }

    if scale_changed {
        log::info!(
            "Surface {:?} scale factor changed: {} -> {}",
            id,
            surface.previous_scale_factor,
            scale_factor
        );
        surface.previous_scale_factor = scale_factor;
    }

    // Check render conditions
    let fully_initialized = first_frame_presented && scale_factor_received;
    let force_render_surface = !fully_initialized;
    let frame_requested = take_frame_request();
    let has_pending_layouts = !layout_roots.is_empty();

    // Only render if something changed (or during initialization)
    if force_render_surface
        || frame_requested
        || has_pending_layouts
        || needs_resize
        || scale_changed
    {
        // Update renderer for this surface
        renderer.set_screen_size(physical_width as f32, physical_height as f32);
        renderer.set_scale_factor(scale_factor);

        // Process pending jobs from signal updates (reconciliation + layout marking)
        let jobs = drain_pending_jobs();

        handle_unregister_jobs(&jobs, tree);

        // Mark paint-dirty widgets from Paint jobs
        handle_paint_jobs(&jobs, tree);

        // Collect layout roots from jobs
        for root in handle_reconcile_jobs(&jobs, tree) {
            layout_roots.insert(root);
        }
        for root in handle_layout_jobs(&jobs, tree) {
            layout_roots.insert(root);
        }

        // Re-layout using partial layout from boundaries when available
        let constraints = Constraints::new(0.0, 0.0, width as f32, height as f32);
        if !layout_roots.is_empty() {
            // Partial layout: only update dirty subtrees starting from boundaries
            let roots: Vec<_> = layout_roots.drain().collect();
            for root_id in &roots {
                // Use cached constraints for boundaries, or fall back to parent constraints
                let cached = tree.cached_constraints(*root_id).unwrap_or(constraints);

                tree.with_widget_mut(*root_id, |widget, id, tree| {
                    widget.layout(tree, id, cached);
                });
            }
            // Layout may reposition children — conservatively mark subtrees as needing paint
            for root_id in &roots {
                tree.mark_subtree_needs_paint(*root_id);
            }
        } else if needs_resize {
            // Full layout from root only when explicitly needed (first frame, resize, etc.)
            tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
                widget.layout(tree, id, constraints);
            });
            tree.mark_subtree_needs_paint(surface.widget_id);
        }
        // If neither condition is true, skip layout entirely - nothing is dirty

        // Force full repaint on resize, scale change, or during initialization
        if force_render_surface || needs_resize || scale_changed {
            tree.mark_subtree_needs_paint(surface.widget_id);
        }

        // Clear and reuse render tree (preserves capacity)
        surface.render_tree.clear();
        surface.root_node.clear();
        surface.root_node.bounds = widgets::Rect::new(0.0, 0.0, width as f32, height as f32);

        tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
            let mut ctx = PaintContext::new(&mut surface.root_node);
            widget.paint(tree, id, &mut ctx);
        });

        // Cache paint results and clear needs_paint flags
        cache_paint_results(tree, &surface.root_node);

        // Take ownership of root node temporarily, add to tree, then restore
        let root = std::mem::replace(&mut surface.root_node, renderer::RenderNode::new(0));
        surface.render_tree.add_root(root);

        // Flatten tree into reused buffer
        flatten_tree_into(&surface.render_tree, &mut surface.flattened_commands);
        renderer.render(
            wgpu_surface,
            &surface.flattened_commands,
            surface.config.background_color,
        );

        // Restore root_node for next frame (take it back from render_tree)
        if let Some(root) = surface.render_tree.roots.pop() {
            surface.root_node = root;
        }

        // Process Animation jobs AFTER paint (advance for next frame)
        // This only processes widgets with Animation jobs, not the entire tree
        handle_animation_jobs(&jobs, tree);

        // Track layout stats (when compiled with --features layout-stats)
        layout_stats::end_frame();

        // Commit surface
        wl_surface.commit();

        // Request frame callback if not yet initialized
        if !first_frame_presented {
            wl_surface.frame(qh, wl_surface.clone());
        }
    }
}

pub struct App {
    /// Surface definitions added via add_surface()
    surface_definitions: Vec<SurfaceDefinition>,
    /// The layout tree for widget storage (owned by App)
    tree: Tree,
    /// Set of layout roots that need re-layout (scheduling concern)
    layout_roots: HashSet<WidgetId>,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface_definitions: Vec::new(),
            tree: Tree::new(),
            layout_roots: HashSet::new(),
        }
    }

    /// Set the application-wide default font family.
    ///
    /// This sets the default font family that will be used by all text widgets
    /// that don't explicitly specify a font family.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new()
    ///     .default_font_family(FontFamily::Name("Inter".into()))
    ///     .add_surface(config, || view)
    ///     .run();
    /// ```
    pub fn default_font_family(self, family: FontFamily) -> Self {
        set_default_font_family(family);
        self
    }

    /// Add a surface to the application.
    ///
    /// This method allows creating multiple layer shell surfaces within a single app.
    /// Each surface has its own widget tree but all surfaces share the same reactive
    /// signals and app state.
    ///
    /// The widget factory closure creates the root widget for the surface.
    ///
    /// Returns a tuple of `(Self, SurfaceId)` where `SurfaceId` can be used to get
    /// a `SurfaceHandle` later via `surface_handle()` to modify surface properties.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let (app, bar_id) = App::new()
    ///     .add_surface(
    ///         SurfaceConfig::new()
    ///             .height(32)
    ///             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    ///             .layer(Layer::Top)
    ///             .namespace("status-bar"),
    ///         || status_bar_widget()
    ///     );
    /// app.run();
    /// ```
    pub fn add_surface<W, F>(mut self, config: SurfaceConfig, widget_fn: F) -> (Self, SurfaceId)
    where
        W: Widget + 'static,
        F: FnOnce() -> W + 'static,
    {
        let id = SurfaceId::next();
        self.surface_definitions.push(SurfaceDefinition {
            id,
            config,
            widget_fn: Box::new(move || Box::new(widget_fn())),
        });
        (self, id)
    }

    /// Run the application.
    ///
    /// This requires at least one surface to have been added via `add_surface()`.
    ///
    /// # Panics
    ///
    /// Panics if no surfaces were added via `add_surface()`.
    pub fn run(mut self) {
        if self.surface_definitions.is_empty() {
            panic!("No surfaces defined. Use add_surface() to add at least one surface.");
        }

        let _ = env_logger::try_init();

        // Initialize the surface command channel for dynamic surface spawning
        let surface_rx = init_surface_commands();

        let (connection, mut event_queue, mut wayland_state, qh) = create_wayland_app();

        // Create surfaces from add_surface() calls
        for def in &self.surface_definitions {
            wayland_state.create_surface_with_id(&qh, def.id, &def.config);
        }

        // Wait for all surfaces to configure
        while !wayland_state.all_surfaces_configured() && !wayland_state.exit {
            event_queue
                .blocking_dispatch(&mut wayland_state)
                .expect("Failed to dispatch events");
        }

        if wayland_state.exit {
            return;
        }

        // Create shared GPU context
        let gpu_context = GpuContext::new();

        // Create surface manager and runtime entries for each surface
        let mut surface_manager = SurfaceManager::new();
        let mut renderer: Option<Renderer> = None;

        // Create entries for surfaces added via add_surface()
        for def in self.surface_definitions.drain(..) {
            let wayland_surface = wayland_state
                .get_surface(def.id)
                .expect("Surface should exist after configure");

            // Create the widget and managed surface
            let widget = (def.widget_fn)();
            let mut managed = ManagedSurface::new(def.id, def.config, widget, &mut self.tree);

            // Initialize GPU surface
            managed.init_gpu(
                &gpu_context,
                &connection,
                &wayland_surface.wl_surface,
                wayland_surface.width,
                wayland_surface.height,
                wayland_surface.scale_factor,
                &mut self.tree,
            );

            // Create renderer from first surface
            if renderer.is_none()
                && let Some(ref wgpu_surface) = managed.wgpu_surface
            {
                let r = Renderer::new(
                    wgpu_surface.device.clone(),
                    wgpu_surface.queue.clone(),
                    wgpu_surface.config.format,
                );
                renderer = Some(r);
            }

            surface_manager.add(managed);
        }

        let mut renderer = renderer.expect("At least one surface should exist");

        // Create calloop event loop for event-driven execution
        let mut event_loop: EventLoop<platform::WaylandState> =
            EventLoop::try_new().expect("Failed to create event loop");
        let loop_handle = event_loop.handle();

        // Create ping mechanism for wakeup on signal changes
        let (ping, ping_source) = make_ping().expect("Failed to create ping");
        init_wakeup(ping);

        // Insert ping source - this wakes the loop when signals change
        loop_handle
            .insert_source(ping_source, |_, _, _| {
                // Ping received - a signal was updated, frame will be rendered
            })
            .expect("Failed to insert ping source");

        // Insert Wayland source - this handles all Wayland protocol events
        WaylandSource::new(connection.clone(), event_queue)
            .insert(loop_handle.clone())
            .expect("Failed to insert Wayland source");

        // Main loop - event-driven, blocks until Wayland event or signal update
        loop {
            // Check if all surfaces are fully initialized
            let any_surface_needs_init = wayland_state.any_surface_needs_render();
            let force_render = any_surface_needs_init;

            // Check if we need to actively poll (jobs pushed during previous frame)
            let has_pending = has_pending_jobs();
            let needs_polling = has_pending || force_render;

            // Dispatch events from calloop:
            // - If polling needed (animations/callbacks/init), use timeout
            // - Otherwise block until event (Wayland or ping wakeup)
            let timeout = if needs_polling {
                Some(std::time::Duration::from_millis(16)) // ~60fps for animations
            } else {
                None // Block indefinitely until event
            };

            event_loop
                .dispatch(timeout, &mut wayland_state)
                .expect("Failed to dispatch event loop");

            if wayland_state.exit {
                break;
            }

            // Process dynamic surface commands
            if !process_surface_commands(
                &surface_rx,
                &mut surface_manager,
                &mut wayland_state,
                &qh,
                &mut self.tree,
            ) {
                break;
            }

            // Initialize GPU for any pending surfaces (newly created dynamic surfaces)
            surface_manager.init_pending_gpu(
                &gpu_context,
                &connection,
                &wayland_state,
                &mut self.tree,
            );

            // Render each surface
            let surface_ids: Vec<SurfaceId> = surface_manager.ids().collect();
            for id in surface_ids {
                let Some(surface) = surface_manager.get_mut(id) else {
                    continue;
                };
                render_surface(
                    id,
                    surface,
                    &mut wayland_state,
                    &mut renderer,
                    &connection,
                    &qh,
                    &mut self.tree,
                    &mut self.layout_roots,
                );
            }

            // Flush the connection once for all surfaces
            connection.flush().expect("Failed to flush connection");
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
