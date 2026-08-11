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

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use smallvec::SmallVec;

use super::invalidation::suspend_widget_tracking;

/// Buffered signal reads for an effect. Most effects read 1–4 signals,
/// so SmallVec avoids heap allocation in the common case.
type EffectReads = SmallVec<[SignalId; 4]>;

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::new());

    /// Stack of (effect_id, buffered_signal_reads) for tracking during effect execution.
    /// Needed because the Runtime RefCell is already borrowed when effects run.
    /// We buffer reads here and apply them after the callback returns.
    static EFFECT_TRACKING: RefCell<Vec<(EffectId, EffectReads)>> = const { RefCell::new(Vec::new()) };

    /// Nesting depth for `batch()`. When > 0, `notify_write()` collects pending
    /// effects but defers `flush_effects()` until the batch completes.
    static BATCH_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Epoch counter for write filtering. Incremented on each runtime reset (App restart).
/// Writes tagged with a stale epoch are silently discarded in `flush_bg_writes()`.
static WRITE_EPOCH: AtomicU64 = AtomicU64::new(0);

/// A queued background write: (epoch at queue time, closure to execute).
type EpochWrite = (u64, Box<dyn FnOnce() + Send>);

/// Background write queue: closures that perform signal writes, queued from bg threads.
/// Each entry is tagged with the epoch at queue time. Writes from a previous epoch
/// are discarded during flush.
static WRITE_QUEUE: Mutex<Vec<EpochWrite>> = Mutex::new(Vec::new());

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
pub(crate) fn effect_tracking_active() -> bool {
    EFFECT_TRACKING.with(|stack| stack.try_borrow().map(|s| !s.is_empty()).unwrap_or(true))
}

/// Buffer a signal read for the currently executing effect.
/// Called from tracked_get/tracked_with. During effect execution, the Runtime
/// RefCell is already borrowed, so reads are buffered here and applied after.
pub fn record_effect_read(signal_id: SignalId) {
    EFFECT_TRACKING.with(|stack| {
        if let Ok(mut s) = stack.try_borrow_mut()
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
/// it, which is what a memo's seed computation needs.
pub(crate) fn suspend_effect_tracking<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved: Vec<_> = EFFECT_TRACKING.with(|stack| stack.borrow_mut().drain(..).collect());
    // Restore on unwind too: losing the saved stack would silently drop the
    // dependencies of every effect currently on the stack.
    let _guard = super::guard::defer(move || {
        EFFECT_TRACKING.with(|stack| *stack.borrow_mut() = saved);
    });
    f()
}

/// Return the current write epoch. Captured by `WriteSignal` at creation
/// time so that writes queued after a restart carry the old epoch.
pub(crate) fn current_write_epoch() -> u64 {
    WRITE_EPOCH.load(Ordering::Acquire)
}

/// Queue a closure for execution on the main thread (next frame).
/// Used by `WriteSignal::set()`/`update()` from background threads.
///
/// The write is tagged with the caller-supplied epoch (captured when the
/// `WriteSignal` was created). If the runtime resets before this write is
/// flushed (e.g. App restart), the epoch will be stale and the write is
/// silently discarded.
pub fn queue_bg_write(epoch: u64, f: impl FnOnce() + Send + 'static) {
    if let Ok(mut q) = WRITE_QUEUE.lock() {
        q.push((epoch, Box::new(f)));
    }
    // Wake the event loop so flush_bg_writes() runs on the next frame.
    // Routed through the calloop ingress channel: its readiness guarantees
    // the loop wakes no matter where in its iteration the write landed.
    crate::ingress::notify(crate::ingress::IngressMessage::BgWritesQueued);
}

/// Drain queued background writes and execute them on the main thread.
/// Called from the main event loop before processing widget jobs.
///
/// Writes tagged with a stale epoch (from a previous App run) are silently
/// discarded. This prevents old service tasks from corrupting the new app's
/// reactive state after a restart.
pub fn flush_bg_writes() {
    let current_epoch = WRITE_EPOCH.load(Ordering::Acquire);
    loop {
        let writes: Vec<(u64, Box<dyn FnOnce() + Send>)> = match WRITE_QUEUE.lock() {
            Ok(mut q) if !q.is_empty() => q.drain(..).collect(),
            _ => return,
        };
        let mut executed = 0usize;
        let mut stale = 0usize;
        for (epoch, write_fn) in writes {
            if epoch == current_epoch {
                write_fn();
                executed += 1;
            } else {
                stale += 1;
            }
        }
        if stale > 0 {
            log::debug!(
                "flush_bg_writes: dropped {} stale writes (old epoch), executed {}",
                stale,
                executed
            );
        } else if executed > 0 {
            log::trace!("flush_bg_writes: processed {} queued writes", executed);
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

/// Storage slot for one effect. The generation survives disposal so recycled
/// indices can be told apart from their previous occupants.
#[derive(Default)]
struct EffectSlot {
    callback: Option<Box<dyn FnMut()>>,
    /// Signals this effect reads. Vec with dedup — most effects depend on
    /// 1–3 signals, making linear scan faster than HashSet.
    dependencies: Vec<SignalId>,
    generation: u32,
    state: EffectState,
}

#[derive(Default)]
pub struct Runtime {
    /// Pending effects to run, in notification order. Deduplicated —
    /// most frames have 0–5 pending effects.
    pending_effects: VecDeque<EffectId>,
    /// Effect slots, indexed by `EffectId::index`.
    effects: Vec<EffectSlot>,
    /// Vacant effect slot indices available for reuse.
    free_effect_indices: Vec<u32>,
    /// Per-signal subscribers (which effects track it), indexed by
    /// `SignalId::index`. Vec with dedup — most signals have 1–5 subscribers.
    signal_subscribers: Vec<Vec<EffectId>>,
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a generation-validated mutable reference to an effect slot.
    /// Returns `None` for stale ids (slot recycled since the id was issued).
    fn effect_slot_mut(&mut self, id: EffectId) -> Option<&mut EffectSlot> {
        self.effects
            .get_mut(id.index())
            .filter(|slot| slot.generation == id.generation)
    }

    /// Register a signal for subscriber tracking (called when signal is created)
    pub fn register_signal(&mut self, id: SignalId) {
        // Ensure we have space for subscribers
        while self.signal_subscribers.len() <= id.index() {
            self.signal_subscribers.push(Vec::new());
        }
        // A signal index only recycles after the previous occupant was
        // disposed, so any leftover subscribers at this index are stale.
        self.signal_subscribers[id.index()].clear();
    }

    /// Remove all effect subscriptions for a signal being disposed, both from
    /// the signal's subscriber list and from each subscriber's dependency list.
    /// Without this, an effect could stay subscribed to a recycled slot index
    /// and get spuriously re-run by an unrelated future signal.
    pub fn dispose_signal_subscriptions(&mut self, id: SignalId) {
        let Some(subs) = self.signal_subscribers.get_mut(id.index()) else {
            return;
        };
        for effect_id in std::mem::take(subs) {
            if let Some(slot) = self.effect_slot_mut(effect_id) {
                vec_remove(&mut slot.dependencies, &id);
            }
        }
    }

    pub fn allocate_effect(&mut self, callback: Box<dyn FnMut()>) -> EffectId {
        // Reuse a freed slot if available, bumping its generation so stale
        // ids for the previous occupant can never act on this effect
        if let Some(index) = self.free_effect_indices.pop() {
            let slot = &mut self.effects[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.callback = Some(callback);
            slot.dependencies.clear();
            slot.state = EffectState::Idle;
            return EffectId {
                index,
                generation: slot.generation,
            };
        }
        // Otherwise allocate new
        let index = self.effects.len() as u32;
        self.effects.push(EffectSlot {
            callback: Some(callback),
            dependencies: Vec::new(),
            generation: 0,
            state: EffectState::Idle,
        });
        EffectId {
            index,
            generation: 0,
        }
    }

    /// Queue all effects subscribed to a signal. Does NOT run them — the
    /// caller decides when to flush (immediately, or at batch end).
    fn enqueue_subscribers(&mut self, signal_id: SignalId) {
        let Some(subs) = self.signal_subscribers.get(signal_id.index()) else {
            return;
        };
        for i in 0..subs.len() {
            let effect_id = self.signal_subscribers[signal_id.index()][i];
            if !self.pending_effects.contains(&effect_id) {
                self.pending_effects.push_back(effect_id);
            }
        }
    }

    /// Pop the next pending effect, if any.
    fn pop_pending_effect(&mut self) -> Option<EffectId> {
        self.pending_effects.pop_front()
    }

    /// Phase 1 of effect execution (under the runtime borrow): validate the
    /// id, clear old dependencies, and hand the callback out so it can run
    /// WITHOUT the runtime borrowed. Returns `None` for stale/disposed ids
    /// or if the effect is already running (re-entrant trigger).
    fn begin_effect(&mut self, effect_id: EffectId) -> Option<Box<dyn FnMut()>> {
        let slot = self.effect_slot_mut(effect_id)?;
        if slot.state != EffectState::Idle {
            return None;
        }
        let callback = slot.callback.take()?;
        slot.state = EffectState::Running;

        // Clear old dependencies; they are re-established from this run's reads
        let old_deps = std::mem::take(&mut slot.dependencies);
        for signal_id in old_deps {
            if let Some(subs) = self.signal_subscribers.get_mut(signal_id.index()) {
                vec_remove(subs, &effect_id);
            }
        }
        Some(callback)
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
                    if signal_id.index() < self.signal_subscribers.len() {
                        vec_insert(&mut self.signal_subscribers[signal_id.index()], effect_id);
                        if let Some(slot) = self.effect_slot_mut(effect_id) {
                            vec_insert(&mut slot.dependencies, signal_id);
                        }
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

    pub fn dispose_effect(&mut self, effect_id: EffectId) {
        // Stale id: the slot was already recycled, nothing to dispose
        let Some(slot) = self.effect_slot_mut(effect_id) else {
            return;
        };
        match slot.state {
            EffectState::Idle => {
                slot.callback = None;
                slot.state = EffectState::Vacant;
                let deps = std::mem::take(&mut slot.dependencies);
                for signal_id in deps {
                    if let Some(subs) = self.signal_subscribers.get_mut(signal_id.index()) {
                        vec_remove(subs, &effect_id);
                    }
                }
                if let Some(pos) = self.pending_effects.iter().position(|e| *e == effect_id) {
                    self.pending_effects.remove(pos);
                }
                self.free_effect_indices.push(effect_id.index);
            }
            EffectState::Running => {
                // The callback is currently executing outside the borrow.
                // Mark for finalization in finish_effect (which pushes the
                // free-list entry); clear what can be cleared now.
                slot.state = EffectState::DisposedWhileRunning;
                let deps = std::mem::take(&mut slot.dependencies);
                for signal_id in deps {
                    if let Some(subs) = self.signal_subscribers.get_mut(signal_id.index()) {
                        vec_remove(subs, &effect_id);
                    }
                }
                if let Some(pos) = self.pending_effects.iter().position(|e| *e == effect_id) {
                    self.pending_effects.remove(pos);
                }
            }
            EffectState::Vacant | EffectState::DisposedWhileRunning => {
                // Already disposed
            }
        }
    }
}

thread_local! {
    /// Reentrancy guard for [`flush_pending_effects`]: when a write happens
    /// inside an effect, the nested flush is skipped and the outermost flush
    /// loop picks up the newly queued effects. This bounds stack depth for
    /// effect chains (they become loop iterations, not recursion).
    static FLUSHING: Cell<bool> = const { Cell::new(false) };
}

/// Run one effect: take its callback out of the runtime, execute it with NO
/// runtime borrow held (so writes and signal creation inside the callback
/// work), then restore it and register the tracked reads.
pub(crate) fn run_effect_by_id(effect_id: EffectId) {
    // Phase 1 (borrow): take the callback out
    let Some(mut callback) = with_runtime(|rt| rt.begin_effect(effect_id)) else {
        return;
    };

    // Phase 2 (no borrow): run the callback with read-tracking buffered.
    // A panicking callback must still reach phase 3: without it the slot
    // would be stuck in Running with its callback lost, and the tracking
    // frame would leak. Catch, restore state, then propagate the panic.
    EFFECT_TRACKING.with(|stack| {
        stack.borrow_mut().push((effect_id, EffectReads::new()));
    });
    let panic_payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        suspend_widget_tracking(&mut *callback);
    }))
    .err();
    let reads = EFFECT_TRACKING
        .with(|stack| stack.borrow_mut().pop())
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
    if FLUSHING.with(|f| f.replace(true)) {
        return;
    }
    // Reset the flag even if an effect callback panics; otherwise every
    // future flush would silently no-op and all effects would stop running.
    struct FlushGuard;
    impl Drop for FlushGuard {
        fn drop(&mut self) {
            FLUSHING.with(|f| f.set(false));
        }
    }
    let _guard = FlushGuard;

    while let Some(effect_id) = with_runtime(|rt| rt.pop_pending_effect()) {
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
    let batching = BATCH_DEPTH.with(|d| d.get() > 0);
    if !batching {
        flush_pending_effects();
    }
}

pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut Runtime) -> R,
{
    RUNTIME.with(|rt| f(&mut rt.borrow_mut()))
}

/// Reset all runtime state (effects, tracking, batch depth, write queue).
///
/// Called during `App::drop()` to ensure the next `App` run starts fresh.
/// Increments the write epoch so that any in-flight background writes from
/// old service tasks are automatically discarded by `flush_bg_writes()`.
pub(crate) fn reset_runtime() {
    RUNTIME.with(|rt| *rt.borrow_mut() = Runtime::new());
    EFFECT_TRACKING.with(|et| et.borrow_mut().clear());
    BATCH_DEPTH.with(|bd| bd.set(0));
    FLUSHING.with(|f| f.set(false));
    // Increment epoch BEFORE clearing — writes queued between now and the next
    // flush_bg_writes() will carry the old epoch and be discarded.
    WRITE_EPOCH.fetch_add(1, Ordering::Release);
    if let Ok(mut q) = WRITE_QUEUE.lock() {
        q.clear();
    }
}

/// Batch multiple signal writes so that shared effects run only once.
///
/// Inside the closure, `notify_write()` collects pending effects but defers
/// `flush_effects()` until the batch completes. Widget invalidation (paint/layout
/// jobs) is NOT batched — widgets still get per-field jobs immediately.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    BATCH_DEPTH.with(|d| d.set(d.get() + 1));
    // Restore the depth even if `f` panics: a caught panic must not leave
    // BATCH_DEPTH stuck > 0 (which would stop every effect in the app from
    // ever flushing again). Effects queued by the failed batch stay pending
    // and run on the next notify — we deliberately don't flush during unwind.
    let guard = super::guard::defer(|| {
        BATCH_DEPTH.with(|d| d.set(d.get() - 1));
    });
    let result = f();
    drop(guard);
    if BATCH_DEPTH.with(|d| d.get()) == 0 {
        flush_pending_effects();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::{create_effect, create_signal};
    use std::cell::Cell;
    use std::rc::Rc;

    /// A panic caught inside batch() must not leave BATCH_DEPTH stuck > 0 —
    /// that would silently stop every effect in the app from ever flushing.
    #[test]
    fn test_caught_panic_in_batch_does_not_wedge_effects() {
        let sig = create_signal(0);
        let observed = Rc::new(Cell::new(0));
        let o = observed.clone();
        create_effect(move || o.set(sig.get())).detach();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            batch(|| {
                sig.set(1);
                panic!("boom");
            })
        }));
        assert!(result.is_err());
        assert_eq!(BATCH_DEPTH.with(|d| d.get()), 0, "batch depth must unwind");

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
        })
        .detach();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sig.set(1)));
        assert!(result.is_err(), "panic must propagate out of set()");
        assert_eq!(observed.get(), 1);
        assert!(!FLUSHING.with(|f| f.get()), "flush guard must have reset");

        // The effect must still be alive, tracked, and re-runnable
        sig.set(2);
        assert_eq!(observed.get(), 2);
    }
}
