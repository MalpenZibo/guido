use std::collections::HashMap;
use std::sync::Arc;

use super::Widget;

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
        let has_dynamic = self.slots.iter().any(|slot| matches!(slot, ChildSlot::Dynamic { .. }));

        if self.needs_reconcile || has_dynamic {
            self.reconcile();
        }
        &mut self.merged
    }

    /// Get immutable access to the children vec
    pub fn get(&self) -> &Vec<Box<dyn Widget>> {
        &self.merged
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.merged.is_empty()
    }

    /// Get the number of children
    pub fn len(&self) -> usize {
        self.merged.len()
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
                ChildSlot::Dynamic { items_fn, cached, order } => {
                    // Get new items from the function
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
                                // Create new widget
                                self.merged.push(item.widget);
                            }
                        }

                        // Update order
                        *order = new_keys;

                        // Clear remaining cache
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


/// Wrapper for dynamic items with key + widget
pub struct DynItem {
    pub key: u64,
    pub widget: Box<dyn Widget>,
}

impl DynItem {
    /// Create a new dynamic item
    pub fn new(key: u64, widget: impl Widget + 'static) -> Self {
        Self {
            key,
            widget: Box::new(widget),
        }
    }
}
