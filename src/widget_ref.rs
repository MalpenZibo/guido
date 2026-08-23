//! WidgetRef — a handle from application code to one widget in the tree.
//!
//! Attach it with `.widget_ref(r)`, then read the widget's surface-relative
//! bounds as a signal, or move the keyboard focus to it.
//!
//! # Why focus goes through a handle
//!
//! Focus lives in the tree: [`request_focus`](crate::reactive::focus::request_focus)
//! resolves the focused widget's ancestors once, when focus moves, because that
//! is the only moment the tree is at hand. Application code has neither the tree
//! nor a `WidgetId` — ids are minted when the tree is built, not when the view is
//! composed — so it needs something to name a widget *with*, and the request has
//! to wait for the next frame to be applied.
//!
//! Every toolkit lands in the same place. Compose has `FocusRequester` and makes
//! you call it from an effect; iced hands you a widget `Id` and returns a `Task`
//! for the runtime to run; Flutter gives you a `FocusNode` and tells you to
//! request from a post-frame callback, because during `initState` the widget is
//! not mounted yet. Guido already had the handle — this gives it the verb.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::reactive::focus::{focus_path, release_focus, request_focus_deferred};
use crate::reactive::{RwSignal, Signal, create_signal};
use crate::tree::{Tree, WidgetId};
use crate::widgets::Rect;

/// A handle to one widget: its bounds, and its focus.
///
/// Created via [`create_widget_ref()`]. Attach to a container or a text input
/// with `.widget_ref(r)`.
#[derive(Clone, Copy)]
pub struct WidgetRef {
    signal: RwSignal<Rect>,
    /// Which widget this ref is attached to, filled in when that widget is laid
    /// out and cleared when it leaves the tree. `None` before the first layout,
    /// which is why [`focus`](Self::focus) has to survive being called early.
    widget: RwSignal<Option<WidgetId>>,
}

impl WidgetRef {
    /// The reactive signal holding this widget's surface-relative bounds (read-only).
    pub fn rect(&self) -> Signal<Rect> {
        self.signal.read_only()
    }

    /// The widget this ref is attached to, once it has been laid out.
    pub fn widget(&self) -> Option<WidgetId> {
        self.widget.get_untracked()
    }

    /// Move the keyboard focus to this widget.
    ///
    /// Applied on the next frame, not inside this call: focus is resolved against
    /// the tree, and the caller does not have one. Calling it before the widget
    /// has ever been laid out is fine — the request waits for the widget to exist
    /// rather than being dropped, so `focus()` from a startup effect works.
    ///
    /// A later request replaces an earlier one that has not been applied yet: the
    /// last thing asked for is what the user asked for.
    pub fn focus(&self) {
        request_focus_deferred(*self);
    }

    /// Take the keyboard focus off this widget, if it has it.
    pub fn blur(&self) {
        if let Some(id) = self.widget() {
            release_focus(id);
        }
    }

    /// Whether this widget holds the keyboard focus.
    ///
    /// Reads the focus signal, so calling it inside a reactive scope subscribes
    /// to focus changes like any other signal read.
    pub fn is_focused(&self) -> bool {
        match self.widget() {
            Some(id) => focus_path().widget() == Some(id),
            None => false,
        }
    }
}

/// Create a new `WidgetRef` initialized with `Rect::default()` (all zeros).
pub fn create_widget_ref() -> WidgetRef {
    WidgetRef {
        signal: create_signal(Rect::default()),
        widget: create_signal(None),
    }
}

// ---------------------------------------------------------------------------
// Thread-local registry: WidgetId → WidgetRef
// ---------------------------------------------------------------------------

thread_local! {
    static WIDGET_REF_REGISTRY: RefCell<HashMap<WidgetId, WidgetRef>> =
        RefCell::new(HashMap::new());
}

/// Register (or re-register) a widget ref mapping.
///
/// Called from the layout of every widget that carries a `WidgetRef`. Idempotent
/// — HashMap insert overwrites.
pub(crate) fn register_widget_ref(id: WidgetId, widget_ref: WidgetRef) {
    // Cheap when unchanged, and the write is what lets `focus()` resolve: a ref
    // that has never been laid out has no widget to focus.
    if widget_ref.widget.get_untracked() != Some(id) {
        widget_ref.widget.set(Some(id));
    }
    WIDGET_REF_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(id, widget_ref);
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
        reg.borrow_mut().retain(|&id, widget_ref| {
            // A ref's signals belong to the scope that created it — a popup's
            // widget tree, a dynamic child — and this registry outlives every
            // one of them. Asking whether the handle is still alive is how an
            // entry learns to go; reading it outright is how the frame after a
            // popup closed used to panic.
            let Some(attached) = widget_ref.widget.try_get_untracked() else {
                return false;
            };
            if let Some(rect) = tree.get_surface_relative_bounds(id) {
                widget_ref.signal.set(rect);
                true
            } else {
                // Widget removed from tree — drop registry entry, and stop
                // claiming the handle points at something.
                if attached == Some(id) {
                    widget_ref.widget.set(None);
                }
                false
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{create_root_owner, dispose_owner_now, with_owner};
    use crate::widgets::container;

    /// A widget ref is created wherever its widget is composed, and a popup's
    /// content is composed inside the popup's own scope. The registry is
    /// process-wide and holds a `Copy` of the handle, so it keeps looking at
    /// refs whose owner is gone — and the frame right after a popup closes is
    /// exactly when it looks.
    #[test]
    fn a_ref_whose_scope_died_leaves_the_registry_instead_of_panicking() {
        create_root_owner();
        let mut popup_tree = Tree::new();
        let id = popup_tree.register(Box::new(container()));

        let ((), popup_scope) = with_owner(|| {
            register_widget_ref(id, create_widget_ref());
        });
        dispose_owner_now(popup_scope);

        // The popup's tree is gone: what the loop lays out next knows nothing
        // about that widget.
        update_widget_refs(&Tree::new());

        assert!(
            WIDGET_REF_REGISTRY.with(|reg| reg.borrow().is_empty()),
            "the dead ref is evicted, not carried into the next frame"
        );
    }
}
