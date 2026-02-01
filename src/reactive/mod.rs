pub mod clipboard;
pub mod computed;
pub mod cursor;
pub mod effect;
pub mod focus;
pub mod invalidation;
pub mod layout_arena;
pub mod maybe_dyn;
pub mod owner;
pub mod runtime;
pub mod service;
pub mod signal;
pub mod storage;

pub use clipboard::{
    clear_system_clipboard, clipboard_copy, clipboard_has_content, clipboard_paste,
    request_clipboard_read, set_system_clipboard, take_clipboard_change,
    take_clipboard_read_request,
};
pub use computed::{Computed, create_computed};
pub use cursor::{CursorIcon, get_current_cursor, set_cursor, take_cursor_change};
pub use effect::{Effect, create_effect};
pub use focus::{clear_focus, focused_widget, has_focus, release_focus, request_focus};
pub use invalidation::{
    ChangeFlags, WidgetId, clear_animation_flag, finish_layout_tracking, init_wakeup,
    mark_needs_layout, request_animation_frame, request_frame, request_layout, request_paint,
    start_layout_tracking, take_frame_request, with_app_state, with_app_state_mut,
};
pub use layout_arena::{
    LayoutArena, LayoutNode, arena_add_layout_root, arena_cache_layout, arena_cached_constraints,
    arena_cached_size, arena_clear_dirty, arena_get_parent, arena_has_layout_roots, arena_is_dirty,
    arena_mark_needs_layout, arena_set_parent, arena_set_relayout_boundary,
    arena_take_layout_roots, register_widget, unregister_widget, with_arena_widget,
    with_arena_widget_mut, with_layout_arena,
};
pub use maybe_dyn::{IntoMaybeDyn, MaybeDyn};
// Only on_cleanup is public API - with_owner, dispose_owner, and OwnerId are
// internal and automatically used by the dynamic children system
pub use owner::on_cleanup;
pub(crate) use owner::{OwnerId, dispose_owner, with_owner};

/// Internal module for macro support. NOT PART OF PUBLIC API.
/// Do not use directly - these are re-exported for proc macros only.
#[doc(hidden)]
pub mod __internal {
    pub use super::owner::{OwnerId, dispose_owner, with_owner};
}
pub use runtime::batch;
pub use service::{Service, ServiceContext, create_service};
pub use signal::{ReadSignal, Signal, WriteSignal, create_signal};
