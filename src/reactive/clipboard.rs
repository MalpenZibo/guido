//! Clipboard support for text copy/paste operations.
//!
//! This module provides a thread-local clipboard buffer for internal clipboard operations.
//! It also coordinates with the Wayland clipboard for system-wide clipboard support.

use std::cell::RefCell;

thread_local! {
    /// Internal clipboard buffer
    static CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };

    /// Flag indicating clipboard was changed and needs to be synced to Wayland
    static CLIPBOARD_CHANGED: RefCell<bool> = const { RefCell::new(false) };

    /// Pending clipboard read request (for async clipboard reading from Wayland)
    static CLIPBOARD_READ_REQUESTED: RefCell<bool> = const { RefCell::new(false) };

    /// System clipboard contents (from Wayland selection offer)
    static SYSTEM_CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Copy text to the clipboard
pub fn clipboard_copy(text: &str) {
    CLIPBOARD.with(|c| {
        *c.borrow_mut() = Some(text.to_string());
    });
    CLIPBOARD_CHANGED.with(|changed| {
        *changed.borrow_mut() = true;
    });
}

/// Take pending clipboard change (returns text if clipboard was changed since last call)
pub fn take_clipboard_change() -> Option<String> {
    let changed = CLIPBOARD_CHANGED.with(|c| {
        let was_changed = *c.borrow();
        *c.borrow_mut() = false;
        was_changed
    });

    if changed {
        CLIPBOARD.with(|c| c.borrow().clone())
    } else {
        None
    }
}

/// Paste text from the clipboard
/// Returns the clipboard contents if available
pub fn clipboard_paste() -> Option<String> {
    // First try system clipboard, fall back to internal
    SYSTEM_CLIPBOARD.with(|sc| {
        if let Some(text) = sc.borrow().as_ref() {
            return Some(text.clone());
        }
        CLIPBOARD.with(|c| c.borrow().clone())
    })
}

/// Check if clipboard has content
pub fn clipboard_has_content() -> bool {
    SYSTEM_CLIPBOARD.with(|sc| {
        if sc.borrow().is_some() {
            return true;
        }
        CLIPBOARD.with(|c| c.borrow().is_some())
    })
}

/// Set system clipboard contents (called from Wayland event handling)
pub fn set_system_clipboard(text: String) {
    SYSTEM_CLIPBOARD.with(|sc| {
        *sc.borrow_mut() = Some(text);
    });
}

/// Clear system clipboard (called when selection is lost)
pub fn clear_system_clipboard() {
    SYSTEM_CLIPBOARD.with(|sc| {
        *sc.borrow_mut() = None;
    });
}

/// Request reading from system clipboard
pub fn request_clipboard_read() {
    CLIPBOARD_READ_REQUESTED.with(|r| {
        *r.borrow_mut() = true;
    });
}

/// Reset all clipboard state.
///
/// Called during `App::drop()` to wipe clipboard buffers.
pub(crate) fn reset_clipboard() {
    CLIPBOARD.with(|c| *c.borrow_mut() = None);
    CLIPBOARD_CHANGED.with(|c| *c.borrow_mut() = false);
    CLIPBOARD_READ_REQUESTED.with(|c| *c.borrow_mut() = false);
    SYSTEM_CLIPBOARD.with(|c| *c.borrow_mut() = None);
}

/// Check and clear clipboard read request
pub fn take_clipboard_read_request() -> bool {
    CLIPBOARD_READ_REQUESTED.with(|r| {
        let requested = *r.borrow();
        if requested {
            *r.borrow_mut() = false;
        }
        requested
    })
}
