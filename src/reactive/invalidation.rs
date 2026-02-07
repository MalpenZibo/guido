//! Signal tracking and widget invalidation system.
//!
//! This module connects the reactive signal system to the widget tree, enabling
//! automatic UI updates when signals change.
//!
//! ## Signal Tracking Context
//!
//! During widget paint/layout, [`with_signal_tracking()`] establishes a context
//! that records which signals are read. These become the widget's dependencies.
//!
//! ## Subscriber Registry
//!
//! A global registry maps signal IDs to their subscribers (widget + job type pairs).
//! When a signal changes, all subscribers receive jobs via the jobs system.
//!
//! ## Integration with Jobs System
//!
//! When a signal is written, [`notify_signal_change()`] creates jobs for all
//! subscribers. The jobs system deduplicates these and wakes the event loop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use crate::jobs::{JobRequest, JobType, request_job};
use crate::tree::WidgetId;

/// Context for tracking signal reads and associating them with a widget
struct SignalTrackingContext {
    widget_id: WidgetId,
    job_type: JobType,
}

thread_local! {
    /// Stack of tracking contexts (supports nesting)
    static TRACKING_CONTEXT: RefCell<Vec<SignalTrackingContext>> = const { RefCell::new(Vec::new()) };
}

/// Run a closure while tracking signal reads for a widget.
/// Any signals read during this closure will register the widget as a subscriber.
pub fn with_signal_tracking<F, R>(widget_id: WidgetId, job_type: JobType, f: F) -> R
where
    F: FnOnce() -> R,
{
    TRACKING_CONTEXT.with(|ctx| {
        ctx.borrow_mut().push(SignalTrackingContext {
            widget_id,
            job_type,
        });
    });
    let result = f();
    TRACKING_CONTEXT.with(|ctx| {
        ctx.borrow_mut().pop();
    });
    result
}

/// Record that a signal was read. Called from Signal::get().
/// If tracking is active, registers the current widget as a subscriber.
pub fn record_signal_read(signal_id: usize) {
    TRACKING_CONTEXT.with(|ctx| {
        if let Some(tracking) = ctx.borrow().last() {
            register_subscriber(tracking.widget_id, signal_id, tracking.job_type);
        }
    });
}

// ============================================================================
// Unified Subscriber Registry
// ============================================================================

/// Subscriber entry with widget ID and job type
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Subscriber {
    widget_id: WidgetId,
    job_type: JobType,
}

/// Thread-safe map from signal ID to subscribers
/// Must be thread-safe because signals can be updated from background threads
static SIGNAL_SUBSCRIBERS: LazyLock<Mutex<HashMap<usize, HashSet<Subscriber>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ============================================================================
// Signal Callbacks (for select() — field-level reactivity)
// ============================================================================

/// Unique ID for a registered signal callback
pub(crate) type CallbackId = usize;

/// A callback invoked when a signal changes.
/// Wrapped in `Arc` so it can be cloned out of the lock before firing.
type SignalCallback = Arc<dyn Fn() + Send + Sync>;

/// Map from signal ID → list of (callback_id, callback)
type CallbackMap = HashMap<usize, Vec<(CallbackId, SignalCallback)>>;
static SIGNAL_CALLBACKS: LazyLock<Mutex<CallbackMap>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Monotonically increasing counter for callback IDs
static NEXT_CALLBACK_ID: AtomicUsize = AtomicUsize::new(0);

/// Register a callback that fires when a signal changes.
/// Returns a `CallbackId` that can be used to unregister later.
pub(crate) fn register_signal_callback(
    signal_id: usize,
    callback: impl Fn() + Send + Sync + 'static,
) -> CallbackId {
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    SIGNAL_CALLBACKS
        .lock()
        .unwrap()
        .entry(signal_id)
        .or_default()
        .push((id, Arc::new(callback)));
    id
}

/// Remove a previously registered signal callback.
pub(crate) fn unregister_signal_callback(signal_id: usize, callback_id: CallbackId) {
    if let Some(callbacks) = SIGNAL_CALLBACKS.lock().unwrap().get_mut(&signal_id) {
        callbacks.retain(|(id, _)| *id != callback_id);
    }
}

/// Register a widget as a subscriber for a signal with a specific job type
pub fn register_subscriber(widget_id: WidgetId, signal_id: usize, job_type: JobType) {
    SIGNAL_SUBSCRIBERS
        .lock()
        .unwrap()
        .entry(signal_id)
        .or_default()
        .insert(Subscriber {
            widget_id,
            job_type,
        });
}

/// Notify all subscribers of a signal change by creating jobs
pub fn notify_signal_change(signal_id: usize) {
    let subscribers: Vec<Subscriber> = SIGNAL_SUBSCRIBERS
        .lock()
        .unwrap()
        .get(&signal_id)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default();

    for sub in &subscribers {
        // Convert JobType to JobRequest for the new API
        let request = match sub.job_type {
            JobType::Layout => JobRequest::Layout,
            JobType::Paint => JobRequest::Paint,
            JobType::Reconcile => JobRequest::Reconcile,
            JobType::Unregister => JobRequest::Unregister,
            JobType::Animation => JobRequest::Animation(crate::jobs::RequiredJob::None),
        };
        request_job(sub.widget_id, request);
    }

    // Fire signal callbacks (for select() derived signals).
    // Clone Arc handles under the lock, then fire after releasing — prevents
    // deadlock when a callback triggers cascading signal updates (which re-lock).
    let callbacks: Vec<SignalCallback> = SIGNAL_CALLBACKS
        .lock()
        .unwrap()
        .get(&signal_id)
        .map(|cbs| cbs.iter().map(|(_, cb)| Arc::clone(cb)).collect())
        .unwrap_or_default();

    for cb in &callbacks {
        cb();
    }
}

/// Clear signal subscribers and callbacks for a specific signal (when signal is disposed)
pub fn clear_signal_subscribers(signal_id: usize) {
    SIGNAL_SUBSCRIBERS.lock().unwrap().remove(&signal_id);
    SIGNAL_CALLBACKS.lock().unwrap().remove(&signal_id);
}

/// Register a layout dependency: when the signal changes, the widget needs re-layout.
/// Called at widget construction time when a layout-affecting property is set to a signal.
/// This is an alias for register_subscriber with JobType::Layout for backward compatibility.
pub fn register_layout_signal(widget_id: WidgetId, signal_id: usize) {
    register_subscriber(widget_id, signal_id, JobType::Layout);
}

/// Register a paint dependency: when the signal changes, the widget needs repaint.
/// Called at widget construction time when a paint-affecting property (e.g. transform) is set to a signal.
pub fn register_paint_signal(widget_id: WidgetId, signal_id: usize) {
    register_subscriber(widget_id, signal_id, JobType::Paint);
}

/// Notify all layout subscribers of a signal that it has changed.
/// Called from Signal::set() and Signal::update().
///
/// This function now calls notify_signal_change which pushes jobs to the
/// pending queue. The main loop calls `process_pending_jobs()` to process them.
///
/// This is kept for backward compatibility - new code should use notify_signal_change.
pub fn notify_layout_subscribers(signal_id: usize) {
    notify_signal_change(signal_id);
}

/// Clear layout subscribers for a specific signal (when signal is disposed)
/// This is an alias for clear_signal_subscribers for backward compatibility.
pub fn clear_layout_subscribers(signal_id: usize) {
    clear_signal_subscribers(signal_id);
}
