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

use smallvec::SmallVec;

use crate::layout::{Constraints, Size};
use crate::widgets::{Rect, Widget};

/// Inline capacity for children. Most widgets have 0–4 children,
/// so this avoids a heap allocation for the common case.
type ChildrenVec = SmallVec<[WidgetId; 4]>;

/// Accumulated damage region for a frame.
#[derive(Debug, Clone)]
pub enum DamageRegion {
    /// No damage — nothing changed.
    None,
    /// Partial damage — only the given rect needs redraw.
    Partial(Rect),
    /// Full damage — the entire surface needs redraw.
    Full,
}

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

    /// Convert to a u64 for external use (e.g., render layer IDs).
    /// Combines generation (high bits) with index (low bits).
    pub fn as_u64(self) -> u64 {
        ((self.generation as u64) << 32) | (self.index as u64)
    }

    /// Reconstruct a WidgetId from a u64 (reverse of `as_u64`).
    pub fn from_u64(val: u64) -> Self {
        Self {
            index: val as u32,
            generation: (val >> 32) as u32,
        }
    }
}

/// Slot in the sparse map, pointing to a dense array slot.
///
/// The generation survives vacancy: it is bumped when the slot is reused,
/// so a stale `WidgetId` can never alias a widget that later recycled the
/// same index.
struct SparseSlot {
    /// Index into the dense array; `None` while the slot is vacant
    dense_index: Option<usize>,
    /// Generation of this slot (for validation)
    generation: u32,
}

/// One widget's slot in the arena: the widget itself plus everything the tree
/// knows about it.
///
/// Called a slot rather than a node because "node" already means two other
/// things here — the render tree's `RenderNode`, and informally any widget.
/// This is neither: it is the arena record, and a widget composed but not yet
/// registered has none.
struct Slot {
    /// The widget stored at this node
    widget: Box<dyn Widget>,
    /// Parent widget ID (None for root)
    parent: Option<WidgetId>,
    /// Child widget IDs (inline for ≤4 children to avoid heap allocation)
    children: ChildrenVec,
    /// Whether this widget needs layout
    needs_layout: bool,
    /// Whether this widget needs paint
    needs_paint: bool,
    /// Whether this widget is a relayout boundary
    is_relayout_boundary: bool,
    /// Cached constraints from last layout
    cached_constraints: Option<Constraints>,
    /// Cached size from last layout
    cached_size: Option<Size>,
    /// Widget origin (set after layout by parent)
    origin: (f32, f32),
    /// Back-pointer to sparse array index (for swap-remove fixup)
    sparse_index: u32,
    /// Cached paint output from last frame
    cached_paint: Option<std::rc::Rc<crate::renderer::RenderNode>>,
    /// The interaction unit this node declares, if it is one. Descendants walk
    /// up to the nearest, and resolve hover, press and focus from it.
    control: Option<crate::widgets::Control>,
    /// Distance from this widget's top edge to the baseline of its first line
    /// of text, if it has one. Reported by leaves during layout and read by a
    /// parent aligning on `CrossAlignment::Baseline`.
    baseline: Option<f32>,
    /// How far this widget paints outside its own bounds, in logical pixels.
    ///
    /// A shadow — a box's or a glyph's — lands outside the box that cast it,
    /// and damage is computed from the bounds. Without this, repainting such a
    /// widget tells the compositor to re-composite a rect that stops short of
    /// the shadow, leaving the old one on screen as a fringe.
    paint_overflow: f32,
}

/// Central tree for widget storage using arena-based sparse-set architecture.
///
/// The tree stores all widgets in a dense Vec for cache-friendly iteration,
/// with a sparse map for O(1) lookup by WidgetId. Generational indices
/// prevent use-after-free bugs.
pub struct Tree {
    /// Dense array of nodes (widgets + metadata)
    dense: Vec<Slot>,
    /// Sparse map from index to dense position + generation
    sparse: Vec<SparseSlot>,
    /// Free list of reusable sparse indices
    free_indices: Vec<u32>,
    /// Accumulated damage region for the current frame, keyed by the
    /// surface's root widget. Per-surface so that one surface's render
    /// cannot consume (or misreport) damage accumulated by another.
    damage: std::collections::HashMap<WidgetId, DamageRegion>,
    /// What time it is, for the pass currently running.
    ///
    /// A frame advances every animation in it, and asking the clock once per
    /// animation makes one frame several instants a few microseconds apart.
    /// More to the point, a widget that asks the clock cannot be asked about
    /// the middle of an animation: a test can only sleep and assert a band
    /// wide enough to survive a loaded machine, which is a band both a linear
    /// and an eased curve fit inside.
    ///
    /// `render_surface` writes it, around the three passes a frame is made of:
    /// the jobs advance the animations, layout measures what they produced,
    /// paint draws it. `None` between frames, where nobody should be reading
    /// it.
    frame_instant: Option<std::time::Instant>,
}

impl Tree {
    /// Create a new empty tree.
    pub fn new() -> Self {
        Self {
            dense: Vec::new(),
            sparse: Vec::new(),
            free_indices: Vec::new(),
            damage: std::collections::HashMap::new(),
            frame_instant: None,
        }
    }

    /// What time it is for the pass now running.
    ///
    /// Falls back to the clock for a call that arrives outside a pass — the
    /// behaviour every caller had before there was a frame instant at all.
    pub fn frame_instant(&self) -> std::time::Instant {
        self.frame_instant.unwrap_or_else(std::time::Instant::now)
    }

    /// Declare the instant of the frame about to run, or `None` when it is
    /// over.
    ///
    /// `render_surface` is the caller in the loop. The other one is a test —
    /// including a test of a widget written outside this crate, which is why
    /// this is public: a widget that can read the frame's instant is a widget
    /// whose behaviour over time can be asked about, and that needs somebody
    /// able to name the moment.
    pub fn set_frame_instant(&mut self, now: Option<std::time::Instant>) {
        self.frame_instant = now;
    }

    /// Register a widget in the tree and return its unique ID.
    ///
    /// This stores the widget and creates metadata for it.
    /// Parent-child relationships are set separately via `set_parent`.
    pub fn register(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        // Allocate a sparse index (reuse from free list or allocate new)
        let (sparse_index, generation) = if let Some(idx) = self.free_indices.pop() {
            // Reuse a freed slot - increment the surviving generation so
            // stale IDs from the previous occupant can never match
            let generation = self.sparse[idx as usize].generation.wrapping_add(1);
            (idx, generation)
        } else {
            // Allocate new slot
            let idx = self.sparse.len() as u32;
            self.sparse.push(SparseSlot {
                dense_index: None,
                generation: 0,
            });
            (idx, 0)
        };

        let dense_index = self.dense.len();

        // Create the widget ID
        let id = WidgetId::new(sparse_index, generation);

        // Create the node (widget ID is passed to methods, not stored in widget)
        self.dense.push(Slot {
            widget,
            parent: None,
            children: ChildrenVec::new(),
            needs_layout: false,
            needs_paint: true,
            is_relayout_boundary: false,
            cached_constraints: None,
            cached_size: None,
            origin: (0.0, 0.0),
            sparse_index,
            cached_paint: None,
            control: None,
            baseline: None,
            paint_overflow: 0.0,
        });

        // Update sparse map
        self.sparse[sparse_index as usize] = SparseSlot {
            dense_index: Some(dense_index),
            generation,
        };

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

        // A widget leaving the tree takes the focus with it. The focus path is
        // stored state, so unlike the bare generational id it used to be, it
        // does not stop matching on its own: its ancestors would go on
        // answering "the focus is inside me" for a widget that is gone.
        crate::reactive::release_focus_if_within(id);

        // First, remove from parent's children list (before modifying dense array)
        if let Some(parent_id) = self.dense[dense_index].parent
            && let Some(parent_dense) = self.get_dense_index(parent_id)
        {
            self.dense[parent_dense].children.retain(|c| *c != id);
        }

        // Take ownership of the widget to drop it AFTER fixing up indices
        // This is critical for recursive unregistration during Drop
        let last_dense_index = self.dense.len() - 1;

        // Swap-remove: move last element to this position
        let removed_node = self.dense.swap_remove(dense_index);

        // Fix up the moved node's sparse entry (if we didn't remove the last element)
        if dense_index != last_dense_index && !self.dense.is_empty() {
            let moved_sparse_idx = self.dense[dense_index].sparse_index;
            self.sparse[moved_sparse_idx as usize].dense_index = Some(dense_index);
        }

        // Vacate the sparse slot, keeping its generation for the next allocation
        self.sparse[id.index as usize].dense_index = None;
        self.free_indices.push(id.index);

        // Now drop the removed widget (may trigger recursive unregisters)
        drop(removed_node);
    }

    /// Get the dense array index for a WidgetId, validating generation.
    fn get_dense_index(&self, id: WidgetId) -> Option<usize> {
        self.sparse
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.dense_index)
    }

    /// Access a widget via a closure.
    pub fn with_widget<R>(&self, id: WidgetId, f: impl FnOnce(&dyn Widget) -> R) -> Option<R> {
        self.get_dense_index(id)
            .map(|idx| f(&*self.dense[idx].widget))
    }

    /// Mutate a widget via a closure.
    ///
    /// The closure receives the widget ID, mutable access to the widget, and the tree,
    /// allowing operations that need all three (like calling layout on children).
    ///
    /// The widget is temporarily extracted from the tree during the closure execution.
    /// Returns `None` if the widget is not found (invalid or stale ID).
    pub fn with_widget_mut<R>(
        &mut self,
        id: WidgetId,
        f: impl FnOnce(&mut dyn Widget, WidgetId, &mut Tree) -> R,
    ) -> Option<R> {
        let dense_index = self.get_dense_index(id)?;

        // Placeholder widget for extraction
        struct PlaceholderWidget;
        impl Widget for PlaceholderWidget {
            fn layout(&mut self, _: &mut Tree, _: WidgetId, _: Constraints) -> Size {
                Size::zero()
            }
            fn paint(&self, _: &Tree, _: WidgetId, _: &mut crate::renderer::PaintContext) {}
        }

        // Extract widget
        let mut widget = std::mem::replace(
            &mut self.dense[dense_index].widget,
            Box::new(PlaceholderWidget),
        );

        // Run closure with &mut dyn Widget, WidgetId, and &mut Tree
        let result = f(&mut *widget, id, self);

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

    /// Record that this node is an interaction unit.
    ///
    /// Containers call this as they enter the tree, exactly like the style
    /// declarations beside it.
    pub(crate) fn set_control(&mut self, id: WidgetId, control: Option<crate::widgets::Control>) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].control = control;
        }
    }

    /// The interaction unit a widget belongs to: itself if it is one,
    /// otherwise the nearest ancestor that is.
    ///
    /// Returns a handle holding signals, not values, on the same contract as
    /// the style walks: reading them is what subscribes, so the caller must do
    /// it inside its own tracking scope.
    pub fn nearest_control(&self, id: WidgetId) -> Option<crate::widgets::Control> {
        let mut cursor = Some(id);
        while let Some(node) = cursor {
            let idx = self.get_dense_index(node)?;
            if let Some(control) = self.dense[idx].control {
                return Some(control);
            }
            cursor = self.dense[idx].parent;
        }
        None
    }

    /// Declare how far this widget paints outside its own bounds.
    ///
    /// Widgets that cast a shadow or stroke set this during layout; it widens
    /// the damage they report without touching the size they occupy.
    /// Report where this widget's text sits on its baseline.
    ///
    /// Only leaves that draw text have one. A parent aligning on
    /// `CrossAlignment::Baseline` shifts its children so these coincide;
    /// anything without a baseline is aligned by its bottom edge, as CSS does.
    pub fn set_baseline(&mut self, id: WidgetId, baseline: f32) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].baseline = Some(baseline);
        }
    }

    /// The widget's baseline, if it reported one.
    pub fn baseline(&self, id: WidgetId) -> Option<f32> {
        self.get_dense_index(id)
            .and_then(|idx| self.dense[idx].baseline)
    }

    pub fn set_paint_overflow(&mut self, id: WidgetId, overflow: f32) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].paint_overflow = overflow.max(0.0);
        }
    }

    /// How far this widget's paint reaches beyond its bounds.
    #[cfg(test)]
    pub(crate) fn paint_overflow(&self, id: WidgetId) -> f32 {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].paint_overflow)
            .unwrap_or(0.0)
    }

    /// Get the children of a widget (returns a slice to avoid heap allocation).
    pub fn get_children(&self, id: WidgetId) -> &[WidgetId] {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].children.as_slice())
            .unwrap_or(&[])
    }

    /// Collect a subtree in post-order (children before parents).
    ///
    /// Used for surface teardown: unregistering in this order means each
    /// widget's Drop finds its children already gone, so the deferred
    /// Unregister jobs it schedules become no-ops.
    pub fn collect_subtree_post_order(&self, root: WidgetId) -> Vec<WidgetId> {
        let mut ordered = Vec::new();
        // Pre-order DFS, then reverse: yields children before parents
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if self.get_dense_index(id).is_none() {
                continue;
            }
            ordered.push(id);
            stack.extend_from_slice(self.get_children(id));
        }
        ordered.reverse();
        ordered
    }

    /// Mark a widget as needing layout, returning the layout root.
    ///
    /// The needs_layout flag bubbles up to the nearest relayout boundary or root.
    /// Returns `Some(root_id)` if a layout root was found and should be queued.
    /// Returns `None` if the widget already needs layout (boundary already queued).
    ///
    /// Optimization: If a widget already needs layout, we stop early since its
    /// boundary must already be in the queue. This requires all widgets to
    /// call `clear_needs_layout` after completing layout.
    pub fn mark_needs_layout(&mut self, widget_id: WidgetId) -> Option<WidgetId> {
        let mut current = widget_id;

        loop {
            let dense_idx = self.get_dense_index(current)?;

            // Optimization: Stop if already marked - boundary is already in queue
            if self.dense[dense_idx].needs_layout {
                return None;
            }

            // Mark as needing layout
            self.dense[dense_idx].needs_layout = true;

            // Check if this is a relayout boundary
            if self.dense[dense_idx].is_relayout_boundary {
                return Some(current);
            }

            // Move up to parent
            match self.dense[dense_idx].parent {
                Some(parent) => {
                    current = parent;
                }
                None => {
                    // Reached root
                    return Some(current);
                }
            }
        }
    }

    /// Clear the needs-layout flag for a widget.
    pub fn clear_needs_layout(&mut self, id: WidgetId) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].needs_layout = false;
        }
    }

    /// Check if a widget needs layout.
    pub fn needs_layout(&self, id: WidgetId) -> bool {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].needs_layout)
            .unwrap_or(false)
    }

    /// Mark a widget as needing paint, propagating up to the root.
    ///
    /// Similar to `mark_needs_layout`, this bubbles the paint-dirty flag
    /// upward so that ancestors know to repaint. Early-exits if a node
    /// is already marked (its ancestors must already be marked too).
    ///
    /// Also accumulates the widget's surface-relative bounds into the
    /// damage region for Wayland damage reporting.
    pub fn mark_needs_paint(&mut self, widget_id: WidgetId) {
        // The damage walk comes before the flag early-out below, and the order
        // is load-bearing. `cache_layout` damages the rect a widget vacated and
        // then delegates the *new* one here — so an early-out on an
        // already-set flag would drop the new rect whenever a resize follows a
        // paint mark, which is most resizes. Moving the early-out first looks
        // like a free saving and is not.

        // Accumulate damage for the actual dirty widget (before propagation),
        // attributed to the widget's surface root
        if let Some((root, bounds)) = self.surface_relative_bounds_and_root(widget_id) {
            self.expand_damage_rect(root, bounds);
        }

        let mut current = widget_id;
        loop {
            let Some(dense_idx) = self.get_dense_index(current) else {
                return;
            };
            if self.dense[dense_idx].needs_paint {
                return; // Already marked — ancestors are too
            }
            self.dense[dense_idx].needs_paint = true;
            match self.dense[dense_idx].parent {
                Some(parent) => current = parent,
                None => return,
            }
        }
    }

    /// Clear the needs-paint flag for a widget.
    pub fn clear_needs_paint(&mut self, id: WidgetId) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].needs_paint = false;
        }
    }

    /// Check if a widget needs paint.
    pub fn needs_paint(&self, id: WidgetId) -> bool {
        self.get_dense_index(id)
            .map(|idx| self.dense[idx].needs_paint)
            .unwrap_or(true) // Default to true for unknown widgets
    }

    /// Mark a widget and all its descendants as needing paint.
    ///
    /// Sets full damage for the widget's surface since the entire subtree
    /// is being repainted.
    pub fn mark_subtree_needs_paint(&mut self, widget_id: WidgetId) {
        if let Some(root) = self.surface_root_of(widget_id) {
            self.damage.insert(root, DamageRegion::Full);
        }
        self.mark_subtree_needs_paint_inner(widget_id);
        // Ancestors must learn a descendant is dirty: the per-surface
        // skip check reads the surface root's flag and paint-cache reuse
        // consults each ancestor. Without this, a reconcile-only update
        // (a closure child whose signals no paint pass ever tracked, e.g.
        // a calendar month grid) relaid out its subtree and then the
        // frame was skipped forever — widgets current, screen stale.
        let mut current = widget_id;
        loop {
            let Some(dense_idx) = self.get_dense_index(current) else {
                return;
            };
            let Some(parent) = self.dense[dense_idx].parent else {
                return;
            };
            let Some(parent_idx) = self.get_dense_index(parent) else {
                return;
            };
            if self.dense[parent_idx].needs_paint {
                return; // Already marked — its ancestors are too
            }
            self.dense[parent_idx].needs_paint = true;
            current = parent;
        }
    }

    fn mark_subtree_needs_paint_inner(&mut self, widget_id: WidgetId) {
        // Iterative DFS to avoid cloning children SmallVecs for borrow checker
        let mut stack = vec![widget_id];
        while let Some(id) = stack.pop() {
            let Some(dense_idx) = self.get_dense_index(id) else {
                continue;
            };
            self.dense[dense_idx].needs_paint = true;
            stack.extend_from_slice(&self.dense[dense_idx].children);
        }
    }

    // -------------------------------------------------------------------------
    // Damage Region Tracking
    // -------------------------------------------------------------------------

    /// Get the surface-relative bounds of a widget by walking up the parent chain
    /// and summing origins.
    pub fn get_surface_relative_bounds(&self, id: WidgetId) -> Option<Rect> {
        self.surface_relative_bounds_and_root(id)
            .map(|(_, bounds)| bounds)
    }

    /// Walk to the surface root, returning it together with the widget's
    /// surface-relative bounds (origins summed along the chain).
    fn surface_relative_bounds_and_root(&self, id: WidgetId) -> Option<(WidgetId, Rect)> {
        let idx = self.get_dense_index(id)?;
        let size = self.dense[idx].cached_size?;
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut current = id;
        loop {
            let dense_idx = self.get_dense_index(current)?;
            x += self.dense[dense_idx].origin.0;
            y += self.dense[dense_idx].origin.1;
            match self.dense[dense_idx].parent {
                Some(parent) => current = parent,
                None => break,
            }
        }
        // Widen by whatever this widget paints outside itself, so a shadow is
        // re-composited along with the thing that cast it.
        let overflow = self.dense[idx].paint_overflow;
        Some((
            current,
            Rect::new(
                x - overflow,
                y - overflow,
                size.width + overflow * 2.0,
                size.height + overflow * 2.0,
            ),
        ))
    }

    /// Find the surface root (topmost ancestor) of a widget.
    pub(crate) fn surface_root_of(&self, id: WidgetId) -> Option<WidgetId> {
        let mut current = id;
        loop {
            let dense_idx = self.get_dense_index(current)?;
            match self.dense[dense_idx].parent {
                Some(parent) => current = parent,
                None => return Some(current),
            }
        }
    }

    /// Union a rect into the damage region of the given surface root.
    fn expand_damage_rect(&mut self, root: WidgetId, rect: Rect) {
        let entry = self.damage.entry(root).or_insert(DamageRegion::None);
        *entry = match entry {
            DamageRegion::None => DamageRegion::Partial(rect),
            DamageRegion::Partial(existing) => {
                let min_x = existing.x.min(rect.x);
                let min_y = existing.y.min(rect.y);
                let max_x = (existing.x + existing.width).max(rect.x + rect.width);
                let max_y = (existing.y + existing.height).max(rect.y + rect.height);
                DamageRegion::Partial(Rect::new(min_x, min_y, max_x - min_x, max_y - min_y))
            }
            DamageRegion::Full => DamageRegion::Full,
        };
    }

    /// Set full-surface damage for a surface root (e.g., on resize,
    /// initialization, or a failed present that must be retried).
    pub fn set_full_damage(&mut self, root: WidgetId) {
        self.damage.insert(root, DamageRegion::Full);
    }

    /// Take the accumulated damage region for a surface root, resetting it.
    pub fn take_damage(&mut self, root: WidgetId) -> DamageRegion {
        self.damage.remove(&root).unwrap_or(DamageRegion::None)
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

    /// Cache the constraints and size for a widget, and invalidate its paint.
    ///
    /// This is only reached by a widget that actually ran its layout — the
    /// ones that hit the early-out (clean, same constraints) return before
    /// getting here — so it is the point where the layout pass tells paint
    /// what it touched. Same rule as Flutter, whose `RenderObject.layout()`
    /// ends in `markNeedsPaint()` for the objects that laid out; everything
    /// the pass skipped keeps its cached paint.
    ///
    /// Running layout is the invalidation signal, not a size change: a
    /// widget's painted output can depend on state it refreshes during
    /// layout (a `Text` re-reads its content there and paints from the
    /// cached copy), so a clock going from `15:23` to `15:24` at the very
    /// same width still has to be redrawn.
    ///
    /// When the size changes, the damage covers both the rectangle the
    /// widget used to occupy and the one it occupies now.
    pub fn cache_layout(&mut self, id: WidgetId, constraints: Constraints, size: Size) {
        let Some(idx) = self.get_dense_index(id) else {
            return;
        };
        let previous_size = self.dense[idx].cached_size;
        if previous_size.is_some_and(|prev| prev != size)
            && let Some((root, vacated)) = self.surface_relative_bounds_and_root(id)
        {
            self.expand_damage_rect(root, vacated);
        }
        let idx = self.get_dense_index(id).expect("checked above");
        self.dense[idx].cached_constraints = Some(constraints);
        self.dense[idx].cached_size = Some(size);
        // Damages the new rect and marks the ancestors, which have to redraw
        // to re-emit this widget at its new geometry.
        self.mark_needs_paint(id);
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

    /// Set the origin (position) for a widget.
    ///
    /// A widget that moves damages the pixels it leaves behind as well as the
    /// ones it now covers. It does not need repainting for that: its painted
    /// content is position-independent and the parent re-emits it at the new
    /// offset (paint-cache reuse re-parents the cached node). The parent is
    /// already marked, since moving a child means it ran its own layout.
    pub fn set_origin(&mut self, id: WidgetId, x: f32, y: f32) {
        let Some(idx) = self.get_dense_index(id) else {
            return;
        };
        if self.dense[idx].origin == (x, y) {
            return;
        }
        if let Some((root, vacated)) = self.surface_relative_bounds_and_root(id) {
            self.expand_damage_rect(root, vacated);
        }
        let idx = self.get_dense_index(id).expect("checked above");
        self.dense[idx].origin = (x, y);
        if let Some((root, occupied)) = self.surface_relative_bounds_and_root(id) {
            self.expand_damage_rect(root, occupied);
        }
    }

    /// Get the origin (position) for a widget.
    pub fn get_origin(&self, id: WidgetId) -> Option<(f32, f32)> {
        self.get_dense_index(id).map(|idx| self.dense[idx].origin)
    }

    /// Get the bounds (origin + cached size) for a widget.
    pub fn get_bounds(&self, id: WidgetId) -> Option<crate::widgets::Rect> {
        let idx = self.get_dense_index(id)?;
        let node = &self.dense[idx];
        let size = node.cached_size?;
        Some(crate::widgets::Rect::new(
            node.origin.0,
            node.origin.1,
            size.width,
            size.height,
        ))
    }

    /// Cache a widget's paint output. The node is Rc-shared with the frame's
    /// render tree, so this is a refcount bump, not a deep clone.
    pub fn cache_paint(&mut self, id: WidgetId, node: std::rc::Rc<crate::renderer::RenderNode>) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].cached_paint = Some(node);
        }
    }

    /// Get a widget's cached paint output.
    pub fn cached_paint(&self, id: WidgetId) -> Option<&std::rc::Rc<crate::renderer::RenderNode>> {
        self.get_dense_index(id)
            .and_then(|idx| self.dense[idx].cached_paint.as_ref())
    }

    /// Drop a widget's cached paint output, so the next frame paints it in
    /// full. What a paint that could not be cached has to do with the entry
    /// the last one left: it is a picture of this widget as it no longer is.
    pub fn clear_cached_paint(&mut self, id: WidgetId) {
        if let Some(idx) = self.get_dense_index(id) {
            self.dense[idx].cached_paint = None;
        }
    }

    /// Clear all widgets and metadata.
    pub fn clear(&mut self) {
        self.dense.clear();
        self.sparse.clear();
        self.free_indices.clear();
        self.damage.clear();
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
    struct MockWidget;

    impl MockWidget {
        fn new() -> Self {
            Self
        }
    }

    impl Widget for MockWidget {
        fn layout(&mut self, _tree: &mut Tree, _id: WidgetId, constraints: Constraints) -> Size {
            Size::new(constraints.max_width, constraints.max_height)
        }

        fn paint(&self, _tree: &Tree, _id: WidgetId, _ctx: &mut crate::renderer::PaintContext) {}
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

    /// Regression test: the generation must keep advancing across repeated
    /// recycles of the same slot. The old implementation read the generation
    /// from a sparse entry that `unregister` had already cleared, so every
    /// reuse produced generation 1 and a stale ID from cycle N aliased the
    /// live widget of cycle N+2.
    #[test]
    fn test_tree_generation_advances_across_recycles() {
        let mut tree = Tree::new();

        let mut previous_ids = Vec::new();
        let mut current = tree.register(Box::new(MockWidget::new()));

        for _ in 0..5 {
            tree.unregister(current);
            let next = tree.register(Box::new(MockWidget::new()));
            assert_eq!(next.index, current.index, "slot should be recycled");
            previous_ids.push(current);
            current = next;

            // No previously issued ID may resolve to the new widget
            for stale in &previous_ids {
                assert!(
                    !tree.contains(*stale),
                    "stale id {stale:?} aliases live widget {current:?}"
                );
            }
            assert!(tree.contains(current));
        }
    }

    #[test]
    fn test_tree_parent_child() {
        let mut tree = Tree::new();
        let parent_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, parent_id);

        assert_eq!(tree.get_parent(child_id), Some(parent_id));
        assert_eq!(tree.get_children(parent_id), &[child_id]);
    }

    #[test]
    fn test_tree_needs_layout_propagation() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));
        let grandchild_id = tree.register(Box::new(MockWidget::new()));

        // Build tree: root -> child -> grandchild
        tree.set_parent(child_id, root_id);
        tree.set_parent(grandchild_id, child_id);

        // Mark grandchild needs_layout - should bubble to root and return root
        let layout_root = tree.mark_needs_layout(grandchild_id);

        assert!(tree.needs_layout(grandchild_id));
        assert!(tree.needs_layout(child_id));
        assert!(tree.needs_layout(root_id));

        // Root should be returned as the layout root
        assert_eq!(layout_root, Some(root_id));
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

        // Mark leaf needs_layout - should stop at boundary and return boundary
        let layout_root = tree.mark_needs_layout(leaf_id);

        assert!(tree.needs_layout(leaf_id));
        assert!(tree.needs_layout(boundary_id));
        assert!(!tree.needs_layout(root_id)); // Root should NOT need layout

        // Boundary should be returned as the layout root, not root
        assert_eq!(layout_root, Some(boundary_id));
    }

    #[test]
    fn test_tree_needs_layout_optimization() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, root_id);

        // Mark child needs_layout - root should be returned
        let layout_root = tree.mark_needs_layout(child_id);
        assert!(tree.needs_layout(child_id));
        assert!(tree.needs_layout(root_id));
        assert_eq!(layout_root, Some(root_id));

        // Simulate layout running: clear ALL needs_layout flags
        // (this is what widgets should do after layout)
        tree.clear_needs_layout(root_id);
        tree.clear_needs_layout(child_id);

        // Mark child again - should return root again
        let layout_root = tree.mark_needs_layout(child_id);
        assert_eq!(layout_root, Some(root_id));

        // Clear only root's flag, leave child marked
        tree.clear_needs_layout(root_id);

        // Mark child again - should return None (already marked)
        let layout_root = tree.mark_needs_layout(child_id);
        assert_eq!(layout_root, None);
    }

    #[test]
    fn test_tree_with_widget() {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(MockWidget::new()));

        // Test that we can access widget through with_widget
        let exists = tree.with_widget(id, |_w| true);
        assert!(exists.is_some());
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

    #[test]
    fn test_widget_id_from_u64_roundtrip() {
        let id = WidgetId::new(42, 7);
        let val = id.as_u64();
        let id2 = WidgetId::from_u64(val);
        assert_eq!(id, id2);
    }

    #[test]
    fn test_new_widget_needs_paint() {
        let mut tree = Tree::new();
        let id = tree.register(Box::new(MockWidget::new()));
        assert!(tree.needs_paint(id));
    }

    #[test]
    fn test_needs_paint_propagation() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));
        let grandchild_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, root_id);
        tree.set_parent(grandchild_id, child_id);

        // Clear all paint flags
        tree.clear_needs_paint(root_id);
        tree.clear_needs_paint(child_id);
        tree.clear_needs_paint(grandchild_id);

        assert!(!tree.needs_paint(root_id));
        assert!(!tree.needs_paint(child_id));
        assert!(!tree.needs_paint(grandchild_id));

        // Mark grandchild - should propagate to root
        tree.mark_needs_paint(grandchild_id);
        assert!(tree.needs_paint(grandchild_id));
        assert!(tree.needs_paint(child_id));
        assert!(tree.needs_paint(root_id));
    }

    #[test]
    fn test_needs_paint_early_exit() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, root_id);

        // Clear all
        tree.clear_needs_paint(root_id);
        tree.clear_needs_paint(child_id);

        // Mark child → root marked
        tree.mark_needs_paint(child_id);
        assert!(tree.needs_paint(root_id));

        // Clear child but leave root marked
        tree.clear_needs_paint(child_id);

        // Mark child again — should early-exit at root (already marked)
        tree.mark_needs_paint(child_id);
        assert!(tree.needs_paint(child_id));
        assert!(tree.needs_paint(root_id));
    }

    #[test]
    fn test_mark_subtree_needs_paint() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child1_id = tree.register(Box::new(MockWidget::new()));
        let child2_id = tree.register(Box::new(MockWidget::new()));
        let grandchild_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child1_id, root_id);
        tree.set_parent(child2_id, root_id);
        tree.set_parent(grandchild_id, child1_id);

        // Clear all
        tree.clear_needs_paint(root_id);
        tree.clear_needs_paint(child1_id);
        tree.clear_needs_paint(child2_id);
        tree.clear_needs_paint(grandchild_id);

        // Mark subtree at root — all should be marked
        tree.mark_subtree_needs_paint(root_id);
        assert!(tree.needs_paint(root_id));
        assert!(tree.needs_paint(child1_id));
        assert!(tree.needs_paint(child2_id));
        assert!(tree.needs_paint(grandchild_id));
    }

    /// Regression test: marking a subtree must also inform the ANCESTORS —
    /// the per-surface skip check reads the surface root's flag, so a
    /// reconcile-only update (partial layout root deep in the tree, no
    /// paint-tracked signals anywhere on the path) would otherwise relayout
    /// its subtree and then never produce a frame: widgets current, screen
    /// permanently stale.
    #[test]
    fn test_mark_subtree_needs_paint_propagates_to_ancestors() {
        let mut tree = Tree::new();
        let root_id = tree.register(Box::new(MockWidget::new()));
        let child_id = tree.register(Box::new(MockWidget::new()));
        let grandchild_id = tree.register(Box::new(MockWidget::new()));
        let leaf_id = tree.register(Box::new(MockWidget::new()));

        tree.set_parent(child_id, root_id);
        tree.set_parent(grandchild_id, child_id);
        tree.set_parent(leaf_id, grandchild_id);

        tree.clear_needs_paint(root_id);
        tree.clear_needs_paint(child_id);
        tree.clear_needs_paint(grandchild_id);
        tree.clear_needs_paint(leaf_id);

        // Mark a subtree deep in the tree: the surface root must see it
        tree.mark_subtree_needs_paint(grandchild_id);
        assert!(tree.needs_paint(grandchild_id));
        assert!(tree.needs_paint(leaf_id));
        assert!(
            tree.needs_paint(child_id),
            "parent of the marked subtree must be marked"
        );
        assert!(
            tree.needs_paint(root_id),
            "surface root must see the dirty descendant or the frame is skipped"
        );
    }

    /// Build `root -> [a, b]`, both laid out and clean.
    fn two_children_tree() -> (Tree, WidgetId, WidgetId, WidgetId) {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        let a = tree.register(Box::new(MockWidget::new()));
        let b = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(a, root);
        tree.set_parent(b, root);

        let c = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(root, c, Size::new(100.0, 100.0));
        tree.cache_layout(a, c, Size::new(40.0, 20.0));
        tree.set_origin(a, 0.0, 0.0);
        tree.cache_layout(b, c, Size::new(40.0, 20.0));
        tree.set_origin(b, 0.0, 20.0);
        for id in [root, a, b] {
            tree.clear_needs_paint(id);
        }
        let _ = tree.take_damage(root);
        (tree, root, a, b)
    }

    /// The point of incremental invalidation: re-laying out one subtree must
    /// leave its siblings' paint caches alone. Marking the layout root's whole
    /// subtree instead (the old behaviour) repainted every widget under it,
    /// which in a content-sized tree is the entire surface.
    #[test]
    fn relayout_of_one_child_leaves_its_sibling_paintable_from_cache() {
        let (mut tree, root, a, b) = two_children_tree();

        // `a` re-runs its layout; `b` is untouched
        tree.cache_layout(
            a,
            Constraints::new(0.0, 0.0, 100.0, 100.0),
            Size::new(60.0, 20.0),
        );

        assert!(tree.needs_paint(a), "the widget that laid out repaints");
        assert!(
            tree.needs_paint(root),
            "its ancestors repaint to re-emit it at the new geometry"
        );
        assert!(
            !tree.needs_paint(b),
            "an untouched sibling must keep its cached paint"
        );
    }

    /// A widget can run layout and come out the same size while painting
    /// something else — a clock going from `15:23` to `15:24` refreshes its
    /// glyphs during layout. Running layout is the invalidation signal.
    #[test]
    fn running_layout_repaints_even_when_the_size_is_unchanged() {
        let (mut tree, _root, a, _b) = two_children_tree();
        let same = tree.cached_size(a).unwrap();

        tree.cache_layout(a, Constraints::new(0.0, 0.0, 100.0, 100.0), same);

        assert!(tree.needs_paint(a));
    }

    /// Shrinking must damage the rectangle the widget stops covering, or the
    /// compositor keeps showing the pixels it left behind.
    #[test]
    fn shrinking_damages_the_vacated_rectangle() {
        let (mut tree, root, a, _b) = two_children_tree();

        tree.cache_layout(
            a,
            Constraints::new(0.0, 0.0, 100.0, 100.0),
            Size::new(10.0, 20.0),
        );

        match tree.take_damage(root) {
            DamageRegion::Partial(rect) => {
                assert!(
                    rect.width >= 40.0,
                    "damage must cover the old 40px-wide box, got {rect:?}"
                );
            }
            other => panic!("expected partial damage, got {other:?}"),
        }
    }

    /// Moving a widget damages both where it was and where it lands. Its own
    /// paint stays valid — the parent re-emits it at the new offset.
    #[test]
    fn moving_a_widget_damages_both_positions() {
        let (mut tree, root, a, _b) = two_children_tree();

        tree.set_origin(a, 0.0, 60.0);

        match tree.take_damage(root) {
            DamageRegion::Partial(rect) => {
                assert!(
                    rect.y <= 0.0,
                    "damage must reach the old position, got {rect:?}"
                );
                assert!(
                    rect.y + rect.height >= 80.0,
                    "damage must reach the new position, got {rect:?}"
                );
            }
            other => panic!("expected partial damage, got {other:?}"),
        }
        assert!(
            !tree.needs_paint(a),
            "a widget that only moved keeps its cached paint"
        );
    }

    /// Whatever a widget paints outside its own bounds — an elevation shadow,
    /// a glyph shadow — has to be inside the damage it reports, or repainting
    /// it re-composites a rect that stops short and the old one stays on
    /// screen as a fringe.
    #[test]
    fn damage_covers_what_a_widget_paints_outside_its_bounds() {
        let (mut tree, root, a, _b) = two_children_tree();
        tree.set_paint_overflow(a, 8.0);

        tree.mark_needs_paint(a);

        match tree.take_damage(root) {
            DamageRegion::Partial(rect) => {
                assert!(
                    rect.x <= -8.0 && rect.y <= -8.0,
                    "damage must reach above and left of the widget, got {rect:?}"
                );
                assert!(
                    rect.width >= 40.0 + 16.0 && rect.height >= 20.0 + 16.0,
                    "and past its far edges, got {rect:?}"
                );
            }
            other => panic!("expected partial damage, got {other:?}"),
        }
    }

    #[test]
    fn a_widget_with_no_overflow_damages_exactly_its_bounds() {
        let (mut tree, root, a, _b) = two_children_tree();

        tree.mark_needs_paint(a);

        match tree.take_damage(root) {
            DamageRegion::Partial(rect) => {
                assert_eq!((rect.x, rect.y), (0.0, 0.0));
                assert_eq!((rect.width, rect.height), (40.0, 20.0));
            }
            other => panic!("expected partial damage, got {other:?}"),
        }
    }
}
