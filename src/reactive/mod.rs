pub mod callback;
pub mod clipboard;
pub mod context;
pub mod cursor;
pub mod diagnostics;
pub mod effect;
pub mod focus;
pub(crate) mod guard;
pub mod into_signal;
pub mod invalidation;
pub mod memo;
pub mod owner;
pub mod runtime;
pub mod service;
pub mod signal;
pub mod storage;
mod trigger;

pub(crate) use clipboard::selection_change_pending;
pub(crate) use clipboard::{
    clear_system_clipboard, set_system_clipboard, set_system_primary, take_clipboard_change,
    take_primary_change,
};
pub use clipboard::{
    clipboard_copy, clipboard_has_content, clipboard_paste, primary_copy, primary_paste,
};
pub use context::{
    expect_context, has_context, provide_context, provide_signal_context, use_context, with_context,
};
pub use cursor::{CursorIcon, set_cursor};
pub(crate) use cursor::{cursor_change_pending, take_cursor_change};
pub use effect::{Effect, create_effect};
pub(crate) use focus::{
    focus_path, has_focus, init_focus, release_focus, release_focus_if_within, request_focus,
};
#[doc(hidden)]
pub use into_signal::{
    ClosureMarker, LossyMarker, MemoMarker, RwSignalMarker, SignalMarker, ValueMarker,
};
pub use into_signal::{IntoSignal, IntoVal};
pub(crate) use invalidation::with_signal_tracking;
pub use memo::{Memo, create_memo};
// with_owner and OwnerId are internal and automatically used by the
// dynamic children system; the public dispose_owner is deferred (safe to
// call from anywhere), the synchronous engine stays crate-internal.
pub(crate) use owner::{OwnerId, create_root_owner, dispose_owner_now, with_owner};
pub use owner::{dispose_owner, on_cleanup};
pub use trigger::{Trigger, create_trigger};

/// Internal module for macro support. NOT PART OF PUBLIC API.
/// Do not use directly - these are re-exported for proc macros only.
#[doc(hidden)]
pub mod __internal {
    pub use super::owner::{OwnerId, dispose_owner_now as dispose_owner, with_owner};
    pub use super::runtime::batch;
}
pub use callback::Callback;
pub(crate) use runtime::{bg_writes_pending, flush_bg_writes};
pub use service::{Service, ServiceContext, create_service};
pub use signal::{
    OptionSignalExt, RwSignal, Signal, WriteSignal, create_derived, create_signal, create_stored,
};

/// Reset all reactive system state.
///
/// Called during `App::drop()` to wipe all thread-local reactive state,
/// enabling clean restart of the application.
pub(crate) fn reset_reactive() {
    owner::reset_owners();
    runtime::reset_runtime();
    // Before `reset_storage`: the focus path lives in a signal now, held by a
    // thread-local that outlives the storage. It releases the handle rather
    // than writing through it, so the next `App` on this thread gets a fresh
    // one instead of a silently dead signal.
    focus::reset_focus();
    storage::reset_storage();
    invalidation::reset_invalidation();
    clipboard::reset_clipboard();
    cursor::reset_cursor();
    context::reset_contexts();
    diagnostics::reset();
}
