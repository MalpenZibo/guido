use std::collections::HashMap;
use std::sync::Arc;

use super::Widget;

/// Represents the source of children for a container
pub enum ChildrenSource {
    /// Static children built with .child() calls
    Static(Vec<Box<dyn Widget>>),

    /// Dynamic children with keyed reconciliation (Floem-style)
    Dynamic(DynamicChildren),
}

impl ChildrenSource {
    /// Get mutable access to the children vec, reconciling if dynamic
    pub fn reconcile_and_get_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        match self {
            ChildrenSource::Static(children) => children,
            ChildrenSource::Dynamic(dynamic) => {
                dynamic.reconcile();
                &mut dynamic.widgets
            }
        }
    }

    /// Get immutable access to the children vec
    pub fn get(&self) -> &Vec<Box<dyn Widget>> {
        match self {
            ChildrenSource::Static(children) => children,
            ChildrenSource::Dynamic(dynamic) => &dynamic.widgets,
        }
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.get().is_empty()
    }

    /// Get the number of children
    pub fn len(&self) -> usize {
        self.get().len()
    }
}

impl Default for ChildrenSource {
    fn default() -> Self {
        ChildrenSource::Static(Vec::new())
    }
}

/// Dynamic children with keyed reconciliation
pub struct DynamicChildren {
    /// Function returning current items (widgets with keys)
    items_fn: Arc<dyn Fn() -> Vec<DynItem> + Send + Sync>,

    /// Current widgets in display order
    widgets: Vec<Box<dyn Widget>>,

    /// Cached widgets keyed by their ID (for reuse during reconciliation)
    cached: HashMap<u64, Box<dyn Widget>>,

    /// Current order of keys
    order: Vec<u64>,
}

impl DynamicChildren {
    /// Create a new DynamicChildren with the given items function
    pub fn new<F>(items_fn: F) -> Self
    where
        F: Fn() -> Vec<DynItem> + Send + Sync + 'static,
    {
        Self {
            items_fn: Arc::new(items_fn),
            widgets: Vec::new(),
            cached: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Reconcile the children: compare new items with cached widgets and update
    pub fn reconcile(&mut self) {
        // Get new items from the function
        let new_items = (self.items_fn)();

        // Extract new keys
        let new_keys: Vec<u64> = new_items.iter().map(|item| item.key).collect();

        // If keys haven't changed, skip reconciliation
        if new_keys == self.order {
            return;
        }

        // Move all current widgets to cache with their keys
        for (i, widget) in self.widgets.drain(..).enumerate() {
            if let Some(&key) = self.order.get(i) {
                self.cached.insert(key, widget);
            }
        }

        // Build new widgets list by reusing or creating
        for item in new_items {
            if let Some(widget) = self.cached.remove(&item.key) {
                // Reuse existing widget (preserves state!)
                self.widgets.push(widget);
            } else {
                // Create new widget
                self.widgets.push(item.widget);
            }
        }

        // Update order
        self.order = new_keys;

        // Clear any remaining cached widgets (they're no longer needed)
        self.cached.clear();
    }
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
