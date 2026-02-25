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

pub type SignalId = usize;
pub type EffectId = usize;

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

/// Queue a closure for execution on the main thread (next frame).
/// Used by `WriteSignal::set()`/`update()` from background threads.
///
/// The write is tagged with the current epoch. If the runtime resets before
/// this write is flushed (e.g. App restart), it will be silently discarded.
pub fn queue_bg_write(f: impl FnOnce() + Send + 'static) {
    let epoch = WRITE_EPOCH.load(Ordering::Acquire);
    if let Ok(mut q) = WRITE_QUEUE.lock() {
        q.push((epoch, Box::new(f)));
    }
    // Wake the event loop so flush_bg_writes() runs on the next frame
    crate::jobs::request_frame();
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

#[derive(Default)]
pub struct Runtime {
    current_effect: Option<EffectId>,
    /// Pending effects to run. Uses Vec with dedup — most frames have 0–5 pending effects.
    pending_effects: Vec<EffectId>,
    effect_callbacks: Vec<Option<Box<dyn FnMut()>>>,
    /// Per-effect dependencies (which signals it reads). Vec with dedup — most effects
    /// depend on 1–3 signals, making linear scan faster than HashSet.
    effect_dependencies: Vec<Vec<SignalId>>,
    /// Per-signal subscribers (which effects track it). Vec with dedup — most signals
    /// have 1–5 subscribers.
    signal_subscribers: Vec<Vec<EffectId>>,
    next_effect_id: EffectId,
    /// Free list of reusable effect IDs (from disposed effects).
    free_effect_ids: Vec<EffectId>,
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a signal for subscriber tracking (called when signal is created)
    pub fn register_signal(&mut self, id: SignalId) {
        // Ensure we have space for subscribers
        while self.signal_subscribers.len() <= id {
            self.signal_subscribers.push(Vec::new());
        }
    }

    pub fn allocate_effect(&mut self, callback: Box<dyn FnMut()>) -> EffectId {
        // Reuse a freed slot if available
        if let Some(id) = self.free_effect_ids.pop() {
            self.effect_callbacks[id] = Some(callback);
            self.effect_dependencies[id].clear();
            return id;
        }
        // Otherwise allocate new
        let id = self.next_effect_id;
        self.next_effect_id += 1;
        self.effect_callbacks.push(Some(callback));
        self.effect_dependencies.push(Vec::new());
        id
    }

    pub fn notify_write(&mut self, signal_id: SignalId) {
        // Check if this signal exists in our runtime (it might not if called from another thread)
        if signal_id >= self.signal_subscribers.len() {
            return;
        }

        // Iterate subscribers by index — avoids temporary Vec allocation
        for i in 0..self.signal_subscribers[signal_id].len() {
            let effect_id = self.signal_subscribers[signal_id][i];
            vec_insert(&mut self.pending_effects, effect_id);
        }

        // When inside a batch(), defer effect execution until the batch completes
        let batching = BATCH_DEPTH.with(|d| d.get() > 0);
        if !batching {
            self.flush_effects();
        }
    }

    pub fn run_effect(&mut self, effect_id: EffectId) {
        // Clear old dependencies
        let old_deps = std::mem::take(&mut self.effect_dependencies[effect_id]);
        for signal_id in old_deps {
            vec_remove(&mut self.signal_subscribers[signal_id], &effect_id);
        }

        // Push tracking context (signal reads are buffered here since
        // the Runtime RefCell is already borrowed during callback execution)
        EFFECT_TRACKING.with(|stack| {
            stack.borrow_mut().push((effect_id, EffectReads::new()));
        });

        // Run effect
        let prev_effect = self.current_effect;
        self.current_effect = Some(effect_id);

        if let Some(callback) = self.effect_callbacks[effect_id].as_mut() {
            suspend_widget_tracking(callback);
        }

        self.current_effect = prev_effect;

        // Pop tracking context and register buffered reads as dependencies
        let reads = EFFECT_TRACKING.with(|stack| stack.borrow_mut().pop());
        if let Some((_eid, signal_ids)) = reads {
            for signal_id in signal_ids {
                if signal_id < self.signal_subscribers.len() {
                    vec_insert(&mut self.signal_subscribers[signal_id], effect_id);
                }
                vec_insert(&mut self.effect_dependencies[effect_id], signal_id);
            }
        }
    }

    pub fn flush_effects(&mut self) {
        // Use swap + drain to preserve Vec capacity across frames.
        // mem::take would replace with a 0-capacity Vec, forcing re-allocation next frame.
        let mut to_run = Vec::new();
        while !self.pending_effects.is_empty() {
            std::mem::swap(&mut to_run, &mut self.pending_effects);
            for effect_id in to_run.drain(..) {
                self.run_effect(effect_id);
            }
        }
    }

    pub fn dispose_effect(&mut self, effect_id: EffectId) {
        // Clear dependencies
        let deps = std::mem::take(&mut self.effect_dependencies[effect_id]);
        for signal_id in deps {
            if signal_id < self.signal_subscribers.len() {
                vec_remove(&mut self.signal_subscribers[signal_id], &effect_id);
            }
        }
        self.effect_callbacks[effect_id] = None;
        vec_remove(&mut self.pending_effects, &effect_id);
        self.free_effect_ids.push(effect_id);
    }
}

pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut Runtime) -> R,
{
    RUNTIME.with(|rt| f(&mut rt.borrow_mut()))
}

/// Try to access the runtime. This is safe to call from any thread.
/// On the main thread, runs the callback. On other threads, does nothing.
/// This enables signals to be updated from background threads without panicking.
pub fn try_with_runtime<F>(f: F)
where
    F: FnOnce(&mut Runtime),
{
    RUNTIME.with(|rt| {
        if let Ok(mut runtime) = rt.try_borrow_mut() {
            f(&mut runtime);
        }
        // If borrow fails (already borrowed), skip - this can happen during effect execution
    });
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
    let result = f();
    BATCH_DEPTH.with(|d| {
        let new = d.get() - 1;
        d.set(new);
        if new == 0 {
            try_with_runtime(|rt| rt.flush_effects());
        }
    });
    result
}
