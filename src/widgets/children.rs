use std::collections::HashMap;
use std::sync::Arc;

use crate::layout::{Constraints, Size};
use crate::reactive::{ChangeFlags, OwnerId, WidgetId, dispose_owner};
use crate::renderer::PaintContext;

use super::Widget;
use super::widget::{Event, EventResponse, Rect};

/// A slot in the children list - either a static widget or a dynamic source
enum ChildSlot {
    /// Static widget (pending to be added to merged list)
    StaticPending(Box<dyn Widget>),

    /// Static widget (already in merged list at the given index)
    Static,

    /// Dynamic children source with keyed reconciliation
    Dynamic {
        items_fn: Arc<dyn Fn() -> Vec<DynItem> + Send + Sync>,
        cached: HashMap<u64, Box<dyn Widget>>,
        order: Vec<u64>,
    },
}

/// Represents the source of children for a container
///
/// Uses a slot-based architecture where each `.child()` call adds a slot.
/// Slots can be either static (one widget) or dynamic (0+ widgets from a closure).
#[derive(Default)]
pub struct ChildrenSource {
    /// Slots in the order they were added
    slots: Vec<ChildSlot>,

    /// Merged widgets from all slots (rebuilt during reconciliation)
    merged: Vec<Box<dyn Widget>>,

    /// Whether reconciliation is needed
    needs_reconcile: bool,
}

impl ChildrenSource {
    /// Add a static child widget
    pub fn add_static(&mut self, widget: Box<dyn Widget>) {
        self.slots.push(ChildSlot::StaticPending(widget));
        self.needs_reconcile = true;
    }

    /// Add a dynamic children source
    pub fn add_dynamic(&mut self, items_fn: impl Fn() -> Vec<DynItem> + Send + Sync + 'static) {
        self.slots.push(ChildSlot::Dynamic {
            items_fn: Arc::new(items_fn),
            cached: HashMap::new(),
            order: Vec::new(),
        });
        self.needs_reconcile = true;
    }

    /// Get mutable access to the children vec, reconciling if needed
    pub fn reconcile_and_get_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        // Always reconcile if we have dynamic slots (they need to be re-evaluated each frame)
        let has_dynamic = self
            .slots
            .iter()
            .any(|slot| matches!(slot, ChildSlot::Dynamic { .. }));

        if self.needs_reconcile || has_dynamic {
            self.reconcile();
        }
        &mut self.merged
    }

    /// Get immutable access to the children vec
    pub fn get(&self) -> &Vec<Box<dyn Widget>> {
        &self.merged
    }

    /// Get mutable access to the children vec without reconciliation
    pub fn get_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
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

    /// Check if reconciliation is needed (children were added but not yet merged)
    pub fn needs_reconcile(&self) -> bool {
        self.needs_reconcile
    }

    /// Reconcile all slots and rebuild the merged children list
    fn reconcile(&mut self) {
        // Take the current merged list and convert to iterator
        let old_merged = std::mem::take(&mut self.merged);
        let mut old_iter = old_merged.into_iter();

        // Process each slot
        for slot in &mut self.slots {
            match slot {
                ChildSlot::StaticPending(widget) => {
                    // Move the widget from the slot to merged
                    let w = std::mem::replace(widget, Box::new(DummyWidget));
                    self.merged.push(w);
                    // Mark slot as no longer pending
                    *slot = ChildSlot::Static;
                }
                ChildSlot::Static => {
                    // Widget is already in old_merged - take next one
                    if let Some(widget) = old_iter.next() {
                        self.merged.push(widget);
                    }
                }
                ChildSlot::Dynamic {
                    items_fn,
                    cached,
                    order,
                } => {
                    // Get new items from the function (closures, not constructed widgets!)
                    let new_items = items_fn();
                    let new_keys: Vec<u64> = new_items.iter().map(|item| item.key).collect();

                    // Check if keys changed
                    if new_keys != *order {
                        // Save current widgets from old_iter to cache
                        for (i, widget) in old_iter.by_ref().take(order.len()).enumerate() {
                            if let Some(&key) = order.get(i) {
                                cached.insert(key, widget);
                            }
                        }

                        // Build new widgets list by reusing or creating
                        for item in new_items {
                            if let Some(widget) = cached.remove(&item.key) {
                                // Reuse existing widget (preserves state!)
                                self.merged.push(widget);
                            } else {
                                // Create new widget - only now do we call the closure!
                                self.merged.push((item.widget_fn)());
                            }
                        }

                        // Update order
                        *order = new_keys;

                        // Clear remaining cache (drops removed widgets, triggering cleanup)
                        cached.clear();
                    } else {
                        // Keys unchanged - reuse widgets from old_iter
                        for widget in old_iter.by_ref().take(order.len()) {
                            self.merged.push(widget);
                        }
                    }
                }
            }
        }

        self.needs_reconcile = false;
    }
}

/// Dummy widget used as placeholder during slot state transitions
struct DummyWidget;

impl super::Widget for DummyWidget {
    fn advance_animations(&mut self) -> bool {
        false
    }

    fn layout(&mut self, _constraints: crate::layout::Constraints) -> crate::layout::Size {
        crate::layout::Size::zero()
    }

    fn paint(&self, _ctx: &mut crate::renderer::PaintContext) {}

    fn event(&mut self, _event: &super::widget::Event) -> super::widget::EventResponse {
        super::widget::EventResponse::Ignored
    }

    fn set_origin(&mut self, _x: f32, _y: f32) {}

    fn bounds(&self) -> super::widget::Rect {
        super::widget::Rect::new(0.0, 0.0, 0.0, 0.0)
    }

    fn id(&self) -> crate::reactive::WidgetId {
        crate::reactive::WidgetId::next()
    }

    fn mark_dirty(&mut self, _flags: crate::reactive::ChangeFlags) {}

    fn needs_layout(&self) -> bool {
        false
    }

    fn needs_paint(&self) -> bool {
        false
    }

    fn clear_dirty(&mut self) {}
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

    fn mark_dirty(&mut self, flags: ChangeFlags) {
        self.inner.mark_dirty(flags)
    }

    fn mark_dirty_recursive(&mut self, flags: ChangeFlags) {
        self.inner.mark_dirty_recursive(flags)
    }

    fn needs_layout(&self) -> bool {
        self.inner.needs_layout()
    }

    fn needs_paint(&self) -> bool {
        self.inner.needs_paint()
    }

    fn clear_dirty(&mut self) {
        self.inner.clear_dirty()
    }

    fn has_focus_descendant(&self, id: WidgetId) -> bool {
        self.inner.has_focus_descendant(id)
    }

    fn is_relayout_boundary(&self) -> bool {
        self.inner.is_relayout_boundary()
    }

    fn mark_needs_layout(&mut self) {
        self.inner.mark_needs_layout()
    }

    fn mark_needs_paint(&mut self) {
        self.inner.mark_needs_paint()
    }
}
