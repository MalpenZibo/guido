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

pub(crate) use clipboard::{
    clipboard_copy, clipboard_paste, set_system_clipboard, take_clipboard_change,
};
pub(crate) use cursor::take_cursor_change;
pub use cursor::{CursorIcon, set_cursor};
pub use effect::{Effect, create_effect};
pub(crate) use focus::{focused_widget, has_focus, release_focus, request_focus};
pub(crate) use invalidation::with_signal_tracking;
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
    pub use super::runtime::batch;
}
pub(crate) use runtime::flush_bg_writes;
pub use service::{Service, ServiceContext, create_service};
pub use signal::{Signal, WriteSignal, create_signal};
