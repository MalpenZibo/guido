//! The reactive runtime's own bookkeeping, in one place.
//!
//! The twin of [`AppState`](crate::app_state::AppState), and the same shape
//! for the same reasons: these were twelve `thread_local!` cells, and
//! `reset_reactive` forgot them by calling six functions in a row, each of
//! them a hand-written list. [`reset`] destructures the struct instead, so a
//! field added here and forgotten there is a compile error.
//!
//! The line between the two structs is what the state is *for*. This side is
//! the machinery that makes a `move ||` closure reactive at all: the arena the
//! signals live in, the owners that dispose them, who is reading right now,
//! and who has to be told when a value changes. The other side is what the
//! running application has queued, mirrored or declared. Neither is going
//! away: implicit tracking has to know who is reading, which is what floem
//! and leptos both keep a runtime for, and #372 groups these cells rather
//! than removing them.
//!
//! **The fields keep their own cells; the struct is not itself a `RefCell`.**
//! This is the half where that matters most — running an effect takes the
//! callback out of `runtime` with no borrow held, precisely so that writes and
//! signal creation inside it can borrow `storage` and `owners`. One borrow
//! over the whole struct would make every one of those a panic.
//!
//! What stays out of it: the diagnostics, which are debug-build counters that
//! outlive an `App` on purpose, and the background-write queue, which is a
//! `static` rather than a thread's, because the thread that queues a write is
//! the one this state cannot reach.

use std::any::TypeId;
use std::cell::{Cell, RefCell};

use rustc_hash::FxHashMap;

use super::invalidation::{SignalTrackingContext, SubscriberRegistry};
use super::owner::{OwnerArena, OwnerId};
use super::runtime::{EffectId, EffectReads, Runtime, SignalId};
use super::storage::SignalStorage;
use crate::deferred::DeferredQueue;

thread_local! {
    static REACTIVE: ReactiveState = ReactiveState::default();
}

/// Reach the reactive runtime. Every one of the fields below is read and
/// written through here and nowhere else.
pub(crate) fn with_reactive<R>(f: impl FnOnce(&ReactiveState) -> R) -> R {
    REACTIVE.with(f)
}

/// The reactive system's state, for as long as one `App` lives on this thread.
#[derive(Default)]
pub(crate) struct ReactiveState {
    // --- Who owns what, and who is reading ---------------------------------
    /// The owner tree that disposal walks, reached from handles that carry
    /// only an id.
    pub(crate) owners: RefCell<OwnerArena>,
    /// Which scope a signal created now belongs to: `create_signal` takes no
    /// scope argument. A `Cell`, because an `OwnerId` is `Copy`, nothing holds
    /// a borrow of it across a call, and every derived read swaps it.
    pub(crate) current_owner: Cell<Option<OwnerId>>,
    /// The application's own scope, reachable from any depth for state with
    /// one instance per application.
    pub(crate) root_owner: Cell<Option<OwnerId>>,
    /// Disposal is deferred to the loop, and the code that asks for it holds
    /// no loop.
    pub(crate) pending_disposals: DeferredQueue<OwnerId>,
    /// Which widget and job a read belongs to: a read inside `layout` or
    /// `paint` has no argument naming the widget. A stack, because the scopes
    /// nest and the innermost wins.
    pub(crate) tracking_context: RefCell<Vec<SignalTrackingContext>>,

    // --- The values, and who has to hear about them ------------------------
    /// Signal values, reached from `Copy` handles that carry only an index.
    pub(crate) storage: RefCell<SignalStorage>,
    /// Effects and their subscriptions: a `set` anywhere must reach them, and
    /// a signal handle is `Copy` with nothing to point through.
    pub(crate) runtime: RefCell<Runtime>,
    /// Reads made while an effect runs, buffered because `runtime` is already
    /// borrowed then.
    pub(crate) effect_tracking: RefCell<Vec<(EffectId, EffectReads)>>,
    /// Signal-to-widget subscriptions, written by a read and consumed by a
    /// write that share no argument. A `RefCell` rather than a lock because
    /// all access is on the main thread: a background write goes through
    /// `queue_bg_write` and is executed by `flush_bg_writes`, which is not.
    pub(crate) subscribers: RefCell<SubscriberRegistry>,
    /// Which signal each `GlobalSignal` resolved to, keyed by the address of
    /// its `static` and tagged with the type it was declared as: a `static`
    /// cannot hold a thread's signal.
    pub(crate) globals: RefCell<FxHashMap<usize, (SignalId, TypeId)>>,

    // --- Guards that nest across calls sharing no argument -----------------
    /// `batch()` nesting: above zero, a write collects pending effects and
    /// leaves the flush to whoever opened the batch.
    pub(crate) batch_depth: Cell<u32>,
    /// Reentrancy guard for a write made inside an effect, which has no handle
    /// to the flush it is inside. It bounds stack depth: an effect chain
    /// becomes iterations of the outermost flush loop rather than recursion.
    pub(crate) flushing: Cell<bool>,
}

/// Forget everything this `App`'s reactive system holds, so the next one on
/// this thread starts clean.
///
/// Read `src/app_state.rs`'s `reset` for why this is a destructure and a `take`
/// each: the same two guarantees, from the same two compiler errors.
///
/// The order *is* load-bearing, in one place. Three of these hold values the
/// application wrote — a signal's `T`, an effect's callback, an owner's
/// cleanups — so dropping them runs whatever `Drop` those have, and this
/// crate's own `OwnerGuard` and `OwnedWidget` dispose an owner from theirs.
/// A disposal queued that way has to be forgotten *after* it can be queued,
/// which is why `pending_disposals` is cleared below the three and not above
/// them: owner ids restart from zero with the arena, so a disposal left over
/// from the `App` that is going away names a live owner in the one that comes
/// next — the same hazard `App::drop` clears the tree for, where a late
/// Unregister job would destroy a new widget that reused its id.
pub(crate) fn reset() {
    with_reactive(|reactive| {
        let ReactiveState {
            owners,
            current_owner,
            root_owner,
            pending_disposals,
            tracking_context,
            storage,
            runtime,
            effect_tracking,
            subscribers,
            globals,
            batch_depth,
            flushing,
        } = reactive;

        // What the application wrote, first: dropping these is what may queue
        // one last disposal.
        storage.take();
        runtime.take();
        owners.take();

        // Then the bookkeeping about them, including anything the drops above
        // asked for.
        pending_disposals.clear();
        subscribers.take();
        globals.take();
        effect_tracking.take();
        tracking_context.take();

        current_owner.take();
        root_owner.take();
        batch_depth.take();
        flushing.take();
    });
}
