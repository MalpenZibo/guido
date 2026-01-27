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
