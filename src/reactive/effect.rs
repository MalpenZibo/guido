use super::owner::{child_scope, register_effect};
use super::runtime::{EffectId, run_effect_by_id, with_runtime};

/// Run `f` now, and again whenever a signal it read changes.
///
/// The effect belongs to the enclosing scope — the surface's, a component's,
/// or the root one `App::run` opens — and is disposed with it. Created outside
/// any scope it runs for the lifetime of the application.
///
/// Each run owns what it makes. A signal, effect, timer, task or
/// [`on_cleanup`](super::on_cleanup) created in the body is disposed before
/// the next run, and with the effect. A body that makes something to keep
/// makes it under a scope captured outside, with
/// [`OwnerId::run`](super::OwnerId::run).
///
/// ```no_run
/// # use guido::prelude::*;
/// # let count = create_signal(0);
/// create_effect(move || println!("count is {}", count.get()));
/// ```
///
/// There is deliberately no handle to keep: an effect's lifetime is its
/// scope's. To end one earlier, put it in a scope that ends earlier.
pub fn create_effect<F>(f: F)
where
    F: FnMut() + 'static,
{
    create_effect_id(f);
}

/// [`create_effect`], returning the id.
///
/// Only the ownership tests reach for this: nothing in the running library needs
/// an effect's id, because nothing disposes an effect other than the scope that
/// owns it. It exists so those tests can ask whether registration happened —
/// which is only worth asking if it is the *same* three steps the real path
/// takes, in the same order. So `create_effect` delegates here rather than the
/// two keeping their own copies.
pub(crate) fn create_effect_id<F>(f: F) -> EffectId
where
    F: FnMut() + 'static,
{
    let scope = child_scope();
    let id = with_runtime(|rt| rt.allocate_effect(Box::new(f), scope));

    // Initial run establishes dependencies. Runs outside the runtime borrow, so
    // the callback can freely write signals or create new ones.
    run_effect_by_id(id);

    // Register with the current owner for automatic cleanup
    register_effect(id);

    id
}

#[cfg(test)]
mod tests {
    use super::super::signal::create_signal;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// An effect created where nobody owns it survives the statement that
    /// created it. It used to depend on a guard the caller had to remember to
    /// `detach()`, so forgetting silently produced an effect that never ran
    /// again.
    #[test]
    fn an_unowned_effect_keeps_running() {
        let signal = create_signal(0);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        create_effect(move || {
            let _ = signal.get();
            ran_clone.store(true, Ordering::SeqCst);
        });

        assert!(ran.load(Ordering::SeqCst), "it runs once on creation");

        // Trigger re-run by changing signal
        ran.store(false, Ordering::SeqCst);
        signal.set(1);

        // Effect should still be alive and re-run
        assert!(ran.load(Ordering::SeqCst));
    }

    /// Regression test: a signal write performed INSIDE an effect must
    /// propagate to other effects. Previously the runtime RefCell was held
    /// across effect callbacks, so the nested notify was silently dropped
    /// and effect→effect chains never fired.
    #[test]
    fn test_write_inside_effect_triggers_other_effects() {
        use std::cell::Cell;
        use std::rc::Rc;

        let source = create_signal(0);
        let intermediate = create_signal(0);
        let observed = Rc::new(Cell::new(-1));

        // Effect A: writes `intermediate` whenever `source` changes
        create_effect(move || {
            intermediate.set(source.get() + 1);
        });

        // Effect B: observes `intermediate`
        let observed_b = observed.clone();
        create_effect(move || {
            observed_b.set(intermediate.get());
        });

        assert_eq!(observed.get(), 1, "initial chain should have run");

        source.set(10);
        assert_eq!(
            observed.get(),
            11,
            "write inside effect A must re-run effect B"
        );
    }

    /// Signals created inside an effect must be fully reactive. Previously
    /// runtime registration was silently skipped during effect execution,
    /// leaving the signal permanently unable to notify — and arming an
    /// out-of-bounds panic on the effect's next run.
    ///
    /// The signal is kept past the run that made it, so it is made under the
    /// scope outside the effect.
    #[test]
    fn test_signal_created_inside_effect_is_reactive() {
        use crate::reactive::owner::{current_owner, with_owner};
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let trigger = create_signal(0);
        let created: Rc<RefCell<Option<crate::reactive::RwSignal<i32>>>> =
            Rc::new(RefCell::new(None));

        let created_in_effect = created.clone();
        with_owner(|| {
            let outer = current_owner().unwrap();
            create_effect(move || {
                let t = trigger.get();
                if created_in_effect.borrow().is_none() {
                    *created_in_effect.borrow_mut() = Some(outer.run(|| create_signal(t)));
                }
            });
        });

        // Re-run the effect a few times (previously panicked out-of-bounds)
        trigger.set(1);
        trigger.set(2);

        let inner = created.borrow().expect("signal created inside effect");
        let observed = Rc::new(Cell::new(-1));
        let observed_c = observed.clone();
        create_effect(move || {
            observed_c.set(inner.get());
        });

        inner.set(42);
        assert_eq!(
            observed.get(),
            42,
            "signal created inside an effect must notify its subscribers"
        );
    }

    /// Each run's `on_cleanup` runs before the next run, and the last one runs
    /// when the scope that owns the effect goes.
    #[test]
    fn a_cleanup_registered_by_a_run_runs_before_the_next() {
        use crate::reactive::owner::{dispose_owner_now, on_cleanup, with_owner};
        use std::cell::RefCell;
        use std::rc::Rc;

        let trigger = create_signal(0);
        let log = Rc::new(RefCell::new(Vec::new()));
        let in_effect = log.clone();
        let ((), scope) = with_owner(|| {
            create_effect(move || {
                let run = trigger.get();
                in_effect.borrow_mut().push(format!("run {run}"));
                let in_cleanup = in_effect.clone();
                on_cleanup(move || in_cleanup.borrow_mut().push(format!("cleanup {run}")));
            });
        });
        trigger.set(1);
        trigger.set(2);
        dispose_owner_now(scope);

        assert_eq!(
            *log.borrow(),
            [
                "run 0",
                "cleanup 0",
                "run 1",
                "cleanup 1",
                "run 2",
                "cleanup 2"
            ]
        );
    }

    /// A signal or an effect one run makes is gone once the next has started.
    #[test]
    fn what_a_run_creates_is_disposed_by_the_next() {
        use crate::reactive::Signal;
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let trigger = create_signal(0);
        let nested_source = create_signal(0);
        let made: Rc<RefCell<Vec<Signal<i32>>>> = Rc::default();
        let nested_runs = Rc::new(Cell::new(0));

        let in_effect = made.clone();
        let counted = nested_runs.clone();
        create_effect(move || {
            in_effect
                .borrow_mut()
                .push(create_signal(trigger.get()).read_only());
            let counted = counted.clone();
            create_effect(move || {
                nested_source.get();
                counted.set(counted.get() + 1);
            });
        });
        let first = made.borrow()[0];
        assert!(first.is_live());
        assert_eq!(nested_runs.get(), 1);

        trigger.set(1);
        assert!(!first.is_live(), "the first run's signal went with it");
        assert!(
            made.borrow()[1].is_live(),
            "the second run's is still there"
        );
        assert_eq!(
            nested_runs.get(),
            2,
            "one nested effect, made by the new run"
        );

        nested_source.set(1);
        assert_eq!(
            nested_runs.get(),
            3,
            "only the newest nested effect runs; the first run's is gone"
        );
    }

    /// The last run's scope belongs to the effect's owner, and goes with it.
    #[test]
    fn disposing_the_owner_disposes_the_last_run() {
        use crate::reactive::Signal;
        use crate::reactive::owner::{dispose_owner_now, with_owner};
        use std::cell::Cell;
        use std::rc::Rc;

        let trigger = create_signal(0);
        let made: Rc<Cell<Option<Signal<i32>>>> = Rc::default();
        let in_effect = made.clone();
        let ((), scope) = with_owner(|| {
            create_effect(move || in_effect.set(Some(create_signal(trigger.get()).read_only())));
        });
        trigger.set(1);
        let last = made.get().unwrap();
        assert!(last.is_live());

        dispose_owner_now(scope);
        assert!(!last.is_live());
    }

    /// What a run makes under a scope captured from outside lives as long as
    /// that scope, not as long as the run.
    #[test]
    fn the_opt_out_keeps_what_it_creates() {
        use crate::reactive::Signal;
        use crate::reactive::owner::{current_owner, dispose_owner_now, with_owner};
        use std::cell::RefCell;
        use std::rc::Rc;

        let trigger = create_signal(0);
        let made: Rc<RefCell<Vec<Signal<i32>>>> = Rc::default();
        let in_effect = made.clone();
        let ((), outer) = with_owner(|| {
            let outer = current_owner().unwrap();
            create_effect(move || {
                let kept = outer.run(|| create_signal(trigger.get()));
                in_effect.borrow_mut().push(kept.read_only());
            });
        });
        trigger.set(1);
        trigger.set(2);
        assert!(made.borrow().iter().all(Signal::is_live));

        dispose_owner_now(outer);
        assert!(!made.borrow().iter().any(Signal::is_live));
    }

    /// Emptying a run's scope is disposing it: what the run's nested scopes
    /// hold first, then the run's own cleanups last-first, then its effects
    /// and signals — so a cleanup can still read a signal its run made.
    #[test]
    fn cleanups_run_children_first() {
        use crate::reactive::owner::{on_cleanup, with_owner};
        use std::cell::RefCell;
        use std::rc::Rc;

        let trigger = create_signal(0);
        let log = Rc::new(RefCell::new(Vec::new()));
        let in_effect = log.clone();
        create_effect(move || {
            if trigger.get() > 0 {
                return;
            }
            let pushed = |what: &'static str| {
                let log = in_effect.clone();
                move || log.borrow_mut().push(what)
            };
            on_cleanup(pushed("first"));
            with_owner(|| on_cleanup(pushed("child")));
            let made = create_signal("signal still live");
            let log = in_effect.clone();
            on_cleanup(move || log.borrow_mut().push(made.get()));
            on_cleanup(pushed("second"));
        });
        assert!(log.borrow().is_empty());

        trigger.set(1);
        assert_eq!(
            *log.borrow(),
            ["child", "second", "signal still live", "first"]
        );
    }
}
