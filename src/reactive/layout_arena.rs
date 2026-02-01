//! Arena-based widget storage for efficient partial layout.
//!
//! The LayoutArena provides centralized widget storage and layout metadata,
//! enabling efficient partial layout by only re-laying out dirty subtrees.
//!
//! ## Key Features
//!
//! - **Central Widget Storage**: All widgets are stored in a single arena,
//!   with containers holding child IDs rather than owned widgets.
//!
//! - **Layout Metadata**: Each widget has associated metadata tracking
//!   parent/child relationships, dirty state, and cached constraints/size.
//!
//! - **Partial Layout**: When a widget is marked dirty, the dirty flag
//!   bubbles up to the nearest relayout boundary, which is added to the
//!   layout queue. Only dirty subtrees are re-laid out.
//!
//! ## Interior Mutability
//!
//! The arena uses `RefCell` for interior mutability of:
//! - Widget storage (`widgets`)
//! - Layout nodes (`nodes`)
//! - Layout roots queue (`layout_roots`)
//!
//! This allows the arena to be borrowed immutably while individual
//! widgets are borrowed mutably for layout, and metadata can be updated
//! during the layout pass.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::layout::{Constraints, Size};
use crate::widgets::Widget;

use super::WidgetId;

/// Metadata for a widget in the layout tree.
#[derive(Default)]
pub struct LayoutNode {
    /// Parent widget ID (None for root)
    pub parent: Option<WidgetId>,
    /// Child widget IDs
    pub children: Vec<WidgetId>,
    /// Whether this widget needs layout
    pub is_dirty: bool,
    /// Whether this widget is a relayout boundary
    pub is_relayout_boundary: bool,
    /// Cached constraints from last layout
    pub cached_constraints: Option<Constraints>,
    /// Cached size from last layout
    pub cached_size: Option<Size>,
}

impl LayoutNode {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Reference-counted widget cell for safe nested access.
///
/// Using Rc allows us to clone the reference, release the HashMap borrow,
/// and then access the widget - avoiding RefCell borrow conflicts when
/// callbacks need to register/unregister widgets.
type WidgetCell = Rc<RefCell<Box<dyn Widget>>>;

/// Central arena for widget storage and layout metadata.
///
/// The arena stores all widgets in a HashMap with interior mutability,
/// allowing widgets to be accessed and modified during layout and event
/// handling without requiring mutable access to the entire arena.
pub struct LayoutArena {
    /// Central widget storage - uses Rc to allow cloning references
    /// before releasing the HashMap borrow
    widgets: RefCell<HashMap<WidgetId, WidgetCell>>,

    /// Tree metadata for each widget (interior mutability)
    nodes: RefCell<HashMap<WidgetId, LayoutNode>>,

    /// Set of relayout boundaries that need layout (the layout queue)
    layout_roots: RefCell<HashSet<WidgetId>>,
}

impl LayoutArena {
    /// Create a new empty arena.
    pub fn new() -> Self {
        Self {
            widgets: RefCell::new(HashMap::new()),
            nodes: RefCell::new(HashMap::new()),
            layout_roots: RefCell::new(HashSet::new()),
        }
    }

    /// Register a widget in the arena.
    ///
    /// This stores the widget and creates an empty node for it.
    /// Parent-child relationships are set separately via `set_parent`.
    pub fn register(&self, id: WidgetId, widget: Box<dyn Widget>) {
        self.widgets
            .borrow_mut()
            .insert(id, Rc::new(RefCell::new(widget)));
        self.nodes.borrow_mut().entry(id).or_default();
    }

    /// Remove a widget from the arena.
    ///
    /// Also removes the widget from its parent's children list.
    pub fn unregister(&self, id: WidgetId) {
        // Remove from parent's children list
        let parent_id = self.nodes.borrow().get(&id).and_then(|n| n.parent);
        if let Some(parent_id) = parent_id
            && let Some(parent_node) = self.nodes.borrow_mut().get_mut(&parent_id)
        {
            parent_node.children.retain(|&child_id| child_id != id);
        }

        // Remove widget from HashMap, but drop it AFTER releasing the borrow.
        // This is critical because dropping a Container triggers ChildrenSource::drop,
        // which recursively calls unregister_widget for all children.
        let removed_widget = self.widgets.borrow_mut().remove(&id);
        self.nodes.borrow_mut().remove(&id);
        self.layout_roots.borrow_mut().remove(&id);

        // Now the borrow is released, we can safely drop the widget
        // (which may trigger recursive unregisters)
        drop(removed_widget);
    }

    /// Access a widget via a closure.
    ///
    /// This clones the Rc before releasing the HashMap borrow, allowing
    /// the callback to safely register/unregister other widgets.
    pub fn with_widget<R>(&self, id: WidgetId, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
        // Clone the Rc so we can release the HashMap borrow
        let widget_cell = self.widgets.borrow().get(&id).cloned();
        widget_cell.map(|cell| {
            let widget = cell.borrow();
            f(&**widget)
        })
    }

    /// Get a widget for mutation via a closure.
    ///
    /// This clones the Rc before releasing the HashMap borrow, allowing
    /// the callback to safely register/unregister other widgets.
    pub fn with_widget_mut<R>(
        &self,
        id: WidgetId,
        f: impl FnOnce(&mut dyn Widget) -> R,
    ) -> Option<R> {
        // Clone the Rc so we can release the HashMap borrow
        let widget_cell = self.widgets.borrow().get(&id).cloned();
        widget_cell.map(|cell| {
            let mut widget = cell.borrow_mut();
            f(&mut **widget)
        })
    }

    /// Check if a widget is registered.
    pub fn contains(&self, id: WidgetId) -> bool {
        self.widgets.borrow().contains_key(&id)
    }

    /// Set the parent of a widget.
    ///
    /// Also adds the widget to the parent's children list.
    pub fn set_parent(&self, child_id: WidgetId, parent_id: WidgetId) {
        let mut nodes = self.nodes.borrow_mut();

        // Update child's parent reference
        nodes.entry(child_id).or_default().parent = Some(parent_id);

        // Add to parent's children list (if not already present)
        let parent_node = nodes.entry(parent_id).or_default();
        if !parent_node.children.contains(&child_id) {
            parent_node.children.push(child_id);
        }
    }

    /// Get the parent of a widget.
    pub fn get_parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes.borrow().get(&id).and_then(|n| n.parent)
    }

    /// Get the children of a widget.
    pub fn get_children(&self, id: WidgetId) -> Vec<WidgetId> {
        self.nodes
            .borrow()
            .get(&id)
            .map(|n| n.children.clone())
            .unwrap_or_default()
    }

    /// Mark a widget as needing layout.
    ///
    /// The dirty flag bubbles up to the nearest relayout boundary,
    /// which is added to the layout queue.
    ///
    /// Optimization: If a widget is already dirty, we stop early since its
    /// boundary must already be in the queue. This requires all widgets to
    /// call `clear_dirty` after completing layout.
    pub fn mark_needs_layout(&self, widget_id: WidgetId) {
        let mut current = widget_id;

        loop {
            let mut nodes = self.nodes.borrow_mut();
            let node = nodes.entry(current).or_default();

            // Optimization: Stop if already dirty - boundary is already in queue
            if node.is_dirty {
                return;
            }

            // Mark as dirty
            node.is_dirty = true;

            // Check if this is a relayout boundary
            if node.is_relayout_boundary {
                // Stop! Add to layout queue
                drop(nodes); // Release borrow before borrowing layout_roots
                self.layout_roots.borrow_mut().insert(current);
                return;
            }

            // Move up to parent
            match node.parent {
                Some(parent) => {
                    drop(nodes); // Release borrow before next iteration
                    current = parent;
                }
                None => {
                    // Reached root, add to queue
                    drop(nodes);
                    self.layout_roots.borrow_mut().insert(current);
                    return;
                }
            }
        }
    }

    /// Clear dirty flag for a widget.
    pub fn clear_dirty(&self, id: WidgetId) {
        if let Some(node) = self.nodes.borrow_mut().get_mut(&id) {
            node.is_dirty = false;
        }
    }

    /// Check if a widget is dirty.
    pub fn is_dirty(&self, id: WidgetId) -> bool {
        self.nodes.borrow().get(&id).is_some_and(|n| n.is_dirty)
    }

    /// Set whether a widget is a relayout boundary.
    pub fn set_relayout_boundary(&self, id: WidgetId, is_boundary: bool) {
        self.nodes
            .borrow_mut()
            .entry(id)
            .or_default()
            .is_relayout_boundary = is_boundary;
    }

    /// Check if a widget is a relayout boundary.
    pub fn is_relayout_boundary(&self, id: WidgetId) -> bool {
        self.nodes
            .borrow()
            .get(&id)
            .is_some_and(|n| n.is_relayout_boundary)
    }

    /// Cache the constraints and size for a widget.
    pub fn cache_layout(&self, id: WidgetId, constraints: Constraints, size: Size) {
        if let Some(node) = self.nodes.borrow_mut().get_mut(&id) {
            node.cached_constraints = Some(constraints);
            node.cached_size = Some(size);
        }
    }

    /// Get cached constraints for a widget.
    pub fn cached_constraints(&self, id: WidgetId) -> Option<Constraints> {
        self.nodes
            .borrow()
            .get(&id)
            .and_then(|n| n.cached_constraints)
    }

    /// Get cached size for a widget.
    pub fn cached_size(&self, id: WidgetId) -> Option<Size> {
        self.nodes.borrow().get(&id).and_then(|n| n.cached_size)
    }

    /// Take all layout roots (clears the set).
    pub fn take_layout_roots(&self) -> Vec<WidgetId> {
        self.layout_roots.borrow_mut().drain().collect()
    }

    /// Check if any layout roots are pending.
    pub fn has_layout_roots(&self) -> bool {
        !self.layout_roots.borrow().is_empty()
    }

    /// Add a layout root directly.
    pub fn add_layout_root(&self, id: WidgetId) {
        self.layout_roots.borrow_mut().insert(id);
    }

    /// Clear all widgets and metadata.
    pub fn clear(&self) {
        self.widgets.borrow_mut().clear();
        self.nodes.borrow_mut().clear();
        self.layout_roots.borrow_mut().clear();
    }

    /// Get the number of registered widgets.
    pub fn widget_count(&self) -> usize {
        self.widgets.borrow().len()
    }
}

impl Default for LayoutArena {
    fn default() -> Self {
        Self::new()
    }
}

// Thread-local arena instance
thread_local! {
    static LAYOUT_ARENA: LayoutArena = LayoutArena::new();
}

/// Access the layout arena.
pub fn with_layout_arena<F, R>(f: F) -> R
where
    F: FnOnce(&LayoutArena) -> R,
{
    LAYOUT_ARENA.with(|arena| f(arena))
}

/// Register a widget in the global arena.
pub fn register_widget(id: WidgetId, widget: Box<dyn Widget>) {
    with_layout_arena(|arena| arena.register(id, widget));
}

/// Unregister a widget from the global arena.
pub fn unregister_widget(id: WidgetId) {
    with_layout_arena(|arena| arena.unregister(id));
}

/// Set the parent of a widget in the global arena.
pub fn arena_set_parent(child_id: WidgetId, parent_id: WidgetId) {
    with_layout_arena(|arena| arena.set_parent(child_id, parent_id));
}

/// Get the parent of a widget from the global arena.
pub fn arena_get_parent(id: WidgetId) -> Option<WidgetId> {
    with_layout_arena(|arena| arena.get_parent(id))
}

/// Mark a widget as needing layout in the global arena.
pub fn arena_mark_needs_layout(widget_id: WidgetId) {
    with_layout_arena(|arena| arena.mark_needs_layout(widget_id));
}

/// Set whether a widget is a relayout boundary in the global arena.
pub fn arena_set_relayout_boundary(id: WidgetId, is_boundary: bool) {
    with_layout_arena(|arena| arena.set_relayout_boundary(id, is_boundary));
}

/// Check if a widget is dirty in the global arena.
pub fn arena_is_dirty(id: WidgetId) -> bool {
    with_layout_arena(|arena| arena.is_dirty(id))
}

/// Clear dirty flag for a widget in the global arena.
pub fn arena_clear_dirty(id: WidgetId) {
    with_layout_arena(|arena| arena.clear_dirty(id));
}

/// Cache layout results in the global arena.
pub fn arena_cache_layout(id: WidgetId, constraints: Constraints, size: Size) {
    with_layout_arena(|arena| arena.cache_layout(id, constraints, size));
}

/// Get cached constraints from the global arena.
pub fn arena_cached_constraints(id: WidgetId) -> Option<Constraints> {
    with_layout_arena(|arena| arena.cached_constraints(id))
}

/// Get cached size from the global arena.
pub fn arena_cached_size(id: WidgetId) -> Option<Size> {
    with_layout_arena(|arena| arena.cached_size(id))
}

/// Take all layout roots from the global arena.
pub fn arena_take_layout_roots() -> Vec<WidgetId> {
    with_layout_arena(|arena| arena.take_layout_roots())
}

/// Check if any layout roots are pending in the global arena.
pub fn arena_has_layout_roots() -> bool {
    with_layout_arena(|arena| arena.has_layout_roots())
}

/// Add a layout root to the global arena.
pub fn arena_add_layout_root(id: WidgetId) {
    with_layout_arena(|arena| arena.add_layout_root(id));
}

/// Access a widget in the global arena via a closure.
pub fn with_arena_widget<R>(id: WidgetId, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
    with_layout_arena(|arena| arena.with_widget(id, f))
}

/// Access a widget mutably in the global arena via a closure.
pub fn with_arena_widget_mut<R>(id: WidgetId, f: impl FnOnce(&mut dyn Widget) -> R) -> Option<R> {
    with_layout_arena(|arena| arena.with_widget_mut(id, f))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock widget for testing
    struct MockWidget {
        id: WidgetId,
    }

    impl Widget for MockWidget {
        fn layout(&mut self, constraints: Constraints) -> Size {
            Size::new(constraints.max_width, constraints.max_height)
        }

        fn paint(&self, _ctx: &mut crate::renderer::PaintContext) {}

        fn set_origin(&mut self, _x: f32, _y: f32) {}

        fn bounds(&self) -> crate::widgets::Rect {
            crate::widgets::Rect::new(0.0, 0.0, 0.0, 0.0)
        }

        fn id(&self) -> WidgetId {
            self.id
        }
    }

    #[test]
    fn test_arena_register_unregister() {
        let arena = LayoutArena::new();
        let id = WidgetId::next();
        let widget = Box::new(MockWidget { id });

        arena.register(id, widget);
        assert!(arena.contains(id));

        arena.unregister(id);
        assert!(!arena.contains(id));
    }

    #[test]
    fn test_arena_parent_child() {
        let arena = LayoutArena::new();
        let parent_id = WidgetId::next();
        let child_id = WidgetId::next();

        arena.register(parent_id, Box::new(MockWidget { id: parent_id }));
        arena.register(child_id, Box::new(MockWidget { id: child_id }));

        arena.set_parent(child_id, parent_id);

        assert_eq!(arena.get_parent(child_id), Some(parent_id));
        assert_eq!(arena.get_children(parent_id), vec![child_id]);
    }

    #[test]
    fn test_arena_dirty_propagation() {
        let arena = LayoutArena::new();
        let root_id = WidgetId::next();
        let child_id = WidgetId::next();
        let grandchild_id = WidgetId::next();

        // Build tree: root -> child -> grandchild
        arena.register(root_id, Box::new(MockWidget { id: root_id }));
        arena.register(child_id, Box::new(MockWidget { id: child_id }));
        arena.register(grandchild_id, Box::new(MockWidget { id: grandchild_id }));

        arena.set_parent(child_id, root_id);
        arena.set_parent(grandchild_id, child_id);

        // Mark grandchild dirty - should bubble to root
        arena.mark_needs_layout(grandchild_id);

        assert!(arena.is_dirty(grandchild_id));
        assert!(arena.is_dirty(child_id));
        assert!(arena.is_dirty(root_id));

        // Root should be in layout_roots
        let roots = arena.take_layout_roots();
        assert!(roots.contains(&root_id));
    }

    #[test]
    fn test_arena_relayout_boundary_stops_propagation() {
        let arena = LayoutArena::new();
        let root_id = WidgetId::next();
        let boundary_id = WidgetId::next();
        let leaf_id = WidgetId::next();

        // Build tree: root -> boundary (relayout) -> leaf
        arena.register(root_id, Box::new(MockWidget { id: root_id }));
        arena.register(boundary_id, Box::new(MockWidget { id: boundary_id }));
        arena.register(leaf_id, Box::new(MockWidget { id: leaf_id }));

        arena.set_parent(boundary_id, root_id);
        arena.set_parent(leaf_id, boundary_id);

        // Mark boundary as relayout boundary
        arena.set_relayout_boundary(boundary_id, true);

        // Mark leaf dirty - should stop at boundary
        arena.mark_needs_layout(leaf_id);

        assert!(arena.is_dirty(leaf_id));
        assert!(arena.is_dirty(boundary_id));
        assert!(!arena.is_dirty(root_id)); // Root should NOT be dirty

        // Boundary should be in layout_roots, not root
        let roots = arena.take_layout_roots();
        assert!(roots.contains(&boundary_id));
        assert!(!roots.contains(&root_id));
    }

    #[test]
    fn test_arena_dirty_optimization() {
        let arena = LayoutArena::new();
        let root_id = WidgetId::next();
        let child_id = WidgetId::next();

        arena.register(root_id, Box::new(MockWidget { id: root_id }));
        arena.register(child_id, Box::new(MockWidget { id: child_id }));
        arena.set_parent(child_id, root_id);

        // Mark child dirty - root should be added to layout_roots
        arena.mark_needs_layout(child_id);
        assert!(arena.is_dirty(child_id));
        assert!(arena.is_dirty(root_id));
        assert!(arena.has_layout_roots());

        // Simulate layout running: take roots and clear ALL dirty flags
        // (this is what widgets should do after layout)
        arena.take_layout_roots();
        arena.clear_dirty(root_id);
        arena.clear_dirty(child_id);

        // Mark child dirty again - should add root to layout_roots
        arena.mark_needs_layout(child_id);
        assert!(arena.has_layout_roots());

        // Now test the optimization: if child is still dirty, stop early
        arena.take_layout_roots();
        // Don't clear dirty flags this time

        // Mark child dirty again - should stop early (already dirty)
        arena.mark_needs_layout(child_id);

        // layout_roots should be empty because we stopped at the dirty child
        assert!(!arena.has_layout_roots());
    }

    #[test]
    fn test_arena_with_widget() {
        let arena = LayoutArena::new();
        let id = WidgetId::next();
        arena.register(id, Box::new(MockWidget { id }));

        // Read widget
        let widget_id = arena.with_widget(id, |w| w.id());
        assert_eq!(widget_id, Some(id));

        // Mutate widget (layout)
        let size =
            arena.with_widget_mut(id, |w| w.layout(Constraints::new(0.0, 0.0, 100.0, 100.0)));
        assert_eq!(size, Some(Size::new(100.0, 100.0)));
    }
}
