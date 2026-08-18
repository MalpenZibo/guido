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
//! Signal IDs are dense sequential integers so we use `Vec` for direct O(1) indexing.
//! A reverse index maps widget IDs to their subscribed signals for O(1) cleanup.
//!
//! ## Integration with Jobs System
//!
//! When a signal is written, [`notify_signal_change()`] creates jobs for all
//! subscribers. The jobs system deduplicates these and wakes the event loop.

use std::cell::RefCell;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::jobs::{JobRequest, JobType, request_job};
use crate::reactive::runtime::SignalId;
use crate::tree::WidgetId;

/// Context for tracking signal reads and associating them with a widget
struct SignalTrackingContext {
    widget_id: WidgetId,
    job_type: JobType,
    /// For reconciliation: which dynamic-children segment of the widget is
    /// reading. Lets a signal write dirty exactly one segment instead of
    /// re-running every dynamic segment of the container.
    segment: Option<u32>,
}

thread_local! {
    /// Stack of tracking contexts (supports nesting)
    static TRACKING_CONTEXT: RefCell<Vec<SignalTrackingContext>> = const { RefCell::new(Vec::new()) };
}

/// Run a closure while tracking signal reads for a widget.
///
/// Any signal read inside registers `widget_id` as a subscriber for
/// `job_type`, so a later write to it queues exactly that job for exactly this
/// widget. A widget implementing [`Widget`](crate::widgets::Widget) opens one
/// around each of its own phases: `JobType::Layout` around what it measures
/// from, `JobType::Paint` around what it draws from.
///
/// Scopes nest, and the innermost wins, so a widget that opens its own is
/// claiming its reads back from its parent. One that does not is not
/// unreactive — its reads land on whichever ancestor opened the enclosing
/// scope — but it is *imprecise*: a change to its content marks the parent
/// container for layout, which re-lays-out every sibling as well.
///
/// ```ignore
/// fn layout(&mut self, tree: &mut Tree, id: WidgetId, c: Constraints) -> Size {
///     with_signal_tracking(id, JobType::Layout, || {
///         let text = self.content.get();   // subscribes this widget, not its parent
///         measure(&text, c)
///     })
/// }
/// ```
pub fn with_signal_tracking<F, R>(widget_id: WidgetId, job_type: JobType, f: F) -> R
where
    F: FnOnce() -> R,
{
    with_tracking_context(widget_id, job_type, None, f)
}

/// Run a closure while tracking signal reads for one dynamic-children
/// segment of a widget. Reads register the widget for `Reconcile` jobs and
/// additionally mark the segment, so reconciliation re-runs only the
/// segments whose dependencies actually changed.
pub(crate) fn with_segment_tracking<F, R>(widget_id: WidgetId, segment: u32, f: F) -> R
where
    F: FnOnce() -> R,
{
    with_tracking_context(widget_id, JobType::Reconcile, Some(segment), f)
}

fn with_tracking_context<F, R>(
    widget_id: WidgetId,
    job_type: JobType,
    segment: Option<u32>,
    f: F,
) -> R
where
    F: FnOnce() -> R,
{
    TRACKING_CONTEXT.with(|ctx| {
        ctx.borrow_mut().push(SignalTrackingContext {
            widget_id,
            job_type,
            segment,
        });
    });
    // Pop on unwind too: a leaked frame would silently attribute every
    // later signal read in the app to this widget.
    let _guard = crate::reactive::guard::defer(|| {
        TRACKING_CONTEXT.with(|ctx| {
            ctx.borrow_mut().pop();
        });
    });
    f()
}

/// Suspend widget-level signal tracking during the given closure.
///
/// Signal reads inside the closure will NOT register any widget as a subscriber.
/// Used during effect execution to prevent effects from polluting the widget
/// tracking context when an effect runs inside a factory during reconciliation.
///
/// Effect-level tracking (via EFFECT_TRACKING in runtime.rs) is unaffected.
pub fn suspend_widget_tracking<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let saved: Vec<_> = TRACKING_CONTEXT.with(|ctx| ctx.borrow_mut().drain(..).collect());
    // Restore on unwind too: losing the saved stack would permanently
    // disable widget invalidation for every context that was active.
    let _guard = crate::reactive::guard::defer(move || {
        TRACKING_CONTEXT.with(|ctx| *ctx.borrow_mut() = saved);
    });
    f()
}

/// Whether a widget tracking scope is currently active (layout/paint/reconcile).
///
/// Only the non-reactive-read diagnostic asks, and that lives behind
/// `debug_assertions` — so does this, or release builds warn about it.
#[cfg(debug_assertions)]
pub(crate) fn widget_tracking_active() -> bool {
    TRACKING_CONTEXT.with(|ctx| !ctx.borrow().is_empty())
}

/// Record that a signal was read. Called from Signal::get().
/// If tracking is active, registers the current widget as a subscriber.
pub fn record_signal_read(signal_id: SignalId) {
    TRACKING_CONTEXT.with(|ctx| {
        if let Some(tracking) = ctx.borrow().last() {
            register_subscriber_impl(
                tracking.widget_id,
                signal_id,
                tracking.job_type,
                tracking.segment,
            );
        }
    });
}

// ============================================================================
// Unified Subscriber Registry
// ============================================================================

/// Subscriber entry with widget ID, job type and (for reconciliation) the
/// dynamic-children segment that performed the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Subscriber {
    widget_id: WidgetId,
    job_type: JobType,
    segment: Option<u32>,
}

/// Most signals have 1-2 widget subscribers (e.g. one paint, one layout).
type SubscriberList = SmallVec<[Subscriber; 2]>;

/// Most widgets subscribe to 2-6 signals (background, padding, etc.).
type SignalList = SmallVec<[SignalId; 4]>;

struct SubscriberRegistry {
    /// Forward index: signal index → subscribers. Direct Vec indexing
    /// (signal slot indices are dense; disposal clears the entry, so a
    /// recycled index always starts with an empty list).
    signal_to_widgets: Vec<SubscriberList>,
    /// Reverse index: widget_id → subscribed signal IDs. For O(1) widget cleanup.
    widget_to_signals: FxHashMap<WidgetId, SignalList>,
    /// All live (signal index, subscriber) pairs. This is the hot-path
    /// dedup: widgets re-read their signals every frame, and without this
    /// set each re-read paid a linear scan over the signal's subscriber
    /// list — O(N²) per frame for a signal read by N widgets (e.g. a theme
    /// color). With it, an already-registered read is one hash lookup.
    active: FxHashSet<(usize, Subscriber)>,
}

impl SubscriberRegistry {
    fn new() -> Self {
        Self {
            signal_to_widgets: Vec::new(),
            widget_to_signals: FxHashMap::default(),
            active: FxHashSet::default(),
        }
    }

    /// Ensure the forward index has capacity for the given signal ID.
    fn ensure_signal_capacity(&mut self, signal_id: SignalId) {
        if signal_id.index() >= self.signal_to_widgets.len() {
            self.signal_to_widgets
                .resize_with(signal_id.index() + 1, SmallVec::new);
        }
    }
}

thread_local! {
    /// Subscriber registry. All access is on the main thread — background writes go
    /// through `queue_bg_write()` → `flush_bg_writes()` which executes on the main thread.
    static REGISTRY: RefCell<SubscriberRegistry> = RefCell::new(SubscriberRegistry::new());

    /// Dynamic-children segments dirtied by signal writes, per container.
    /// Consumed by reconciliation via `take_dirty_segments()`.
    static DIRTY_SEGMENTS: RefCell<FxHashMap<WidgetId, SmallVec<[u32; 4]>>> =
        RefCell::new(FxHashMap::default());
}

/// Take (and clear) the set of dirty dynamic-children segments for a widget.
/// `None` means no segment-tracked signal of this widget changed since the
/// last call.
pub(crate) fn take_dirty_segments(widget_id: WidgetId) -> Option<SmallVec<[u32; 4]>> {
    DIRTY_SEGMENTS.with(|d| d.borrow_mut().remove(&widget_id))
}

/// Register a widget as a subscriber for a signal with a specific job type.
///
/// Called on every tracked signal read, so the already-registered case (the
/// overwhelming majority — widgets re-read their signals every frame) is a
/// single hash-set lookup.
pub fn register_subscriber(widget_id: WidgetId, signal_id: SignalId, job_type: JobType) {
    register_subscriber_impl(widget_id, signal_id, job_type, None);
}

fn register_subscriber_impl(
    widget_id: WidgetId,
    signal_id: SignalId,
    job_type: JobType,
    segment: Option<u32>,
) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();

        let sub = Subscriber {
            widget_id,
            job_type,
            segment,
        };
        if !reg.active.insert((signal_id.index(), sub)) {
            return; // Already subscribed — the hot path
        }

        reg.ensure_signal_capacity(signal_id);
        reg.signal_to_widgets[signal_id.index()].push(sub);

        // Update reverse index (deduped: the same widget/signal pair can
        // arrive with several job types, but only one entry is needed)
        let signals = reg.widget_to_signals.entry(widget_id).or_default();
        if !signals.contains(&signal_id) {
            signals.push(signal_id);
        }
    });
}

/// Notify all subscribers of a signal change by creating jobs.
///
/// Iterates under the registry borrow: `request_job` only touches the job
/// queue and wake state, never the registry, so no re-entrant
/// borrow is possible and no snapshot allocation is needed.
pub fn notify_signal_change(signal_id: SignalId) {
    REGISTRY.with(|reg| {
        let reg = reg.borrow();
        let Some(subs) = reg.signal_to_widgets.get(signal_id.index()) else {
            return;
        };
        for sub in subs {
            if let Some(segment) = sub.segment {
                DIRTY_SEGMENTS.with(|d| {
                    let mut d = d.borrow_mut();
                    let dirty = d.entry(sub.widget_id).or_default();
                    if !dirty.contains(&segment) {
                        dirty.push(segment);
                    }
                });
            }
            let request = match sub.job_type {
                JobType::Layout => JobRequest::Layout,
                JobType::Paint => JobRequest::Paint,
                JobType::Reconcile => JobRequest::Reconcile,
                JobType::Unregister => JobRequest::Unregister,
                JobType::Animation => JobRequest::Animation(crate::jobs::RequiredJob::None),
            };
            request_job(sub.widget_id, request);
        }
    });
}

/// Clear signal subscribers for a specific signal (when signal is disposed)
pub fn clear_signal_subscribers(signal_id: SignalId) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        if signal_id.index() < reg.signal_to_widgets.len() {
            // Remove this signal from the reverse index of each subscriber
            let subs = std::mem::take(&mut reg.signal_to_widgets[signal_id.index()]);
            for sub in &subs {
                reg.active.remove(&(signal_id.index(), *sub));
                if let Some(signals) = reg.widget_to_signals.get_mut(&sub.widget_id) {
                    signals.retain(|&mut s| s != signal_id);
                    if signals.is_empty() {
                        reg.widget_to_signals.remove(&sub.widget_id);
                    }
                }
            }
        }
    });
}

/// Remove a widget from all signal subscriber sets.
/// Called when a widget is unregistered to prevent stale subscribers
/// from causing wasted job creation.
pub fn clear_widget_subscribers(widget_id: WidgetId) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        // Use reverse index: only touch the signals this widget actually subscribes to
        if let Some(signal_ids) = reg.widget_to_signals.remove(&widget_id) {
            for signal_id in signal_ids {
                let Some(subs) = reg.signal_to_widgets.get_mut(signal_id.index()) else {
                    continue;
                };
                // Collect the widget's exact entries first so `active` drops
                // precisely what was registered (job types and segments are
                // unknown to the caller).
                let removed: SmallVec<[Subscriber; 2]> = subs
                    .iter()
                    .filter(|s| s.widget_id == widget_id)
                    .copied()
                    .collect();
                subs.retain(|s| s.widget_id != widget_id);
                for sub in removed {
                    reg.active.remove(&(signal_id.index(), sub));
                }
            }
        }
    });
    DIRTY_SEGMENTS.with(|d| {
        d.borrow_mut().remove(&widget_id);
    });
}

/// Reset all invalidation state (tracking context + subscriber registry).
///
/// Called during `App::drop()` to wipe stale widget-signal subscriptions.
pub(crate) fn reset_invalidation() {
    TRACKING_CONTEXT.with(|ctx| ctx.borrow_mut().clear());
    REGISTRY.with(|reg| *reg.borrow_mut() = SubscriberRegistry::new());
    DIRTY_SEGMENTS.with(|d| d.borrow_mut().clear());
}

/// Get the number of signals with active subscribers (for testing).
#[cfg(test)]
fn subscriber_count() -> usize {
    REGISTRY.with(|reg| {
        reg.borrow()
            .signal_to_widgets
            .iter()
            .filter(|s| !s.is_empty())
            .count()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::WidgetId;

    fn widget_id(n: u64) -> WidgetId {
        WidgetId::from_u64(n)
    }

    fn signal_id(n: u32) -> SignalId {
        SignalId::new(n, 0)
    }

    /// Which widget a read belongs to is decided by the innermost scope, and
    /// a widget that opens none inherits its parent's — the whole reason
    /// `with_signal_tracking` is public. A leaf that skips it is not
    /// unreactive; it is imprecise, because its parent is what gets marked.
    #[test]
    fn the_innermost_scope_owns_the_read() {
        let parent = widget_id(300);
        let leaf = widget_id(301);

        with_signal_tracking(parent, JobType::Layout, || {
            // A leaf that opens its own scope claims the read back.
            with_signal_tracking(leaf, JobType::Layout, || {
                record_signal_read(signal_id(70));
            });
            // One that does not leaves it with the parent.
            record_signal_read(signal_id(71));
        });

        let subscribers = |sig: u32| {
            REGISTRY.with(|reg| {
                let reg = reg.borrow();
                reg.signal_to_widgets
                    .get(sig as usize)
                    .map(|s| s.iter().map(|e| e.widget_id).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
        };

        assert_eq!(subscribers(70), vec![leaf]);
        assert_eq!(subscribers(71), vec![parent]);
    }

    #[test]
    fn test_clear_signal_subscribers_removes_entry() {
        let wid = widget_id(100);
        register_subscriber(wid, signal_id(42), JobType::Paint);
        assert!(subscriber_count() > 0);

        clear_signal_subscribers(signal_id(42));

        // Signal 42 should have no subscribers
        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            assert!(reg.signal_to_widgets.get(42).is_none_or(|s| s.is_empty()));
        });
    }

    #[test]
    fn test_clear_widget_subscribers_removes_from_all_signals() {
        let wid = widget_id(200);
        let other = widget_id(201);

        // Widget 200 subscribes to signals 10 and 11
        register_subscriber(wid, signal_id(10), JobType::Paint);
        register_subscriber(wid, signal_id(11), JobType::Layout);
        // Widget 201 subscribes to signal 10
        register_subscriber(other, signal_id(10), JobType::Paint);

        clear_widget_subscribers(wid);

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            // Signal 10 should still have widget 201
            let s10 = &reg.signal_to_widgets[10];
            assert!(s10.iter().all(|s| s.widget_id != wid));
            assert!(s10.iter().any(|s| s.widget_id == other));
            // Signal 11 should be empty (only widget 200 subscribed)
            assert!(reg.signal_to_widgets[11].is_empty());
        });
    }

    #[test]
    fn test_with_signal_tracking_registers_subscriber() {
        let wid = widget_id(300);
        let sid = signal_id(99);

        with_signal_tracking(wid, JobType::Paint, || {
            record_signal_read(sid);
        });

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            let s = &reg.signal_to_widgets[sid.index()];
            assert!(s.contains(&Subscriber {
                widget_id: wid,
                job_type: JobType::Paint,
                segment: None,
            }));
        });

        // Clean up
        clear_signal_subscribers(sid);
    }

    #[test]
    fn test_reverse_index_consistency() {
        let wid = widget_id(400);
        register_subscriber(wid, signal_id(50), JobType::Paint);
        register_subscriber(wid, signal_id(51), JobType::Layout);

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            let signals = reg.widget_to_signals.get(&wid).unwrap();
            assert!(signals.contains(&signal_id(50)));
            assert!(signals.contains(&signal_id(51)));
        });

        clear_widget_subscribers(wid);

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            assert!(!reg.widget_to_signals.contains_key(&wid));
        });
    }
}
