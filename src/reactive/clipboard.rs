//! Clipboard support for text copy/paste operations.
//!
//! This module provides the application's own clipboard buffer for internal
//! clipboard operations. It also coordinates with the Wayland clipboard for
//! system-wide clipboard support. The buffers themselves are fields of the
//! application's state, in `src/app_state.rs`.

use crate::app_state::with_app_state;

/// Copy text to the clipboard.
pub fn clipboard_copy(text: &str) {
    with_app_state(|app| {
        *app.clipboard.borrow_mut() = Some(text.to_string());
        // Setting the slot is what wakes the loop that hands the copy over.
        app.outgoing_clipboard.set(text.to_string());
    });
}

/// Take the copy waiting to go out to the compositor, if any.
pub fn take_clipboard_change() -> Option<String> {
    with_app_state(|app| app.outgoing_clipboard.take())
}

/// Paste text from the clipboard
/// Returns the clipboard contents if available
pub fn clipboard_paste() -> Option<String> {
    // First try system clipboard, fall back to internal
    with_app_state(|app| {
        app.system_clipboard
            .borrow()
            .clone()
            .or_else(|| app.clipboard.borrow().clone())
    })
}

/// Check if clipboard has content
pub fn clipboard_has_content() -> bool {
    with_app_state(|app| {
        app.system_clipboard.borrow().is_some() || app.clipboard.borrow().is_some()
    })
}

/// Set system clipboard contents (called from Wayland event handling)
pub fn set_system_clipboard(text: String) {
    with_app_state(|app| *app.system_clipboard.borrow_mut() = Some(text));
}

/// Clear system clipboard (called when selection is lost)
pub fn clear_system_clipboard() {
    with_app_state(|app| *app.system_clipboard.borrow_mut() = None);
}

/// Copy text to the primary selection (select-to-copy).
pub fn primary_copy(text: &str) {
    with_app_state(|app| {
        *app.primary.borrow_mut() = Some(text.to_string());
        app.outgoing_primary.set(text.to_string());
    });
}

/// Take the select-to-copy waiting to go out to the compositor, if any.
pub(crate) fn take_primary_change() -> Option<String> {
    with_app_state(|app| app.outgoing_primary.take())
}

/// Paste text from the primary selection (middle-click paste).
pub fn primary_paste() -> Option<String> {
    with_app_state(|app| {
        app.system_primary
            .borrow()
            .clone()
            .or_else(|| app.primary.borrow().clone())
    })
}

/// Set/clear system primary-selection contents (from Wayland)
pub(crate) fn set_system_primary(text: Option<String>) {
    with_app_state(|app| *app.system_primary.borrow_mut() = text);
}
