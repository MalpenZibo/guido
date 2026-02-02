// ============================================================================
// Job-Based Reactive Invalidation System
// ============================================================================

use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::tree::WidgetId;

/// Thread-safe job queue for pending reactive updates
static PENDING_JOBS: OnceLock<Mutex<Vec<Job>>> = OnceLock::new();

fn pending_jobs_queue() -> &'static Mutex<Vec<Job>> {
    PENDING_JOBS.get_or_init(|| Mutex::new(Vec::new()))
}

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
    /// Widget has active animations that need advancement
    Animation,
}

/// A reactive update job
#[derive(Clone, Copy, Debug)]
pub struct Job {
    pub widget_id: WidgetId,
    pub job_type: JobType,
}

/// Push a job to the queue (thread-safe)
/// Animation jobs are routed to a separate queue for processing after paint.
pub fn push_job(widget_id: WidgetId, job_type: JobType) {
    pending_jobs_queue().lock().unwrap().push(Job {
        widget_id,
        job_type,
    });
    request_frame();
}

/// Drain all pending jobs
pub fn drain_pending_jobs() -> Vec<Job> {
    std::mem::take(&mut *pending_jobs_queue().lock().unwrap())
}

/// Check if there are pending jobs (thread-safe)
/// This includes both regular jobs and animation jobs.
pub fn has_pending_jobs() -> bool {
    !pending_jobs_queue().lock().unwrap().is_empty()
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
fn request_frame() {
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
