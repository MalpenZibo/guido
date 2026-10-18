use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::effect::create_memo_effect;
use super::into_signal::{IntoSignal, MemoMarker};
use super::signal::{RwSignal, Signal, create_signal};

/// Eager computed value that recomputes immediately when dependencies change.
///
/// A `Memo<T>` updates eagerly whenever any dependency signal changes.
/// It only notifies downstream subscribers when the computed result actually
/// differs (`PartialEq`), which prevents unnecessary repaints/relayouts.
///
/// `Memo<T>` is `Copy` (like `Signal<T>`) and can be used directly as a
/// widget property via `IntoSignal`.
///
/// # Example
///
/// ```no_run
/// # use guido::prelude::*;
/// let count = create_signal(0);
/// let doubled = create_memo(move || count.get() * 2);
///
/// container().background(move || {
///     if doubled.get() > 10 { Color::RED } else { Color::BLUE }
/// });
/// ```
pub struct Memo<T: Clone + PartialEq + Send + 'static> {
    signal: RwSignal<T>,
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T: Clone + PartialEq + Send + 'static> Clone for Memo<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + PartialEq + Send + 'static> Copy for Memo<T> {}

/// Create an eagerly-evaluated memo that recomputes when dependencies change.
///
/// The memo only notifies subscribers when its computed value actually changes
/// (compared via `PartialEq`), preventing unnecessary downstream updates.
///
/// # Example
///
/// ```no_run
/// # use guido::prelude::*;
/// let count = create_signal(0);
/// let label = create_memo(move || format!("Count: {}", count.get()));
/// text(label);  // Only repaints when the formatted string actually changes
/// ```
pub fn create_memo<T, F>(f: F) -> Memo<T>
where
    T: Clone + PartialEq + Send + 'static,
    F: Fn() -> T + 'static,
{
    // `f` runs once, as the effect's first run: in the effect's own scope, so
    // what it makes is disposed when it recomputes, and with the effect's own
    // read tracking, so whatever is tracking around the caller — a dynamic
    // children closure, a paint pass, another effect — is not handed the
    // dependencies the memo exists to absorb.
    //
    // The signal is made afterwards, in the caller's scope, from the value
    // that run left behind. A memo created outside any scope keeps updating
    // for as long as the application runs.
    let seed = Rc::new(RefCell::new(None));
    let target = Rc::new(Cell::new(None::<RwSignal<T>>));
    let (first, later) = (seed.clone(), target.clone());
    create_memo_effect(move || {
        let value = f();
        match later.get() {
            // `set` compares with `PartialEq`, so an unchanged value notifies nobody.
            Some(signal) => signal.set(value),
            None => *first.borrow_mut() = Some(value),
        }
    });
    let initial = seed.take().expect("a memo's effect runs when it is made");
    let signal = create_signal(initial);
    target.set(Some(signal));
    Memo { signal }
}

impl<T: Clone + PartialEq + Send + 'static> Memo<T> {
    /// Get the current memo value (tracked for dependency tracking).
    pub fn get(&self) -> T {
        self.signal.get()
    }

    /// Borrow the current value (tracked for dependency tracking).
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.signal.with(f)
    }

    /// Watch this memo from a background task.
    ///
    /// See [`RwSignal::watch`](super::RwSignal::watch). A memo is usually what
    /// you want to watch: it only notifies when the derived value actually
    /// changes, so the task wakes on real transitions.
    pub fn watch(&self) -> tokio::sync::watch::Receiver<T>
    where
        T: Send + Sync,
    {
        self.signal.watch()
    }

    /// Extract as a read-only signal.
    pub fn into_signal(self) -> Signal<T> {
        self.signal.read_only()
    }
}

impl<T: Clone + PartialEq + Send + 'static> IntoSignal<T, MemoMarker> for Memo<T> {
    fn into_signal(self) -> Signal<T> {
        self.signal.read_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memo_initial_value() {
        let signal = create_signal(5);
        let memo = create_memo(move || signal.get() * 2);
        assert_eq!(memo.get(), 10);
    }

    #[test]
    fn test_memo_is_copy() {
        let signal = create_signal(1);
        let memo = create_memo(move || signal.get());
        let memo2 = memo; // Copy
        assert_eq!(memo.get(), 1);
        assert_eq!(memo2.get(), 1);
    }

    #[test]
    fn test_memo_with() {
        let signal = create_signal(String::from("hello"));
        let memo = create_memo(move || signal.get());
        let len = memo.with(|s| s.len());
        assert_eq!(len, 5);
    }

    #[test]
    fn test_memo_into_signal() {
        let signal = create_signal(7);
        let memo = create_memo(move || signal.get() + 3);
        let sig: Signal<i32> = memo.into_signal();
        assert_eq!(sig.get(), 10);
    }

    /// Regression test: effects depending on a memo must re-run when the
    /// memo recomputes. A memo is "set a signal inside an effect", which was
    /// silently dropped while the runtime borrow was held across callbacks.
    #[test]
    fn test_effect_depending_on_memo_reruns() {
        use std::cell::Cell;
        use std::rc::Rc;

        let count = create_signal(1);
        let doubled = create_memo(move || count.get() * 2);

        let observed = Rc::new(Cell::new(0));
        let observed_c = observed.clone();
        crate::reactive::create_effect(move || {
            observed_c.set(doubled.get());
        });

        assert_eq!(observed.get(), 2);

        count.set(5);
        assert_eq!(observed.get(), 10, "memo change must propagate to effects");
    }

    /// A memo built inside a widget's tracked scope must not hand that widget
    /// its dependencies. This is the whole point of memoizing: a component
    /// created inside a dynamic-children closure that memoizes a hot signal
    /// (a per-frame audio level, a clock tick) must not make the closure —
    /// and with it the entire subtree it builds — rebuild on every write.
    #[test]
    fn memo_created_in_a_tracked_scope_does_not_leak_its_dependencies() {
        use crate::jobs::{self, JobType};
        use crate::reactive::invalidation::with_signal_tracking;
        use crate::tree::WidgetId;

        let hot = create_signal(0);
        let wid = WidgetId::from_u64(4242);

        let memo = with_signal_tracking(wid, JobType::Reconcile, || {
            create_memo(move || hot.get() * 2)
        });
        assert_eq!(memo.get(), 0);

        // Discard anything queued while building
        jobs::clear_pending_jobs();

        hot.set(21);

        assert!(
            !jobs::has_pending_jobs(),
            "writing a signal only the memo reads must not invalidate the \
             widget whose scope happened to build the memo"
        );
        assert_eq!(memo.get(), 42, "the memo itself must still track it");
    }

    /// Same isolation for the effect that happens to be running: creating a
    /// memo inside an effect must not add the memo's dependencies to it.
    #[test]
    fn memo_created_inside_an_effect_does_not_leak_its_dependencies() {
        use std::cell::Cell;
        use std::rc::Rc;

        let hot = create_signal(0);
        let trigger = create_signal(0);
        let runs = Rc::new(Cell::new(0));
        let runs_c = runs.clone();

        crate::reactive::create_effect(move || {
            trigger.get();
            runs_c.set(runs_c.get() + 1);
            let _memo = create_memo(move || hot.get() * 2);
        });

        assert_eq!(runs.get(), 1);

        hot.set(1);
        assert_eq!(
            runs.get(),
            1,
            "the enclosing effect must not depend on what the memo reads"
        );

        trigger.set(1);
        assert_eq!(runs.get(), 2, "its own dependencies must still fire");
    }

    /// `f` runs once when the memo is made, in the memo's own scope, so what
    /// that run registers goes when the memo recomputes and not with the scope
    /// that made the memo.
    #[test]
    fn a_memo_runs_once_at_creation_and_its_first_run_is_disposed_on_the_next() {
        use crate::reactive::owner::{on_cleanup, with_owner};

        let source = create_signal(0);
        let calls = Rc::new(Cell::new(0));
        let cleanups = Rc::new(Cell::new(0));
        let (calls_f, cleanups_f) = (calls.clone(), cleanups.clone());
        let (memo, _scope) = with_owner(|| {
            create_memo(move || {
                calls_f.set(calls_f.get() + 1);
                let cleanups = cleanups_f.clone();
                on_cleanup(move || cleanups.set(cleanups.get() + 1));
                source.get()
            })
        });
        assert_eq!(calls.get(), 1, "f runs once when the memo is made");
        assert_eq!(memo.get(), 0);

        let registered = calls.get();
        source.set(1);
        assert_eq!(
            cleanups.get(),
            registered,
            "every cleanup registered before the change has run"
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(memo.get(), 1);
    }

    /// A recompute that lands on the same value notifies nobody.
    #[test]
    fn a_memo_that_recomputes_the_same_value_does_not_notify() {
        let source = create_signal(1);
        let parity = create_memo(move || source.get() % 2);
        let runs = Rc::new(Cell::new(0));
        let runs_c = runs.clone();
        crate::reactive::create_effect(move || {
            parity.get();
            runs_c.set(runs_c.get() + 1);
        });

        source.set(3);
        assert_eq!(runs.get(), 1, "the value did not change");
        source.set(4);
        assert_eq!(runs.get(), 2);
    }

    /// Memo-of-memo chains must propagate through both levels.
    #[test]
    fn test_memo_chains() {
        let base = create_signal(1);
        let doubled = create_memo(move || base.get() * 2);
        let quadrupled = create_memo(move || doubled.get() * 2);

        assert_eq!(quadrupled.get(), 4);

        base.set(3);
        assert_eq!(doubled.get(), 6);
        assert_eq!(quadrupled.get(), 12, "second-level memo must recompute");
    }

    /// What an effect reading `len` and `index` saw on each run.
    type Seen = std::rc::Rc<std::cell::RefCell<Vec<(i32, i32)>>>;

    /// An effect that records `(len, index)` each time it runs.
    fn record(len: RwSignal<i32>, index: Memo<i32>) -> Seen {
        let seen = Seen::default();
        let sink = seen.clone();
        crate::reactive::create_effect(move || sink.borrow_mut().push((len.get(), index.get())));
        seen
    }

    /// A list that shrinks under a memo clamping an index into it: the effect
    /// reading both must never see the new length beside the old index. It
    /// once did whenever another effect also read the length, because the memo
    /// was queued behind it like any effect, and `items[index]` would panic.
    #[test]
    fn an_effect_never_sees_a_memo_older_than_the_signal_it_reads() {
        for other_readers in 1..=2 {
            let len = create_signal(5);
            let selected = create_signal(4);
            let clamped = create_memo(move || selected.get().min(len.get() - 1));
            let seen = record(len, clamped);
            for _ in 0..other_readers {
                crate::reactive::create_effect(move || {
                    len.get();
                });
            }

            for shorter in [4, 3, 2, 1] {
                len.set(shorter);
            }

            let seen = seen.borrow();
            assert!(
                seen.iter().all(|&(len, index)| index < len),
                "with {other_readers} other reader(s), an index was not below its length: {seen:?}"
            );
            assert_eq!(
                *seen,
                [(5, 4), (4, 3), (3, 2), (2, 1), (1, 0)],
                "with {other_readers} other reader(s), one run per write"
            );
        }
    }

    /// The same through a memo of a memo: the second memo is queued only when
    /// the first one changes, after the effect that reads it already was, and
    /// still recomputes before that effect runs.
    #[test]
    fn a_memo_of_a_memo_recomputes_before_the_effect_reading_it() {
        let len = create_signal(5);
        let selected = create_signal(4);
        let clamped = create_memo(move || selected.get().min(len.get() - 1));
        let index = create_memo(move || clamped.get());
        let seen = record(len, index);
        crate::reactive::create_effect(move || {
            len.get();
        });

        for shorter in [4, 3, 2, 1] {
            len.set(shorter);
        }

        assert_eq!(*seen.borrow(), [(5, 4), (4, 3), (3, 2), (2, 1), (1, 0)]);
    }

    /// Resuming a paused scope queues what it missed, and a memo among them
    /// still goes before the effect reading it. The memo is made in a scope of
    /// its own so that the walk hands the effect back first.
    #[test]
    fn a_resumed_memo_runs_before_the_resumed_effect_reading_it() {
        use crate::reactive::owner::{pause_owner, resume_owner, with_owner};

        let len = create_signal(5);
        let selected = create_signal(4);
        let (seen, scope) = with_owner(|| {
            let (clamped, _) =
                with_owner(|| create_memo(move || selected.get().min(len.get() - 1)));
            record(len, clamped)
        });

        pause_owner(scope);
        len.set(4);
        resume_owner(scope);

        assert_eq!(*seen.borrow(), [(5, 4), (4, 3)]);
    }

    /// A diamond among memos: `pair` reads `len` and `clamped`, which reads
    /// `len` too. Memos run in the order they were notified, so `pair` itself
    /// may compute once with the new length and the old index — that is the
    /// limit of running memos first rather than by depth, and nothing here
    /// claims otherwise. What holds is the effect's side: it runs after every
    /// memo has settled, once per write, and never sees that pair.
    #[test]
    fn an_effect_below_a_diamond_of_memos_sees_only_settled_values() {
        let len = create_signal(5);
        let selected = create_signal(4);
        let clamped = create_memo(move || selected.get().min(len.get() - 1));
        let pair = create_memo(move || (len.get(), clamped.get()));
        let seen = Seen::default();
        let sink = seen.clone();
        crate::reactive::create_effect(move || sink.borrow_mut().push(pair.get()));

        for shorter in [4, 3, 2, 1] {
            len.set(shorter);
        }

        assert_eq!(*seen.borrow(), [(5, 4), (4, 3), (3, 2), (2, 1), (1, 0)]);
    }
}
