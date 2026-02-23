//! Cursor management for changing the mouse cursor appearance.
//!
//! Widgets can request a cursor change by calling `set_cursor(CursorIcon::Text)`.
//! The main event loop will pick up cursor changes and apply them via Wayland.

use std::cell::RefCell;

/// Standard cursor icons that can be displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorIcon {
    /// The default arrow cursor.
    #[default]
    Default,
    /// Text selection cursor (I-beam).
    Text,
    /// Pointer/hand cursor for clickable elements.
    Pointer,
    /// Crosshair cursor.
    Crosshair,
    /// Move/drag cursor.
    Move,
    /// Not allowed cursor.
    NotAllowed,
    /// Grab cursor (open hand).
    Grab,
    /// Grabbing cursor (closed hand).
    Grabbing,
    /// Resize cursors for window edges.
    ResizeNorth,
    ResizeSouth,
    ResizeEast,
    ResizeWest,
    ResizeNorthEast,
    ResizeNorthWest,
    ResizeSouthEast,
    ResizeSouthWest,
    /// Column resize cursor.
    ColResize,
    /// Row resize cursor.
    RowResize,
    /// Wait/loading cursor.
    Wait,
    /// Progress cursor (arrow with spinner).
    Progress,
}

thread_local! {
    /// Current requested cursor
    static CURRENT_CURSOR: RefCell<CursorIcon> = const { RefCell::new(CursorIcon::Default) };

    /// Flag indicating cursor was changed and needs to be synced to Wayland
    static CURSOR_CHANGED: RefCell<bool> = const { RefCell::new(false) };
}

/// Set the cursor to display.
/// This should be called by widgets when they want to change the cursor appearance.
pub fn set_cursor(cursor: CursorIcon) {
    CURRENT_CURSOR.with(|c| {
        let current = *c.borrow();
        if current != cursor {
            *c.borrow_mut() = cursor;
            CURSOR_CHANGED.with(|changed| {
                *changed.borrow_mut() = true;
            });
        }
    });
}

/// Take pending cursor change (returns cursor if it was changed since last call).
/// Called by the main event loop to sync cursor to Wayland.
pub fn take_cursor_change() -> Option<CursorIcon> {
    let changed = CURSOR_CHANGED.with(|c| {
        let was_changed = *c.borrow();
        *c.borrow_mut() = false;
        was_changed
    });

    if changed {
        Some(CURRENT_CURSOR.with(|c| *c.borrow()))
    } else {
        None
    }
}

/// Reset cursor state to defaults.
///
/// Called during `App::drop()` to clear cursor state.
pub(crate) fn reset_cursor() {
    CURRENT_CURSOR.with(|c| *c.borrow_mut() = CursorIcon::Default);
    CURSOR_CHANGED.with(|c| *c.borrow_mut() = false);
}

/// Get the current cursor without clearing the change flag.
pub fn get_current_cursor() -> CursorIcon {
    CURRENT_CURSOR.with(|c| *c.borrow())
}
