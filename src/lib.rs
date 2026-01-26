pub mod animation;
pub mod image_metadata;
pub mod layout;
pub mod reactive;
pub mod surface;
pub mod transform;
pub mod transform_origin;
pub mod widgets;

// These modules are public for advanced use cases
pub mod platform;
pub mod renderer;

// Re-export macros
pub use guido_macros::component;

use std::collections::HashMap;

use layout::Constraints;
use platform::{WaylandWindowWrapper, create_wayland_app};
use reactive::{
    clear_animation_flag, init_wakeup, set_system_clipboard, take_clipboard_change,
    take_cursor_change, take_frame_request, with_app_state, with_app_state_mut,
};
use renderer::{GpuContext, Renderer, SurfaceState};
use surface::{SurfaceCommand, SurfaceConfig, SurfaceId, init_surface_commands};
use widgets::Widget;

// Calloop imports for event-driven main loop (via smithay-client-toolkit re-exports)
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::ping::make_ping;
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;

pub mod prelude {
    pub use crate::animation::{SpringConfig, TimingFunction, Transition};
    pub use crate::layout::{
        Axis, Constraints, CrossAxisAlignment, Flex, Length, MainAxisAlignment, Overlay, Size,
        at_least, at_most, fill,
    };
    pub use crate::platform::{Anchor, Layer};
    pub use crate::reactive::{
        Computed, CursorIcon, Effect, IntoMaybeDyn, MaybeDyn, ReadSignal, Signal, WriteSignal,
        batch, create_computed, create_effect, create_signal, set_cursor,
    };
    pub use crate::renderer::primitives::Shadow;
    pub use crate::renderer::{PaintContext, measure_text};
    pub use crate::surface::{SurfaceConfig, SurfaceHandle, SurfaceId, spawn_surface};
    pub use crate::transform::Transform;
    pub use crate::transform_origin::{HorizontalAnchor, TransformOrigin, VerticalAnchor};
    pub use crate::widgets::{
        Border, Color, Container, ContentFit, Event, EventResponse, GradientDirection, Image,
        ImageSource, IntoChildren, Key, LinearGradient, Modifiers, MouseButton, Overflow, Padding,
        Rect, ScrollAxis, ScrollSource, ScrollbarBuilder, ScrollbarVisibility, Selection,
        StateStyle, Text, TextInput, Widget, container, image, text, text_input,
    };
    pub use crate::{App, component};
}

/// A callback that gets called each frame before rendering.
/// Use this to process external events (like channel messages) and update signals.
pub type UpdateCallback = Box<dyn FnMut()>;

/// A surface definition that stores configuration and widget factory.
struct SurfaceDefinition {
    id: SurfaceId,
    config: SurfaceConfig,
    widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
}

/// Per-surface runtime state during the event loop.
struct SurfaceEntry {
    #[allow(dead_code)] // Keep for debugging purposes
    id: SurfaceId,
    config: SurfaceConfig,
    widget: Box<dyn Widget>,
    paint_ctx: renderer::PaintContext,
    wgpu_surface: Option<SurfaceState>,
    previous_scale_factor: f32,
}

pub struct App {
    on_update: Option<UpdateCallback>,
    /// Surface definitions added via add_surface()
    surface_definitions: Vec<SurfaceDefinition>,
}

impl App {
    pub fn new() -> Self {
        Self {
            on_update: None,
            surface_definitions: Vec::new(),
        }
    }

    /// Set a callback that gets called each frame before rendering.
    /// Use this to process external events (like channel messages from background threads)
    /// and update signals.
    ///
    /// # Example
    /// ```ignore
    /// let (tx, rx) = std::sync::mpsc::channel();
    /// let count = create_signal(0);
    ///
    /// // Spawn background thread that sends updates
    /// std::thread::spawn(move || {
    ///     loop {
    ///         std::thread::sleep(Duration::from_secs(1));
    ///         tx.send(1).ok();
    ///     }
    /// });
    ///
    /// App::new()
    ///     .on_update(move || {
    ///         // Process all pending messages
    ///         while let Ok(delta) = rx.try_recv() {
    ///             count.update(|c| *c += delta);
    ///         }
    ///     })
    ///     .run(view);
    /// ```
    pub fn on_update<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_update = Some(Box::new(callback));
        self
    }

    /// Add a surface to the application.
    ///
    /// This method allows creating multiple layer shell surfaces within a single app.
    /// Each surface has its own widget tree but all surfaces share the same reactive
    /// signals and app state.
    ///
    /// # Example
    ///
    /// ```ignore
    /// App::new()
    ///     .add_surface(
    ///         SurfaceConfig::new()
    ///             .height(32)
    ///             .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
    ///             .layer(Layer::Top)
    ///             .namespace("status-bar"),
    ///         move || status_bar_widget()
    ///     )
    ///     .add_surface(
    ///         SurfaceConfig::new()
    ///             .height(48)
    ///             .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
    ///             .layer(Layer::Top)
    ///             .namespace("dock"),
    ///         move || dock_widget()
    ///     )
    ///     .run();
    /// ```
    pub fn add_surface<W, F>(mut self, config: SurfaceConfig, widget_fn: F) -> Self
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
        self
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
            wayland_state.create_surface_with_id(
                &qh,
                def.id,
                def.config.width,
                def.config.height,
                def.config.anchor,
                def.config.layer,
                &def.config.namespace,
                def.config.exclusive_zone,
            );
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

        // Create runtime entries for each surface
        let mut surface_entries: HashMap<SurfaceId, SurfaceEntry> = HashMap::new();
        let mut renderer: Option<Renderer> = None;

        // Create entries for surfaces added via add_surface()
        for def in self.surface_definitions.drain(..) {
            let wayland_surface = wayland_state
                .get_surface(def.id)
                .expect("Surface should exist after configure");

            let window_handle = WaylandWindowWrapper::new(&connection, &wayland_surface.wl_surface);

            let initial_scale = wayland_surface.scale_factor.max(1.0) as u32;
            let physical_width = wayland_surface.width * initial_scale;
            let physical_height = wayland_surface.height * initial_scale;

            log::info!(
                "Creating wgpu surface for {:?}: logical {}x{}, physical {}x{}, scale {}",
                def.id,
                wayland_surface.width,
                wayland_surface.height,
                physical_width,
                physical_height,
                initial_scale
            );

            let wgpu_surface =
                gpu_context.create_surface(window_handle, physical_width, physical_height);

            if renderer.is_none() {
                let r = Renderer::new(
                    wgpu_surface.device.clone(),
                    wgpu_surface.queue.clone(),
                    wgpu_surface.config.format,
                );
                renderer = Some(r);
            }

            let mut widget = (def.widget_fn)();

            let constraints = Constraints::new(
                0.0,
                0.0,
                wayland_surface.width as f32,
                wayland_surface.height as f32,
            );
            widget.layout(constraints);
            widget.set_origin(0.0, 0.0);

            surface_entries.insert(
                def.id,
                SurfaceEntry {
                    id: def.id,
                    config: def.config,
                    widget,
                    paint_ctx: renderer::PaintContext::with_capacity(32, 16, 8),
                    wgpu_surface: Some(wgpu_surface),
                    previous_scale_factor: wayland_surface.scale_factor,
                },
            );
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

            // Check if we need to actively poll (from previous frame's animations)
            let has_animations = with_app_state(|state| state.has_animations);
            let needs_polling = has_animations || self.on_update.is_some() || force_render;

            // Clear animation flag - widgets will set it during layout if they need another frame
            clear_animation_flag();

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
            while let Ok(cmd) = surface_rx.try_recv() {
                match cmd {
                    SurfaceCommand::Create {
                        id,
                        config,
                        widget_fn,
                    } => {
                        log::info!("Creating dynamic surface {:?}", id);

                        // Create Wayland surface
                        wayland_state.create_surface_with_id(
                            &qh,
                            id,
                            config.width,
                            config.height,
                            config.anchor,
                            config.layer,
                            &config.namespace,
                            config.exclusive_zone,
                        );

                        // Create the widget
                        let widget = widget_fn();

                        surface_entries.insert(
                            id,
                            SurfaceEntry {
                                id,
                                config,
                                widget,
                                paint_ctx: renderer::PaintContext::with_capacity(32, 16, 8),
                                wgpu_surface: None, // Will be created after configure
                                previous_scale_factor: 1.0,
                            },
                        );
                    }
                    SurfaceCommand::Close(id) => {
                        log::info!("Closing dynamic surface {:?}", id);
                        wayland_state.destroy_surface(id);
                        surface_entries.remove(&id);

                        // If no surfaces left, exit
                        if surface_entries.is_empty() {
                            wayland_state.exit = true;
                        }
                    }
                }
            }

            // Call the update callback to process external events
            if let Some(ref mut callback) = self.on_update {
                callback();
            }

            // Process each surface
            let surface_ids: Vec<SurfaceId> = surface_entries.keys().copied().collect();

            for id in surface_ids {
                // Get wayland surface state
                let Some(wayland_surface) = wayland_state.get_surface_mut(id) else {
                    continue;
                };

                // Skip if not configured yet
                if !wayland_surface.configured {
                    continue;
                }

                // Get pending events for this surface
                let events = wayland_surface.take_events();
                let scale_factor = wayland_surface.scale_factor;
                let width = wayland_surface.width;
                let height = wayland_surface.height;
                let first_frame_presented = wayland_surface.first_frame_presented;
                let scale_factor_received = wayland_surface.scale_factor_received;
                let wl_surface = wayland_surface.wl_surface.clone();

                // Get surface entry
                let Some(entry) = surface_entries.get_mut(&id) else {
                    continue;
                };

                // Create wgpu surface if needed (for newly configured dynamic surfaces)
                if entry.wgpu_surface.is_none() {
                    let window_handle = WaylandWindowWrapper::new(&connection, &wl_surface);
                    let initial_scale = scale_factor.max(1.0) as u32;
                    let physical_width = width * initial_scale;
                    let physical_height = height * initial_scale;

                    log::info!(
                        "Creating wgpu surface for dynamic surface {:?}: {}x{}",
                        id,
                        physical_width,
                        physical_height
                    );

                    let wgpu_surface =
                        gpu_context.create_surface(window_handle, physical_width, physical_height);
                    entry.wgpu_surface = Some(wgpu_surface);

                    // Initial layout
                    let constraints = Constraints::new(0.0, 0.0, width as f32, height as f32);
                    entry.widget.layout(constraints);
                    entry.widget.set_origin(0.0, 0.0);
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
                if has_paste_event
                    && let Some(text) = wayland_state.read_external_clipboard(&connection)
                {
                    set_system_clipboard(text);
                }

                // Dispatch events to widget
                for event in events {
                    entry.widget.event(&event);
                }

                // Sync clipboard to Wayland if it changed (copy operations)
                if let Some(text) = take_clipboard_change() {
                    wayland_state.set_clipboard(text, &qh);
                }

                // Sync cursor to Wayland if it changed
                if let Some(cursor) = take_cursor_change() {
                    wayland_state.set_cursor(cursor, &qh);
                }

                // Calculate physical pixel dimensions (for HiDPI)
                let scale = scale_factor as u32;
                let physical_width = width * scale;
                let physical_height = height * scale;

                let wgpu_surface = entry.wgpu_surface.as_mut().unwrap();

                // Check for resize or scale change
                let needs_resize = wgpu_surface.width() != physical_width
                    || wgpu_surface.height() != physical_height;
                let scale_changed = scale_factor != entry.previous_scale_factor;

                if needs_resize {
                    log::info!(
                        "Resizing surface {:?} to {}x{} (physical), scale {}",
                        id,
                        physical_width,
                        physical_height,
                        scale
                    );
                    wgpu_surface.resize(physical_width, physical_height);

                    // Mark that we need layout and paint due to resize
                    with_app_state_mut(|state| {
                        state.change_flags |= reactive::ChangeFlags::NEEDS_LAYOUT
                            | reactive::ChangeFlags::NEEDS_PAINT;
                    });
                }

                if scale_changed {
                    log::info!(
                        "Surface {:?} scale factor changed: {} -> {}",
                        id,
                        entry.previous_scale_factor,
                        scale_factor
                    );
                    entry.previous_scale_factor = scale_factor;

                    // Mark that we need to re-render with new scale factor
                    with_app_state_mut(|state| {
                        state.change_flags |= reactive::ChangeFlags::NEEDS_PAINT;
                    });
                }

                // Check render conditions
                let fully_initialized = first_frame_presented && scale_factor_received;
                let force_render_surface = !fully_initialized;
                let frame_requested = take_frame_request();
                let needs_layout = with_app_state(|state| state.needs_layout());
                let needs_paint = with_app_state(|state| state.needs_paint());
                let has_animations_now = with_app_state(|state| state.has_animations);

                // Only render if something changed (or during initialization)
                if force_render_surface
                    || frame_requested
                    || needs_layout
                    || needs_paint
                    || needs_resize
                    || scale_changed
                    || has_animations_now
                {
                    // Update renderer for this surface
                    renderer.set_screen_size(physical_width as f32, physical_height as f32);
                    renderer.set_scale_factor(scale_factor);

                    // Re-layout (for reactive updates)
                    let constraints = Constraints::new(0.0, 0.0, width as f32, height as f32);
                    entry.widget.layout(constraints);

                    // Paint - clear and reuse existing context to avoid allocations
                    entry.paint_ctx.clear();
                    entry.widget.paint(&mut entry.paint_ctx);

                    renderer.render(
                        wgpu_surface,
                        &entry.paint_ctx,
                        entry.config.background_color,
                    );

                    // Clear flags after rendering
                    with_app_state_mut(|state| {
                        state.clear_layout_flag();
                        state.clear_paint_flag();
                    });

                    // Commit surface
                    wl_surface.commit();

                    // Request frame callback if not yet initialized
                    if !first_frame_presented {
                        wl_surface.frame(&qh, wl_surface.clone());
                    }
                }
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
