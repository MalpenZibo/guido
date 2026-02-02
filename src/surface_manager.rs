//! Surface lifecycle management for Guido applications.
//!
//! This module provides types for managing the lifecycle of surfaces,
//! including GPU initialization and widget layout.

use std::collections::HashMap;

use smithay_client_toolkit::reexports::client::Connection;

use crate::layout::Constraints;
use crate::platform::{WaylandState, WaylandWindowWrapper};
use crate::reactive::{LayoutArena, WidgetId};
use crate::renderer::{GpuContext, SurfaceState};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::widgets::Widget;

/// A surface with unified GPU lifecycle management.
///
/// This combines the widget tree, GPU surface state, and configuration
/// into a single struct that manages the surface's entire lifecycle.
pub struct ManagedSurface {
    /// The unique identifier for this surface
    pub id: SurfaceId,
    /// Configuration for the surface
    pub config: SurfaceConfig,
    /// The root widget ID (widget is stored in the arena)
    pub widget_id: WidgetId,
    /// The wgpu surface state (None until GPU init)
    pub wgpu_surface: Option<SurfaceState>,
    /// Previous scale factor for detecting changes
    pub previous_scale_factor: f32,
}

impl ManagedSurface {
    /// Create a new managed surface (wgpu_surface is None until GPU init).
    /// The root widget and its children are registered in the arena.
    pub fn new(
        id: SurfaceId,
        config: SurfaceConfig,
        mut widget: Box<dyn Widget>,
        arena: &LayoutArena,
    ) -> Self {
        let widget_id = widget.id();
        // Register children first (recursively), then the root widget
        widget.register_children(arena);
        arena.register(widget_id, widget);
        Self {
            id,
            config,
            widget_id,
            wgpu_surface: None,
            previous_scale_factor: 1.0,
        }
    }

    /// Initialize GPU surface. Returns true if successful.
    #[allow(clippy::too_many_arguments)]
    pub fn init_gpu(
        &mut self,
        gpu_context: &GpuContext,
        connection: &Connection,
        wl_surface: &smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface,
        width: u32,
        height: u32,
        scale_factor: f32,
        arena: &LayoutArena,
    ) -> bool {
        if self.wgpu_surface.is_some() {
            return true; // Already initialized
        }

        let window_handle = WaylandWindowWrapper::new(connection, wl_surface);
        let initial_scale = scale_factor.max(1.0) as u32;
        let physical_width = width * initial_scale;
        let physical_height = height * initial_scale;

        log::info!(
            "Creating wgpu surface for {:?}: logical {}x{}, physical {}x{}, scale {}",
            self.id,
            width,
            height,
            physical_width,
            physical_height,
            initial_scale
        );

        let wgpu_surface =
            gpu_context.create_surface(window_handle, physical_width, physical_height);
        self.wgpu_surface = Some(wgpu_surface);
        self.previous_scale_factor = scale_factor;

        // Perform initial layout
        self.layout_widget(arena, width as f32, height as f32);

        true
    }

    /// Check if GPU is initialized.
    pub fn is_gpu_ready(&self) -> bool {
        self.wgpu_surface.is_some()
    }

    /// Perform widget layout with the given dimensions.
    pub fn layout_widget(&self, arena: &LayoutArena, width: f32, height: f32) {
        let constraints = Constraints::new(0.0, 0.0, width, height);
        arena.with_widget_mut(self.widget_id, |widget| {
            widget.layout(arena, constraints);
            widget.set_origin(0.0, 0.0);
        });
    }
}

/// Manages all surfaces in the application.
pub struct SurfaceManager {
    surfaces: HashMap<SurfaceId, ManagedSurface>,
}

impl SurfaceManager {
    /// Create a new empty surface manager.
    pub fn new() -> Self {
        Self {
            surfaces: HashMap::new(),
        }
    }

    /// Add a surface.
    pub fn add(&mut self, surface: ManagedSurface) {
        self.surfaces.insert(surface.id, surface);
    }

    /// Remove a surface by ID.
    pub fn remove(&mut self, id: SurfaceId) -> Option<ManagedSurface> {
        self.surfaces.remove(&id)
    }

    /// Get a mutable surface by ID.
    pub fn get_mut(&mut self, id: SurfaceId) -> Option<&mut ManagedSurface> {
        self.surfaces.get_mut(&id)
    }

    /// Iterate over all surface IDs.
    pub fn ids(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.surfaces.keys().copied()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// Initialize GPU for surfaces that need it.
    ///
    /// This iterates over all surfaces and initializes GPU for any
    /// that are configured in Wayland but don't yet have a wgpu surface.
    pub fn init_pending_gpu(
        &mut self,
        gpu_context: &GpuContext,
        connection: &Connection,
        wayland_state: &WaylandState,
        arena: &LayoutArena,
    ) {
        for (id, surface) in self.surfaces.iter_mut() {
            if surface.is_gpu_ready() {
                continue;
            }

            // Get wayland surface state
            let Some(wayland_surface) = wayland_state.get_surface(*id) else {
                continue;
            };

            // Skip if not configured yet
            if !wayland_surface.configured {
                continue;
            }

            surface.init_gpu(
                gpu_context,
                connection,
                &wayland_surface.wl_surface,
                wayland_surface.width,
                wayland_surface.height,
                wayland_surface.scale_factor,
                arena,
            );
        }
    }
}

impl Default for SurfaceManager {
    fn default() -> Self {
        Self::new()
    }
}
