//! WidgetRef — reactive access to a widget's surface-relative bounds.
//!
//! Attach a `WidgetRef` to a `Container` via `.widget_ref(r)` to track its
//! bounding rect after layout. The rect is exposed as a `Signal<Rect>` that
//! updates automatically each frame.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::reactive::{RwSignal, Signal, create_signal};
use crate::tree::{Tree, WidgetId};
use crate::widgets::Rect;

/// A handle to a widget's surface-relative bounding rect.
///
/// Created via [`create_widget_ref()`]. Attach to a container with
/// `.widget_ref(r)` and read bounds reactively via `.rect().get()`.
#[derive(Clone, Copy)]
pub struct WidgetRef {
    signal: RwSignal<Rect>,
}

impl WidgetRef {
    /// The reactive signal holding this widget's surface-relative bounds (read-only).
    pub fn rect(&self) -> Signal<Rect> {
        self.signal.read_only()
    }

    /// Internal: get the read-write signal for updating bounds after layout.
    pub(crate) fn rw_signal(&self) -> RwSignal<Rect> {
        self.signal
    }
}

/// Create a new `WidgetRef` initialized with `Rect::default()` (all zeros).
pub fn create_widget_ref() -> WidgetRef {
    WidgetRef {
        signal: create_signal(Rect::default()),
    }
}

// ---------------------------------------------------------------------------
// Thread-local registry: WidgetId → RwSignal<Rect>
// ---------------------------------------------------------------------------

thread_local! {
    static WIDGET_REF_REGISTRY: RefCell<HashMap<WidgetId, RwSignal<Rect>>> =
        RefCell::new(HashMap::new());
}

/// Register (or re-register) a widget ref mapping.
///
/// Called from `Container::layout` each time a container with a `WidgetRef`
/// is laid out. Idempotent — HashMap insert overwrites.
pub(crate) fn register_widget_ref(id: WidgetId, signal: RwSignal<Rect>) {
    WIDGET_REF_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(id, signal);
    });
}

/// Reset the widget ref registry.
///
/// Called during `App::drop()` to clear stale widget ref entries.
pub(crate) fn reset_widget_refs() {
    WIDGET_REF_REGISTRY.with(|r| r.borrow_mut().clear());
}

/// Update all registered widget ref signals with current bounds from `tree`.
///
/// Entries whose widget no longer exists in the tree are removed (GC).
/// Called once per surface after layout completes.
pub(crate) fn update_widget_refs(tree: &Tree) {
    WIDGET_REF_REGISTRY.with(|reg| {
        reg.borrow_mut().retain(|&id, signal| {
            if let Some(rect) = tree.get_surface_relative_bounds(id) {
                signal.set(rect);
                true
            } else {
                // Widget removed from tree — drop registry entry
                false
            }
        });
    });
}
