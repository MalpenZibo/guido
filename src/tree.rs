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
    /// How far this widget's *own* painting lands outside its bounds — its
    /// shadow, and where its transform can carry it. Published by the widget.
    own_paint_reach: f32,

    /// How far anything its children draw stands outside its box.
    ///
    /// Kept apart from `own_paint_reach` so the two can be maintained on
    /// different schedules: the widget republishes its own on every Paint job,
    /// which must stay cheap, while this one is a fact about a list and is
    /// re-measured only when that list is walked anyway.
    children_outset: f32,

    /// Whether this widget clips its children to its own edges.
    ///
    /// A scroller's content runs far past its viewport by design, and what the
    /// scroller paints is its own box however far the column inside it runs. So
    /// the overhang stops here: it is not gathered into this widget, and
    /// nothing above it hears about it either.
    ///
    /// Kept on the slot rather than asked of the widget because the gather runs
    /// from `set_own_paint_reach`, which has a tree and an id and no widget —
    /// and a one-shot clear at layout would be undone by the next descendant
    /// whose reach moves.
    clips_children: bool,

    /// The widest reach among this widget's children.
    ///
    /// Cached here rather than walked at paint, because the search that reads
    /// it is O(log n) and walking the children to feed it would be O(n) — the
    /// cost that search exists to avoid.
    ///
    /// Kept true between layouts on the two schedules `gather_reach_upward`
    /// describes: a reach that grew widens this by comparison, and one that
    /// shrank re-measures the siblings, because the child that shrank may have
    /// been the widest and the new answer is whatever the others say.
    children_reach: f32,
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
    /// When the event now being dispatched happened.
    ///
    /// Not the same question as `frame_instant`: a frame advances what is
    /// already moving, an event says something started. They are different
    /// moments, and an event's is the older of the two — it was queued when the
    /// compositor sent it and is read when the frame runs.
    ///
    /// `dispatch_events` writes it, once per event, because it delivers them one
    /// at a time. `None` outside a dispatch, where nobody should be reading it.
    event_instant: Option<std::time::Instant>,
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
            event_instant: None,
        }
    }

    /// What time it is for the pass now running.
    ///
    /// Falls back to the clock for a call that arrives outside a pass — the
    /// behaviour every caller had before there was a frame instant at all.
    pub fn frame_instant(&self) -> std::time::Instant {
        self.frame_instant.unwrap_or_else(std::time::Instant::now)
    }

    /// When the event being dispatched happened.
    ///
    /// Falls back to the clock outside a dispatch — the behaviour every caller
    /// had before an event carried its own time.
    pub fn event_instant(&self) -> std::time::Instant {
        self.event_instant.unwrap_or_else(std::time::Instant::now)
    }

    /// Declare when the event about to be dispatched happened, or `None` when
    /// the dispatch is over.
    ///
    /// `dispatch_events` is the caller in the loop; a test that wants to place
    /// an event in time is the other one.
    pub fn set_event_instant(&mut self, at: Option<std::time::Instant>) {
        self.event_instant = at;
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
            own_paint_reach: 0.0,
            children_outset: 0.0,
            clips_children: false,
            children_reach: 0.0,
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

    /// Record how far this widget's own painting lands outside its bounds.
    ///
    /// Named for the half it writes, because `paint_overflow` returns the union
    /// of that with what the children add, and a setter and getter that are not
    /// inverses should not share a name.
    ///
    /// What a widget draws also includes what its descendants draw, and that
    /// half is `children_outset`, gathered upward here when the union a parent
    /// can see actually moves. Without it a row holding a box that a transform
    /// carried outside it reports nothing, an ancestor narrowing by laid-out
    /// bounds drops the row, and the visible box goes with it. Flutter and
    /// Blink union descendant paint bounds up the tree for the same reason.
    ///
    /// A reach that shrinks damages the ring it is vacating before it goes, on
    /// the same contract as `set_origin`: the rect a repaint reports is built
    /// from the reach the widget has *now*, so nothing else would ever name
    /// the pixels it painted a frame ago.
    pub fn set_own_paint_reach(&mut self, id: WidgetId, reach: f32) {
        let Some(idx) = self.get_dense_index(id) else {
            return;
        };
        let reach = reach.max(0.0);
        if self.dense[idx].own_paint_reach == reach {
            return;
        }
        // Against the union, not against the half being written: an ancestor
        // reads `paint_overflow`, so a widget whose children already reach
        // further has moved nothing anyone can see, and the walk above it —
        // whose shrink path re-measures siblings — is pure cost.
        let before = self.paint_overflow_from(self.dense[idx].own_paint_reach, idx);
        let after = self.paint_overflow_from(reach, idx);
        // A reach that shrinks vacates the ring between the two, and the rect
        // built from the new one stops short of it. Damaged before the write,
        // while the old reach is still what this widget answers — the same
        // shape `set_origin` uses for the geometry it owns, and for the same
        // reason. A reach that *grows* needs none of it: the mark that follows
        // builds a rect from the wider value, and it contains the narrower one.
        if after < before
            && let Some((root, vacated)) = self.surface_relative_bounds_and_root(id)
        {
            self.expand_damage_rect(root, vacated);
        }
        self.dense[idx].own_paint_reach = reach;
        if before == after {
            return;
        }
        self.gather_reach_upward(id, before, after > before);
    }

    /// Carry a change in a child's reach into the ancestors it affects.
    ///
    /// A reach that **grew** only ever widens, so each ancestor is compared and
    /// the walk stops at the first one already wide enough — nothing above it
    /// can be narrow either. O(depth), no walk over siblings.
    ///
    /// A reach that **shrank** cannot be answered by comparison: this child may
    /// have been the widest, and the new maximum is whatever the others say. So
    /// that path re-measures — but only where it has to. A child that was not
    /// the maximum on either axis cannot have changed one by getting smaller,
    /// which is the common case and the one that matters: the return leg of a
    /// spring shrinks on *every frame*, so a fold over the siblings there would
    /// be an O(children) walk per frame, and a staggered list of them O(n²).
    fn gather_reach_upward(&mut self, from: WidgetId, was: f32, grew: bool) {
        let mut child = from;
        let mut was = was;
        while let Some(parent) = self.get_parent(child) {
            let Some(parent_idx) = self.get_dense_index(parent) else {
                return;
            };
            let (had_outset, had_reach) = (
                self.dense[parent_idx].children_outset,
                self.dense[parent_idx].children_reach,
            );
            if self.dense[parent_idx].clips_children {
                // The overhang stops here. The search below this widget still
                // wants to know how far its children reach, so that is carried;
                // what this widget paints is its own box, so nothing above it
                // learns anything and the walk is done.
                let (_, widest) = self.measure_children(parent);
                self.dense[parent_idx].children_reach = widest;
                self.dense[parent_idx].children_outset = 0.0;
                return;
            }
            let (outset, widest) = if grew {
                (
                    had_outset.max(self.child_paint_outset(child, parent)),
                    had_reach.max(self.paint_overflow(child)),
                )
            } else if was < had_reach && self.child_outset_from(was, child, parent) < had_outset {
                // Not the widest on either axis before it shrank, so neither
                // maximum can have moved, and nothing above can have either.
                return;
            } else {
                self.measure_children(parent)
            };

            if (outset, widest) == (had_outset, had_reach) {
                // Nothing moved here, so nothing above it moved either.
                return;
            }
            // What this parent's own reach *was*, which is what the guard on
            // the next level up needs — the child's previous `paint_overflow`,
            // not the maximum over its siblings. The two differ wherever a
            // node's children reach less far than they stand outside its box,
            // which is every scroller.
            was = self.dense[parent_idx].own_paint_reach.max(had_outset);
            self.dense[parent_idx].children_outset = outset;
            self.dense[parent_idx].children_reach = widest;
            child = parent;
        }
    }

    /// How far what `child` draws stands outside `parent`'s own box.
    fn child_paint_outset(&self, child: WidgetId, parent: WidgetId) -> f32 {
        self.child_outset_from(self.paint_overflow(child), child, parent)
    }

    /// The same, for a reach the caller names — which is how the shrink path
    /// asks what a child's outset *was* without having kept the rect.
    fn child_outset_from(&self, reach: f32, child: WidgetId, parent: WidgetId) -> f32 {
        let (Some(child_box), Some(parent_box)) = (self.get_bounds(child), self.get_bounds(parent))
        else {
            return 0.0;
        };
        // A child's bounds are relative to its parent, whose own box therefore
        // starts at the origin.
        child_box
            .outset(reach)
            .outset_beyond(crate::widgets::Rect::from_size(crate::layout::Size::new(
                parent_box.width,
                parent_box.height,
            )))
    }

    /// Both facts a narrowing needs about a widget's children: how far the
    /// furthest of them stands outside the box, and the widest reach any of
    /// them has.
    ///
    /// One walk and one lookup per child — the sparse-then-dense resolve is the
    /// expensive half, and every number below comes off the slot it lands on.
    fn measure_children(&self, id: WidgetId) -> (f32, f32) {
        let Some(parent_box) = self
            .get_bounds(id)
            .map(|b| crate::widgets::Rect::from_size(crate::layout::Size::new(b.width, b.height)))
        else {
            return (0.0, 0.0);
        };
        self.get_children(id)
            .iter()
            .filter_map(|&child| self.get_dense_index(child))
            .fold((0.0f32, 0.0f32), |(outset, widest), idx| {
                let slot = &self.dense[idx];
                // The reach counts whether or not the child has been laid out
                // — the grow path in `gather_reach_upward` folds it
                // unconditionally, and the two have to answer the same. Only
                // the outset needs a box to be measured against.
                let reach = slot.own_paint_reach.max(slot.children_outset);
                let widest = widest.max(reach);
                let Some(size) = slot.cached_size else {
                    return (outset, widest);
                };
                let drawn = crate::widgets::Rect::new(
                    slot.origin.0,
                    slot.origin.1,
                    size.width,
                    size.height,
                )
                .outset(reach);
                (
                    outset.max(drawn.outset_beyond(parent_box)),
                    widest.max(reach),
                )
            })
    }

    /// Republish both, as this widget's own layout has just measured them.
    /// Replaces rather than grows, which is what un-sticks a reach that shrank.
    ///
    /// Gathers upward when the union it feeds actually moved — see the body
    /// for why bottom-up layout is not enough on its own.
    pub(crate) fn remeasure_children(&mut self, id: WidgetId) {
        let (outset, widest) = self.measure_children(id);
        let Some(idx) = self.get_dense_index(id) else {
            return;
        };
        let before = self.paint_overflow(id);
        self.dense[idx].children_outset = outset;
        self.dense[idx].children_reach = widest;
        let after = self.paint_overflow(id);
        if before != after {
            // Layout is bottom-up, so an ancestor inside the same pass will
            // measure this for itself — but a widget with a fixed size is its
            // own relayout boundary and the pass starts there, with no ancestor
            // to follow. Then this is the only thing that tells them.
            self.gather_reach_upward(id, before, after > before);
        }
    }

    /// Record whether this widget clips its children to its own edges, and
    /// forget any overhang already gathered into it.
    ///
    /// The widest *reach* is kept: the search that narrows the children still
    /// needs it, because a child inside the clip can still draw outside its own
    /// bounds and into view.
    pub(crate) fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        let Some(idx) = self.get_dense_index(id) else {
            return;
        };
        self.dense[idx].clips_children = clips;
        if clips {
            self.dense[idx].children_outset = 0.0;
            let (_, widest) = self.measure_children(id);
            self.dense[idx].children_reach = widest;
        }
    }

    /// How far the furthest of this widget's children stands outside its box.
    #[cfg(test)]
    pub(crate) fn children_outset(&self, id: WidgetId) -> f32 {
        self.get_dense_index(id)
            .map_or(0.0, |idx| self.dense[idx].children_outset)
    }

    /// The widest reach among this widget's children.
    pub(crate) fn children_reach(&self, id: WidgetId) -> f32 {
        self.get_dense_index(id)
            .map_or(0.0, |idx| self.dense[idx].children_reach)
    }

    /// How far this widget's paint reaches beyond its bounds — a shadow's
    /// falloff, or the distance its own transform can move what it draws.
    ///
    /// Read by damage, so a repaint covers the shadow it moved, and by the two
    /// narrowings in `paint_children`, so neither drops a widget that draws
    /// somewhere other than where it was laid out.
    pub(crate) fn paint_overflow(&self, id: WidgetId) -> f32 {
        self.get_dense_index(id).map_or(0.0, |idx| {
            self.paint_overflow_from(self.dense[idx].own_paint_reach, idx)
        })
    }

    /// The same, for a reach the caller names — which is how a write asks what
    /// its own answer is about to become while the old one is still standing.
    fn paint_overflow_from(&self, reach: f32, idx: usize) -> f32 {
        reach.max(self.dense[idx].children_outset)
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
        // re-composited along with the thing that cast it — and so is a widget
        // its own transform has carried off its laid-out box.
        let overflow = self.paint_overflow(id);
        Some((
            current,
            Rect::new(x, y, size.width, size.height).outset(overflow),
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

    /// The damage rect a surface root is holding, which every question about
    /// damage below is asked of.
    fn partial_damage(tree: &mut Tree, root: WidgetId) -> Rect {
        match tree.take_damage(root) {
            DamageRegion::Partial(rect) => rect,
            other => panic!("expected partial damage, got {other:?}"),
        }
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

        let rect = partial_damage(&mut tree, root);
        assert!(
            rect.width >= 40.0,
            "damage must cover the old 40px-wide box, got {rect:?}"
        );
    }

    /// Moving a widget damages both where it was and where it lands. Its own
    /// paint stays valid — the parent re-emits it at the new offset.
    #[test]
    fn moving_a_widget_damages_both_positions() {
        let (mut tree, root, a, _b) = two_children_tree();

        tree.set_origin(a, 0.0, 60.0);

        let rect = partial_damage(&mut tree, root);
        assert!(
            rect.y <= 0.0,
            "damage must reach the old position, got {rect:?}"
        );
        assert!(
            rect.y + rect.height >= 80.0,
            "damage must reach the new position, got {rect:?}"
        );
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
        tree.set_own_paint_reach(a, 8.0);

        tree.mark_needs_paint(a);

        let rect = partial_damage(&mut tree, root);
        assert!(
            rect.x <= -8.0 && rect.y <= -8.0,
            "damage must reach above and left of the widget, got {rect:?}"
        );
        assert!(
            rect.width >= 40.0 + 16.0 && rect.height >= 20.0 + 16.0,
            "and past its far edges, got {rect:?}"
        );
    }

    /// The widest child shrinking is the case the prune must not skip.
    ///
    /// The guard asks whether this child *was* the maximum and skips the
    /// re-measure when it was not. Equality is the boundary and it belongs on
    /// the re-measuring side: a child exactly as wide as the maximum is the one
    /// holding it up, and nothing else knows what it falls to.
    ///
    /// The two children are chosen so the guard's halves disagree — one stands
    /// furthest outside the box, the other reaches furthest — because when both
    /// halves sit on their boundary together the second rescues the first and a
    /// wrong comparison in either is invisible.
    #[test]
    fn the_widest_child_shrinking_is_not_pruned_away() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        // At the corner, so what it reaches lands outside the box.
        let overhanging = tree.register(Box::new(MockWidget::new()));
        // In the middle, so it reaches further and stands outside less.
        let far_reaching = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(overhanging, root);
        tree.set_parent(far_reaching, root);

        let cons = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(root, cons, Size::new(100.0, 100.0));
        tree.cache_layout(overhanging, cons, Size::new(20.0, 20.0));
        tree.cache_layout(far_reaching, cons, Size::new(20.0, 20.0));
        tree.set_origin(overhanging, 0.0, 0.0);
        tree.set_origin(far_reaching, 40.0, 40.0);

        tree.set_own_paint_reach(overhanging, 30.0);
        tree.set_own_paint_reach(far_reaching, 40.0);
        assert_eq!(
            tree.children_reach(root),
            40.0,
            "the far-reaching one leads"
        );
        assert_eq!(tree.children_outset(root), 30.0, "the overhanging one does");

        tree.set_own_paint_reach(far_reaching, 5.0);
        assert_eq!(
            tree.children_reach(root),
            30.0,
            "the parent kept the width of a child that has let go of it"
        );
    }

    /// And a child that was never the widest costs nothing to shrink.
    ///
    /// This is the prune earning its place: the return leg of a spring shrinks
    /// on every frame, so a fold over the siblings here would be an O(children)
    /// walk per frame and a staggered list of them O(n squared).
    #[test]
    fn a_child_that_was_not_the_widest_leaves_the_parent_alone() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        let wide = tree.register(Box::new(MockWidget::new()));
        let small = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(wide, root);
        tree.set_parent(small, root);

        let cons = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(root, cons, Size::new(100.0, 100.0));
        tree.cache_layout(wide, cons, Size::new(20.0, 20.0));
        tree.cache_layout(small, cons, Size::new(20.0, 20.0));
        tree.set_origin(wide, 40.0, 40.0);
        tree.set_origin(small, 40.0, 40.0);

        tree.set_own_paint_reach(wide, 50.0);
        tree.set_own_paint_reach(small, 10.0);
        assert_eq!(tree.children_reach(root), 50.0);

        tree.set_own_paint_reach(small, 1.0);
        assert_eq!(
            tree.children_reach(root),
            50.0,
            "the widest child still reaches 50, so the parent must not have moved"
        );
    }

    /// But a child that leads on the *other* axis is not "not the widest".
    ///
    /// The guard has two halves because a parent keeps two maxima, and a child
    /// can hold up either. One that reaches less far than its sibling can still
    /// be the one standing furthest outside the box — it only has to sit nearer
    /// the edge — and pruning it on the reach alone leaves the outset stale.
    #[test]
    fn a_child_that_leads_on_outset_alone_is_not_pruned_away() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        let at_the_edge = tree.register(Box::new(MockWidget::new()));
        let in_the_middle = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(at_the_edge, root);
        tree.set_parent(in_the_middle, root);

        let cons = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(root, cons, Size::new(100.0, 100.0));
        tree.cache_layout(at_the_edge, cons, Size::new(20.0, 20.0));
        tree.cache_layout(in_the_middle, cons, Size::new(20.0, 20.0));
        tree.set_origin(at_the_edge, 0.0, 0.0);
        tree.set_origin(in_the_middle, 40.0, 40.0);

        tree.set_own_paint_reach(in_the_middle, 40.0);
        tree.set_own_paint_reach(at_the_edge, 30.0);
        assert_eq!(
            tree.children_reach(root),
            40.0,
            "the middle one reaches most"
        );
        assert_eq!(
            tree.children_outset(root),
            30.0,
            "the edge one stands out most"
        );

        // Shrinks the one that was never the widest by reach, but was the only
        // thing holding the outset up.
        tree.set_own_paint_reach(at_the_edge, 1.0);
        // Its remaining 1px of reach, and nothing of the 30 it used to hold.
        assert_eq!(
            tree.children_outset(root),
            1.0,
            "the parent kept an overhang no child has any more"
        );
    }

    /// A re-measure that moves the union tells the ancestors too.
    ///
    /// Layout is bottom-up, so an ancestor inside the same pass measures this
    /// for itself and the gather is redundant — except that a widget with a
    /// fixed size is its own relayout boundary and the pass *starts* there,
    /// with no ancestor to follow. Then this is the only thing that carries it.
    ///
    /// The case is a child growing under a parent that does not: the parent's
    /// own reach never moves, so `set_own_paint_reach` returns early and cannot
    /// be what tells anyone.
    #[test]
    fn a_re_measure_that_moves_the_union_reaches_the_ancestors() {
        let mut tree = Tree::new();
        let outer = tree.register(Box::new(MockWidget::new()));
        let boundary = tree.register(Box::new(MockWidget::new()));
        let child = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(boundary, outer);
        tree.set_parent(child, boundary);

        let cons = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(outer, cons, Size::new(100.0, 100.0));
        tree.cache_layout(boundary, cons, Size::new(100.0, 100.0));
        tree.cache_layout(child, cons, Size::new(100.0, 100.0));
        tree.remeasure_children(boundary);
        tree.remeasure_children(outer);
        assert_eq!(tree.children_reach(outer), 0.0, "nothing hangs out yet");

        // The child outgrows the box it sits in. Its own reach is still zero,
        // so nothing publishes; only the re-measure knows.
        tree.cache_layout(child, cons, Size::new(100.0, 400.0));
        tree.remeasure_children(boundary);

        assert_eq!(
            tree.children_reach(outer),
            300.0,
            "the boundary took on 300px of overhang and its parent never heard, \
             so the search that narrows it would drop a box that is on screen"
        );
    }

    /// A reach that shrinks lets go all the way up, not one level.
    ///
    /// The shrink path prunes when the child that shrank was not the widest,
    /// and that guard needs the child's own previous `paint_overflow`. Carrying
    /// the parent's widest *child* instead stops the walk wherever the two
    /// differ — which is any node whose children stand further outside it than
    /// they themselves reach, i.e. every scroller over a longer column. The
    /// middle box here is smaller than what it holds, so its outset and its
    /// widest-child are different numbers and the mistake is visible.
    #[test]
    fn a_reach_that_shrinks_releases_every_ancestor() {
        let mut tree = Tree::new();
        let g = tree.register(Box::new(MockWidget::new()));
        let p = tree.register(Box::new(MockWidget::new()));
        let c = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(p, g);
        tree.set_parent(c, p);

        let cons = Constraints::new(0.0, 0.0, 100.0, 100.0);
        tree.cache_layout(g, cons, Size::new(100.0, 100.0));
        tree.cache_layout(p, cons, Size::new(100.0, 100.0));
        tree.cache_layout(c, cons, Size::new(100.0, 500.0));

        tree.set_own_paint_reach(c, 70.0);
        let held = tree.paint_overflow(g);
        assert!(held > 0.0, "nothing to let go of");

        tree.set_own_paint_reach(c, 0.0);
        assert!(
            tree.paint_overflow(g) < held,
            "the grandparent stayed at {held} after its grandchild's reach went \
             to nothing, so the walk stopped one level short"
        );
    }

    /// And by *its own* reach, not by whatever the surface root happens to
    /// report.
    ///
    /// The test above cannot tell the two apart: its widget is a direct child
    /// of the root, so its reach gathers straight into the root's and both
    /// spellings give the same number. A widget sitting well inside its
    /// ancestors — a card with room around it — is where they differ, and where
    /// the shadow is left on screen as a fringe.
    #[test]
    fn damage_is_grown_by_the_widget_s_own_reach_not_the_root_s() {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        let card = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(card, root);

        let c = Constraints::new(0.0, 0.0, 200.0, 200.0);
        tree.cache_layout(root, c, Size::new(200.0, 200.0));
        tree.cache_layout(card, c, Size::new(40.0, 20.0));
        tree.set_origin(card, 80.0, 80.0);
        for id in [root, card] {
            tree.clear_needs_paint(id);
        }
        let _ = tree.take_damage(root);

        // Inset far enough that the shadow stays inside the root, so nothing of
        // this reach reaches the root's own numbers.
        tree.set_own_paint_reach(card, 20.0);
        assert_eq!(
            tree.paint_overflow(root),
            0.0,
            "the root should not have taken this on, which is what makes the \
             distinction visible"
        );

        tree.mark_needs_paint(card);
        let rect = partial_damage(&mut tree, root);
        assert!(
            rect.x <= 60.0 && rect.y <= 60.0 && rect.width >= 80.0 && rect.height >= 60.0,
            "the card's shadow is outside the damage rect, so it is left on \
             screen as a fringe: got {rect:?}"
        );
    }

    #[test]
    fn a_widget_with_no_overflow_damages_exactly_its_bounds() {
        let (mut tree, root, a, _b) = two_children_tree();

        tree.mark_needs_paint(a);

        let rect = partial_damage(&mut tree, root);
        assert_eq!((rect.x, rect.y), (0.0, 0.0));
        assert_eq!((rect.width, rect.height), (40.0, 20.0));
    }

    /// A 50x50 child inset in a 200x200 root, with its damage taken and its
    /// paint flags cleared — the shape both questions below are asked of.
    fn inset_child() -> (Tree, WidgetId, WidgetId) {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(MockWidget::new()));
        let child = tree.register(Box::new(MockWidget::new()));
        tree.set_parent(child, root);

        let c = Constraints::new(0.0, 0.0, 200.0, 200.0);
        tree.cache_layout(root, c, Size::new(200.0, 200.0));
        tree.cache_layout(child, c, Size::new(50.0, 50.0));
        tree.set_origin(child, 10.0, 10.0);
        for id in [root, child] {
            tree.clear_needs_paint(id);
        }
        let _ = tree.take_damage(root);
        (tree, root, child)
    }

    /// A transform carries a widget away from its box and then back, and the
    /// frame it comes back on has to name the pixels it is leaving.
    ///
    /// The outbound frame damages where the widget went, because the reach is
    /// refreshed before the rect is built and the rect is built from the reach.
    /// The return frame is refreshed the same way and reports the widget's own
    /// 50x50 box — so the pixels at its old position are never handed to
    /// `wl_surface.damage_buffer` and the compositor leaves the widget there.
    #[test]
    fn damage_covers_where_a_transform_came_back_from() {
        let (mut tree, root, child) = inset_child();

        // Out: a translate of 100 reaches 100 past every edge.
        tree.set_own_paint_reach(child, 100.0);
        tree.mark_needs_paint(child);
        let went = partial_damage(&mut tree, root);
        assert_eq!(
            Rect::new(10.0, 110.0, 50.0, 50.0).outset_beyond(went),
            0.0,
            "the outbound frame does not even cover where it went: {went:?}"
        );

        tree.clear_needs_paint(child);
        tree.clear_needs_paint(root);

        // Back to rest, on the frame after.
        tree.set_own_paint_reach(child, 0.0);
        tree.mark_needs_paint(child);
        let came_back = partial_damage(&mut tree, root);
        assert_eq!(
            went.outset_beyond(came_back),
            0.0,
            "the return frame damages {came_back:?}, which leaves the pixels \
             the widget occupied at {went:?} on screen as a fringe"
        );
    }

    /// The same defect without a transform anywhere near it.
    ///
    /// An elevation falling to nothing shrinks the same reach by the same
    /// route, and predates transforms contributing to it at all. Here so the
    /// fix is not made transform-shaped when the defect is not.
    #[test]
    fn damage_covers_the_shadow_an_elevation_drop_leaves_behind() {
        let (mut tree, root, child) = inset_child();

        tree.set_own_paint_reach(child, 8.0);
        tree.mark_needs_paint(child);
        let with_shadow = partial_damage(&mut tree, root);

        tree.clear_needs_paint(child);
        tree.clear_needs_paint(root);

        tree.set_own_paint_reach(child, 0.0);
        tree.mark_needs_paint(child);
        let without = partial_damage(&mut tree, root);
        assert_eq!(
            with_shadow.outset_beyond(without),
            0.0,
            "the frame that drops the shadow damages {without:?}, so the ring \
             it cast at {with_shadow:?} stays on screen"
        );
    }
}
