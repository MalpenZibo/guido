use super::owner::{effect_has_owner, register_effect};
use super::runtime::{EffectId, run_effect_by_id, with_runtime};

pub struct Effect {
    id: EffectId,
}

impl Effect {
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        let id = with_runtime(|rt| rt.allocate_effect(Box::new(f)));

        // Initial run establishes dependencies. Runs outside the runtime
        // borrow, so the callback can freely write signals or create new ones.
        run_effect_by_id(id);

        // Register with current owner for automatic cleanup
        register_effect(id);

        Self { id }
    }

    /// Detach this effect from automatic cleanup.
    ///
    /// The effect will run for the lifetime of the application.
    /// Use this for effects created outside of widget/owner scopes
    /// (e.g. in `main()`) that should persist indefinitely.
    ///
    /// # Example
    ///
    /// ```ignore
    /// create_effect(move || {
    ///     println!("Signal changed: {}", my_signal.get());
    /// }).detach();
    /// ```
    pub fn detach(self) {
        std::mem::forget(self);
    }

    /// Get the effect's ID.
    /// Used internally for testing the ownership system.
    #[cfg(test)]
    pub(crate) fn id(&self) -> EffectId {
        self.id
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        // Only dispose if not owned - owned effects are disposed by their owner.
        // This prevents double disposal and allows the owner to control cleanup order.
        if !effect_has_owner(self.id) {
            with_runtime(|rt| rt.dispose_effect(self.id));
        }
    }
}

pub fn create_effect<F>(f: F) -> Effect
where
    F: FnMut() + 'static,
{
    Effect::new(f)
}

#[cfg(test)]
mod tests {
    use super::super::signal::create_signal;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_effect_detach_prevents_disposal() {
        let signal = create_signal(0);
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();

        // Create and immediately detach — effect should survive
        create_effect(move || {
            let _ = signal.get();
            ran_clone.store(true, Ordering::SeqCst);
        })
        .detach();

        // Effect ran during creation and was not disposed
        assert!(ran.load(Ordering::SeqCst));

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
        })
        .detach();

        // Effect B: observes `intermediate`
        let observed_b = observed.clone();
        create_effect(move || {
            observed_b.set(intermediate.get());
        })
        .detach();

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
        })
        .detach();

        // Re-run the effect a few times (previously panicked out-of-bounds)
        trigger.set(1);
        trigger.set(2);

        let inner = created.borrow().expect("signal created inside effect");
        let observed = Rc::new(Cell::new(-1));
        let observed_c = observed.clone();
        create_effect(move || {
            observed_c.set(inner.get());
        })
        .detach();

        inner.set(42);
        assert_eq!(
            observed.get(),
            42,
            "signal created inside an effect must notify its subscribers"
        );
    }
}
