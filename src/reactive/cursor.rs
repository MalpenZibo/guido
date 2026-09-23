//! Cursor management for changing the mouse cursor appearance.
//!
//! Widgets can request a cursor change by calling `set_cursor(CursorIcon::Text)`.
//! The main event loop will pick up cursor changes and apply them via Wayland.

use crate::app_state::with_app_state;

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

/// Set the cursor to display.
/// This should be called by widgets when they want to change the cursor appearance.
pub fn set_cursor(cursor: CursorIcon) {
    with_app_state(|app| {
        if app.current_cursor.replace(cursor) == cursor {
            return;
        }
        // Setting the slot is what wakes the loop that hands the shape over.
        app.outgoing_cursor.set(cursor);
    });
}

/// Send the current shape again, changed or not.
///
/// For a pointer entering a surface: `wl_pointer.enter` leaves the cursor
/// undefined until a shape is set against that enter's serial, so the one the
/// seat last had is no longer on screen — however unchanged it is here. Every
/// toolkit re-applies it there: winit's `reload_cursor_style`, SDL's
/// `Wayland_SeatUpdatePointerCursor`, GTK's
/// `gdk_wayland_device_update_surface_cursor`.
///
/// Through the same slot as [`set_cursor`], so a widget that changes the shape
/// while handling the same enter still has the last word.
pub(crate) fn resend_cursor() {
    with_app_state(|app| app.outgoing_cursor.set(app.current_cursor.get()));
}

/// Take the shape waiting to go out to the compositor, if any.
///
/// Called by the main event loop to sync the cursor to Wayland.
pub fn take_cursor_change() -> Option<CursorIcon> {
    with_app_state(|app| app.outgoing_cursor.take())
}

/// Get the current cursor without clearing the change flag.
pub fn get_current_cursor() -> CursorIcon {
    with_app_state(|app| app.current_cursor.get())
}
