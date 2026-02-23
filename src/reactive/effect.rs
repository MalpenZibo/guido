use super::owner::{effect_has_owner, register_effect};
use super::runtime::{EffectId, with_runtime};

pub struct Effect {
    id: EffectId,
}

impl Effect {
    pub fn new<F>(f: F) -> Self
    where
        F: FnMut() + 'static,
    {
        let id = with_runtime(|rt| {
            let id = rt.allocate_effect(Box::new(f));
            rt.run_effect(id);
            id
        });

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
}
