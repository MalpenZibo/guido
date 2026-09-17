//! Everything the application keeps on its thread, in one place.
//!
//! These were twenty-two separate `thread_local!` cells, and `App::drop` reset
//! them by calling a hand-written list of nine functions. The list was wrong:
//! `DEFAULT_FONT_FAMILY` was not on it, so a second `App` on the same thread
//! inherited the font the first one declared. Nothing failed to build, because
//! nothing collected the cells anywhere that could be counted.
//!
//! One struct behind one thread-local fixes that at the definition: [`reset`]
//! destructures it, so a field added here and forgotten there is a compile
//! error rather than a value the next `App` inherits.
//!
//! **The fields keep their own cells; the struct is not itself a `RefCell`.**
//! Wrapping the whole of it would serialise state that is already interleaved
//! — updating the widget-ref registry writes signals, and a signal write
//! queues a job — so one borrow would be held across a call that takes
//! another, and the application would panic where it now runs. This is
//! floem's shape for the same reason: its `Runtime` is a plain struct of
//! cells behind one thread-local.
//!
//! The line this draws is what the state is *for*, not which module it was
//! written in: everything the running application has queued, mirrored or
//! declared lives here, while the reactive runtime's own bookkeeping — the
//! arena, the owners, the subscriptions, twelve cells of it — stays in
//! `src/reactive/` and is reset with `reset_reactive`. So the clipboard
//! buffers and the cursor come here from `src/reactive/`, and
//! `invalidation.rs` keeps its registry while its dirtied segments, which are
//! a queue the reconciler drains, do not. The diagnostics stay out of both:
//! they outlive an `App` on purpose.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::deferred::{DeferredQueue, DeferredSlot};
use crate::jobs::{JobQueues, ScheduledJob};
use crate::reactive::cursor::CursorIcon;
use crate::renderer::TextMeasurer;
use crate::session_lock::{LockData, LockRequest};
use crate::surface::{SurfaceCommand, SurfaceId};
use crate::tree::WidgetId;
use crate::widget_ref::WidgetRef;
use crate::widgets::font::FontFamily;

thread_local! {
    static APP: AppState = AppState::default();
}

/// Reach the application's state. Every one of the fields below is read and
/// written through here and nowhere else.
pub(crate) fn with_app_state<R>(f: impl FnOnce(&AppState) -> R) -> R {
    APP.with(f)
}

/// The application's state, for as long as one `App` lives on this thread.
#[derive(Default)]
pub(crate) struct AppState {
    // --- What the loop will be asked for on its next iteration -------------
    /// Widget jobs, queued by signal writes that have no `Tree`, sorted per
    /// surface by `distribute_jobs`.
    pub(crate) pending_jobs: RefCell<JobQueues>,
    /// Jobs owed to a clock (the caret), queued from widget code that has no
    /// loop. Outside `pending_jobs` so `has_pending_jobs` stays "there is work
    /// for *this* frame": treating a scheduled job as pending is what turns a
    /// blink into a poll.
    pub(crate) scheduled_jobs: RefCell<Vec<ScheduledJob>>,
    /// Dynamic-children segments a write dirtied, waiting for the
    /// reconciliation that has the tree.
    pub(crate) dirty_segments: RefCell<FxHashMap<WidgetId, SmallVec<[u32; 4]>>>,
    /// `SurfaceHandle` calls from application code, which has no platform.
    pub(crate) surface_commands: DeferredQueue<SurfaceCommand>,
    /// A shape waiting for the loop to hand it to the compositor. Being empty
    /// *is* the "nothing to sync" state — there is no dirty flag beside any of
    /// these four to keep in step with the value.
    pub(crate) outgoing_cursor: DeferredSlot<CursorIcon>,
    /// A copy waiting for the loop to hand it to the compositor.
    pub(crate) outgoing_clipboard: DeferredSlot<String>,
    /// The primary-selection counterpart of `outgoing_clipboard`.
    pub(crate) outgoing_primary: DeferredSlot<String>,
    /// A focus request from application code, which has no tree, parked until
    /// one is laid out. One slot, not a queue: two requests in a frame are two
    /// answers to "where should the keyboard be", and the last one is meant.
    pub(crate) pending_focus: RefCell<Option<WidgetRef>>,
    /// A lock or unlock asked for by application code, waiting for the loop.
    pub(crate) lock_request: DeferredSlot<LockRequest>,

    // --- What the platform last said, and what we last told it -------------
    /// The last shape `set_cursor` was asked for, so asking again for the same
    /// one sends nothing; widget code that calls it has no platform.
    pub(crate) current_cursor: Cell<CursorIcon>,
    /// What a copy in a handler put there, readable by a paste in another.
    pub(crate) clipboard: RefCell<Option<String>>,
    /// The compositor's selection, prefetched so a paste in a handler can
    /// answer synchronously.
    pub(crate) system_clipboard: RefCell<Option<String>>,
    /// The primary-selection counterpart of `clipboard`.
    pub(crate) primary: RefCell<Option<String>>,
    /// The primary-selection counterpart of `system_clipboard`.
    pub(crate) system_primary: RefCell<Option<String>>,
    /// The lock-screen factory and each output's lock surface while locked.
    pub(crate) lock: RefCell<LockData>,
    /// The popups that are still open. A `PopupHandle` is `Copy` and outlives
    /// its popup, so the registry is the only truth about whether it is.
    pub(crate) live_popups: RefCell<FxHashSet<SurfaceId>>,
    /// Which widget each `WidgetRef` points at: application code names a
    /// widget with one before any `WidgetId` exists.
    pub(crate) widget_refs: RefCell<FxHashMap<WidgetId, WidgetRef>>,

    // --- Fonts -------------------------------------------------------------
    /// One shaping cache for every `layout` that measures text, none of which
    /// is handed one. Built on first use — see `with_measurer`.
    pub(crate) text_measurer: RefCell<Option<TextMeasurer>>,
    /// Read by every text widget at construction, before any tree or surface
    /// exists.
    pub(crate) default_font_family: RefCell<FontFamily>,
    /// Font bytes loaded before any renderer exists, handed to each font
    /// system when it is built.
    pub(crate) custom_fonts: RefCell<Vec<Arc<Vec<u8>>>>,
    /// Which of those were already loaded, so loading twice is idempotent.
    pub(crate) custom_font_hashes: RefCell<FxHashSet<u64>>,
    /// Whether a font system already took the list, so a late load can say it
    /// came too late.
    pub(crate) fonts_consumed: Cell<bool>,
}

/// Forget everything this `App` put here, so the next one on this thread
/// starts clean.
///
/// Destructured rather than written as a list of statements: a field left out
/// below is an unused binding, and one added above and not bound at all is a
/// missing-field error. Both are what `-D warnings` and the compiler catch,
/// and neither is what the hand-written list this replaces could catch.
pub(crate) fn reset() {
    with_app_state(|app| {
        let AppState {
            pending_jobs,
            scheduled_jobs,
            dirty_segments,
            surface_commands,
            outgoing_cursor,
            outgoing_clipboard,
            outgoing_primary,
            pending_focus,
            lock_request,
            current_cursor,
            clipboard,
            system_clipboard,
            primary,
            system_primary,
            lock,
            live_popups,
            widget_refs,
            text_measurer,
            default_font_family,
            custom_fonts,
            custom_font_hashes,
            fonts_consumed,
        } = app;

        // `take` rather than a value per line: the struct derives `Default`,
        // so what a field starts an `App` with and what it is given back here
        // are the same expression, and neither can drift from the other.
        pending_jobs.take();
        scheduled_jobs.take();
        dirty_segments.take();
        surface_commands.clear();
        outgoing_cursor.clear();
        outgoing_clipboard.clear();
        outgoing_primary.clear();
        pending_focus.take();
        lock_request.clear();

        current_cursor.take();
        clipboard.take();
        system_clipboard.take();
        primary.take();
        system_primary.take();
        lock.take();
        live_popups.take();
        widget_refs.take();

        text_measurer.take();
        default_font_family.take();
        custom_fonts.take();
        custom_font_hashes.take();
        fonts_consumed.take();
    });
}
