use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

// ============================================================================
// Job-Based Reactive Invalidation System
// ============================================================================

/// Job types for reactive invalidation
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobType {
    /// Widget needs layout recalculation
    Layout,
    /// Widget needs repaint only (future: partial repaint)
    Paint,
    /// Widget needs children reconciliation (implies layout)
    Reconcile,
    /// Widget needs to be unregistered from the tree (deferred cleanup for Drop)
    Unregister,
}

/// A reactive update job
#[derive(Clone, Copy, Debug)]
pub struct Job {
    pub widget_id: WidgetId,
    pub job_type: JobType,
}

/// Thread-safe job queue for pending reactive updates
static PENDING_JOBS: OnceLock<Mutex<Vec<Job>>> = OnceLock::new();

fn pending_jobs_queue() -> &'static Mutex<Vec<Job>> {
    PENDING_JOBS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Push a job to the queue (thread-safe)
pub fn push_job(widget_id: WidgetId, job_type: JobType) {
    pending_jobs_queue().lock().unwrap().push(Job {
        widget_id,
        job_type,
    });
}

/// Drain all pending jobs
fn drain_pending_jobs() -> Vec<Job> {
    std::mem::take(&mut *pending_jobs_queue().lock().unwrap())
}

/// Check if there are pending jobs (thread-safe)
pub fn has_pending_jobs() -> bool {
    !pending_jobs_queue().lock().unwrap().is_empty()
}

use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::tree::{Tree, WidgetId};

// ============================================================================
// Signal Tracking Context System
// ============================================================================

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
static SIGNAL_SUBSCRIBERS: OnceLock<Mutex<HashMap<usize, HashSet<Subscriber>>>> = OnceLock::new();

fn signal_subscribers() -> &'static Mutex<HashMap<usize, HashSet<Subscriber>>> {
    SIGNAL_SUBSCRIBERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a widget as a subscriber for a signal with a specific job type
pub fn register_subscriber(widget_id: WidgetId, signal_id: usize, job_type: JobType) {
    signal_subscribers()
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
    let subscribers: Vec<Subscriber> = signal_subscribers()
        .lock()
        .unwrap()
        .get(&signal_id)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default();

    let has_subscribers = !subscribers.is_empty();

    for sub in &subscribers {
        push_job(sub.widget_id, sub.job_type);
    }

    if has_subscribers {
        request_frame();
    }
}

/// Clear signal subscribers for a specific signal (when signal is disposed)
pub fn clear_signal_subscribers(signal_id: usize) {
    signal_subscribers().lock().unwrap().remove(&signal_id);
}

/// Register a layout dependency: when the signal changes, the widget needs re-layout.
/// Called at widget construction time when a layout-affecting property is set to a signal.
/// This is an alias for register_subscriber with JobType::Layout for backward compatibility.
pub fn register_layout_signal(widget_id: WidgetId, signal_id: usize) {
    register_subscriber(widget_id, signal_id, JobType::Layout);
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

/// Process pending jobs with an explicit tree reference.
///
/// This drains the pending job queue and processes each job:
/// - Unregister jobs: remove widget from tree (deferred cleanup from Drop)
/// - Reconcile jobs: run reconcile_children() on the widget, then mark for layout
/// - Layout jobs: mark the widget as needing layout in the tree
/// - Paint jobs: set the NEEDS_PAINT flag
///
/// Must be called from the main loop which has tree access.
pub fn process_pending_jobs_with_tree(tree: &mut Tree) {
    let jobs = drain_pending_jobs();

    // Deduplicate jobs by widget - keep highest priority job type per widget
    // Priority: Reconcile > Layout > Paint
    let mut widget_jobs: HashMap<WidgetId, JobType> = HashMap::new();

    for job in jobs {
        widget_jobs
            .entry(job.widget_id)
            .and_modify(|existing| {
                // Keep the higher priority job type
                // Priority: Unregister > Reconcile > Layout > Paint
                // Unregister is highest because it means the widget is being removed
                *existing = match (*existing, job.job_type) {
                    (JobType::Unregister, _) => JobType::Unregister,
                    (_, JobType::Unregister) => JobType::Unregister,
                    (JobType::Reconcile, _) => JobType::Reconcile,
                    (_, JobType::Reconcile) => JobType::Reconcile,
                    (JobType::Layout, _) => JobType::Layout,
                    (_, JobType::Layout) => JobType::Layout,
                    _ => JobType::Paint,
                };
            })
            .or_insert(job.job_type);
    }

    for (widget_id, job_type) in widget_jobs {
        match job_type {
            JobType::Unregister => {
                // Deferred unregistration from Drop handlers
                tree.unregister(widget_id);
            }
            JobType::Reconcile => {
                // Run reconciliation, then mark for layout
                let widget_cell = tree.get_widget_mut(widget_id);
                if let Some(widget_cell) = widget_cell {
                    let mut widget = widget_cell.borrow_mut();
                    widget.reconcile_children(tree);
                    tree.mark_needs_layout(widget_id);
                }
            }
            JobType::Layout => {
                // Mark widget as needing layout in the tree
                tree.mark_needs_layout(widget_id);
            }
            JobType::Paint => {
                // Paint-only jobs: frame already requested, no further action needed
                // The main loop will render when it processes the frame request
            }
        }
    }
}

/// Global flag to indicate a frame is requested
static FRAME_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Global wakeup handle for signaling the event loop
static WAKEUP_PING: OnceLock<Ping> = OnceLock::new();

/// Initialize the wakeup mechanism (called from App::run())
pub fn init_wakeup(ping: Ping) {
    let _ = WAKEUP_PING.set(ping);
}

/// Request that the main event loop process a frame
pub fn request_frame() {
    // Only ping on first request - avoids redundant syscalls when multiple signals update
    let was_requested = FRAME_REQUESTED.swap(true, Ordering::Relaxed);
    if !was_requested {
        // Wake up the event loop immediately
        if let Some(ping) = WAKEUP_PING.get() {
            ping.ping();
        }
    }
}

/// Check if a frame has been requested and clear the flag
pub fn take_frame_request() -> bool {
    FRAME_REQUESTED.swap(false, Ordering::Relaxed)
}

/// Mark that layout is needed (global helper, typically for structural changes)
pub fn request_layout() {
    request_frame();
}
