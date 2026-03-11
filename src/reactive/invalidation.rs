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
use std::collections::HashMap;

use smallvec::SmallVec;

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

/// Most signals have 1-2 widget subscribers (e.g. one paint, one layout).
type SubscriberList = SmallVec<[Subscriber; 2]>;

/// Most widgets subscribe to 2-6 signals (background, padding, etc.).
type SignalList = SmallVec<[usize; 4]>;

struct SubscriberRegistry {
    /// Forward index: signal_id → subscribers. Direct Vec indexing (signal IDs are dense).
    signal_to_widgets: Vec<SubscriberList>,
    /// Reverse index: widget_id → subscribed signal IDs. For O(1) widget cleanup.
    widget_to_signals: HashMap<WidgetId, SignalList>,
}

impl SubscriberRegistry {
    fn new() -> Self {
        Self {
            signal_to_widgets: Vec::new(),
            widget_to_signals: HashMap::new(),
        }
    }

    /// Ensure the forward index has capacity for the given signal ID.
    fn ensure_signal_capacity(&mut self, signal_id: usize) {
        if signal_id >= self.signal_to_widgets.len() {
            self.signal_to_widgets
                .resize_with(signal_id + 1, SmallVec::new);
        }
    }
}

thread_local! {
    /// Subscriber registry. All access is on the main thread — background writes go
    /// through `queue_bg_write()` → `flush_bg_writes()` which executes on the main thread.
    static REGISTRY: RefCell<SubscriberRegistry> = RefCell::new(SubscriberRegistry::new());
}

/// Register a widget as a subscriber for a signal with a specific job type
pub fn register_subscriber(widget_id: WidgetId, signal_id: usize, job_type: JobType) {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        reg.ensure_signal_capacity(signal_id);

        let sub = Subscriber {
            widget_id,
            job_type,
        };

        let subs = &mut reg.signal_to_widgets[signal_id];
        if !subs.contains(&sub) {
            subs.push(sub);
        }

        // Update reverse index
        let signals = reg.widget_to_signals.entry(widget_id).or_default();
        if !signals.contains(&signal_id) {
            signals.push(signal_id);
        }
    });
}

/// Notify all subscribers of a signal change by creating jobs
pub fn notify_signal_change(signal_id: usize) {
    // Collect subscribers while holding the borrow, then release before calling request_job
    // (which may trigger further signal reads/writes)
    let subscribers: SmallVec<[Subscriber; 4]> = REGISTRY.with(|reg| {
        let reg = reg.borrow();
        if signal_id < reg.signal_to_widgets.len() {
            reg.signal_to_widgets[signal_id].iter().copied().collect()
        } else {
            SmallVec::new()
        }
    });

    for sub in &subscribers {
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
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        if signal_id < reg.signal_to_widgets.len() {
            // Remove this signal from the reverse index of each subscriber
            let subs = std::mem::take(&mut reg.signal_to_widgets[signal_id]);
            for sub in &subs {
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
                if signal_id < reg.signal_to_widgets.len() {
                    reg.signal_to_widgets[signal_id].retain(|s| s.widget_id != widget_id);
                }
            }
        }
    });
}

/// Reset all invalidation state (tracking context + subscriber registry).
///
/// Called during `App::drop()` to wipe stale widget-signal subscriptions.
pub(crate) fn reset_invalidation() {
    TRACKING_CONTEXT.with(|ctx| ctx.borrow_mut().clear());
    REGISTRY.with(|reg| *reg.borrow_mut() = SubscriberRegistry::new());
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

    #[test]
    fn test_clear_signal_subscribers_removes_entry() {
        let wid = widget_id(100);
        register_subscriber(wid, 42, JobType::Paint);
        assert!(subscriber_count() > 0);

        clear_signal_subscribers(42);

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
        register_subscriber(wid, 10, JobType::Paint);
        register_subscriber(wid, 11, JobType::Layout);
        // Widget 201 subscribes to signal 10
        register_subscriber(other, 10, JobType::Paint);

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
        let signal_id = 99;

        with_signal_tracking(wid, JobType::Paint, || {
            record_signal_read(signal_id);
        });

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            let s = &reg.signal_to_widgets[signal_id];
            assert!(s.contains(&Subscriber {
                widget_id: wid,
                job_type: JobType::Paint,
            }));
        });

        // Clean up
        clear_signal_subscribers(signal_id);
    }

    #[test]
    fn test_reverse_index_consistency() {
        let wid = widget_id(400);
        register_subscriber(wid, 50, JobType::Paint);
        register_subscriber(wid, 51, JobType::Layout);

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            let signals = reg.widget_to_signals.get(&wid).unwrap();
            assert!(signals.contains(&50));
            assert!(signals.contains(&51));
        });

        clear_widget_subscribers(wid);

        REGISTRY.with(|reg| {
            let reg = reg.borrow();
            assert!(!reg.widget_to_signals.contains_key(&wid));
        });
    }
}
