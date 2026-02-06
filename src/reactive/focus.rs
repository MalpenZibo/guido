//! Focus management system for keyboard input routing.
//!
//! This module provides a centralized way to track which widget has keyboard focus.
//! Only one widget can have focus at a time.

use std::cell::RefCell;

use crate::jobs::{JobRequest, request_job};
use crate::tree::WidgetId;

thread_local! {
    /// The currently focused widget ID, if any
    static FOCUSED_WIDGET: RefCell<Option<WidgetId>> = const { RefCell::new(None) };
}

/// Request keyboard focus for a widget.
/// If another widget has focus, it will lose focus and be repainted.
pub fn request_focus(id: WidgetId) {
    FOCUSED_WIDGET.with(|cell| {
        let mut focused = cell.borrow_mut();
        // Repaint the previously focused widget so its parent drops the focused styling
        if let Some(old_id) = *focused
            && old_id != id
        {
            request_job(old_id, JobRequest::Paint);
        }
        *focused = Some(id);
    });
}

/// Release keyboard focus from a widget.
/// Only releases if the given widget currently has focus, and repaints it.
pub fn release_focus(id: WidgetId) {
    FOCUSED_WIDGET.with(|cell| {
        let mut focused = cell.borrow_mut();
        if *focused == Some(id) {
            request_job(id, JobRequest::Paint);
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
/// Repaints the previously focused widget if any.
pub fn clear_focus() {
    FOCUSED_WIDGET.with(|cell| {
        let mut focused = cell.borrow_mut();
        if let Some(old_id) = focused.take() {
            request_job(old_id, JobRequest::Paint);
        }
    });
}
