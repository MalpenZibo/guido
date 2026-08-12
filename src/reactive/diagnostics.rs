//! Debug-build diagnostic for signal reads that cannot be reactive.
//!
//! A tracked read (`get`/`with`) registers the *current* reactive scope as a
//! subscriber. With no scope active there is nobody to register, so the value
//! is a snapshot: whatever it produced is frozen into whatever it was used
//! for. That is the single most common way to write a UI that silently stops
//! updating —
//!
//! ```ignore
//! text(format!("{}", count.get()))          // snapshot: never updates
//! text(move || format!("{}", count.get()))  // reactive: the closure re-runs
//! ```
//!
//! Rust cannot express this at compile time: the two lines differ only in
//! where the read happens, and a type that carried that information would
//! have to poison every signature it touches (Leptos shipped `cx: Scope`
//! everywhere and removed it again). So the check is a debug-build warning at
//! the read site, with `#[track_caller]` giving the exact file and line, one
//! warning per call site, and nothing at all in release builds.
//!
//! Snapshots are legitimate in plenty of places — an event handler wants the
//! value at click time, not a subscription — so guido marks those regions as
//! [`snapshot_zone`]s and the check stays quiet inside them.

/// Run `f` in a region where snapshot reads are the intended semantics.
///
/// Suppresses the debug-build "read without a reactive scope" warning for
/// everything `f` does. guido wraps its own callback regions (event dispatch,
/// service tasks, animation completions) in one of these; apps need it only
/// for their own callback machinery — for a single read, `get_untracked()`
/// says the same thing more locally.
///
/// In release builds this is just a call to `f`.
pub fn snapshot_zone<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(debug_assertions)]
    {
        imp::DEPTH.with(|d| d.set(d.get() + 1));
        let _guard = crate::reactive::guard::defer(|| {
            imp::DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        });
        f()
    }
    #[cfg(not(debug_assertions))]
    {
        f()
    }
}

/// Report a tracked read that had no reactive scope to register with.
///
/// Called from the read itself, so `#[track_caller]` resolves to the user's
/// line. No-op in release builds and inside a [`snapshot_zone`].
#[cfg_attr(debug_assertions, track_caller)]
#[inline]
pub(crate) fn check_reactive_scope() {
    #[cfg(debug_assertions)]
    imp::check();
}

#[cfg(debug_assertions)]
mod imp {
    use std::cell::{Cell, RefCell};

    use rustc_hash::FxHashSet;

    thread_local! {
        /// Nesting depth of `snapshot_zone`.
        pub(super) static DEPTH: Cell<u32> = const { Cell::new(0) };
        /// Call sites already reported, so a read in a hot path warns once.
        static REPORTED: RefCell<FxHashSet<(&'static str, u32, u32)>> =
            RefCell::new(FxHashSet::default());
        /// Number of reports emitted — the hook the diagnostic's own tests use.
        pub(super) static REPORTS: Cell<u64> = const { Cell::new(0) };
    }

    #[track_caller]
    pub(super) fn check() {
        if DEPTH.with(|d| d.get()) > 0 {
            return;
        }
        if crate::reactive::invalidation::widget_tracking_active()
            || crate::reactive::runtime::effect_tracking_active()
        {
            return;
        }

        let loc = std::panic::Location::caller();
        let key = (loc.file(), loc.line(), loc.column());
        let first_time = REPORTED.with(|r| r.borrow_mut().insert(key));
        if !first_time {
            return;
        }

        REPORTS.with(|c| c.set(c.get() + 1));

        let message = format!(
            "{}:{}:{}: signal read with no reactive scope — this value is a \
             snapshot and will not update. Pass a closure instead (e.g. \
             `move || …` rather than the value it computes), or use \
             `get_untracked()`/`with_untracked()` if a snapshot is what you \
             meant. (debug builds only)",
            loc.file(),
            loc.line(),
            loc.column(),
        );
        // The audience for this warning is whoever just wrote their first
        // guido app, who quite likely has no logger installed yet — with the
        // log crate uninitialised `max_level` is Off and a `warn!` would go
        // nowhere, so say it on stderr instead.
        if log::max_level() == log::LevelFilter::Off {
            eprintln!("guido: {message}");
        } else {
            log::warn!("{message}");
        }
    }

    /// Forget every reported call site (used by `reset_reactive`).
    pub(crate) fn reset() {
        REPORTED.with(|r| r.borrow_mut().clear());
        DEPTH.with(|d| d.set(0));
        REPORTS.with(|c| c.set(0));
    }
}

#[cfg(debug_assertions)]
pub(crate) fn reset() {
    imp::reset();
}

#[cfg(not(debug_assertions))]
pub(crate) fn reset() {}

/// Number of snapshot reads reported so far on this thread (debug builds).
#[cfg(all(debug_assertions, test))]
pub(crate) fn report_count() -> u64 {
    imp::REPORTS.with(|c| c.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::JobType;
    use crate::reactive::invalidation::with_signal_tracking;
    use crate::reactive::{create_signal, create_stored};
    use crate::tree::WidgetId;

    fn reports_of(f: impl FnOnce()) -> u64 {
        let before = report_count();
        f();
        report_count() - before
    }

    /// The mistake this exists for: reading a signal while building a widget
    /// tree, with nothing to subscribe to.
    #[test]
    fn a_read_with_no_scope_is_reported() {
        let count = create_signal(0);
        assert_eq!(
            reports_of(|| {
                let _ = count.get();
            }),
            1
        );
    }

    /// Every legitimate shape must stay silent, or the warning becomes noise
    /// and nobody reads it.
    #[test]
    fn legitimate_reads_are_not_reported() {
        let count = create_signal(0);
        let stored = create_stored(7);
        let wid = WidgetId::from_u64(9001);

        // Inside a widget scope: layout/paint/reconcile register a subscriber
        assert_eq!(
            reports_of(|| with_signal_tracking(wid, JobType::Paint, || {
                let _ = count.get();
            })),
            0
        );
        // Explicit snapshot
        assert_eq!(
            reports_of(|| {
                let _ = count.get_untracked();
            }),
            0
        );
        assert_eq!(reports_of(|| count.with_untracked(|_| ())), 0);
        // Inside a region guido marks as callback-like
        assert_eq!(
            reports_of(|| snapshot_zone(|| {
                let _ = count.get();
            })),
            0
        );
        // A stored value cannot change, so reading it is never a missed
        // subscription — component props default to these
        assert_eq!(
            reports_of(|| {
                let _ = stored.get();
            }),
            0
        );
        // Effects establish their own tracking
        assert_eq!(
            reports_of(|| crate::reactive::create_effect(move || {
                let _ = count.get();
            })
            .detach()),
            0
        );
    }

    /// One report per call site, however hot the path.
    #[test]
    fn a_call_site_is_reported_once() {
        let count = create_signal(0);
        assert_eq!(
            reports_of(|| {
                for _ in 0..50 {
                    let _ = count.get();
                }
            }),
            1
        );
    }
}
