pub mod animation;
pub mod image_metadata;
mod jobs;
pub mod layout;
pub mod reactive;
pub mod render_stats;
pub mod surface;
mod surface_manager;
pub mod transform;
pub mod transform_origin;
pub mod tree;
pub mod widget_ref;
pub mod widgets;

// These modules are public for advanced use cases
pub mod platform;
pub mod renderer;

// Re-export macros
pub use guido_macros::{SignalFields, component};

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use layout::Constraints;
use platform::create_wayland_app;
use reactive::owner::with_owner;
use reactive::{OwnerId, set_system_clipboard, take_clipboard_change, take_cursor_change};
use renderer::{GpuContext, PaintContext, Renderer, flatten_tree_into};
use surface::{SurfaceCommand, SurfaceConfig, SurfaceId, drain_surface_commands};
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
    static CUSTOM_FONTS: RefCell<Vec<Arc<Vec<u8>>>> = const { RefCell::new(Vec::new()) };
    static FONTS_CONSUMED: Cell<bool> = const { Cell::new(false) };
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

/// Load custom font data into the application.
///
/// The font bytes will be loaded into all internal FontSystem instances,
/// making the font available for use via `FontFamily::Name(...)`.
///
/// This should be called before creating any widgets or surfaces.
///
/// # Example
///
/// ```ignore
/// const NERD_FONT: &[u8] = include_bytes!("../assets/MyFont.ttf");
/// guido::load_font(NERD_FONT.to_vec());
/// ```
pub fn load_font(data: Vec<u8>) {
    if FONTS_CONSUMED.with(|f| f.get()) {
        log::warn!(
            "load_font() called after FontSystem initialization — \
             this font will not be available. Call load_font() before App::run()."
        );
    }
    CUSTOM_FONTS.with(|fonts| {
        fonts.borrow_mut().push(Arc::new(data));
    });
}

/// Take all registered custom font data (for loading into FontSystems).
///
/// This drains the storage so the `Arc` pointers are released once all
/// FontSystems have been initialized. Subsequent calls return an empty vec.
pub(crate) fn take_registered_fonts() -> Vec<Arc<Vec<u8>>> {
    FONTS_CONSUMED.with(|f| f.set(true));
    CUSTOM_FONTS.with(|fonts| std::mem::take(&mut *fonts.borrow_mut()))
}

pub mod prelude {
    pub use crate::animation::{SpringConfig, TimingFunction, Transition};
    pub use crate::layout::{
        Axis, Constraints, CrossAlignment, Flex, Length, MainAlignment, Overlay, Size, at_least,
        at_most, fill,
    };
    pub use crate::platform::{Anchor, KeyboardInteractivity, Layer};
    pub use crate::reactive::{
        CursorIcon, Memo, Service, Signal, WriteSignal, create_effect, create_memo, create_service,
        create_signal, on_cleanup, set_cursor,
    };
    pub use crate::renderer::{PaintContext, Shadow, measure_text};
    pub use crate::surface::{
        SurfaceConfig, SurfaceHandle, SurfaceId, spawn_surface, surface_handle,
    };
    pub use crate::transform::Transform;
    pub use crate::transform_origin::{HorizontalAnchor, TransformOrigin, VerticalAnchor};
    pub use crate::widget_ref::{WidgetRef, create_widget_ref};
    pub use crate::widgets::{
        Border, Color, Container, ContentFit, Event, EventResponse, FontFamily, FontWeight,
        GradientDirection, Image, ImageSource, IntoChildren, Key, LinearGradient, Modifiers,
        MouseButton, Overflow, Padding, Rect, ScrollAxis, ScrollSource, ScrollbarBuilder,
        ScrollbarVisibility, Selection, StateStyle, Text, TextInput, Widget, container, image,
        text, text_input,
    };
    pub use crate::{
        App, SignalFields, component, default_font_family, load_font, set_default_font_family,
    };
}

use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};

use crate::{
    jobs::{
        drain_non_animation_jobs, drain_pending_jobs, handle_animation_jobs, handle_layout_jobs,
        handle_paint_jobs, handle_reconcile_jobs, handle_unregister_jobs, has_pending_jobs,
        init_wakeup, take_frame_request,
    },
    tree::{DamageRegion, Tree, WidgetId},
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
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut platform::WaylandState,
    qh: &QueueHandle<platform::WaylandState>,
    tree: &mut Tree,
) -> bool {
    for cmd in drain_surface_commands() {
        match cmd {
            SurfaceCommand::Create {
                id,
                config,
                widget_fn,
            } => {
                log::info!("Creating dynamic surface {:?}", id);

                // Create Wayland surface
                wayland_state.create_surface_with_id(qh, id, &config);

                // Create the widget inside an owner scope so that signals/effects
                // created in the factory are properly owned.
                let (widget, owner_id) = with_owner(widget_fn);
                let managed = ManagedSurface::new(id, config, widget, owner_id, tree);
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
/// Skips cache-reused nodes (repainted == false) since their tree cache is already valid.
/// Also clears needs_paint flags for freshly painted widgets.
fn cache_paint_results(tree: &mut Tree, node: &renderer::RenderNode) {
    if node.repainted {
        let widget_id = WidgetId::from_u64(node.id);
        if tree.contains(widget_id) {
            tree.cache_paint(widget_id, node.clone());
            tree.clear_needs_paint(widget_id);
        }
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
    layout_roots: &mut Vec<WidgetId>,
    frame_requested: bool,
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

    // Process ALL pending jobs BEFORE paint.
    // This includes Animation jobs which were previously deferred to end-of-frame.
    // Processing animations here ensures hover/pressed state changes update animation
    // targets before paint, eliminating the 1-frame delay where animated values were
    // stale because advance_animations() hadn't run yet.
    //
    // Order matters:
    // 1. Unregister — cleanup removed widgets first
    // 2. Animation — advance animated values (width/height/bg) so layout and paint
    //    see current values. May push Layout/Paint follow-up jobs.
    // 3. Reconcile — create/remove dynamic children. May push Layout follow-ups.
    // 4. Layout — needs updated animation values + reconciled children
    // 5. Paint — needs final layout positions
    //
    // Cross-surface note: in multi-surface apps, one surface's drain may advance
    // animations for widgets belonging to another surface. The second surface then
    // picks up continuation jobs and re-advances. This is practically harmless —
    // advance() is time-based (computes nearly the same value), and follow-up
    // jobs are deduped by the JobQueue HashSet.
    let mut jobs = drain_pending_jobs();
    handle_unregister_jobs(&jobs, tree);
    handle_animation_jobs(&jobs, tree);
    handle_reconcile_jobs(&jobs, tree, layout_roots);

    // Merge follow-up jobs from animation advances and reconciliation
    jobs.extend(drain_non_animation_jobs());

    handle_paint_jobs(&jobs, tree);
    handle_layout_jobs(&jobs, tree, layout_roots);

    // Check render conditions
    let fully_initialized = first_frame_presented && scale_factor_received;
    let force_render_surface = !fully_initialized;
    let has_pending_layouts = !layout_roots.is_empty();

    // Only render if something changed (or during initialization)
    if force_render_surface
        || frame_requested
        || has_pending_layouts
        || needs_resize
        || scale_changed
        || tree.needs_paint(surface.widget_id)
    {
        // Update renderer for this surface
        renderer.set_screen_size(physical_width as f32, physical_height as f32);
        renderer.set_scale_factor(scale_factor);

        // Re-layout using partial layout from boundaries when available
        let constraints = Constraints::new(0.0, 0.0, width as f32, height as f32);
        if !layout_roots.is_empty() {
            // Partial layout: only update dirty subtrees starting from boundaries
            let mut roots = Vec::new();
            std::mem::swap(&mut roots, layout_roots);
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

        // Update widget ref signals with current bounds after layout
        widget_ref::update_widget_refs(tree);

        // Force full repaint on resize, scale change, or during initialization
        if force_render_surface || needs_resize || scale_changed {
            tree.mark_subtree_needs_paint(surface.widget_id);
        }

        // Skip frame if nothing needs paint
        if !tree.needs_paint(surface.widget_id) {
            render_stats::record_frame_skipped();
            render_stats::end_frame(&DamageRegion::None);
            return;
        }

        // Clear and reuse render tree (preserves capacity)
        surface.render_tree.clear();
        surface.root_node.clear();
        surface.root_node.bounds = widgets::Rect::new(0.0, 0.0, width as f32, height as f32);

        tree.with_widget_mut(surface.widget_id, |widget, id, tree| {
            let mut ctx = PaintContext::new(&mut surface.root_node);
            widget.paint(tree, id, &mut ctx);
        });

        // Take ownership of root node temporarily, add to tree, then restore
        let root = std::mem::replace(
            &mut surface.root_node,
            renderer::RenderNode::new(surface.widget_id.as_u64()),
        );
        surface.render_tree.add_root(root);

        // Flatten tree into reused buffer
        flatten_tree_into(&mut surface.render_tree, &mut surface.flattened_commands);
        renderer.render(
            wgpu_surface,
            &surface.flattened_commands,
            surface.config.background_color,
        );

        // Restore root_node for next frame (take it back from render_tree)
        if let Some(root) = surface.render_tree.roots.pop() {
            surface.root_node = root;
        }

        // Cache paint results AFTER flatten so cached_flatten data is preserved.
        // This enables incremental flatten for paint-cached nodes on subsequent frames.
        cache_paint_results(tree, &surface.root_node);

        // Report damage region to Wayland compositor
        let damage = tree.take_damage();

        // Track render stats (when compiled with --features render-stats)
        render_stats::record_frame_painted();
        render_stats::end_frame(&damage);
        match damage {
            DamageRegion::None => {
                // Shouldn't happen since we're rendering, but report full damage to be safe
                wl_surface.damage_buffer(0, 0, physical_width as i32, physical_height as i32);
            }
            DamageRegion::Partial(rect) => {
                let scale = scale_factor;
                wl_surface.damage_buffer(
                    (rect.x * scale) as i32,
                    (rect.y * scale) as i32,
                    (rect.width * scale).ceil() as i32,
                    (rect.height * scale).ceil() as i32,
                );
            }
            DamageRegion::Full => {
                wl_surface.damage_buffer(0, 0, physical_width as i32, physical_height as i32);
            }
        }

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
    /// Layout roots that need re-layout (Vec with dedup — typically 1–3 per frame)
    layout_roots: Vec<WidgetId>,
    /// Root owner for the reactive graph. When disposed, cascades cleanup
    /// through all signals, effects, and cleanup callbacks.
    root_owner_id: Option<OwnerId>,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface_definitions: Vec::new(),
            tree: Tree::new(),
            layout_roots: Vec::new(),
            root_owner_id: None,
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
    /// Returns a `SurfaceId` that can be used to get a `SurfaceHandle` later via
    /// `surface_handle()` to modify surface properties.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new().run(|app| {
    ///     let bar_id = app.add_surface(
    ///         SurfaceConfig::new()
    ///             .height(32)
    ///             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    ///             .layer(Layer::Top)
    ///             .namespace("status-bar"),
    ///         || status_bar_widget()
    ///     );
    /// });
    /// ```
    pub fn add_surface<W, F>(&mut self, config: SurfaceConfig, widget_fn: F) -> SurfaceId
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
        id
    }

    /// Run the application with a setup closure.
    ///
    /// The setup closure runs inside a root owner scope — all signals, effects,
    /// and other reactive primitives created within it are automatically cleaned
    /// up when the `App` is dropped. Use `app.add_surface()` inside the closure
    /// to define surfaces.
    ///
    /// # Panics
    ///
    /// Panics if no surfaces were added via `add_surface()` inside the closure.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new().run(|app| {
    ///     let count = create_signal(0);
    ///     app.add_surface(config, move || build_ui(count));
    /// });
    /// ```
    pub fn run(mut self, setup: impl FnOnce(&mut Self)) {
        // Create root owner scope — all signals/effects created in setup are owned
        self.root_owner_id = Some(reactive::create_root_owner());
        setup(&mut self);

        if self.surface_definitions.is_empty() {
            panic!("No surfaces defined. Use add_surface() to add at least one surface.");
        }

        let _ = env_logger::try_init();

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

            // Create the widget inside an owner scope so that signals/effects
            // created in the factory (e.g. create_memo) are properly owned.
            let (widget, owner_id) = with_owner(|| (def.widget_fn)());
            let mut managed =
                ManagedSurface::new(def.id, def.config, widget, owner_id, &mut self.tree);

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

            // Flush background-thread signal writes once per frame (queued via WriteSignal).
            // Must run before take_frame_request() so that signal changes from bg writes
            // are processed into jobs before we check the frame request flag.
            reactive::flush_bg_writes();

            // Check frame request once for all surfaces (not per-surface)
            let frame_requested = take_frame_request();

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
                    frame_requested,
                );
            }

            // Flush the connection once for all surfaces
            connection.flush().expect("Failed to flush connection");
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Dispose the root owner first — cascades cleanup through the entire
        // reactive graph (signals, effects, cleanup callbacks).
        if let Some(root_id) = self.root_owner_id {
            reactive::dispose_owner(root_id);
        }

        // Reset all thread-local and static state so the next App can start clean.
        reactive::reset_reactive();
        jobs::reset_jobs();
        surface::reset_surface_commands();
        widget_ref::reset_widget_refs();
        FONTS_CONSUMED.with(|f| f.set(false));
        self.tree.clear();
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
