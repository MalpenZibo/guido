//! Focus management system for keyboard input routing.
//!
//! This module provides a centralized way to track which widget has keyboard
//! focus. Only one widget can have focus at a time.
//!
//! # Why the path and not just the id
//!
//! A container's `when_focused` applies when *it or anything below it* has
//! focus, so answering "is the focus inside me" used to mean walking the
//! container's descendants with the tree in hand. That is fine at paint time
//! and impossible anywhere else — in particular inside a `create_derived`
//! closure, which is where a container has to resolve the text colour it
//! publishes to its descendants.
//!
//! So the focused widget's ancestors are computed *once*, when focus moves and
//! the tree is right there, and stored alongside it. Asking becomes
//! `path.contains(id)`: no tree, and cheap enough to call from a closure.
//!
//! Being a signal, it also means a container that declares `when_focused`
//! subscribes by asking. That is a wider fan-out than the two `request_job`
//! calls it replaces — every container declaring `when_focused` wakes on any
//! focus change, not just the two on the path — but it is bounded by a rare
//! declaration, and it is what lets a focus change reach a descendant's text.

use std::cell::RefCell;

use smallvec::SmallVec;

use crate::jobs::{JobRequest, request_job};
use crate::reactive::global::GlobalSignal;
use crate::reactive::signal::RwSignal;
use crate::tree::{Tree, WidgetId};

/// The focused widget and every ancestor of it, innermost first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FocusPath {
    chain: SmallVec<[WidgetId; 8]>,
}

impl FocusPath {
    fn of(tree: &Tree, id: WidgetId) -> Self {
        let mut chain = SmallVec::new();
        let mut current = Some(id);
        while let Some(node) = current {
            chain.push(node);
            current = tree.get_parent(node);
        }
        Self { chain }
    }

    /// The focused widget itself, if any.
    pub fn widget(&self) -> Option<WidgetId> {
        self.chain.first().copied()
    }

    /// Whether `id` is the focused widget or one of its ancestors.
    pub fn contains(&self, id: WidgetId) -> bool {
        self.chain.contains(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }
}

/// The focused widget and its ancestors. A signal, so that resolving a
/// `when_focused` subscribes to it.
static FOCUS: GlobalSignal<FocusPath> = GlobalSignal::new(FocusPath::default);

fn focus() -> RwSignal<FocusPath> {
    FOCUS.get()
}

/// The current focus path. Reading this subscribes, like any other signal.
pub fn focus_path() -> FocusPath {
    focus().get()
}

/// Request keyboard focus for a widget.
///
/// Takes the tree because the ancestor path is resolved here, once, rather
/// than by every reader later — see the module docs.
pub fn request_focus(tree: &Tree, id: WidgetId) {
    let old = focus().get_untracked();
    if old.widget() == Some(id) {
        return;
    }
    // Repaint the previously focused widget so it drops focused styling. The
    // signal write covers everything that *resolves* focus, but a widget that
    // merely draws a caret (a text input) reads `has_focus` directly.
    if let Some(old_id) = old.widget() {
        request_job(old_id, JobRequest::Paint);
    }
    focus().set(FocusPath::of(tree, id));
    request_job(id, JobRequest::Paint);
}

thread_local! {
    /// A focus request from application code, waiting for a tree.
    ///
    /// One slot, not a queue: two requests in the same frame are two answers to
    /// "where should the keyboard be", and the last one is the one the caller
    /// meant.
    static PENDING: RefCell<Option<crate::widget_ref::WidgetRef>> = const { RefCell::new(None) };
}

/// Ask for the focus to move to whatever `widget_ref` names, on the next frame.
///
/// The caller has no tree — see
/// [`WidgetRef::focus`](crate::widget_ref::WidgetRef::focus) — so the request is
/// parked here and applied by the loop. It is stored as the *handle* rather than
/// an id so that asking before the widget's first layout works: the id does not
/// exist yet, and by the time the loop applies the request, it does.
pub(crate) fn request_focus_deferred(widget_ref: crate::widget_ref::WidgetRef) {
    PENDING.with(|pending| *pending.borrow_mut() = Some(widget_ref));
    // The loop may be blocked with nothing else to do, and a focus change is work.
    crate::jobs::wake_loop();
}

/// Apply a parked focus request. Called by the loop after layout, where the tree
/// is complete and a handle attached during this frame has resolved.
///
/// Public for the same reason [`request_focus`] is: anything driving frames needs
/// it, and it is only meaningful with a laid-out tree in hand.
pub fn apply_pending_focus(tree: &Tree) {
    let Some(widget_ref) = PENDING.with(|pending| *pending.borrow()) else {
        return;
    };
    // A request whose ref died with the scope that made it can never be
    // honoured — `.focus()` from inside a popup that closed before the frame
    // came round. Parked, it would be read again on every frame that follows.
    if !widget_ref.is_alive() {
        PENDING.with(|pending| *pending.borrow_mut() = None);
        return;
    }
    // A request naming a widget that is still not in the tree stays parked: the
    // app asked for a field that has not been built yet, which is the ordinary
    // shape of `focus()` called from a startup effect.
    if let Some(id) = widget_ref.widget() {
        PENDING.with(|pending| *pending.borrow_mut() = None);
        request_focus(tree, id);
    }
}

/// Drop any parked request. Called during `App::drop()`.
pub(crate) fn reset_pending_focus() {
    PENDING.with(|pending| *pending.borrow_mut() = None);
}

/// Release keyboard focus from a widget.
/// Only releases if the given widget currently has focus, and repaints it.
pub fn release_focus(id: WidgetId) {
    if focus().get_untracked().widget() == Some(id) {
        request_job(id, JobRequest::Paint);
        focus().set(FocusPath::default());
    }
}

/// Drop the focus if `id` is anywhere on the path.
///
/// Called when a widget leaves the tree. Before the path existed this was not
/// needed: focus was a generational id, so a dead widget simply stopped
/// matching any live one and `when_focused` resolved to false on its own.
/// A stored path has no such self-correction — the ancestors would keep
/// answering "the focus is inside me" for a widget that no longer exists.
pub(crate) fn release_focus_if_within(id: WidgetId) {
    let path = focus().get_untracked();
    if path.contains(id) {
        focus().set(FocusPath::default());
    }
}

/// Check if a specific widget has keyboard focus.
pub fn has_focus(id: WidgetId) -> bool {
    focus().get_untracked().widget() == Some(id)
}

/// Get the ID of the currently focused widget, if any.
pub fn focused_widget() -> Option<WidgetId> {
    focus().get_untracked().widget()
}

/// Clear all focus (no widget will have focus).
/// Repaints the previously focused widget if any.
pub fn clear_focus() {
    let old = focus().get_untracked();
    if let Some(old_id) = old.widget() {
        request_job(old_id, JobRequest::Paint);
        focus().set(FocusPath::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{create_root_owner, dispose_owner_now, with_owner};

    /// `.focus()` from inside a popup, on a ref composed in that popup, when the
    /// popup closes before the loop comes round: the request is parked by
    /// design — a widget not yet in the tree is the ordinary case — so a dead
    /// one would be read again on every frame from then on.
    #[test]
    fn a_parked_request_whose_ref_died_is_dropped_rather_than_read_forever() {
        create_root_owner();
        let tree = Tree::new();
        let ((), popup_scope) = with_owner(|| {
            request_focus_deferred(crate::widget_ref::create_widget_ref());
        });
        dispose_owner_now(popup_scope);

        apply_pending_focus(&tree);
        apply_pending_focus(&tree);

        assert!(
            PENDING.with(|pending| pending.borrow().is_none()),
            "a request that can never be honoured does not stay parked"
        );
    }
}
