pub mod clipboard;
pub mod cursor;
pub mod effect;
pub mod focus;
pub mod invalidation;
pub mod maybe_dyn;
pub mod memo;
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
pub use cursor::{CursorIcon, get_current_cursor, set_cursor, take_cursor_change};
pub use effect::{Effect, create_effect};
pub use focus::{clear_focus, focused_widget, has_focus, release_focus, request_focus};
pub use invalidation::{
    notify_signal_change, record_signal_read, register_subscriber, with_signal_tracking,
};
pub use maybe_dyn::{IntoMaybeDyn, MaybeDyn};
pub use memo::{Memo, create_memo};
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
pub use runtime::{batch, flush_bg_effect_writes};
pub use service::{Service, ServiceContext, create_service};
pub use signal::{ReadSignal, Signal, WriteSignal, create_signal};
