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
use crate::widget_ref::Attachment;

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

    /// The root the focused widget sits under, if anything is focused.
    ///
    /// The chain was walked to a widget with no parent when the focus moved,
    /// and the only widgets without one are surface roots — so the last link
    /// already names the surface the keyboard is on.
    /// [`Tree::surface_root_of`](crate::tree::Tree::surface_root_of) answers
    /// the same question by walking live parents; this one is here because the
    /// answer is already in hand, and the walk is what the path exists to have
    /// done once.
    pub(crate) fn root(&self) -> Option<WidgetId> {
        self.chain.last().copied()
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
    // One read of the handle, three outcomes. A request whose ref died with
    // the scope that made it can never be honoured — `.focus()` from inside a
    // popup that closed before the frame came round — and parked it would be
    // read again on every frame after. One naming a widget that is not in the
    // tree *yet* stays parked: that is the ordinary shape of `focus()` from a
    // startup effect. One that names a widget applies.
    match widget_ref.attachment() {
        Attachment::Gone => PENDING.with(|pending| *pending.borrow_mut() = None),
        Attachment::Unattached => {}
        Attachment::To(id) => {
            PENDING.with(|pending| *pending.borrow_mut() = None);
            request_focus(tree, id);
        }
    }
}

/// Drop a parked request that names `widget_ref`.
///
/// Called by the widget-ref registry when a ref's widget leaves the tree. A
/// request waits for a widget that has not been laid out *yet* — that is the
/// ordinary shape of `focus()` from a startup effect — but one whose widget
/// has been and is gone is waiting for something that already happened, and
/// left parked it would fire at whatever takes that ref next, stealing the
/// keyboard from wherever the user had put it.
pub(crate) fn drop_pending_focus_for(widget_ref: crate::widget_ref::WidgetRef) {
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if *pending == Some(widget_ref) {
            *pending = None;
        }
    });
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
    if focus_within(id) {
        focus().set(FocusPath::default());
    }
}

/// Whether the focus is on `id` or inside it, without subscribing.
///
/// The tracked spelling of the same question is `focus_path().contains(id)`,
/// and it is the right one for anything resolving a style: it wants to be woken
/// when the answer changes. This one is for the event path, which reads to
/// decide and is already running because something happened — subscribing there
/// would open a subscription from inside an event handler.
pub(crate) fn focus_within(id: WidgetId) -> bool {
    focus().with_untracked(|path| path.contains(id))
}

/// Drop the focus if it belongs to the surface rooted at `root`.
///
/// Focus is one signal for the whole application while one `Tree` holds every
/// surface's root, so "nothing here claimed that press" speaks only for the
/// surface the press landed on. The focus path ends at the root of whichever
/// surface owns the keyboard, and that is the whole test.
pub(crate) fn release_focus_under(root: WidgetId) {
    if focus().with_untracked(|path| path.root() == Some(root)) {
        clear_focus();
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

    /// A request outlives a widget that comes and goes, and must not fire at
    /// whatever takes its ref next: the field it named left the tree, so the
    /// request is for something that already happened.
    #[test]
    fn a_request_dies_with_the_widget_it_named() {
        use crate::layout::Constraints;
        use crate::widgets::{container, text_input};

        create_root_owner();
        let field = crate::widget_ref::create_widget_ref();
        let mut tree = Tree::new();
        let root = tree.register(Box::new(container().child(
            text_input(crate::reactive::create_signal(String::new())).widget_ref(field),
        )));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 200.0, 40.0))
        });
        crate::widget_ref::update_widget_refs(&tree);
        assert!(field.widget().is_some(), "laid out, so it names a widget");

        request_focus_deferred(field);

        // The field is taken out of the tree: the registry notices on its next
        // pass, and the request goes with the widget it was waiting for.
        crate::jobs::teardown_widget_subtree(&mut tree, root);
        crate::widget_ref::update_widget_refs(&tree);

        assert!(
            PENDING.with(|pending| pending.borrow().is_none()),
            "a request for a widget that has left does not wait for its ref to be reused"
        );
    }

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
