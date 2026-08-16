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
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use smallvec::SmallVec;
use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::reactive::invalidation::clear_widget_subscribers;
use crate::tree::{Tree, WidgetId};

/// Job queue with O(1) dedup via HashSet + Vec for ordered iteration.
///
/// Drained buffers come from (and should be returned to, via
/// [`recycle_job_buffer`]) a small spare pool so per-frame drains don't
/// re-allocate from zero.
struct JobQueue {
    set: rustc_hash::FxHashSet<Job>,
    vec: Vec<Job>,
}

impl JobQueue {
    fn new() -> Self {
        Self {
            set: rustc_hash::FxHashSet::default(),
            vec: Vec::new(),
        }
    }

    fn push(&mut self, job: Job) {
        if self.set.insert(job) {
            self.vec.push(job);
        }
    }

    /// Drain everything, installing `replacement` as the new backing buffer.
    fn drain_all(&mut self, replacement: Vec<Job>) -> Vec<Job> {
        self.set.clear();
        std::mem::replace(&mut self.vec, replacement)
    }

    /// Drain everything except Animation jobs into `buf`.
    fn drain_non_animation(&mut self, mut buf: Vec<Job>) -> Vec<Job> {
        self.vec.retain(|job| {
            if job.job_type == JobType::Animation {
                true
            } else {
                self.set.remove(job);
                buf.push(*job);
                false
            }
        });
        buf
    }

    fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}

/// Surface-owned scheduling.
///
/// Jobs are keyed by widget, but their *scheduling domain* is the surface:
/// the frame-pacing gate is per-surface, so the queues must be too — a
/// global queue drained by whichever surface renders first let a gate-open
/// surface advance another surface's animations unpaced (a busy-spin at
/// hundreds of thousands of iterations/s).
///
/// Push sites (`request_job`) have no `Tree` access, so jobs land in a
/// global `inbox` first. [`distribute_jobs`] — the single place where
/// ownership is resolved — sorts them into per-surface queues keyed by the
/// surface root widget. Each surface's render pass drains only its own
/// queue: a frame-gated surface's animation jobs simply sit in its queue
/// until its callback fires. Jobs whose widget has no live surface (mid-
/// teardown, or their surface was destroyed) go to the `orphans` lane,
/// processed once per loop iteration so deferred Unregister cleanup always
/// runs.
struct JobQueues {
    /// Push-side inbox (no ownership resolved yet).
    inbox: JobQueue,
    /// Per-surface queues, keyed by surface root widget.
    per_root: rustc_hash::FxHashMap<WidgetId, JobQueue>,
    /// Jobs with no live owning surface.
    orphans: JobQueue,
    /// Spare buffers for capacity reuse across frames.
    spare: Vec<Vec<Job>>,
}

impl JobQueues {
    fn new() -> Self {
        Self {
            inbox: JobQueue::new(),
            per_root: rustc_hash::FxHashMap::default(),
            orphans: JobQueue::new(),
            spare: Vec::new(),
        }
    }

    fn spare_buf(&mut self) -> Vec<Job> {
        self.spare.pop().unwrap_or_default()
    }

    fn recycle(&mut self, mut buf: Vec<Job>) {
        if self.spare.len() < 4 {
            buf.clear();
            self.spare.push(buf);
        }
    }

    fn is_empty(&self) -> bool {
        self.inbox.is_empty()
            && self.orphans.is_empty()
            && self.per_root.values().all(JobQueue::is_empty)
    }
}

// Thread-local job queues for pending reactive updates.
// All job producers (signal writes, animations) run on the main thread,
// so no Mutex is needed.
thread_local! {
    static PENDING_JOBS: RefCell<JobQueues> = RefCell::new(JobQueues::new());
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
        let inbox = &mut jobs.inbox;
        match request {
            JobRequest::Animation(required) => {
                inbox.push(Job {
                    widget_id,
                    job_type: JobType::Animation,
                });
                match required {
                    RequiredJob::None => {}
                    RequiredJob::Paint => {
                        inbox.push(Job {
                            widget_id,
                            job_type: JobType::Paint,
                        });
                    }
                    RequiredJob::Layout => {
                        inbox.push(Job {
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
                inbox.push(Job {
                    widget_id,
                    job_type,
                });
            }
        }
    });
    request_frame();
}

/// Resolve job ownership: sort the inbox into per-surface queues.
///
/// This is the ONLY place where a job's owning surface is determined
/// (topmost ancestor walk — parent links are complete between loop phases,
/// which is when this runs). `active_roots` is the set of live surface
/// roots: jobs resolving anywhere else — widget already gone, surface
/// destroyed, never parented — go to the orphan lane. Queues whose root is
/// no longer active are retired into the orphan lane too, so a closed
/// surface's deferred Unregister jobs still run and nothing keeps
/// `has_pending_jobs` true forever.
/// Synchronously tear down a widget subtree: clear each widget's signal
/// subscribers (and dirty segments) and unregister it from the tree,
/// children first.
///
/// Used wherever a subtree is discarded while the app keeps running — a
/// closed surface, or an old dynamic/keyed child replaced during
/// reconciliation. The deferred Drop→Unregister cascade is NOT enough
/// there: the discarded root's exclusive owner is disposed immediately,
/// but its descendants would stay in the tree (subscribers live, queued
/// reconciles runnable) until their deferred jobs ran — and a reconcile
/// executing in that window reads owner-disposed state and panics.
///
/// Children-first order means each widget Drop's deferred Unregister
/// requests target already-removed ids and no-op; stale queued jobs no-op
/// too via the tree's generation check.
pub(crate) fn teardown_widget_subtree(tree: &mut crate::tree::Tree, root: WidgetId) {
    for id in tree.collect_subtree_post_order(root) {
        clear_widget_subscribers(id);
        tree.unregister(id);
    }
}

pub fn distribute_jobs(tree: &Tree, active_roots: &rustc_hash::FxHashSet<WidgetId>) {
    PENDING_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();

        // Retire queues for surfaces that no longer exist.
        let dead: SmallVec<[WidgetId; 2]> = jobs
            .per_root
            .keys()
            .filter(|root| !active_roots.contains(root))
            .copied()
            .collect();
        for root in dead {
            if let Some(mut queue) = jobs.per_root.remove(&root) {
                for job in queue.vec.drain(..) {
                    jobs.orphans.push(job);
                }
            }
        }

        // Sort the inbox.
        if jobs.inbox.is_empty() {
            return;
        }
        let buf = jobs.spare_buf();
        let pending = jobs.inbox.drain_all(buf);
        for job in &pending {
            match tree.surface_root_of(job.widget_id) {
                Some(root) if active_roots.contains(&root) => {
                    jobs.per_root
                        .entry(root)
                        .or_insert_with(JobQueue::new)
                        .push(*job);
                }
                _ => jobs.orphans.push(*job),
            }
        }
        jobs.recycle(pending);
    });
}

/// Drain all jobs owned by the given surface root.
pub fn drain_surface_jobs(root: WidgetId) -> Vec<Job> {
    PENDING_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let buf = jobs.spare_buf();
        match jobs.per_root.get_mut(&root) {
            Some(queue) => queue.drain_all(buf),
            None => buf,
        }
    })
}

/// Drain the given surface's jobs EXCEPT Animation jobs.
/// Used to collect follow-up jobs (Paint/Layout) pushed by animation
/// advances and reconciliation, without re-draining Animation jobs.
pub fn drain_surface_non_animation_jobs(root: WidgetId) -> Vec<Job> {
    PENDING_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let buf = jobs.spare_buf();
        match jobs.per_root.get_mut(&root) {
            Some(queue) => queue.drain_non_animation(buf),
            None => buf,
        }
    })
}

/// Drain the orphan lane (jobs with no live owning surface — deferred
/// Unregister cleanup, mostly). Processed once per loop iteration.
pub fn drain_orphan_jobs() -> Vec<Job> {
    PENDING_JOBS.with(|jobs| {
        let mut jobs = jobs.borrow_mut();
        let buf = jobs.spare_buf();
        jobs.orphans.drain_all(buf)
    })
}

/// Return a drained job buffer for capacity reuse on later frames.
pub fn recycle_job_buffer(buf: Vec<Job>) {
    PENDING_JOBS.with(|jobs| jobs.borrow_mut().recycle(buf));
}

/// Process all jobs in a single pass, partitioned by type.
///
/// Order is preserved: Unregister → Animation → Reconcile → Paint → Layout.
/// Uses two passes (partition then process) instead of 5 separate filter scans.
pub fn process_jobs(jobs: &[Job], tree: &mut Tree, layout_roots: &mut Vec<WidgetId>) {
    // Single pass: partition into type buckets
    let mut unregister = SmallVec::<[WidgetId; 4]>::new();
    let mut animation = SmallVec::<[WidgetId; 8]>::new();
    let mut reconcile = SmallVec::<[WidgetId; 4]>::new();
    let mut paint = SmallVec::<[WidgetId; 8]>::new();
    let mut layout = SmallVec::<[WidgetId; 8]>::new();

    for job in jobs {
        match job.job_type {
            JobType::Unregister => unregister.push(job.widget_id),
            JobType::Animation => animation.push(job.widget_id),
            JobType::Reconcile => reconcile.push(job.widget_id),
            JobType::Paint => paint.push(job.widget_id),
            JobType::Layout => layout.push(job.widget_id),
        }
    }

    // Process in required order
    for id in unregister {
        clear_widget_subscribers(id);
        tree.unregister(id);
    }
    for id in animation {
        tree.with_widget_mut(id, |widget, wid, tree| {
            widget.advance_animations(tree, wid);
        });
    }
    for id in reconcile {
        tree.with_widget_mut(id, |widget, wid, tree| {
            widget.reconcile_children(tree, wid);
        });
        if let Some(root) = tree.mark_needs_layout(id)
            && !layout_roots.contains(&root)
        {
            layout_roots.push(root);
        }
    }
    for id in paint {
        tree.mark_needs_paint(id);
    }
    for id in layout {
        if let Some(root) = tree.mark_needs_layout(id)
            && !layout_roots.contains(&root)
        {
            layout_roots.push(root);
        }
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
        *jobs.borrow_mut() = JobQueues::new();
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
/// Whether a wakeup ping is already in flight for the current blocked/idle
/// period. Cleared by `mark_loop_awake()` right after dispatch returns.
static PING_SENT: AtomicBool = AtomicBool::new(false);

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
    FRAME_REQUESTED.store(true, Ordering::Relaxed);
    // Coalesce pings per loop iteration via a dedicated flag, NOT via
    // FRAME_REQUESTED: the frame flag is consumed mid-iteration by
    // take_frame_request(), so gating the ping on it lost wakeups — a
    // request landing while the flag happened to be set sent no ping, the
    // take then cleared the flag, and the loop blocked indefinitely with
    // work queued (only an unrelated Wayland event like mouse movement
    // would revive it). PING_SENT is instead cleared exactly once per
    // wakeup (mark_loop_awake), so during any blocked period the first
    // request always pings.
    let already_pinged = PING_SENT.swap(true, Ordering::Relaxed);
    if !already_pinged
        && let Ok(guard) = WAKEUP_PING.lock()
        && let Some(ref ping) = *guard
    {
        ping.ping();
    }
}

/// Reset ping coalescing after the event loop woke up. Any `request_frame`
/// from here until the next dispatch sends (at most) one fresh ping, which
/// keeps the eventfd readable so that dispatch returns immediately.
pub(crate) fn mark_loop_awake() {
    PING_SENT.store(false, Ordering::Relaxed);
}

/// Reset all job state (pending jobs, frame request flag, wakeup ping).
///
/// Called during `App::drop()` to clear stale jobs and allow re-initialization.
pub(crate) fn reset_jobs() {
    PENDING_JOBS.with(|jobs| {
        *jobs.borrow_mut() = JobQueues::new();
    });
    FRAME_REQUESTED.store(false, Ordering::Relaxed);
    PING_SENT.store(false, Ordering::Relaxed);
    EXIT_REQUEST.store(ExitRequest::Running as u8, Ordering::Relaxed);
    if let Ok(mut guard) = WAKEUP_PING.lock() {
        *guard = None;
    }
}

/// Check if a frame has been requested and clear the flag
pub fn take_frame_request() -> bool {
    FRAME_REQUESTED.swap(false, Ordering::Relaxed)
}

/// Peek the frame-request flag without clearing it.
///
/// Used by the main loop before blocking: a request that landed after this
/// iteration's `take_frame_request()` (e.g. from a background thread) leaves
/// the flag set, and `request_frame`'s ping-on-first-request optimization
/// then suppresses every later ping — blocking indefinitely on a set flag
/// would make the app deaf to background wakeups until an unrelated Wayland
/// event (mouse movement) arrives.
pub fn frame_request_pending() -> bool {
    FRAME_REQUESTED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Constraints, Size};
    use crate::widgets::Widget;

    struct TestWidget;
    impl Widget for TestWidget {
        fn layout(&mut self, _: &mut Tree, _: WidgetId, _: Constraints) -> Size {
            Size::zero()
        }
        fn paint(&self, _: &Tree, _: WidgetId, _: &mut crate::renderer::PaintContext) {}
    }

    /// Two surfaces (roots), one child each. Returns (tree, roots, children).
    fn two_surface_tree() -> (Tree, [WidgetId; 2], [WidgetId; 2]) {
        let mut tree = Tree::new();
        let root_a = tree.register(Box::new(TestWidget));
        let root_b = tree.register(Box::new(TestWidget));
        let child_a = tree.register(Box::new(TestWidget));
        let child_b = tree.register(Box::new(TestWidget));
        tree.set_parent(child_a, root_a);
        tree.set_parent(child_b, root_b);
        (tree, [root_a, root_b], [child_a, child_b])
    }

    fn roots_set(roots: &[WidgetId]) -> rustc_hash::FxHashSet<WidgetId> {
        roots.iter().copied().collect()
    }

    fn push(widget_id: WidgetId, job_type: JobType) -> Job {
        let job = Job {
            widget_id,
            job_type,
        };
        PENDING_JOBS.with(|pending| pending.borrow_mut().inbox.push(job));
        job
    }

    #[test]
    fn jobs_are_routed_to_their_owning_surface() {
        clear_pending_jobs();
        let (tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        let job_a = push(children[0], JobType::Paint);
        let job_b = push(children[1], JobType::Layout);
        distribute_jobs(&tree, &active);

        let drained_a = drain_surface_jobs(roots[0]);
        assert_eq!(drained_a, vec![job_a]);
        let drained_b = drain_surface_jobs(roots[1]);
        assert_eq!(drained_b, vec![job_b]);

        // A second drain returns nothing — queues are per surface and empty
        assert!(drain_surface_jobs(roots[0]).is_empty());
        assert!(!has_pending_jobs() || drain_orphan_jobs().is_empty());
    }

    #[test]
    fn gated_surface_animations_survive_other_surfaces_drains() {
        // The core invariant behind the pacing fix: draining surface B can
        // never touch surface A's animation continuations.
        clear_pending_jobs();
        let (tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        let anim_a = push(children[0], JobType::Animation);
        distribute_jobs(&tree, &active);

        // Surface B renders (A is frame-gated and never drains)
        assert!(drain_surface_jobs(roots[1]).is_empty());
        assert!(drain_surface_non_animation_jobs(roots[1]).is_empty());

        // A's animation is still queued, untouched
        let drained_a = drain_surface_jobs(roots[0]);
        assert_eq!(drained_a, vec![anim_a]);
    }

    #[test]
    fn drain_surface_non_animation_keeps_animation_jobs() {
        clear_pending_jobs();
        let (tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        let anim = push(children[0], JobType::Animation);
        let paint = push(children[0], JobType::Paint);
        let layout = push(roots[0], JobType::Layout);
        distribute_jobs(&tree, &active);

        let drained = drain_surface_non_animation_jobs(roots[0]);
        assert_eq!(drained.len(), 2);
        assert!(drained.contains(&paint));
        assert!(drained.contains(&layout));

        let remaining = drain_surface_jobs(roots[0]);
        assert_eq!(remaining, vec![anim]);
    }

    #[test]
    fn jobs_without_a_live_surface_go_to_the_orphan_lane() {
        clear_pending_jobs();
        let (mut tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        // Widget removed from the tree before distribution (deferred
        // Unregister after teardown)
        let ghost = children[1];
        tree.unregister(ghost);
        let orphan_job = push(ghost, JobType::Unregister);
        distribute_jobs(&tree, &active);

        assert_eq!(drain_orphan_jobs(), vec![orphan_job]);
        assert!(drain_surface_jobs(roots[1]).is_empty());
    }

    #[test]
    fn dead_surface_queues_are_retired_into_the_orphan_lane() {
        clear_pending_jobs();
        let (tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        let job_a = push(children[0], JobType::Unregister);
        distribute_jobs(&tree, &active);

        // Surface A closes: its root disappears from the active set. The
        // queued job must not be stranded (it would keep has_pending_jobs
        // true forever with no render pass to drain it).
        let only_b = roots_set(&roots[1..]);
        distribute_jobs(&tree, &only_b);

        assert_eq!(drain_orphan_jobs(), vec![job_a]);
        assert!(!has_pending_jobs());
    }

    #[test]
    fn inbox_dedup_survives_distribution() {
        clear_pending_jobs();
        let (tree, roots, children) = two_surface_tree();
        let active = roots_set(&roots);

        let job = push(children[0], JobType::Paint);
        // Same job pushed twice — deduped in the inbox
        push(children[0], JobType::Paint);
        distribute_jobs(&tree, &active);
        // ...and pushing again after distribution dedups in the surface queue
        push(children[0], JobType::Paint);
        distribute_jobs(&tree, &active);

        assert_eq!(drain_surface_jobs(roots[0]), vec![job]);
    }
}

#[cfg(test)]
mod wakeup_contract {
    use super::*;
    use smithay_client_toolkit::reexports::calloop::{EventLoop, ping::make_ping};
    use std::time::Duration;

    /// The one invariant in this file that had no test, and the one that
    /// cost a hung loop once: the wakeup ping must not be gated on
    /// `FRAME_REQUESTED`.
    ///
    /// That flag is consumed mid-iteration by `take_frame_request()`. Gating
    /// the ping on it meant a request arriving while the flag happened to be
    /// set sent nothing, the take then cleared the flag, and the loop blocked
    /// with work already queued — until an unrelated Wayland event, a mouse
    /// movement, happened to wake it.
    ///
    /// So: with `FRAME_REQUESTED` already set and the loop having woken since
    /// the last ping, a request must still ping.
    ///
    /// The globals are process-wide and other tests in this binary reach
    /// `request_frame` too. An interfering ping can only mask a failure here,
    /// never cause one, and the window between arming and dispatching is a
    /// few microseconds.
    #[test]
    fn a_request_pings_even_when_the_frame_flag_is_already_set() {
        let mut event_loop: EventLoop<bool> = EventLoop::try_new().expect("event loop");
        let (ping, source) = make_ping().expect("ping");
        event_loop
            .handle()
            .insert_source(source, |_, _, woken: &mut bool| *woken = true)
            .expect("insert ping source");

        init_wakeup(ping);

        // Drain anything already pending so the assertion below can only be
        // satisfied by our own request.
        let mut woken = false;
        let _ = event_loop.dispatch(Some(Duration::ZERO), &mut woken);

        // The state the old bug keyed on: a frame is already requested.
        FRAME_REQUESTED.store(true, Ordering::Relaxed);
        // The loop has woken since the last ping, so coalescing is reset.
        mark_loop_awake();

        request_frame();

        let mut woken = false;
        event_loop
            .dispatch(Some(Duration::from_millis(50)), &mut woken)
            .expect("dispatch");
        assert!(
            woken,
            "request_frame must ping while FRAME_REQUESTED is set — gating on \
             that flag is what lost wakeups"
        );

        reset_jobs();
    }

    /// The other half: within one awake iteration the ping is coalesced, so a
    /// burst of requests does not write the eventfd once per signal.
    #[test]
    fn further_requests_in_the_same_iteration_do_not_re_ping() {
        reset_jobs();
        mark_loop_awake();

        request_frame();
        assert!(PING_SENT.load(Ordering::Relaxed));

        // Already armed: the swap reports a ping is in flight, so no second
        // one is sent until the loop wakes and clears the flag.
        assert!(PING_SENT.swap(true, Ordering::Relaxed));

        reset_jobs();
    }
}
