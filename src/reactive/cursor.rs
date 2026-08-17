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

    /// A shape waiting to be handed to the compositor. Being empty *is* the
    /// "nothing to sync" state — no separate dirty flag to keep in step.
    static OUTGOING_CURSOR: RefCell<Option<CursorIcon>> = const { RefCell::new(None) };
}

/// Set the cursor to display.
/// This should be called by widgets when they want to change the cursor appearance.
pub fn set_cursor(cursor: CursorIcon) {
    let changed = CURRENT_CURSOR.with(|c| {
        let current = *c.borrow();
        if current == cursor {
            return false;
        }
        *c.borrow_mut() = cursor;
        true
    });
    if changed {
        OUTGOING_CURSOR.with(|out| {
            *out.borrow_mut() = Some(cursor);
        });
        // Queueing and waking are one gesture — see `jobs::wake_loop`.
        crate::jobs::wake_loop();
    }
}

/// Take the shape waiting to go out to the compositor, if any.
///
/// Called by the main event loop to sync the cursor to Wayland.
pub fn take_cursor_change() -> Option<CursorIcon> {
    OUTGOING_CURSOR.with(|out| out.borrow_mut().take())
}

/// Reset cursor state to defaults.
///
/// Called during `App::drop()` to clear cursor state.
pub(crate) fn reset_cursor() {
    CURRENT_CURSOR.with(|c| *c.borrow_mut() = CursorIcon::Default);
    OUTGOING_CURSOR.with(|o| *o.borrow_mut() = None);
}

/// Get the current cursor without clearing the change flag.
pub fn get_current_cursor() -> CursorIcon {
    CURRENT_CURSOR.with(|c| *c.borrow())
}

/// Whether a cursor shape change is queued for the compositor.
///
/// Part of the loop's wakeup check — see `queued_but_unwoken` in `lib.rs`.
pub(crate) fn cursor_change_pending() -> bool {
    OUTGOING_CURSOR.with(|o| o.borrow().is_some())
}
