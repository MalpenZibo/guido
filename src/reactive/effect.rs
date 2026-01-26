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

        Self { id }
    }
}

impl Drop for Effect {
    fn drop(&mut self) {
        with_runtime(|rt| rt.dispose_effect(self.id));
    }
}

pub fn create_effect<F>(f: F) -> Effect
where
    F: FnMut() + 'static,
{
    Effect::new(f)
}
