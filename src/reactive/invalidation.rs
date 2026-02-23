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
    TRACKING_CONTEXT.with(|ctx| {
        let saved: Vec<_> = ctx.borrow_mut().drain(..).collect();
        let result = f();
        *ctx.borrow_mut() = saved;
        result
    })
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

thread_local! {
    /// Map from signal ID to subscribers.
    /// All access is on the main thread — background writes go through
    /// `queue_bg_write()` → `flush_bg_writes()` which executes on the main thread.
    static SIGNAL_SUBSCRIBERS: RefCell<HashMap<usize, HashSet<Subscriber>>> =
        RefCell::new(HashMap::new());
}

/// Register a widget as a subscriber for a signal with a specific job type
pub fn register_subscriber(widget_id: WidgetId, signal_id: usize, job_type: JobType) {
    SIGNAL_SUBSCRIBERS.with(|subs| {
        subs.borrow_mut()
            .entry(signal_id)
            .or_default()
            .insert(Subscriber {
                widget_id,
                job_type,
            });
    });
}

/// Notify all subscribers of a signal change by creating jobs
pub fn notify_signal_change(signal_id: usize) {
    let subscribers: Vec<Subscriber> = SIGNAL_SUBSCRIBERS.with(|subs| {
        subs.borrow()
            .get(&signal_id)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default()
    });

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
}

/// Clear signal subscribers for a specific signal (when signal is disposed)
pub fn clear_signal_subscribers(signal_id: usize) {
    SIGNAL_SUBSCRIBERS.with(|subs| {
        subs.borrow_mut().remove(&signal_id);
    });
}

/// Remove a widget from all signal subscriber sets.
/// Called when a widget is unregistered to prevent stale subscribers
/// from causing wasted job creation.
pub fn clear_widget_subscribers(widget_id: WidgetId) {
    SIGNAL_SUBSCRIBERS.with(|subs| {
        let mut subs = subs.borrow_mut();
        subs.retain(|_, subscribers| {
            subscribers.retain(|s| s.widget_id != widget_id);
            !subscribers.is_empty()
        });
    });
}

/// Get the number of signals with active subscribers (for testing).
#[cfg(test)]
fn subscriber_count() -> usize {
    SIGNAL_SUBSCRIBERS.with(|subs| subs.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::WidgetId;

    fn widget_id(n: u64) -> WidgetId {
        WidgetId::from_u64(n)
    }

    #[test]
    fn test_clear_signal_subscribers_removes_entry() {
        let wid = widget_id(100);
        register_subscriber(wid, 42, JobType::Paint);
        assert!(subscriber_count() > 0);

        clear_signal_subscribers(42);

        // Signal 42 should be removed
        SIGNAL_SUBSCRIBERS.with(|subs| {
            assert!(!subs.borrow().contains_key(&42));
        });
    }

    #[test]
    fn test_clear_widget_subscribers_removes_from_all_signals() {
        let wid = widget_id(200);
        let other = widget_id(201);

        // Widget 200 subscribes to signals 10 and 11
        register_subscriber(wid, 10, JobType::Paint);
        register_subscriber(wid, 11, JobType::Layout);
        // Widget 201 subscribes to signal 10
        register_subscriber(other, 10, JobType::Paint);

        clear_widget_subscribers(wid);

        SIGNAL_SUBSCRIBERS.with(|subs| {
            let subs = subs.borrow();
            // Signal 10 should still exist (widget 201 still subscribes)
            let s10 = subs.get(&10).unwrap();
            assert!(s10.iter().all(|s| s.widget_id != wid));
            assert!(s10.iter().any(|s| s.widget_id == other));
            // Signal 11 should be removed entirely (only widget 200 subscribed)
            assert!(!subs.contains_key(&11));
        });
    }

    #[test]
    fn test_with_signal_tracking_registers_subscriber() {
        let wid = widget_id(300);
        let signal_id = 99;

        with_signal_tracking(wid, JobType::Paint, || {
            record_signal_read(signal_id);
        });

        SIGNAL_SUBSCRIBERS.with(|subs| {
            let subs = subs.borrow();
            let s = subs.get(&signal_id).unwrap();
            assert!(s.contains(&Subscriber {
                widget_id: wid,
                job_type: JobType::Paint,
            }));
        });

        // Clean up
        clear_signal_subscribers(signal_id);
    }
}
