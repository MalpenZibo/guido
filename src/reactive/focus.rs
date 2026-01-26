//! Focus management system for keyboard input routing.
//!
//! This module provides a centralized way to track which widget has keyboard focus.
//! Only one widget can have focus at a time.

use std::cell::RefCell;

use super::WidgetId;

thread_local! {
    /// The currently focused widget ID, if any
    static FOCUSED_WIDGET: RefCell<Option<WidgetId>> = const { RefCell::new(None) };
}

/// Request keyboard focus for a widget.
/// If another widget has focus, it will lose focus.
pub fn request_focus(id: WidgetId) {
    FOCUSED_WIDGET.with(|cell| {
        *cell.borrow_mut() = Some(id);
    });
}

/// Release keyboard focus from a widget.
/// Only releases if the given widget currently has focus.
pub fn release_focus(id: WidgetId) {
    FOCUSED_WIDGET.with(|cell| {
        let mut focused = cell.borrow_mut();
        if *focused == Some(id) {
            *focused = None;
        }
    });
}

/// Check if a specific widget has keyboard focus.
pub fn has_focus(id: WidgetId) -> bool {
    FOCUSED_WIDGET.with(|cell| *cell.borrow() == Some(id))
}

/// Get the ID of the currently focused widget, if any.
pub fn focused_widget() -> Option<WidgetId> {
    FOCUSED_WIDGET.with(|cell| *cell.borrow())
}

/// Clear all focus (no widget will have focus).
pub fn clear_focus() {
    FOCUSED_WIDGET.with(|cell| {
        *cell.borrow_mut() = None;
    });
}
