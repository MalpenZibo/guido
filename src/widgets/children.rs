use std::collections::HashMap;
use std::sync::Arc;

use crate::layout::{Constraints, Size};
use crate::reactive::{
    JobType, OwnerId, WidgetId, dispose_owner, register_widget, unregister_widget,
    with_signal_tracking,
};
use crate::renderer::PaintContext;

use super::Widget;
use super::widget::{Event, EventResponse, Rect};

/// Segment metadata - tracks what kind of source each segment is
enum SegmentType {
    /// Static widgets - just a count (widget IDs stored in merged)
    Static(usize),
    /// Dynamic source with keyed reconciliation
    Dynamic {
        items_fn: Arc<dyn Fn() -> Vec<DynItem> + Send + Sync>,
        /// Cached widget IDs by key (for reuse during reconciliation)
        cached: HashMap<u64, WidgetId>,
        /// Current keys in display order
        current_keys: Vec<u64>,
    },
}

/// Represents the source of children for a container
///
/// Uses a segment-based architecture:
/// - Static segments: widgets added directly, IDs stored in `merged`
/// - Dynamic segments: widgets from reactive closures with keyed reconciliation
///
/// Widget IDs are stored in the `merged` vec, actual widgets are stored in the
/// global LayoutArena. Segment metadata tracks boundaries and reconciliation state.
#[derive(Default)]
pub struct ChildrenSource {
    /// All widget IDs in order (static and dynamic interleaved)
    merged: Vec<WidgetId>,
    /// Segment metadata (boundaries and reconciliation info)
    segments: Vec<SegmentType>,
    /// Parent container ID for subscriber registration
    container_id: Option<WidgetId>,
    /// Whether initial reconciliation has been done
    initial_reconcile_done: bool,
}

impl ChildrenSource {
    /// Set the container ID for subscriber registration
    pub fn set_container_id(&mut self, id: WidgetId) {
        self.container_id = Some(id);
    }

    /// Add a static child widget
    pub fn add_static(&mut self, widget: Box<dyn Widget>) {
        let widget_id = widget.id();
        // Register widget in the global arena
        register_widget(widget_id, widget);
        self.merged.push(widget_id);

        // Track in segment metadata
        if let Some(SegmentType::Static(count)) = self.segments.last_mut() {
            *count += 1;
        } else {
            self.segments.push(SegmentType::Static(1));
        }
    }

    /// Add a dynamic children source
    pub fn add_dynamic(&mut self, items_fn: impl Fn() -> Vec<DynItem> + Send + Sync + 'static) {
        self.segments.push(SegmentType::Dynamic {
            items_fn: Arc::new(items_fn),
            cached: HashMap::new(),
            current_keys: Vec::new(),
        });
    }

    /// Check if any dynamic segments exist
    pub fn has_dynamic(&self) -> bool {
        self.segments
            .iter()
            .any(|s| matches!(s, SegmentType::Dynamic { .. }))
    }

    /// Reconcile all dynamic segments and rebuild merged list
    fn reconcile(&mut self) {
        // First pass: check if any dynamic segment needs reconciliation
        let mut segments_with_changes: Vec<(usize, Vec<DynItem>)> = Vec::new();

        for (idx, segment) in self.segments.iter().enumerate() {
            if let SegmentType::Dynamic {
                items_fn,
                current_keys,
                ..
            } = segment
            {
                let new_items = items_fn();
                let new_keys: Vec<u64> = new_items.iter().map(|i| i.key).collect();

                if new_keys != *current_keys {
                    segments_with_changes.push((idx, new_items));
                }
            }
        }

        // If nothing changed, skip the entire rebuild
        if segments_with_changes.is_empty() {
            return;
        }

        // Take ownership of old merged vec to avoid borrow conflicts
        let old_merged = std::mem::take(&mut self.merged);
        let mut old_merged_iter = old_merged.into_iter();

        // Build new merged vec by walking through segments
        let mut new_merged = Vec::with_capacity(old_merged_iter.len());
        let mut change_idx = 0;

        for (idx, segment) in self.segments.iter_mut().enumerate() {
            match segment {
                SegmentType::Static(count) => {
                    // Static widgets: take IDs from old merged
                    for _ in 0..*count {
                        if let Some(widget_id) = old_merged_iter.next() {
                            new_merged.push(widget_id);
                        }
                    }
                }
                SegmentType::Dynamic {
                    cached,
                    current_keys,
                    ..
                } => {
                    // Check if this segment has changes
                    let has_changes = change_idx < segments_with_changes.len()
                        && segments_with_changes[change_idx].0 == idx;

                    if has_changes {
                        // Keys changed - reconcile using pre-computed items
                        let (_, new_items) = std::mem::take(&mut segments_with_changes[change_idx]);
                        change_idx += 1;

                        let new_keys: Vec<u64> = new_items.iter().map(|i| i.key).collect();

                        // Move current widget IDs to cache
                        for key in current_keys.drain(..) {
                            if let Some(widget_id) = old_merged_iter.next() {
                                cached.insert(key, widget_id);
                            }
                        }

                        // Build new widgets list by reusing or creating
                        for item in new_items {
                            if let Some(widget_id) = cached.remove(&item.key) {
                                // Reuse existing widget (preserves state!)
                                new_merged.push(widget_id);
                            } else {
                                // Create new widget and register in arena
                                let widget = (item.widget_fn)();
                                let widget_id = widget.id();
                                register_widget(widget_id, widget);
                                new_merged.push(widget_id);
                            }
                        }

                        // Update current keys
                        *current_keys = new_keys;

                        // Unregister removed widgets from arena (triggers Drop/cleanup)
                        for old_id in cached.values() {
                            unregister_widget(*old_id);
                        }
                        cached.clear();
                    } else {
                        // Keys unchanged - just move widget IDs from old merged to new merged
                        for _ in 0..current_keys.len() {
                            if let Some(widget_id) = old_merged_iter.next() {
                                new_merged.push(widget_id);
                            }
                        }
                    }
                }
            }
        }

        self.merged = new_merged;
    }

    /// Reconcile with signal tracking. Called from main loop job processing.
    /// Returns true if children changed.
    pub fn reconcile_with_tracking(&mut self) -> bool {
        if !self.has_dynamic() {
            return false;
        }

        let container_id = self
            .container_id
            .expect("container_id must be set before reconcile_with_tracking");

        let prev_count = self.merged.len();

        // Track signal reads during reconciliation
        // This registers container as subscriber for any signals read
        with_signal_tracking(container_id, JobType::Reconcile, || {
            self.reconcile();
        });
        self.initial_reconcile_done = true;

        // Return true if children count changed
        prev_count != self.merged.len()
    }

    /// Reconcile and get widget IDs (for layout)
    /// Does lazy initial reconciliation with tracking if needed.
    pub fn reconcile_and_get(&mut self) -> &Vec<WidgetId> {
        // Lazy initial reconciliation (for first frame before any jobs exist)
        if self.has_dynamic() && !self.initial_reconcile_done {
            if let Some(container_id) = self.container_id {
                with_signal_tracking(container_id, JobType::Reconcile, || {
                    self.reconcile();
                });
                self.initial_reconcile_done = true;
            } else {
                // Fallback: reconcile without tracking (shouldn't happen in practice)
                self.reconcile();
            }
        } else if self.has_dynamic() {
            // Subsequent reconciliations (triggered by jobs) already done by process_pending_jobs
            // Just return the already-reconciled list
        }
        &self.merged
    }

    /// Get widget IDs (for paint and events)
    /// After first frame, this just returns the already-reconciled children.
    pub fn get(&self) -> &Vec<WidgetId> {
        &self.merged
    }

    /// Get mutable reference to widget IDs
    pub fn get_mut(&mut self) -> &mut Vec<WidgetId> {
        &mut self.merged
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.merged.is_empty()
    }

    /// Get the number of children
    pub fn len(&self) -> usize {
        self.merged.len()
    }
}

impl Drop for ChildrenSource {
    fn drop(&mut self) {
        // Unregister all widgets from the arena
        for widget_id in self.merged.drain(..) {
            unregister_widget(widget_id);
        }
        // Also unregister any widgets still in dynamic caches
        for segment in &mut self.segments {
            if let SegmentType::Dynamic { cached, .. } = segment {
                for widget_id in cached.values() {
                    unregister_widget(*widget_id);
                }
                cached.clear();
            }
        }
    }
}

/// Wrapper for dynamic items with key + widget factory.
///
/// `DynItem` represents a single item in a dynamic children list. Each item has:
/// - A unique `key` used for reconciliation (matching old/new items)
/// - A `widget_fn` factory that creates the widget lazily
///
/// # Ownership and Automatic Cleanup
///
/// When using the `.children()` method with a closure returning keyed items,
/// the `IntoChildren` trait implementation automatically wraps each item's
/// widget factory with an ownership scope. This means:
///
/// 1. **Signals created inside the factory are automatically cleaned up** when the
///    item is removed from the dynamic children list.
/// 2. **Effects created inside the factory are automatically disposed** when removed.
/// 3. **Custom cleanup callbacks** can be registered with [`on_cleanup`](crate::reactive::on_cleanup)
///    to clean up other resources (timers, connections, etc.).
///
/// Note: `DynItem::new` itself does NOT create the ownership scope. The wrapping
/// happens in the `IntoChildren<DynamicChildren>` trait implementation which calls
/// `with_owner()` and wraps the widget in `OwnedWidget`.
///
/// # Performance Characteristics
///
/// - Widget factories are only called for NEW keys (not seen before)
/// - Existing widgets are reused when their key persists across updates
/// - Cleanup runs synchronously when items are removed during reconciliation
/// - The reverse mapping ensures O(1) lookup for effect ownership checks
///
/// # Example
///
/// ```ignore
/// use guido::{create_signal, create_effect, on_cleanup};
///
/// // Using .children() with dynamic keyed items - ownership is automatic
/// container().children(move || {
///     data.get().iter().map(|item| {
///         (item.id, move || {
///             // Signals created here are automatically cleaned up
///             let local_state = create_signal(0);
///
///             // Effects are automatically disposed
///             create_effect(move || {
///                 println!("Item {} state: {}", item.id, local_state.get());
///             });
///
///             // Register custom cleanup for non-reactive resources
///             on_cleanup(|| {
///                 println!("Item {} was removed!", item.id);
///             });
///
///             text(move || format!("Item: {}", item.name))
///         })
///     })
/// });
/// ```
pub struct DynItem {
    pub key: u64,
    /// Factory function to create the widget. Only called for NEW keys.
    pub widget_fn: Box<dyn FnOnce() -> Box<dyn Widget>>,
}

impl DynItem {
    /// Create a new dynamic item with a widget factory closure.
    pub fn new<W: Widget + 'static>(key: u64, widget_fn: impl FnOnce() -> W + 'static) -> Self {
        Self {
            key,
            widget_fn: Box::new(move || Box::new(widget_fn())),
        }
    }
}

/// Widget wrapper that owns a reactive scope.
///
/// `OwnedWidget` wraps another widget and associates it with a reactive owner.
/// When the `OwnedWidget` is dropped (e.g., when removed from a dynamic children list),
/// it automatically disposes the owner and all resources created within that scope:
///
/// - **Signals** are disposed and subsequent access will panic
/// - **Effects** are stopped and will no longer run
/// - **Cleanup callbacks** registered via [`on_cleanup`](crate::reactive::on_cleanup)
///   are executed in reverse order (LIFO)
///
/// # When Ownership Wrapping Happens
///
/// Ownership wrapping is automatic when using the `.children()` method with dynamic
/// keyed items. The `IntoChildren<DynamicChildren>` trait implementation creates
/// `OwnedWidget` instances for each new item. You typically don't need to create
/// `OwnedWidget` directly.
///
/// # Performance Characteristics
///
/// - Disposal is synchronous and happens during reconciliation
/// - Child owners are disposed before parent owners (depth-first)
/// - The cleanup cost is O(S + E + C) where S = signals, E = effects, C = cleanup callbacks
///
/// # Example
///
/// ```ignore
/// use guido::{OwnedWidget, with_owner, create_signal, text};
///
/// // Manual ownership wrapping (usually not needed - use .children() instead)
/// let (widget, owner_id) = with_owner(|| {
///     let signal = create_signal(42);
///     text(move || format!("Value: {}", signal.get()))
/// });
/// let owned = OwnedWidget::new(Box::new(widget), owner_id);
///
/// // When `owned` is dropped, the signal is disposed
/// ```
pub struct OwnedWidget {
    inner: Box<dyn Widget>,
    owner_id: OwnerId,
}

impl OwnedWidget {
    /// Create a new owned widget with the given inner widget and owner ID.
    pub fn new(inner: Box<dyn Widget>, owner_id: OwnerId) -> Self {
        Self { inner, owner_id }
    }
}

impl Drop for OwnedWidget {
    fn drop(&mut self) {
        dispose_owner(self.owner_id);
    }
}

impl Widget for OwnedWidget {
    fn advance_animations(&mut self) -> bool {
        self.inner.advance_animations()
    }

    fn reconcile_children(&mut self) -> bool {
        self.inner.reconcile_children()
    }

    fn layout(&mut self, constraints: Constraints) -> Size {
        self.inner.layout(constraints)
    }

    fn paint(&self, ctx: &mut PaintContext) {
        self.inner.paint(ctx)
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        self.inner.event(event)
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.inner.set_origin(x, y)
    }

    fn bounds(&self) -> Rect {
        self.inner.bounds()
    }

    fn id(&self) -> WidgetId {
        self.inner.id()
    }

    fn has_focus_descendant(&self, id: WidgetId) -> bool {
        self.inner.has_focus_descendant(id)
    }

    fn is_relayout_boundary(&self) -> bool {
        self.inner.is_relayout_boundary()
    }
}
