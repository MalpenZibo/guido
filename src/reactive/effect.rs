use super::owner::register_effect;
use super::runtime::{EffectId, run_effect_by_id, with_runtime};

/// Run `f` now, and again whenever a signal it read changes.
///
/// The effect belongs to the enclosing scope — the surface's, a component's,
/// or the root one `App::run` opens — and is disposed with it. Created outside
/// any scope it runs for the lifetime of the application.
///
/// ```ignore
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
    let id = with_runtime(|rt| rt.allocate_effect(Box::new(f)));

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
    #[test]
    fn test_signal_created_inside_effect_is_reactive() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let trigger = create_signal(0);
        let created: Rc<RefCell<Option<crate::reactive::RwSignal<i32>>>> =
            Rc::new(RefCell::new(None));

        let created_in_effect = created.clone();
        create_effect(move || {
            let t = trigger.get();
            if created_in_effect.borrow().is_none() {
                *created_in_effect.borrow_mut() = Some(create_signal(t));
            }
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
}
