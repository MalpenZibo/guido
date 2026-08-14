//! Surface lifecycle management for Guido applications.
//!
//! This module provides types for managing the lifecycle of surfaces,
//! including GPU initialization and widget layout.

use std::collections::HashMap;

use smithay_client_toolkit::reexports::client::Connection;

use crate::layout::Constraints;
use crate::platform::{WaylandState, WaylandWindowWrapper};
use crate::reactive::owner::{OwnerId, dispose_owner_now};
use crate::renderer::{CommandLayer, FlattenedCommand, GpuContext, RenderNode, SurfaceState};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::tree::{Tree, WidgetId};
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
    /// The root widget ID (widget is stored in the tree)
    pub widget_id: WidgetId,
    /// Owner for reactive primitives created in the widget factory
    owner_id: OwnerId,
    /// The wgpu surface state (None until GPU init)
    pub wgpu_surface: Option<SurfaceState>,
    /// Previous scale factor for detecting changes
    pub previous_scale_factor: f32,
    /// Root render node (reused across frames to avoid allocation)
    pub root_node: RenderNode,
    /// Flattened commands buffer (reused across frames to avoid allocation)
    pub flattened_commands: Vec<FlattenedCommand>,
    /// Draw groups over `flattened_commands`, in draw order.
    pub command_layers: Vec<CommandLayer>,
}

impl ManagedSurface {
    /// Create a new managed surface (wgpu_surface is None until GPU init).
    /// The root widget and its children are registered in the tree.
    pub fn new(
        id: SurfaceId,
        config: SurfaceConfig,
        widget: Box<dyn Widget>,
        owner_id: OwnerId,
        tree: &mut Tree,
    ) -> Self {
        // Register root widget - tree assigns the ID
        let widget_id = tree.register(widget);

        // Register children recursively with the assigned widget ID
        tree.with_widget_mut(widget_id, |widget, id, tree| {
            widget.register_children(tree, id);
        });

        Self {
            id,
            config,
            widget_id,
            owner_id,
            wgpu_surface: None,
            previous_scale_factor: 1.0,
            root_node: RenderNode::new(widget_id.as_u64()),
            flattened_commands: Vec::new(),
            command_layers: Vec::new(),
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
        tree: &mut Tree,
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
        self.layout_widget(tree, width as f32, height as f32);

        true
    }

    /// Check if GPU is initialized.
    pub fn is_gpu_ready(&self) -> bool {
        self.wgpu_surface.is_some()
    }

    /// Perform widget layout with the given dimensions.
    pub fn layout_widget(&self, tree: &mut Tree, width: f32, height: f32) {
        let constraints = Constraints::new(0.0, 0.0, width, height);

        tree.with_widget_mut(self.widget_id, |widget, id, tree| {
            widget.layout(tree, id, constraints);
        });
        // Set root widget origin after layout
        tree.set_origin(self.widget_id, 0.0, 0.0);
    }
}

impl Drop for ManagedSurface {
    fn drop(&mut self) {
        dispose_owner_now(self.owner_id);
    }
}

/// Tear down a closed surface's widget tree synchronously.
///
/// Removing a `ManagedSurface` only disposes its reactive owner; without
/// this, the widget subtree stays registered in the [`Tree`] forever (a
/// leak that grows with every closed popup) and its signal subscribers
/// stay live — the next write to a subscribed signal reconciles a dead
/// dynamic child whose closure reads owner-disposed state and panics.
///
/// Widgets are unregistered children-first, so each Drop's deferred
/// Unregister requests target already-removed ids and no-op; any stale
/// queued job no-ops too because the ids fail the tree's generation check.
///
/// Call this BEFORE dropping the `ManagedSurface`: subscribers must be
/// gone before the owner (and the signals it holds) is disposed.
pub(crate) fn teardown_widget_subtree(tree: &mut Tree, root: WidgetId) {
    crate::jobs::teardown_widget_subtree(tree, root);
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

    /// Get a surface by ID.
    pub fn get(&self, id: SurfaceId) -> Option<&ManagedSurface> {
        self.surfaces.get(&id)
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

    /// First initialized wgpu surface, if any.
    ///
    /// Used to create the shared `Renderer` lazily: apps may start with zero
    /// surfaces and spawn them dynamically (e.g. one bar per output).
    pub fn first_gpu_surface(&self) -> Option<&SurfaceState> {
        self.surfaces.values().find_map(|s| s.wgpu_surface.as_ref())
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
        tree: &mut Tree,
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
                tree,
            );
        }
    }
}

impl Default for SurfaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::layout::{Constraints, Size};
    use crate::reactive::owner::{dispose_owner_now as dispose, with_owner};
    use crate::reactive::{create_memo, create_signal};
    use crate::widgets::Widget;
    use crate::widgets::children::ChildrenSource;
    use crate::widgets::into_child::{DynamicChild, IntoChild};

    struct TestWidget;
    impl Widget for TestWidget {
        fn layout(&mut self, _: &mut Tree, _: crate::tree::WidgetId, _: Constraints) -> Size {
            Size::zero()
        }
        fn paint(&self, _: &Tree, _: crate::tree::WidgetId, _: &mut crate::renderer::PaintContext) {
        }
    }

    /// The surface-close crash scenario: a dynamic child reads a memo owned
    /// by the surface factory's owner scope; the surface closes (owner
    /// disposed); a later write to the tracked signal must NOT re-run the
    /// dead closure (which would read the disposed memo and panic) — and the
    /// subtree must actually leave the tree (the popup leak).
    #[test]
    fn teardown_clears_subscribers_and_empties_tree() {
        let mut tree = Tree::new();
        let parent = tree.register(Box::new(TestWidget));
        let mut source = ChildrenSource::default();
        source.set_container_id(parent);

        let sig = create_signal(0u64);
        let builds = Rc::new(Cell::new(0usize));

        // Mimic the surface factory: reactive state owned by a scope that
        // dies with the surface
        let (memo, owner_id) = with_owner(|| create_memo(move || sig.get() + 1));

        let counter = builds.clone();
        let closure = move || {
            let _ = memo.get();
            counter.set(counter.get() + 1);
            TestWidget
        };
        IntoChild::<DynamicChild>::add_to_container(closure, &mut source);
        source.reconcile_and_get(&mut tree);
        assert_eq!(builds.get(), 1);
        assert!(tree.widget_count() > 1);

        // Surface close: subtree teardown, THEN owner disposal
        teardown_widget_subtree(&mut tree, parent);
        dispose(owner_id);

        // The write that used to reconcile the dead child into a panic
        sig.set(1);
        source.reconcile_with_tracking(&mut tree);

        assert_eq!(builds.get(), 1, "dead closure must not re-run");
        assert_eq!(tree.widget_count(), 0, "subtree must leave the tree");
    }
}
