//! Focus management system for keyboard input routing.
//!
//! This module provides a centralized way to track which widget has keyboard
//! focus. Only one widget can have focus at a time.
//!
//! # Why the path and not just the id
//!
//! A container's `focused_state` applies when *it or anything below it* has
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
//! Being a signal, it also means a container that declares `focused_state`
//! subscribes by asking. That is a wider fan-out than the two `request_job`
//! calls it replaces — every container declaring `focused_state` wakes on any
//! focus change, not just the two on the path — but it is bounded by a rare
//! declaration, and it is what lets a focus change reach a descendant's text.

use smallvec::SmallVec;

use crate::jobs::{JobRequest, request_job};
use crate::reactive::signal::{RwSignal, create_signal};
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

thread_local! {
    /// The focused widget and its ancestors. A signal, so that resolving a
    /// `focused_state` subscribes to it.
    static FOCUS: RwSignal<FocusPath> = create_signal(FocusPath::default());
}

fn focus() -> RwSignal<FocusPath> {
    FOCUS.with(|f| *f)
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
/// matching any live one and `focused_state` resolved to false on its own.
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

/// Reset focus state (without paint jobs — used during App teardown).
///
/// Called during `App::drop()` to clear focus state.
pub(crate) fn reset_focus() {
    focus().set(FocusPath::default());
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
