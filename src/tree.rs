//! Arena-based widget storage for efficient partial layout.
//!
//! The Tree provides centralized widget storage using a sparse-set architecture
//! with generational indices, enabling efficient partial layout by only
//! re-laying out dirty subtrees.
//!
//! ## Key Features
//!
//! - **Generational Indices**: WidgetId contains index + generation to prevent
//!   ABA problems (detecting stale references to reallocated slots).
//!
//! - **Dense Storage**: Widgets stored contiguously for cache-friendly iteration
//!   during layout and paint passes.
//!
//! - **Sparse Map**: O(1) lookup from stable WidgetId to dense array index.
//!
//! - **Swap-Remove**: O(1) removal without creating holes in dense storage.
//!
//! - **Layout Metadata**: Each widget has associated metadata tracking
//!   parent/child relationships, dirty state, and cached constraints/size.
//!
//! - **Partial Layout**: When a widget is marked dirty, the dirty flag
//!   bubbles up to the nearest relayout boundary, which is added to the
//!   layout queue. Only dirty subtrees are re-laid out.

use std::collections::HashSet;

use crate::layout::{Constraints, Size};
use crate::widgets::Widget;

/// Unique identifier for a widget in the tree.
///
/// Uses a generational index design:
/// - `index`: Position in the sparse array (reusable after removal)
/// - `generation`: Version counter that increments when a slot is reused
///
/// This prevents ABA problems where a stale ID might accidentally refer
/// to a new widget that was allocated in the same slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WidgetId {
    index: u32,
    generation: u32,
}

impl WidgetId {
    /// Create a new WidgetId with the given index and generation.
    /// This is internal - users get IDs from Tree::register().
    fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Create a placeholder WidgetId for widget construction.
    /// This will be replaced with a real ID when the widget is registered.
    /// Uses max values to make it obvious if accidentally used.
    pub fn placeholder() -> Self {
        Self {
            index: u32::MAX,
            generation: u32::MAX,
        }
    }

    /// Check if this is a placeholder ID (not yet registered).
    pub fn is_placeholder(&self) -> bool {
        self.index == u32::MAX && self.generation == u32::MAX
    }

    /// Convert to a u64 for external use (e.g., render layer IDs).
    /// Combines generation (high bits) with index (low bits).
    pub fn as_u64(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }
}

/// Entry in the sparse map, pointing to a dense array slot.
struct SparseEntry {
    /// Index into the dense array
    dense_index: usize,
    /// Generation of this entry (for validation)
    generation: u32,
}

/// A node in the tree, containing a widget and its metadata.
struct Node {
    /// The widget stored at this node
    widget: Box<dyn Widget>,
    /// Parent widget ID (None for root)
    parent: Option<WidgetId>,
    /// Child widget IDs
    children: Vec<WidgetId>,
    /// Whether this widget needs layout
    is_dirty: bool,
    /// Whether this widget is a relayout boundary
    is_relayout_boundary: bool,
    /// Cached constraints from last layout
    cached_constraints: Option<Constraints>,
    /// Cached size from last layout
    cached_size: Option<Size>,
    /// Back-pointer to sparse array index (for swap-remove fixup)
    sparse_index: u32,
}

/// Central tree for widget storage using arena-based sparse-set architecture.
///
/// The tree stores all widgets in a dense Vec for cache-friendly iteration,
/// with a sparse map for O(1) lookup by WidgetId. Generational indices
/// prevent use-after-free bugs.
pub struct Tree {
    /// Dense array of nodes (widgets + metadata)
    dense: Vec<Node>,
    /// Sparse map from index to dense position + generation
    sparse: Vec<Option<SparseEntry>>,
    /// Free list of reusable sparse indices
    free_indices: Vec<u32>,
    /// Set of relayout boundaries that need layout (the layout queue)
    layout_roots: HashSet<WidgetId>,
}

impl Tree {
    /// Create a new empty tree.
    pub fn new() -> Self {
        Self {
            dense: Vec::new(),
            sparse: Vec::new(),
            free_indices: Vec::new(),
            layout_roots: HashSet::new(),
        }
    }

    /// Register a widget in the tree and return its unique ID.
    ///
    /// This stores the widget and creates metadata for it.
    /// Parent-child relationships are set separately via `set_parent`.
    pub fn register(&mut self, mut widget: Box<dyn Widget>) -> WidgetId {
        // Allocate a sparse index (reuse from free list or allocate new)
        let (sparse_index, generation) = if let Some(idx) = self.free_indices.pop() {
            // Reuse a freed slot - increment generation
            let old_gen = self.sparse[idx as usize]
                .as_ref()
                .map(|e| e.generation)
                .unwrap_or(0);
            (idx, old_gen.wrapping_add(1))
        } else {
            // Allocate new slot
            let idx = self.sparse.len() as u32;
            self.sparse.push(None);
            (idx, 0)
        };

        let dense_index = self.dense.len();

        // Create the widget ID
        let id = WidgetId::new(sparse_index, generation);

        // Set the widget's ID before storing
        widget.set_id(id);

        // Create the node
        self.dense.push(Node {
            widget,
            parent: None,
            children: Vec::new(),
            is_dirty: false,
            is_relayout_boundary: false,
            cached_constraints: None,
            cached_size: None,
            sparse_index,
        });

        // Update sparse map
        self.sparse[sparse_index as usize] = Some(SparseEntry {
            dense_index,
            generation,
        });

        id
    }

    /// Remove a widget from the tree.
    ///
    /// Uses swap-remove to maintain dense storage without holes.
    /// Also removes the widget from its parent's children list.
    pub fn unregister(&mut self, id: WidgetId) {
        // Validate and get dense index
        let dense_index = match self.get_dense_index(id) {
            Some(idx) => idx,
            None => return, // Invalid or stale ID
        };

        // First, remove from parent's children list (before modifying dense array)
        if let Some(parent_id) = self.dense[dense_index].parent
            && let Some(parent_dense) = self.get_dense_index(parent_id)
        {
            self.dense[parent_dense].children.retain(|&c| c != id);
        }

        // Take ownership of the widget to drop it AFTER fixing up indices
        // This is critical for recursive unregistration during Drop
        let last_dense_index = self.dense.len() - 1;

        // Swap-remove: move last element to this position
        let removed_node = self.dense.swap_remove(dense_index);

        // Fix up the moved node's sparse entry (if we didn't remove the last element)
        if dense_index != last_dense_index && !self.dense.is_empty() {
            let moved_sparse_idx = self.dense[dense_index].sparse_index;
            if let Some(ref mut entry) = self.sparse[moved_sparse_idx as usize] {
                entry.dense_index = dense_index;
            }
        }

        // Invalidate the sparse entry (keep generation for next allocation)
        self.sparse[id.index as usize] = None;
        self.free_indices.push(id.index);
        self.layout_roots.remove(&id);

        // Now drop the removed widget (may trigger recursive unregisters)
        drop(removed_node);
    }

    /// Get the dense array index for a WidgetId, validating generation.
    fn get_dense_index(&self, id: WidgetId) -> Option<usize> {
        self.sparse
            .get(id.index as usize)
            .and_then(|e| e.as_ref())
            .filter(|e| e.generation == id.generation)
            .map(|e| e.dense_index)
    }

    /// Access a widget via a closure.
    pub fn with_widget<R>(&self, id: WidgetId, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
        self.get_dense_index(id)
            .map(|idx| f(&*self.dense[idx].widget))
    }

    /// Mutate a widget via a closure.
    ///
    /// The closure receives mutable access to the widget and the tree, allowing
    /// operations that need both (like calling layout on children).
    ///
    /// The widget is temporarily extracted from the tree during the closure execution.
    /// Returns `None` if the widget is not found (invalid or stale ID).
    pub fn with_widget_mut<R>(
        &mut self,
        id: WidgetId,
        f: impl FnOnce(&mut dyn Widget, &mut Tree) -> R,
    ) -> Option<R> {
        let dense_index = self.get_dense_index(id)?;

        // Placeholder widget for extraction
        struct PlaceholderWidget(WidgetId);
        impl Widget for PlaceholderWidget {
            fn layout(&mut self, _: &mut Tree, _: Constraints) -> Size {
                Size::zero()
            }
            fn paint(&self, _: &Tree, _: &mut crate::renderer::PaintContext) {}
            fn set_origin(&mut self, _: f32, _: f32) {}
            fn bounds(&self) -> crate::widgets::Rect {
                crate::widgets::Rect::new(0.0, 0.0, 0.0, 0.0)
            }
            fn id(&self) -> WidgetId {
                self.0
            }
            fn set_id(&mut self, id: WidgetId) {
                self.0 = id;
            }
        }

        // Extract widget
        let mut widget = std::mem::replace(
            &mut self.dense[dense_index].widget,
            Box::new(PlaceholderWidget(id)),
        );

        // Run closure with &mut dyn Widget (not Box)
        let result = f(&mut *widget, self);

        // Restore widget
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].widget = widget;
        }

        Some(result)
    }

    /// Check if a widget is registered.
    pub fn contains(&self, id: WidgetId) -> bool {
        self.get_dense_index(id).is_some()
    }

    /// Set the parent of a widget.
    ///
    /// Also adds the widget to the parent's children list.
    pub fn set_parent(&mut self, child_id: WidgetId, parent_id: WidgetId) {
        // Update child's parent reference
        if let Some(child_dense) = self.get_dense_index(child_id) {
            self.dense[child_dense].parent = Some(parent_id);
        }

        // Add to parent's children list (if not already present)
        if let Some(parent_dense) = self.get_dense_index(parent_id) {
            let children = &mut self.dense[parent_dense].children;
            if !children.contains(&child_id) {
                children.push(child_id);
            }
        }
    }

    /// Get the parent of a widget.
    pub fn get_parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.get_dense_index(id)
            .and_then(|idx| self.dense[idx].parent)
    }

    /// Get the children of a widget.
    pub fn get_children(&self, id: WidgetId) -> Vec<WidgetId> {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].children.clone())
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
    pub fn mark_needs_layout(&mut self, widget_id: WidgetId) {
        let mut current = widget_id;

        loop {
            let dense_idx = match self.get_dense_index(current) {
                Some(idx) => idx,
                None => return,
            };

            // Optimization: Stop if already dirty - boundary is already in queue
            if self.dense[dense_idx].is_dirty {
                return;
            }

            // Mark as dirty
            self.dense[dense_idx].is_dirty = true;

            // Check if this is a relayout boundary
            if self.dense[dense_idx].is_relayout_boundary {
                // Stop! Add to layout queue
                self.layout_roots.insert(current);
                return;
            }

            // Move up to parent
            match self.dense[dense_idx].parent {
                Some(parent) => {
                    current = parent;
                }
                None => {
                    // Reached root, add to queue
                    self.layout_roots.insert(current);
                    return;
                }
            }
        }
    }

    /// Clear dirty flag for a widget.
    pub fn clear_dirty(&mut self, id: WidgetId) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].is_dirty = false;
        }
    }

    /// Check if a widget is dirty.
    pub fn is_dirty(&self, id: WidgetId) -> bool {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].is_dirty)
            .unwrap_or(false)
    }

    /// Set whether a widget is a relayout boundary.
    pub fn set_relayout_boundary(&mut self, id: WidgetId, is_boundary: bool) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].is_relayout_boundary = is_boundary;
        }
    }

    /// Check if a widget is a relayout boundary.
    pub fn is_relayout_boundary(&self, id: WidgetId) -> bool {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].is_relayout_boundary)
            .unwrap_or(false)
    }

    /// Cache the constraints and size for a widget.
    pub fn cache_layout(&mut self, id: WidgetId, constraints: Constraints, size: Size) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].cached_constraints = Some(constraints);
            self.dense[idx].cached_size = Some(size);
        }
    }

    /// Get cached constraints for a widget.
    pub fn cached_constraints(&self, id: WidgetId) -> Option<Constraints> {
        self.get_dense_index(id)
            .and_then(|idx| self.dense[idx].cached_constraints)
    }

    /// Get cached size for a widget.
    pub fn cached_size(&self, id: WidgetId) -> Option<Size> {
        self.get_dense_index(id)
            .and_then(|idx| self.dense[idx].cached_size)
    }

    /// Take all layout roots (clears the set).
    pub fn take_layout_roots(&mut self) -> Vec<WidgetId> {
        self.layout_roots.drain().collect()
    }

    /// Check if any layout roots are pending.
    pub fn has_layout_roots(&self) -> bool {
        !self.layout_roots.is_empty()
    }

    /// Add a layout root directly.
    pub fn add_layout_root(&mut self, id: WidgetId) {
        self.layout_roots.insert(id);
    }

    /// Clear all widgets and metadata.
    pub fn clear(&mut self) {
        self.dense.clear();
        self.sparse.clear();
        self.free_indices.clear();
        self.layout_roots.clear();
    }

    /// Get the number of registered widgets.
    pub fn widget_count(&self) -> usize {
        self.dense.len()
    }
}

impl Default for Tree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock widget for testing
    struct MockWidget {
        id: WidgetId,
    }

    impl MockWidget {
        fn new() -> Self {
            Self {
                id: WidgetId::new(0, 0), // Will be set by tree.register()
            }
        }
    }

    impl Widget for MockWidget {
        fn layout(&mut self, _tree: &mut Tree, constraints: Constraints) -> Size {
            Size::new(constraints.max_width, constraints.max_height)
        }

        fn paint(&self, _tree: &Tree, _ctx: &mut crate::renderer::PaintContext) {}

        fn set_origin(&mut self, _x: f32, _y: f32) {}

        fn bounds(&self) -> crate::widgets::Rect {
            crate::widgets::Rect::new(0.0, 0.0, 0.0, 0.0)
        }

        fn id(&self) -> WidgetId {
            self.id
        }

        fn set_id(&mut self, id: WidgetId) {
            self.id = id;
        }
    }

    #[test]
    fn test_tree_register_unregister() {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(MockWidget::new()));
        assert!(tree.contains(id));

        tree.unregister(id);
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_tree_generational_index() {
        let mut tree = Tree::new();

        // Register and unregister a widget
        let id1 = tree.register(Box::new(MockWidget::new()));
        tree.unregister(id1);

        // Register a new widget (should reuse the slot)
        let id2 = tree.register(Box::new(MockWidget::new()));

        // id1 should be invalid (different generation)
        assert!(!tree.contains(id1));
        assert!(tree.contains(id2));

        // They should have the same index but different generations
        assert_eq!(id1.index, id2.index);
        assert_ne!(id1.generation, id2.generation);
    }

    #[test]
    fn test_tree_parent_child() {
        let mut tree = Tree::new();
        let parent_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, parent_id);

        assert_eq!(tree.get_parent(child_id), Some(parent_id));
        assert_eq!(tree.get_children(parent_id), vec![child_id]);
    }

    #[test]
    fn test_tree_dirty_propagation() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));
        let grandchild_id = tree.register(Box::new(MockWidget::new()));

        // Build tree: root -> child -> grandchild
        tree.set_parent(child_id, root_id);
        tree.set_parent(grandchild_id, child_id);

        // Mark grandchild dirty - should bubble to root
        tree.mark_needs_layout(grandchild_id);

        assert!(tree.is_dirty(grandchild_id));
        assert!(tree.is_dirty(child_id));
        assert!(tree.is_dirty(root_id));

        // Root should be in layout_roots
        let roots = tree.take_layout_roots();
        assert!(roots.contains(&root_id));
    }

    #[test]
    fn test_tree_relayout_boundary_stops_propagation() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let boundary_id = tree.register(Box::new(MockWidget::new()));
        let leaf_id = tree.register(Box::new(MockWidget::new()));

        // Build tree: root -> boundary (relayout) -> leaf
        tree.set_parent(boundary_id, root_id);
        tree.set_parent(leaf_id, boundary_id);

        // Mark boundary as relayout boundary
        tree.set_relayout_boundary(boundary_id, true);

        // Mark leaf dirty - should stop at boundary
        tree.mark_needs_layout(leaf_id);

        assert!(tree.is_dirty(leaf_id));
        assert!(tree.is_dirty(boundary_id));
        assert!(!tree.is_dirty(root_id)); // Root should NOT be dirty

        // Boundary should be in layout_roots, not root
        let roots = tree.take_layout_roots();
        assert!(roots.contains(&boundary_id));
        assert!(!roots.contains(&root_id));
    }

    #[test]
    fn test_tree_dirty_optimization() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, root_id);

        // Mark child dirty - root should be added to layout_roots
        tree.mark_needs_layout(child_id);
        assert!(tree.is_dirty(child_id));
        assert!(tree.is_dirty(root_id));
        assert!(tree.has_layout_roots());

        // Simulate layout running: take roots and clear ALL dirty flags
        // (this is what widgets should do after layout)
        tree.take_layout_roots();
        tree.clear_dirty(root_id);
        tree.clear_dirty(child_id);

        // Mark child dirty again - should add root to layout_roots
        tree.mark_needs_layout(child_id);
        assert!(tree.has_layout_roots());

        // Now test the optimization: if child is still dirty, stop early
        tree.take_layout_roots();
        // Don't clear dirty flags this time

        // Mark child dirty again - should stop early (already dirty)
        tree.mark_needs_layout(child_id);

        // layout_roots should be empty because we stopped at the dirty child
        assert!(!tree.has_layout_roots());
    }

    #[test]
    fn test_tree_with_widget() {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(MockWidget::new()));

        // Read widget
        let widget_id = tree.with_widget(id, |w| w.id());
        assert_eq!(widget_id, Some(id));
    }

    #[test]
    fn test_tree_swap_remove_fixup() {
        let mut tree = Tree::new();

        // Register three widgets
        let id1 = tree.register(Box::new(MockWidget::new()));
        let id2 = tree.register(Box::new(MockWidget::new()));
        let id3 = tree.register(Box::new(MockWidget::new()));

        // Remove the first one - id3 should be moved to its position
        tree.unregister(id1);

        // id1 should be invalid
        assert!(!tree.contains(id1));

        // id2 and id3 should still be valid
        assert!(tree.contains(id2));
        assert!(tree.contains(id3));

        // We should still be able to access them
        assert!(tree.with_widget(id2, |_| ()).is_some());
        assert!(tree.with_widget(id3, |_| ()).is_some());
    }
}
