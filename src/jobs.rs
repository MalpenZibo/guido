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
//! Jobs are stored in a `JobQueue` with `HashSet` for O(1) dedup + `Vec` for ordered
//! iteration. Each `(widget_id, job_type)` pair is unique. Multiple signals updating
//! the same widget in one frame result in a single job.
//!
//! ## Frame Request
//!
//! When a job is pushed, the system automatically wakes the event loop via a ping
//! mechanism, ensuring the frame is processed promptly.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::reactive::invalidation::clear_widget_subscribers;
use crate::tree::{Tree, WidgetId};

/// Job queue with O(1) dedup via HashSet + Vec for ordered iteration.
struct JobQueue {
    set: HashSet<Job>,
    vec: Vec<Job>,
}

impl JobQueue {
    fn new() -> Self {
        Self {
            set: HashSet::new(),
            vec: Vec::new(),
        }
    }

    fn push(&mut self, job: Job) {
        if self.set.insert(job) {
            self.vec.push(job);
        }
    }

    fn drain_all(&mut self) -> Vec<Job> {
        self.set.clear();
        std::mem::take(&mut self.vec)
    }

    fn drain_non_animation(&mut self) -> Vec<Job> {
        let mut non_anim = Vec::new();
        self.vec.retain(|job| {
            if job.job_type == JobType::Animation {
                true
            } else {
                self.set.remove(job);
                non_anim.push(*job);
                false
            }
        });
        non_anim
    }

    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}

// Thread-local job queue for pending reactive updates.
// All job producers (signal writes, animations) run on the main thread,
// so no Mutex is needed.
thread_local! {
    static PENDING_JOBS: RefCell<JobQueue> = RefCell::new(JobQueue::new());
}

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
    PENDING_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        match request {
            JobRequest::Animation(required) => {
                jobs.push(Job {
                    widget_id,
                    job_type: JobType::Animation,
                });
                match required {
                    RequiredJob::None => {}
                    RequiredJob::Paint => {
                        jobs.push(Job {
                            widget_id,
                            job_type: JobType::Paint,
                        });
                    }
                    RequiredJob::Layout => {
                        jobs.push(Job {
                            widget_id,
                            job_type: JobType::Layout,
                        });
                    }
                }
            }
            _ => {
                let job_type = match request {
                    JobRequest::Layout => JobType::Layout,
                    JobRequest::Paint => JobType::Paint,
                    JobRequest::Reconcile => JobType::Reconcile,
                    JobRequest::Unregister => JobType::Unregister,
                    JobRequest::Animation(_) => unreachable!(),
                };
                jobs.push(Job {
                    widget_id,
                    job_type,
                });
            }
        }
    });
    request_frame();
}

/// Drain all pending jobs
pub fn drain_pending_jobs() -> Vec<Job> {
    PENDING_JOBS.with(|jobs| jobs.borrow_mut().drain_all())
}

/// Drain all pending jobs EXCEPT Animation jobs.
/// Used to collect follow-up jobs (Paint/Layout) pushed by animation
/// advances and reconciliation, without re-draining Animation jobs.
pub fn drain_non_animation_jobs() -> Vec<Job> {
    PENDING_JOBS.with(|jobs| jobs.borrow_mut().drain_non_animation())
}

pub fn handle_unregister_jobs(jobs: &[Job], tree: &mut Tree) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Unregister) {
        clear_widget_subscribers(job.widget_id);
        tree.unregister(job.widget_id);
    }
}

pub fn handle_reconcile_jobs(jobs: &[Job], tree: &mut Tree, layout_roots: &mut Vec<WidgetId>) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Reconcile) {
        tree.with_widget_mut(job.widget_id, |widget, id, tree| {
            widget.reconcile_children(tree, id);
        });
        if let Some(root) = tree.mark_needs_layout(job.widget_id)
            && !layout_roots.contains(&root)
        {
            layout_roots.push(root);
        }
    }
}

pub fn handle_layout_jobs(jobs: &[Job], tree: &mut Tree, layout_roots: &mut Vec<WidgetId>) {
    for job in jobs.iter().filter(|j| j.job_type == JobType::Layout) {
        if let Some(root) = tree.mark_needs_layout(job.widget_id)
            && !layout_roots.contains(&root)
        {
            layout_roots.push(root);
        }
    }
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

/// Check if there are pending jobs.
/// This includes both regular jobs and animation jobs.
pub fn has_pending_jobs() -> bool {
    PENDING_JOBS.with(|jobs| !jobs.borrow().is_empty())
}

/// Clear all pending jobs (for testing)
#[cfg(test)]
fn clear_pending_jobs() {
    PENDING_JOBS.with(|jobs| {
        jobs.borrow_mut().drain_all();
    });
}

/// Reason code stored in the atomic exit flag.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitRequest {
    /// No exit requested — keep running.
    Running = 0,
    /// Clean quit (compositor closed, user requested, etc.).
    Quit = 1,
    /// Restart requested (e.g. config change).
    Restart = 2,
}

/// Atomic exit flag. Written by `quit_app()` / `restart_app()`, read by the main loop.
static EXIT_REQUEST: AtomicU8 = AtomicU8::new(ExitRequest::Running as u8);

/// Request application exit with the given reason.
/// Wakes the event loop so the main loop checks promptly.
pub(crate) fn set_exit_request(req: ExitRequest) {
    EXIT_REQUEST.store(req as u8, Ordering::Release);
    // Wake the event loop so it notices the exit request
    if let Ok(guard) = WAKEUP_PING.lock()
        && let Some(ref ping) = *guard
    {
        ping.ping();
    }
}

/// Read the current exit request (non-destructive — persists until `reset_jobs()`).
pub(crate) fn get_exit_request() -> ExitRequest {
    match EXIT_REQUEST.load(Ordering::Acquire) {
        1 => ExitRequest::Quit,
        2 => ExitRequest::Restart,
        _ => ExitRequest::Running,
    }
}

/// Global flag to indicate a frame is requested
static FRAME_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Global wakeup handle for signaling the event loop.
/// Uses `Mutex<Option<Ping>>` instead of `OnceLock` so it can be reset on App drop.
static WAKEUP_PING: Mutex<Option<Ping>> = Mutex::new(None);

/// Initialize the wakeup mechanism (called from App::run())
pub fn init_wakeup(ping: Ping) {
    if let Ok(mut guard) = WAKEUP_PING.lock() {
        *guard = Some(ping);
    }
}

/// Request that the main event loop process a frame
pub(crate) fn request_frame() {
    // Only ping on first request - avoids redundant syscalls when multiple signals update
    let was_requested = FRAME_REQUESTED.swap(true, Ordering::Relaxed);
    if !was_requested {
        // Wake up the event loop immediately
        if let Ok(guard) = WAKEUP_PING.lock()
            && let Some(ref ping) = *guard
        {
            ping.ping();
        }
    }
}

/// Reset all job state (pending jobs, frame request flag, wakeup ping).
///
/// Called during `App::drop()` to clear stale jobs and allow re-initialization.
pub(crate) fn reset_jobs() {
    PENDING_JOBS.with(|jobs| {
        jobs.borrow_mut().drain_all();
    });
    FRAME_REQUESTED.store(false, Ordering::Relaxed);
    EXIT_REQUEST.store(ExitRequest::Running as u8, Ordering::Relaxed);
    if let Ok(mut guard) = WAKEUP_PING.lock() {
        *guard = None;
    }
}

/// Check if a frame has been requested and clear the flag
pub fn take_frame_request() -> bool {
    FRAME_REQUESTED.swap(false, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::WidgetId;

    fn widget_id(n: u64) -> WidgetId {
        WidgetId::from_u64(n)
    }

    /// Helper: directly push jobs into PENDING_JOBS for testing.
    fn push_test_jobs(jobs: &[Job]) {
        PENDING_JOBS.with(|pending| {
            let mut pending = pending.borrow_mut();
            for job in jobs {
                pending.push(*job);
            }
        });
    }

    #[test]
    fn drain_non_animation_keeps_animation_jobs() {
        // Clear any leftover state from other tests
        clear_pending_jobs();

        let anim_job = Job {
            widget_id: widget_id(1),
            job_type: JobType::Animation,
        };
        let paint_job = Job {
            widget_id: widget_id(2),
            job_type: JobType::Paint,
        };
        let layout_job = Job {
            widget_id: widget_id(3),
            job_type: JobType::Layout,
        };

        push_test_jobs(&[anim_job, paint_job, layout_job]);

        // drain_non_animation_jobs should return Paint + Layout, keep Animation
        let drained = drain_non_animation_jobs();
        assert_eq!(drained.len(), 2);
        assert!(drained.contains(&paint_job));
        assert!(drained.contains(&layout_job));

        // Animation job should still be in PENDING_JOBS
        let remaining = drain_pending_jobs();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0], anim_job);
    }

    #[test]
    fn drain_non_animation_with_no_animations() {
        clear_pending_jobs();

        let paint_job = Job {
            widget_id: widget_id(1),
            job_type: JobType::Paint,
        };
        let unregister_job = Job {
            widget_id: widget_id(2),
            job_type: JobType::Unregister,
        };

        push_test_jobs(&[paint_job, unregister_job]);

        let drained = drain_non_animation_jobs();
        assert_eq!(drained.len(), 2);

        // Nothing should remain
        let remaining = drain_pending_jobs();
        assert!(remaining.is_empty());
    }

    #[test]
    fn drain_non_animation_with_only_animations() {
        clear_pending_jobs();

        let anim1 = Job {
            widget_id: widget_id(1),
            job_type: JobType::Animation,
        };
        let anim2 = Job {
            widget_id: widget_id(2),
            job_type: JobType::Animation,
        };

        push_test_jobs(&[anim1, anim2]);

        // Should return empty — all jobs are animations
        let drained = drain_non_animation_jobs();
        assert!(drained.is_empty());

        // Both should remain
        let remaining = drain_pending_jobs();
        assert_eq!(remaining.len(), 2);
    }
}
