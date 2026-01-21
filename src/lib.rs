pub mod layout;
pub mod reactive;
pub mod widgets;

// These modules are public for advanced use cases
pub mod platform;
pub mod renderer;

use layout::Constraints;
use platform::{create_wayland_app, Anchor, Layer, WaylandWindowWrapper};
use reactive::{
    clear_animation_flag, init_wakeup, take_frame_request, with_app_state, with_app_state_mut,
};
use renderer::{GpuContext, Renderer};
use widgets::{Color, Widget};

// Calloop imports for event-driven main loop
use calloop::ping::make_ping;
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;

pub mod prelude {
    pub use crate::layout::{
        Axis, Constraints, CrossAxisAlignment, Flex, MainAxisAlignment, Size,
    };
    pub use crate::platform::{Anchor, Layer};
    pub use crate::reactive::{
        batch, create_computed, create_effect, create_signal, Computed, Effect, IntoMaybeDyn,
        MaybeDyn, ReadSignal, Signal, WriteSignal,
    };
    pub use crate::renderer::primitives::Shadow;
    pub use crate::renderer::{measure_text, PaintContext};
    pub use crate::widgets::{
        container, text, Border, Color, Container, Event, EventResponse,
        GradientDirection, IntoChildren, LinearGradient, MouseButton, Padding, Rect, ScrollSource, Text, Widget,
    };
    pub use crate::{App, AppConfig};
}

pub struct AppConfig {
    pub width: u32,
    pub height: u32,
    pub anchor: Anchor,
    pub layer: Layer,
    pub namespace: String,
    pub background_color: Color,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 32,
            anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
            layer: Layer::Top,
            namespace: "guido".to_string(),
            background_color: Color::rgb(0.1, 0.1, 0.15),
        }
    }
}

/// A callback that gets called each frame before rendering.
/// Use this to process external events (like channel messages) and update signals.
pub type UpdateCallback = Box<dyn FnMut()>;

pub struct App {
    config: AppConfig,
    on_update: Option<UpdateCallback>,
}

impl App {
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
            on_update: None,
        }
    }

    pub fn with_config(config: AppConfig) -> Self {
        Self {
            config,
            on_update: None,
        }
    }

    pub fn width(mut self, width: u32) -> Self {
        self.config.width = width;
        self
    }

    pub fn height(mut self, height: u32) -> Self {
        self.config.height = height;
        self
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.config.anchor = anchor;
        self
    }

    pub fn layer(mut self, layer: Layer) -> Self {
        self.config.layer = layer;
        self
    }

    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.config.namespace = namespace.into();
        self
    }

    pub fn background_color(mut self, color: Color) -> Self {
        self.config.background_color = color;
        self
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

    pub fn run<W: Widget + 'static>(mut self, mut root: W) {
        env_logger::init();

        let (connection, mut event_queue, mut wayland_state, qh) = create_wayland_app();

        wayland_state.create_layer_surface(
            &qh,
            self.config.width,
            self.config.height,
            self.config.anchor,
            self.config.layer,
            &self.config.namespace,
        );

        // Wait for configure
        while !wayland_state.configured && !wayland_state.exit {
            event_queue
                .blocking_dispatch(&mut wayland_state)
                .expect("Failed to dispatch events");
        }

        if wayland_state.exit {
            return;
        }

        let gpu_context = GpuContext::new();
        let window_handle = WaylandWindowWrapper::new(
            &connection,
            wayland_state.surface.as_ref().expect("No surface"),
        );

        // Use physical pixel dimensions for the surface
        let initial_scale = wayland_state.scale_factor.max(1.0) as u32;
        let physical_width = wayland_state.width * initial_scale;
        let physical_height = wayland_state.height * initial_scale;

        log::info!(
            "Creating surface: logical {}x{}, physical {}x{}, scale {}",
            wayland_state.width,
            wayland_state.height,
            physical_width,
            physical_height,
            initial_scale
        );

        let mut surface =
            gpu_context.create_surface(window_handle, physical_width, physical_height);

        let mut renderer = Renderer::new(
            surface.device.clone(),
            surface.queue.clone(),
            surface.config.format,
        );

        renderer.set_screen_size(physical_width as f32, physical_height as f32);
        renderer.set_scale_factor(wayland_state.scale_factor);

        // Initial layout
        let constraints = Constraints::new(
            0.0,
            0.0,
            wayland_state.width as f32,
            wayland_state.height as f32,
        );
        root.layout(constraints);
        root.set_origin(0.0, 0.0);

        // Track previous scale factor to detect changes
        let mut previous_scale_factor = wayland_state.scale_factor;

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
            // Event-driven initialization detection
            let fully_initialized = wayland_state.first_frame_presented
                && wayland_state.scale_factor_received;
            let force_render = !fully_initialized;

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

            // Call the update callback to process external events
            if let Some(ref mut callback) = self.on_update {
                callback();
            }

            // Dispatch input events to widgets
            for event in wayland_state.take_events() {
                root.event(&event);
            }

            // Calculate physical pixel dimensions (for HiDPI)
            let scale = wayland_state.scale_factor as u32;
            let physical_width = wayland_state.width * scale;
            let physical_height = wayland_state.height * scale;

            // Check for resize or scale change
            let needs_resize =
                surface.width() != physical_width || surface.height() != physical_height;
            let scale_changed = wayland_state.scale_factor != previous_scale_factor;

            if needs_resize {
                log::info!(
                    "Resizing surface to {}x{} (physical), scale {}",
                    physical_width,
                    physical_height,
                    scale
                );
                surface.resize(physical_width, physical_height);
                renderer.set_screen_size(physical_width as f32, physical_height as f32);

                // Mark that we need layout and paint due to resize
                with_app_state_mut(|state| {
                    state.change_flags |=
                        reactive::ChangeFlags::NEEDS_LAYOUT | reactive::ChangeFlags::NEEDS_PAINT;
                });
            }

            // Update scale factor and mark for re-render if changed
            if scale_changed {
                log::info!(
                    "Scale factor changed: {} -> {}",
                    previous_scale_factor,
                    wayland_state.scale_factor
                );
                previous_scale_factor = wayland_state.scale_factor;

                // Mark that we need to re-render with new scale factor
                with_app_state_mut(|state| {
                    state.change_flags |= reactive::ChangeFlags::NEEDS_PAINT;
                });
            }

            // Update scale factor
            renderer.set_scale_factor(wayland_state.scale_factor);

            // Re-check render conditions after event dispatch (events may have triggered changes)
            let frame_requested = take_frame_request();
            let needs_layout = with_app_state(|state| state.needs_layout());
            let needs_paint = with_app_state(|state| state.needs_paint());
            // Re-check animations after event dispatch (events may have started new animations)
            let has_animations_now = with_app_state(|state| state.has_animations);

            // Only render if something changed (or during initialization)
            if force_render || frame_requested || needs_layout || needs_paint || needs_resize || scale_changed || has_animations_now {
                // Re-layout (for reactive updates)
                let constraints = Constraints::new(
                    0.0,
                    0.0,
                    wayland_state.width as f32,
                    wayland_state.height as f32,
                );
                root.layout(constraints);

                // Paint
                let mut paint_ctx = renderer.create_paint_context();
                root.paint(&mut paint_ctx);

                renderer.render(&mut surface, &paint_ctx, self.config.background_color);

                // Clear flags after rendering
                with_app_state_mut(|state| {
                    state.clear_layout_flag();
                    state.clear_paint_flag();
                });

                // Commit surface
                if let Some(ref surface) = wayland_state.surface {
                    surface.commit();

                    // Request frame callback if not yet initialized
                    // This allows us to know when the compositor has presented our first frame
                    if !wayland_state.first_frame_presented {
                        surface.frame(&qh, surface.clone());
                    }
                }

                // Flush the connection
                connection.flush().expect("Failed to flush connection");
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
