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
/// What a [`WidgetRef`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Attachment {
    /// Laid out, and this is the widget.
    To(WidgetId),
    /// Alive, but not laid out yet — or no longer.
    Unattached,
    /// The scope that created the ref is gone.
    Gone,
}

/// Two refs are the same ref when they name the same pair of signals — which
/// is what a `Copy` of one is.
#[derive(Clone, Copy, PartialEq, Eq)]
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
    ///
    /// `None` before that first layout — and `None` again once the scope that
    /// created the ref is gone, which is the same answer to the same question:
    /// this handle does not point at a live widget. A `WidgetRef` is `Copy` and
    /// can outlive the tree it named, so asking has to be safe whenever anyone
    /// still holds one.
    pub fn widget(&self) -> Option<WidgetId> {
        self.widget.try_get_untracked().flatten()
    }

    /// What this ref points at, with the two cases [`widget`](Self::widget)
    /// folds together kept apart.
    ///
    /// One read of the handle, for the callers that must tell "waiting for a
    /// widget that has not been laid out yet" from "the scope that made this
    /// ref is gone" — a parked focus request waits for the first and can never
    /// be honoured by the second.
    pub(crate) fn attachment(&self) -> Attachment {
        match self.widget.try_get_untracked() {
            None => Attachment::Gone,
            Some(None) => Attachment::Unattached,
            Some(Some(id)) => Attachment::To(id),
        }
    }

    /// Whether this ref's own signals are still alive — that is, whether the
    /// scope that created it is.
    pub(crate) fn is_alive(&self) -> bool {
        !matches!(self.attachment(), Attachment::Gone)
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
    // A ref whose scope is gone has nothing left to point at, and the registry
    // nothing to keep for it: the read below would panic, and the write after
    // it would be dropped anyway. The claim this handle makes — that it answers
    // after its subject is gone — has to hold on the write path too.
    let Some(attached) = widget_ref.widget.try_get_untracked() else {
        return;
    };
    // Cheap when unchanged, and the write is what lets `focus()` resolve: a ref
    // that has never been laid out has no widget to focus.
    if attached != Some(id) {
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
            // one of them, so the handle is asked whether it is still there
            // before it is read.
            //
            // Dead is not the same as detached: a `WidgetRef` is `Copy`, so one
            // can be registered under two ids — the same view function used for
            // two surfaces does it — and the entry whose widget left the tree
            // clears the handle for both. The other entry is still live and
            // still laid out, and evicting it would freeze its `rect()` until
            // its container next re-registers.
            if !widget_ref.is_alive() {
                return false;
            }
            let attached = widget_ref.widget();
            if let Some(rect) = tree.get_surface_relative_bounds(id) {
                widget_ref.signal.set(rect);
                true
            } else {
                // Widget removed from tree — drop registry entry, and stop
                // claiming the handle points at something. A focus request
                // parked on this ref was waiting for the widget that just
                // left, so it goes with it.
                if attached == Some(id) {
                    widget_ref.widget.set(None);
                    crate::reactive::focus::drop_pending_focus_for(*widget_ref);
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

    /// Every question a handle answers about its own identity has to survive
    /// the scope that made it: it is `Copy`, so anyone can still be holding one.
    #[test]
    fn a_dead_ref_answers_instead_of_panicking() {
        create_root_owner();
        let (widget_ref, scope) = with_owner(create_widget_ref);
        dispose_owner_now(scope);

        assert_eq!(widget_ref.widget(), None, "it points at no live widget");
        assert!(!widget_ref.is_focused(), "and it does not hold the focus");
        assert!(!widget_ref.is_alive());
    }

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
