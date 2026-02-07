//! Job-based reactive invalidation system.
//!
//! This module provides the mechanism for connecting signal changes to widget updates.
//! When a signal changes, the system creates jobs that are processed by the main event loop.
//!
//! ## Job Types
//!
//! - **Layout**: Widget needs layout recalculation (size/position changed)
//! - **Paint**: Widget needs repaint only (visual properties changed)
//! - **Reconcile**: Widget needs children reconciliation (dynamic children changed)
//! - **Unregister**: Widget needs cleanup (deferred from Drop)
//! - **Animation**: Widget has active animations that need advancement
//!
//! ## Deduplication
//!
//! Jobs are stored in a `HashSet`, so each `(widget_id, job_type)` pair is unique.
//! Multiple signals updating the same widget in one frame result in a single job.
//!
//! ## Frame Request
//!
//! When a job is pushed, the system automatically wakes the event loop via a ping
//! mechanism, ensuring the frame is processed promptly.

use std::collections::HashSet;
use std::sync::{
    LazyLock, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::tree::{Tree, WidgetId};

/// Thread-safe job queue for pending reactive updates.
/// Uses HashSet to deduplicate jobs - each (widget_id, job_type) pair is unique.
static PENDING_JOBS: LazyLock<Mutex<HashSet<Job>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Job types for reactive invalidation (stored in the queue)
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

/// What additional job an animation requires
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequiredJob {
    /// Animation continuation only - no repaint needed (value hasn't changed)
    None,
    /// Animation + Paint (for paint-only animations like background, transform)
    Paint,
    /// Animation + Layout (for layout-affecting animations like width, height)
    Layout,
}

/// Job request from callers - richer than what's stored
#[derive(Clone, Copy, Debug)]
pub enum JobRequest {
    Layout,
    Paint,
    Reconcile,
    Unregister,
    /// Animation with required follow-up job (Paint or Layout)
    Animation(RequiredJob),
}

/// A reactive update job
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Job {
    pub widget_id: WidgetId,
    pub job_type: JobType,
}

/// Request a job (handles animation follow-up jobs automatically).
/// For animations, this inserts both the Animation job and any required follow-up job.
pub fn request_job(widget_id: WidgetId, request: JobRequest) {
    let mut jobs = PENDING_JOBS.lock().unwrap();
    match request {
        JobRequest::Animation(required) => {
            jobs.insert(Job {
                widget_id,
                job_type: JobType::Animation,
            });
            match required {
                RequiredJob::None => {}
                RequiredJob::Paint => {
                    jobs.insert(Job {
                        widget_id,
                        job_type: JobType::Paint,
                    });
                }
                RequiredJob::Layout => {
                    jobs.insert(Job {
                        widget_id,
                        job_type: JobType::Layout,
                    });
                }
            }
        }
        JobRequest::Layout => {
            jobs.insert(Job {
                widget_id,
                job_type: JobType::Layout,
            });
        }
        JobRequest::Paint => {
            jobs.insert(Job {
                widget_id,
                job_type: JobType::Paint,
            });
        }
        JobRequest::Reconcile => {
            jobs.insert(Job {
                widget_id,
                job_type: JobType::Reconcile,
            });
        }
        JobRequest::Unregister => {
            jobs.insert(Job {
                widget_id,
                job_type: JobType::Unregister,
            });
        }
    }
    drop(jobs);
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

pub fn handle_reconcile_jobs(jobs: &[Job], tree: &mut Tree) -> Vec<WidgetId> {
    let mut roots = Vec::new();
    for job in jobs.iter().filter(|j| j.job_type == JobType::Reconcile) {
        tree.with_widget_mut(job.widget_id, |widget, id, tree| {
            widget.reconcile_children(tree, id);
        });
        if let Some(root) = tree.mark_needs_layout(job.widget_id) {
            roots.push(root);
        }
    }
    roots
}

pub fn handle_layout_jobs(jobs: &[Job], tree: &mut Tree) -> Vec<WidgetId> {
    jobs.iter()
        .filter(|j| j.job_type == JobType::Layout)
        .filter_map(|job| tree.mark_needs_layout(job.widget_id))
        .collect()
}

pub fn handle_paint_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Paint) {
        tree.mark_needs_paint(job.widget_id);
    }
}

pub fn handle_animation_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Animation) {
        tree.with_widget_mut(job.widget_id, |widget, id, tree| {
            widget.advance_animations(tree, id);
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
