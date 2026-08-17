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

    /// System clipboard contents (prefetched from the Wayland selection offer)
    static SYSTEM_CLIPBOARD: RefCell<Option<String>> = const { RefCell::new(None) };

    /// Internal primary-selection buffer (our outgoing content)
    static PRIMARY: RefCell<Option<String>> = const { RefCell::new(None) };

    /// Flag indicating the primary selection changed and needs syncing
    static PRIMARY_CHANGED: RefCell<bool> = const { RefCell::new(false) };

    /// System primary-selection contents (prefetched from other apps)
    static SYSTEM_PRIMARY: RefCell<Option<String>> = const { RefCell::new(None) };
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

/// Copy text to the primary selection (select-to-copy).
pub fn primary_copy(text: &str) {
    PRIMARY.with(|c| {
        *c.borrow_mut() = Some(text.to_string());
    });
    PRIMARY_CHANGED.with(|changed| {
        *changed.borrow_mut() = true;
    });
}

/// Take pending primary-selection change (for syncing to Wayland)
pub(crate) fn take_primary_change() -> Option<String> {
    let changed = PRIMARY_CHANGED.with(|c| {
        let was_changed = *c.borrow();
        *c.borrow_mut() = false;
        was_changed
    });

    if changed {
        PRIMARY.with(|c| c.borrow().clone())
    } else {
        None
    }
}

/// Paste text from the primary selection (middle-click paste).
pub fn primary_paste() -> Option<String> {
    SYSTEM_PRIMARY.with(|sc| {
        if let Some(text) = sc.borrow().as_ref() {
            return Some(text.clone());
        }
        PRIMARY.with(|c| c.borrow().clone())
    })
}

/// Set/clear system primary-selection contents (from Wayland)
pub(crate) fn set_system_primary(text: Option<String>) {
    SYSTEM_PRIMARY.with(|sc| {
        *sc.borrow_mut() = text;
    });
}

/// Reset all clipboard state.
///
/// Called during `App::drop()` to wipe clipboard buffers.
pub(crate) fn reset_clipboard() {
    CLIPBOARD.with(|c| *c.borrow_mut() = None);
    CLIPBOARD_CHANGED.with(|c| *c.borrow_mut() = false);
    SYSTEM_CLIPBOARD.with(|c| *c.borrow_mut() = None);
    PRIMARY.with(|c| *c.borrow_mut() = None);
    PRIMARY_CHANGED.with(|c| *c.borrow_mut() = false);
    SYSTEM_PRIMARY.with(|c| *c.borrow_mut() = None);
}

/// Whether a copy is queued for the compositor and not yet handed over.
///
/// Part of the loop's wakeup check — see `queued_but_unwoken` in `lib.rs`.
pub(crate) fn selection_change_pending() -> bool {
    CLIPBOARD_CHANGED.with(|c| *c.borrow()) || PRIMARY_CHANGED.with(|c| *c.borrow())
}
