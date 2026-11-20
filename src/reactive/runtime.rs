//! Thread-local reactive runtime for effect execution and dependency tracking.
//!
//! The runtime manages the relationship between signals and effects, tracking which
//! effects depend on which signals and re-running effects when their dependencies change.
//!
//! ## Thread Safety
//!
//! The runtime is thread-local, meaning each thread has its own isolated runtime.
//! Signals can be updated from any thread (via the global storage), but effects
//! only execute on the main thread where they were created.
//!
//! ## Dependency Tracking
//!
//! When an effect runs, the runtime tracks which signals it reads. These become
//! the effect's dependencies. When any dependency changes, the effect is scheduled
//! to re-run.
//!
//! ## Usage
//!
//! Most code should use the higher-level APIs in the `reactive` module rather than
//! interacting with the runtime directly.

use std::cell::Cell;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use smallvec::SmallVec;

use super::invalidation::suspend_widget_tracking;
use super::owner::{OwnerId, empty_owner, under_owner};
use super::state::with_reactive;

/// Buffered signal reads for an effect. Most effects read 1–4 signals,
/// so SmallVec avoids heap allocation in the common case.
pub(crate) type EffectReads = SmallVec<[SignalId; 4]>;

/// The reactive runtime a background write is for.
///
/// A signal id names a slot in one thread's storage, and another runtime in
/// the process hands out the same ids, so a queued write carries the runtime
/// it belongs to as well. A runtime opens its route the first time it hands
/// out a writer and closes it when it is reset or its thread ends; a reset
/// opens a new one, so a writer left behind by the last application finds its
/// route closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WriteRoute(u64);

/// Where the next route's number comes from.
static NEXT_WRITE_ROUTE: AtomicU64 = AtomicU64::new(0);

type QueuedWrite = Box<dyn FnOnce() + Send>;

/// The queued writes of each open route. A write for a route that is not here
/// is dropped as it is queued.
static WRITE_QUEUES: Mutex<Vec<(WriteRoute, Vec<QueuedWrite>)>> = Mutex::new(Vec::new());

/// A runtime's own route, open or not yet.
#[derive(Default)]
pub(crate) struct OwnRoute(Cell<Option<WriteRoute>>);

impl OwnRoute {
    fn open(&self) -> WriteRoute {
        if let Some(route) = self.0.get() {
            return route;
        }
        let route = WriteRoute(NEXT_WRITE_ROUTE.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut queues) = WRITE_QUEUES.lock() {
            queues.push((route, Vec::new()));
        }
        self.0.set(Some(route));
        route
    }

    fn current(&self) -> Option<WriteRoute> {
        self.0.get()
    }

    /// Drop the writes queued on this route, and those queued on it later.
    pub(crate) fn close(&self) {
        let Some(route) = self.0.take() else { return };
        let closed = WRITE_QUEUES.lock().ok().and_then(|mut queues| {
            let at = queues.iter().position(|(open, _)| *open == route)?;
            Some(queues.swap_remove(at))
        });
        // Dropped with the lock released: a write's captures run their own
        // `Drop`, which may queue another.
        drop(closed);
    }
}

impl Drop for OwnRoute {
    fn drop(&mut self) {
        self.close();
    }
}

/// Unique identifier for a signal.
///
/// Generational: storage recycles slot indices and bumps the generation on
/// every reuse. Signal handles are `Copy` and freely captured in closures, so
/// without the generation a stale handle would silently read/write whatever
/// unrelated signal later recycled the same slot. With it, stale handles are
/// reliably detected forever.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SignalId {
    index: u32,
    generation: u32,
}

impl SignalId {
    pub(crate) fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Slot index for direct Vec indexing in storage and subscriber registries.
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    #[inline]
    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

impl std::fmt::Display for SignalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// Unique identifier for an effect. Generational, like [`SignalId`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EffectId {
    index: u32,
    generation: u32,
}

impl EffectId {
    #[inline]
    fn index(self) -> usize {
        self.index as usize
    }
}

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// Insert into a Vec only if not already present (dedup).
fn vec_insert<T: PartialEq>(vec: &mut Vec<T>, value: T) {
    if !vec.contains(&value) {
        vec.push(value);
    }
}

/// Remove first occurrence of a value from a Vec using swap_remove (O(1) unstable).
fn vec_remove<T: PartialEq>(vec: &mut Vec<T>, value: &T) {
    if let Some(pos) = vec.iter().position(|x| x == value) {
        vec.swap_remove(pos);
    }
}

/// Whether an effect (or memo) is currently executing and collecting reads.
///
/// Behind `debug_assertions` like its only caller, the non-reactive-read
/// diagnostic; without that, release builds warn about it.
#[cfg(debug_assertions)]
pub(crate) fn effect_tracking_active() -> bool {
    with_reactive(|reactive| {
        reactive
            .effect_tracking
            .try_borrow()
            .map(|stack| !stack.is_empty())
            .unwrap_or(true)
    })
}

/// Buffer a signal read for the currently executing effect.
/// Called from tracked_get/tracked_with. During effect execution, the Runtime
/// RefCell is already borrowed, so reads are buffered here and applied after.
pub fn record_effect_read(signal_id: SignalId) {
    with_reactive(|reactive| {
        if let Ok(mut s) = reactive.effect_tracking.try_borrow_mut()
            && let Some(entry) = s.last_mut()
            && !entry.1.contains(&signal_id)
        {
            entry.1.push(signal_id);
        }
    });
}

/// Suspend effect-level read tracking during the given closure.
///
/// Reads inside the closure are attributed to no effect. The counterpart of
/// [`suspend_widget_tracking`](super::invalidation::suspend_widget_tracking):
/// together they make a computation invisible to whatever is tracking around
/// it, which is what an untracked read of a derived signal needs.
pub(crate) fn suspend_effect_tracking<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved: Vec<_> =
        with_reactive(|reactive| reactive.effect_tracking.borrow_mut().drain(..).collect());
    // Restore on unwind too: losing the saved stack would silently drop the
    // dependencies of every effect currently on the stack.
    let _guard = super::guard::defer(move || {
        with_reactive(|reactive| *reactive.effect_tracking.borrow_mut() = saved);
    });
    f()
}

/// This thread's route, opened if it is not yet: what a `WriteSignal` is
/// stamped with when it is made.
pub(crate) fn write_route() -> WriteRoute {
    with_reactive(|reactive| reactive.write_route.open())
}

/// Whether `route` is this thread's runtime.
pub(crate) fn is_own_route(route: WriteRoute) -> bool {
    with_reactive(|reactive| reactive.write_route.current() == Some(route))
}

/// Queue a write for the runtime `route` names, to run on its thread at its
/// next flush. A route that has closed takes nothing.
/// Used by `WriteSignal::set()`/`update()` from background threads.
pub(crate) fn queue_bg_write(route: WriteRoute, f: impl FnOnce() + Send + 'static) {
    {
        let Ok(mut queues) = WRITE_QUEUES.lock() else {
            return;
        };
        let Some((_, queue)) = queues.iter_mut().find(|(open, _)| *open == route) else {
            log::debug!("dropping a background write for a runtime that has gone");
            return;
        };
        queue.push(Box::new(f));
    }
    // Queueing and waking are one gesture here too — this is the cross-thread
    // one, so the wakeup goes through the calloop ingress channel rather than
    // the ping: its readiness guarantees the loop wakes no matter where in its
    // iteration the write landed.
    crate::ingress::notify(crate::ingress::IngressMessage::BgWritesQueued);
}

/// Run the background writes queued for this thread's runtime, and only
/// those. Called from the main event loop before processing widget jobs.
pub fn flush_bg_writes() {
    let Some(route) = with_reactive(|reactive| reactive.write_route.current()) else {
        return;
    };
    loop {
        let writes = WRITE_QUEUES.lock().ok().and_then(|mut queues| {
            let (_, queue) = queues.iter_mut().find(|(open, _)| *open == route)?;
            (!queue.is_empty()).then(|| std::mem::take(queue))
        });
        let Some(writes) = writes else { return };
        log::trace!("flush_bg_writes: processed {} queued writes", writes.len());
        for write in writes {
            write();
        }
    }
}

/// Lifecycle state of an effect slot.
///
/// `Running` exists because effect callbacks execute *outside* the runtime
/// borrow (see [`run_effect_by_id`]): the callback is taken out of the slot
/// while it runs, and disposal during execution must be remembered so
/// [`Runtime::finish_effect`] can drop the callback instead of restoring it.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum EffectState {
    /// No live effect in this slot (never used, or disposed).
    #[default]
    Vacant,
    /// Live effect, callback stored in the slot.
    Idle,
    /// Live effect, callback currently executing (taken out of the slot).
    Running,
    /// Disposed while its callback was executing; finalized in `finish_effect`.
    DisposedWhileRunning,
}

/// An effect's callback, taken out of its slot to be run, and the scope to run
/// it in.
type EffectRun = (Box<dyn FnMut()>, OwnerId);

/// Storage slot for one effect. The generation survives disposal so recycled
/// indices can be told apart from their previous occupants.
struct EffectSlot {
    callback: Option<Box<dyn FnMut()>>,
    /// The effect's own scope, a child of the one it was created in. Every run
    /// happens under it, so a context read in the body answers the same on each
    /// (#337), and it is emptied before each run, so what a run made lasts
    /// until the next.
    scope: OwnerId,
    /// Signals this effect reads. Vec with dedup — most effects depend on
    /// 1–3 signals, making linear scan faster than HashSet.
    dependencies: Vec<SignalId>,
    generation: u32,
    state: EffectState,
    /// The effect's scope is paused (see `owner::pause_owner`): it keeps its
    /// dependencies but does not run.
    paused: bool,
    /// A dependency changed while it was paused; it runs once on resume.
    missed: bool,
    /// The effect is a memo's: it is queued with the memos, which run first.
    memo: bool,
}

/// Which effects read each signal.
///
/// Most signals are read by no effect at all, so a signal holds a four-byte
/// handle here and nothing else until one does: `0` for none, otherwise one
/// more than the index of its list in a pool. A hash map would cost no handle
/// either, but an effect's run takes its subscriptions out and puts them back,
/// and on that path a handle is an index where a map is a hash.
///
/// A list emptied by an effect's run keeps its handle and its allocation, so
/// the run that refills it allocates nothing — and so does one emptied because
/// its effect stopped reading the signal, until the signal goes. It goes back
/// to the pool when its signal is disposed, or when disposing an effect leaves
/// it empty.
#[derive(Default)]
struct EffectSubscribers {
    /// By `SignalId::index`; as long as the highest index ever subscribed to.
    handles: Vec<u32>,
    lists: Vec<Vec<EffectId>>,
    /// Pool indices whose lists belong to no signal.
    free: Vec<u32>,
}

impl EffectSubscribers {
    fn handle(&self, signal_id: SignalId) -> Option<usize> {
        match self.handles.get(signal_id.index()) {
            Some(&handle) if handle != 0 => Some(handle as usize - 1),
            _ => None,
        }
    }

    fn get(&self, signal_id: SignalId) -> Option<&Vec<EffectId>> {
        self.handle(signal_id).map(|list| &self.lists[list])
    }

    fn get_mut(&mut self, signal_id: SignalId) -> Option<&mut Vec<EffectId>> {
        self.handle(signal_id).map(|list| &mut self.lists[list])
    }

    /// The signal's list, given one from the pool if it has none.
    fn get_or_insert(&mut self, signal_id: SignalId) -> &mut Vec<EffectId> {
        let list = match self.handle(signal_id) {
            Some(list) => list,
            None => {
                let list = self.free.pop().unwrap_or_else(|| {
                    self.lists.push(Vec::new());
                    self.lists.len() as u32 - 1
                });
                if signal_id.index() >= self.handles.len() {
                    self.handles.resize(signal_id.index() + 1, 0);
                }
                self.handles[signal_id.index()] = list + 1;
                list as usize
            }
        };
        &mut self.lists[list]
    }

    /// Take the signal's list, and give its place in the pool back.
    fn remove(&mut self, signal_id: SignalId) -> Option<Vec<EffectId>> {
        let list = self.handle(signal_id)?;
        self.handles[signal_id.index()] = 0;
        self.free.push(list as u32);
        Some(std::mem::take(&mut self.lists[list]))
    }

    /// Take `effect_id` out of the signal's list, and give the list back to the
    /// pool if that leaves it empty.
    fn unsubscribe(&mut self, signal_id: SignalId, effect_id: EffectId) {
        if let Some(subs) = self.get_mut(signal_id) {
            vec_remove(subs, &effect_id);
            if subs.is_empty() {
                self.remove(signal_id);
            }
        }
    }

    /// How many signals hold a list.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.lists.len() - self.free.len()
    }
}

/// Effects waiting to run, in notification order and deduplicated — most
/// frames have 0–5.
///
/// Memos wait apart from plain effects, and a flush takes every memo before
/// any plain effect. A memo that changes queues the memos reading it behind
/// those still pending, so an effect never runs before a memo it reads has
/// caught up with the write that queued both.
///
/// Among memos the order is still the order they were notified in. A memo that
/// reads a signal and another memo of that signal can run once with the new
/// signal and the old memo, then again; the effects reading it run after both
/// and see only the second.
#[derive(Default)]
struct PendingEffects {
    memos: VecDeque<EffectId>,
    effects: VecDeque<EffectId>,
}

impl PendingEffects {
    fn queue(&mut self, memo: bool) -> &mut VecDeque<EffectId> {
        if memo {
            &mut self.memos
        } else {
            &mut self.effects
        }
    }

    /// Queue an effect, unless it is queued already.
    fn push(&mut self, effect_id: EffectId, memo: bool) {
        let queue = self.queue(memo);
        if !queue.contains(&effect_id) {
            queue.push_back(effect_id);
        }
    }

    /// The next effect to run: a memo while any is queued.
    fn pop(&mut self) -> Option<EffectId> {
        self.memos.pop_front().or_else(|| self.effects.pop_front())
    }

    fn remove(&mut self, effect_id: EffectId, memo: bool) {
        let queue = self.queue(memo);
        if let Some(pos) = queue.iter().position(|e| *e == effect_id) {
            queue.remove(pos);
        }
    }
}

#[derive(Default)]
pub struct Runtime {
    pending: PendingEffects,
    /// Effect slots, indexed by `EffectId::index`.
    effects: Vec<EffectSlot>,
    /// Vacant effect slot indices available for reuse.
    free_effect_indices: Vec<u32>,
    /// Per-signal subscribers (which effects track it). Each list is a Vec
    /// with dedup — most signals have 1–5 subscribers.
    signal_subscribers: EffectSubscribers,
}

impl Runtime {
    /// Get a generation-validated mutable reference to an effect slot.
    /// Returns `None` for stale ids (slot recycled since the id was issued).
    fn effect_slot_mut(&mut self, id: EffectId) -> Option<&mut EffectSlot> {
        self.effects
            .get_mut(id.index())
            .filter(|slot| slot.generation == id.generation)
    }

    /// Register a signal for subscriber tracking (called when signal is created)
    pub fn register_signal(&mut self, id: SignalId) {
        // A signal index only recycles after the previous occupant was
        // disposed, but an effect registers its reads when its run ends: one
        // that read a signal and then disposed it subscribed to the slot after
        // the disposal had cleared it. That subscription is stale.
        self.signal_subscribers.remove(id);
    }

    /// Remove all effect subscriptions for a signal being disposed, both from
    /// the signal's subscriber list and from each subscriber's dependency list.
    /// Without this, an effect could stay subscribed to a recycled slot index
    /// and get spuriously re-run by an unrelated future signal.
    pub fn dispose_signal_subscriptions(&mut self, id: SignalId) {
        let Some(subs) = self.signal_subscribers.remove(id) else {
            return;
        };
        for effect_id in subs {
            if let Some(slot) = self.effect_slot_mut(effect_id) {
                vec_remove(&mut slot.dependencies, &id);
            }
        }
    }

    /// `memo` says the effect is a memo's (see `PendingEffects`).
    pub fn allocate_effect(
        &mut self,
        callback: Box<dyn FnMut()>,
        scope: OwnerId,
        memo: bool,
    ) -> EffectId {
        let paused = super::owner::current_owner_is_paused();
        // Reuse a freed slot if available, bumping its generation so stale
        // ids for the previous occupant can never act on this effect
        if let Some(index) = self.free_effect_indices.pop() {
            let slot = &mut self.effects[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.callback = Some(callback);
            slot.scope = scope;
            slot.dependencies.clear();
            slot.state = EffectState::Idle;
            slot.paused = paused;
            slot.missed = false;
            slot.memo = memo;
            return EffectId {
                index,
                generation: slot.generation,
            };
        }
        // Otherwise allocate new
        let index = self.effects.len() as u32;
        self.effects.push(EffectSlot {
            callback: Some(callback),
            scope,
            dependencies: Vec::new(),
            generation: 0,
            state: EffectState::Idle,
            paused,
            missed: false,
            memo,
        });
        EffectId {
            index,
            generation: 0,
        }
    }

    /// Queue all effects subscribed to a signal. Does NOT run them — the
    /// caller decides when to flush (immediately, or at batch end).
    fn enqueue_subscribers(&mut self, signal_id: SignalId) {
        let Self {
            effects,
            signal_subscribers,
            pending,
            ..
        } = self;
        let Some(subs) = signal_subscribers.get(signal_id) else {
            return;
        };
        for &effect_id in subs {
            let memo = effects[effect_id.index()].memo;
            pending.push(effect_id, memo);
        }
    }

    /// Pause or resume `effects`. Resuming queues each one that missed a run
    /// while it was paused.
    pub(crate) fn set_effects_paused(&mut self, effects: &[EffectId], paused: bool) {
        for &effect_id in effects {
            if let Some(slot) = self.effect_slot_mut(effect_id) {
                slot.paused = paused;
                if !paused && std::mem::take(&mut slot.missed) {
                    let memo = slot.memo;
                    self.pending.push(effect_id, memo);
                }
            }
        }
    }

    /// Phase 1 of effect execution (under the runtime borrow): validate the
    /// id, clear old dependencies, and hand the callback out so it can run
    /// WITHOUT the runtime borrowed. Returns `None` for stale/disposed ids
    /// or if the effect is already running (re-entrant trigger).
    ///
    /// A paused effect does not run either: it keeps its dependencies, and is
    /// marked as having missed the run.
    fn begin_effect(&mut self, effect_id: EffectId) -> Option<EffectRun> {
        // `effect_slot_mut`'s lookup, over the field, so the slot's dependency
        // list can be walked while the index is changed.
        let Self {
            effects,
            signal_subscribers,
            ..
        } = self;
        let slot = effects
            .get_mut(effect_id.index())
            .filter(|slot| slot.generation == effect_id.generation)?;
        if slot.state != EffectState::Idle {
            return None;
        }
        if slot.paused {
            slot.missed = true;
            return None;
        }
        let callback = slot.callback.take()?;
        let scope = slot.scope;
        slot.state = EffectState::Running;

        // Clear old dependencies; they are re-established from this run's
        // reads. Every list is emptied in place — the signals' stay with their
        // signals — so putting the same subscriptions back allocates nothing.
        for &signal_id in &slot.dependencies {
            if let Some(subs) = signal_subscribers.get_mut(signal_id) {
                vec_remove(subs, &effect_id);
            }
        }
        slot.dependencies.clear();
        Some((callback, scope))
    }

    /// Phase 3 of effect execution (under the runtime borrow): restore the
    /// callback and register the reads buffered during the run as
    /// dependencies. If the effect was disposed while running, finalize the
    /// disposal instead.
    fn finish_effect(
        &mut self,
        effect_id: EffectId,
        callback: Box<dyn FnMut()>,
        reads: EffectReads,
    ) {
        let Some(slot) = self.effect_slot_mut(effect_id) else {
            return; // Cannot happen (slot recycles only after free), but stay safe
        };
        match slot.state {
            EffectState::Running => {
                slot.callback = Some(callback);
                slot.state = EffectState::Idle;
                for signal_id in reads {
                    vec_insert(self.signal_subscribers.get_or_insert(signal_id), effect_id);
                    if let Some(slot) = self.effect_slot_mut(effect_id) {
                        vec_insert(&mut slot.dependencies, signal_id);
                    }
                }
            }
            EffectState::DisposedWhileRunning => {
                // The callback disposed its own effect (owner disposal from
                // within). Drop the callback and complete the disposal.
                drop(callback);
                slot.state = EffectState::Vacant;
                self.free_effect_indices.push(effect_id.index);
            }
            EffectState::Vacant | EffectState::Idle => {
                // Unreachable by construction; drop the callback defensively.
                drop(callback);
            }
        }
    }

    /// The scope an effect belongs to, or `None` for a stale id or a slot that
    /// has been disposed.
    ///
    /// Only the ownership tests ask in this form: the running library gets the
    /// scope from [`begin_effect`](Self::begin_effect), which hands it out with
    /// the callback it is about to run.
    #[cfg(test)]
    pub(crate) fn effect_scope(&self, effect_id: EffectId) -> Option<OwnerId> {
        let slot = self.effects.get(effect_id.index as usize)?;
        if slot.generation != effect_id.generation || slot.state == EffectState::Vacant {
            return None;
        }
        Some(slot.scope)
    }

    pub fn dispose_effect(&mut self, effect_id: EffectId) {
        // Stale id: the slot was already recycled, nothing to dispose
        let Some(slot) = self.effect_slot_mut(effect_id) else {
            return;
        };
        let memo = slot.memo;
        match slot.state {
            EffectState::Idle => {
                slot.callback = None;
                slot.state = EffectState::Vacant;
                let deps = std::mem::take(&mut slot.dependencies);
                for signal_id in deps {
                    self.signal_subscribers.unsubscribe(signal_id, effect_id);
                }
                self.pending.remove(effect_id, memo);
                self.free_effect_indices.push(effect_id.index);
            }
            EffectState::Running => {
                // The callback is currently executing outside the borrow.
                // Mark for finalization in finish_effect (which pushes the
                // free-list entry); clear what can be cleared now.
                slot.state = EffectState::DisposedWhileRunning;
                let deps = std::mem::take(&mut slot.dependencies);
                for signal_id in deps {
                    self.signal_subscribers.unsubscribe(signal_id, effect_id);
                }
                self.pending.remove(effect_id, memo);
            }
            EffectState::Vacant | EffectState::DisposedWhileRunning => {
                // Already disposed
            }
        }
    }
}

/// Run one effect: take its callback out of the runtime, execute it with NO
/// runtime borrow held (so writes and signal creation inside the callback
/// work), then restore it and register the tracked reads.
pub(crate) fn run_effect_by_id(effect_id: EffectId) {
    // Phase 1 (borrow): take the callback out
    let Some((mut callback, scope)) = with_runtime(|rt| rt.begin_effect(effect_id)) else {
        return;
    };

    // Phase 2 (no borrow): run the callback with read-tracking buffered.
    // A panicking callback must still reach phase 3: without it the slot
    // would be stuck in Running with its callback lost, and the tracking
    // frame would leak. Catch, restore state, then propagate the panic.
    with_reactive(|reactive| {
        reactive
            .effect_tracking
            .borrow_mut()
            .push((effect_id, EffectReads::new()));
    });
    // What the last run made goes first, then the body runs under the
    // effect's own scope rather than the one the flush happens to be under.
    let panic_payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        empty_owner(scope);
        under_owner(scope, || suspend_widget_tracking(&mut *callback));
    }))
    .err();
    let reads = with_reactive(|reactive| reactive.effect_tracking.borrow_mut().pop())
        .map(|(_eid, reads)| reads)
        .unwrap_or_default();

    // Phase 3 (borrow): restore the callback and register dependencies
    // (possibly partial if the callback panicked mid-run)
    with_runtime(|rt| rt.finish_effect(effect_id, callback, reads));

    if let Some(payload) = panic_payload {
        std::panic::resume_unwind(payload);
    }
}

/// Drain the pending-effect queue, running each effect. No-op when already
/// flushing higher up the stack (the outer loop drains everything).
pub(crate) fn flush_pending_effects() {
    if with_reactive(|reactive| reactive.flushing.replace(true)) {
        return;
    }
    // Reset the flag even if an effect callback panics; otherwise every
    // future flush would silently no-op and all effects would stop running.
    struct FlushGuard;
    impl Drop for FlushGuard {
        fn drop(&mut self) {
            with_reactive(|reactive| reactive.flushing.set(false));
        }
    }
    let _guard = FlushGuard;

    while let Some(effect_id) = with_runtime(|rt| rt.pending.pop()) {
        run_effect_by_id(effect_id);
    }
}

/// Notify subscribers that a signal changed. Effects run immediately unless
/// inside a `batch()` (deferred to batch end) or already inside an effect
/// flush (picked up by the outer loop).
///
/// Safe to call from anywhere on the main thread, including from within
/// effect callbacks — the runtime borrow is never held across user code.
pub(crate) fn notify_write(signal_id: SignalId) {
    with_runtime(|rt| rt.enqueue_subscribers(signal_id));
    flush_unless_batching();
}

/// Run what is queued now, unless a `batch()` will run it when it ends.
pub(crate) fn flush_unless_batching() {
    let batching = with_reactive(|reactive| reactive.batch_depth.get() > 0);
    if !batching {
        flush_pending_effects();
    }
}

pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut Runtime) -> R,
{
    with_reactive(|reactive| f(&mut reactive.runtime.borrow_mut()))
}

/// Batch multiple signal writes so that shared effects run only once.
///
/// Inside the closure, `notify_write()` collects pending effects but defers
/// `flush_effects()` until the batch completes. Widget invalidation (paint/layout
/// jobs) is NOT batched — widgets still get per-field jobs immediately.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    with_reactive(|reactive| reactive.batch_depth.set(reactive.batch_depth.get() + 1));
    // Restore the depth even if `f` panics: a caught panic must not leave
    // the batch depth stuck > 0 (which would stop every effect in the app from
    // ever flushing again). Effects queued by the failed batch stay pending
    // and run on the next notify — we deliberately don't flush during unwind.
    let guard = super::guard::defer(|| {
        with_reactive(|reactive| reactive.batch_depth.set(reactive.batch_depth.get() - 1));
    });
    let result = f();
    drop(guard);
    flush_unless_batching();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{create_effect, create_signal};
    use std::cell::Cell;
    use std::rc::Rc;

    /// How many signals the effect index holds a list for.
    fn effect_index_entries() -> usize {
        with_runtime(|rt| rt.signal_subscribers.len())
    }

    /// A signal no effect reads costs the effect index no list: the thousand
    /// made here add none, and the one an effect reads adds exactly one.
    #[test]
    fn only_a_signal_an_effect_reads_has_a_list() {
        let before = effect_index_entries();
        let signals: Vec<_> = (0..1000).map(|_| create_signal(0u32)).collect();
        assert_eq!(effect_index_entries(), before, "nothing has read them");

        let last = *signals.last().unwrap();
        create_effect(move || {
            last.get();
        });
        assert_eq!(effect_index_entries(), before + 1, "one reader, one list");
    }

    /// A signal's list goes back to the pool when the signal goes, and when the
    /// last effect reading it does — not only when a slot is reused.
    #[test]
    fn a_list_goes_back_when_its_signal_or_its_last_reader_goes() {
        use crate::reactive::owner::{dispose_owner_now, with_owner};

        let before = effect_index_entries();
        let (signal, signal_scope) = with_owner(|| create_signal(0u32));
        let read = move || {
            signal.get();
        };

        let ((), reader) = with_owner(|| create_effect(read));
        assert_eq!(effect_index_entries(), before + 1, "the control: one list");
        dispose_owner_now(reader);
        assert_eq!(
            effect_index_entries(),
            before,
            "its only reader went, and the list with it"
        );

        let ((), reader) = with_owner(|| create_effect(read));
        assert_eq!(effect_index_entries(), before + 1);
        dispose_owner_now(signal_scope);
        assert_eq!(
            effect_index_entries(),
            before,
            "the signal went, and its list with it"
        );
        dispose_owner_now(reader);
    }

    /// An effect's reads are registered when its run ends, so a signal it read
    /// and then disposed in the same run is subscribed to after it is gone. The
    /// next signal to take its slot must not inherit that: writing it would
    /// re-run an effect that never read it.
    #[test]
    fn a_signal_disposed_by_the_run_that_read_it_leaves_its_slot_unsubscribed() {
        use crate::reactive::owner::{dispose_owner_now, with_owner};

        let (doomed, scope) = with_owner(|| create_signal(0u32));
        let runs = Rc::new(Cell::new(0));
        let counter = runs.clone();
        create_effect(move || {
            counter.set(counter.get() + 1);
            if counter.get() == 1 {
                doomed.get();
                dispose_owner_now(scope);
            }
        });
        assert_eq!(runs.get(), 1);

        let successor = create_signal(0u32);
        successor.set(1);
        assert_eq!(runs.get(), 1, "the effect never read the successor");
    }

    /// A panic caught inside batch() must not leave the batch depth stuck > 0 —
    /// that would silently stop every effect in the app from ever flushing.
    #[test]
    fn test_caught_panic_in_batch_does_not_wedge_effects() {
        let sig = create_signal(0);
        let observed = Rc::new(Cell::new(0));
        let o = observed.clone();
        create_effect(move || o.set(sig.get()));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            batch(|| {
                sig.set(1);
                panic!("boom");
            })
        }));
        assert!(result.is_err());
        assert_eq!(
            with_reactive(|reactive| reactive.batch_depth.get()),
            0,
            "batch depth must unwind"
        );

        // Effects must still flush on the next write
        sig.set(2);
        assert_eq!(observed.get(), 2);
    }

    /// A panicking effect callback must be restored into its slot so the
    /// effect stays alive, and the flush machinery must stay usable.
    #[test]
    fn test_panicking_effect_survives_and_reruns() {
        let sig = create_signal(0);
        let observed = Rc::new(Cell::new(0));
        let o = observed.clone();
        create_effect(move || {
            let v = sig.get();
            o.set(v);
            if v == 1 {
                panic!("effect panic");
            }
        });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sig.set(1)));
        assert!(result.is_err(), "panic must propagate out of set()");
        assert_eq!(observed.get(), 1);
        assert!(
            !with_reactive(|reactive| reactive.flushing.get()),
            "flush guard must have reset"
        );

        // The effect must still be alive, tracked, and re-runnable
        sig.set(2);
        assert_eq!(observed.get(), 2);
    }
}
