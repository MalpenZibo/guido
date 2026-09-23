//! The pointer's shape: declared by widgets, resolved from the point.
//!
//! A container says which shape it wants with
//! [`cursor`](crate::widgets::Container::cursor), and a widget of its own says
//! it through [`Tree::point_shows_cursor`](crate::tree::Tree::point_shows_cursor).
//! After every positioned pointer event the loop takes the innermost
//! declaration under the point — the dispatch's own hit test, read at the end —
//! and hands it here; with none, the arrow. There is no imperative setter: a
//! declaration and an override are two ways of saying one thing, and they
//! fought (#496).

use crate::app_state::with_app_state;
use crate::reactive::owner::with_root_owner;
use crate::reactive::{Prop, create_effect, dispose_owner_now, with_owner};

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

/// Show the shape the widget under the point declares, and go on showing it
/// as long as the pointer stays there.
///
/// Called by the loop after every positioned pointer event. `Unset` is the
/// arrow. A declaration that is a signal is watched, so a shape that changes
/// while the pointer stays still — busy while something loads — goes out
/// without waiting for the pointer to move; the watch ends when the pointer
/// finds another declaration.
///
/// The same declaration again is nothing to do: every move over a button is
/// one of these, and it must not cost a new watch.
pub(crate) fn point_at(declared: Prop<CursorIcon>) {
    let unchanged = with_app_state(|app| app.pointed_cursor.replace(declared) == declared);
    if unchanged {
        return;
    }
    if let Some(watch) = with_app_state(|app| app.cursor_watch.take()) {
        dispose_owner_now(watch);
    }
    if let Prop::Reactive(signal) = declared {
        // Under the root rather than whatever scope the dispatch happens to be
        // in, so the watch lives until the next declaration ends it. That can
        // be after the widget that declared it has gone, and a declaration
        // that has gone has no shape to show.
        let ((), watch) = with_root_owner(|| {
            with_owner(|| {
                create_effect(move || {
                    if signal.is_live() {
                        show(signal.get());
                    }
                })
            })
        });
        with_app_state(|app| app.cursor_watch.set(Some(watch)));
    } else {
        show(declared.get_or_untracked(CursorIcon::Default));
    }
}

/// Hand `cursor` to the loop, unless it is what the seat already shows.
fn show(cursor: CursorIcon) {
    with_app_state(|app| {
        if app.current_cursor.replace(cursor) != cursor {
            // Setting the slot is what wakes the loop that hands the shape over.
            app.outgoing_cursor.set(cursor);
        }
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
/// Through the same slot as [`point_at`], so the shape resolved from the
/// enter's own point still has the last word.
pub(crate) fn resend_cursor() {
    with_app_state(|app| app.outgoing_cursor.set(app.current_cursor.get()));
}

/// Take the shape waiting to go out to the compositor, if any.
///
/// Called by the main event loop to sync the cursor to Wayland.
pub(crate) fn take_cursor_change() -> Option<CursorIcon> {
    with_app_state(|app| app.outgoing_cursor.take())
}
