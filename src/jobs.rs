// ============================================================================
// Job-Based Reactive Invalidation System
// ============================================================================

use std::collections::HashSet;
use std::sync::{
    LazyLock, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::tree::{Tree, WidgetId};

/// Thread-safe job queue for pending reactive updates.
/// Uses HashSet to deduplicate jobs - each (widget_id, job_type) pair is unique.
static PENDING_JOBS: LazyLock<Mutex<HashSet<Job>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Job {
    pub widget_id: WidgetId,
    pub job_type: JobType,
}

/// Push a job to the queue (thread-safe).
/// Duplicate jobs (same widget_id + job_type) are automatically ignored.
pub fn push_job(widget_id: WidgetId, job_type: JobType) {
    PENDING_JOBS.lock().unwrap().insert(Job {
        widget_id,
        job_type,
    });
    request_frame();
}

/// Drain all pending jobs
pub fn drain_pending_jobs() -> Vec<Job> {
    std::mem::take(&mut *PENDING_JOBS.lock().unwrap())
        .into_iter()
        .collect()
}

pub fn handle_unregister_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Unregister) {
        tree.unregister(job.widget_id);
    }
}

pub fn handle_reconcile_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Reconcile) {
        let widget_cell = tree.get_widget_mut(job.widget_id);
        if let Some(widget_cell) = widget_cell {
            let mut widget = widget_cell.borrow_mut();
            widget.reconcile_children(tree);
            tree.mark_needs_layout(job.widget_id);
        }
    }
}

pub fn handle_layout_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Layout) {
        tree.mark_needs_layout(job.widget_id);
    }
}

pub fn handle_animation_jobs(jobs: &[Job], tree: &Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Animation) {
        tree.with_widget_mut(job.widget_id, |widget| {
            widget.advance_animations(tree);
        });
    }
}

/// Check if there are pending jobs (thread-safe)
/// This includes both regular jobs and animation jobs.
pub fn has_pending_jobs() -> bool {
    !PENDING_JOBS.lock().unwrap().is_empty()
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
