use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::new());
}

pub type SignalId = usize;
pub type EffectId = usize;

#[derive(Default)]
pub struct Runtime {
    current_effect: Option<EffectId>,
    pending_effects: HashSet<EffectId>,
    effect_callbacks: Vec<Option<Box<dyn FnMut()>>>,
    effect_dependencies: Vec<HashSet<SignalId>>,
    signal_subscribers: Vec<HashSet<EffectId>>,
    next_signal_id: SignalId,
    next_effect_id: EffectId,
    batch_depth: usize,
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allocate_signal(&mut self) -> SignalId {
        let id = self.next_signal_id;
        self.next_signal_id += 1;
        self.signal_subscribers.push(HashSet::new());
        id
    }

    pub fn allocate_effect(&mut self, callback: Box<dyn FnMut()>) -> EffectId {
        let id = self.next_effect_id;
        self.next_effect_id += 1;
        self.effect_callbacks.push(Some(callback));
        self.effect_dependencies.push(HashSet::new());
        id
    }

    pub fn track_read(&mut self, signal_id: SignalId) {
        // Check if this signal exists in our runtime (it might not if called from another thread)
        if signal_id >= self.signal_subscribers.len() {
            return;
        }

        if let Some(effect_id) = self.current_effect {
            self.signal_subscribers[signal_id].insert(effect_id);
            self.effect_dependencies[effect_id].insert(signal_id);
        }
    }

    pub fn notify_write(&mut self, signal_id: SignalId) {
        // Check if this signal exists in our runtime (it might not if called from another thread)
        if signal_id >= self.signal_subscribers.len() {
            return;
        }

        let subscribers: Vec<_> = self.signal_subscribers[signal_id].iter().copied().collect();
        for effect_id in subscribers {
            self.pending_effects.insert(effect_id);
        }

        if self.batch_depth == 0 {
            self.flush_effects();
        }
    }

    pub fn run_effect(&mut self, effect_id: EffectId) {
        // Clear old dependencies
        let old_deps = std::mem::take(&mut self.effect_dependencies[effect_id]);
        for signal_id in old_deps {
            self.signal_subscribers[signal_id].remove(&effect_id);
        }

        // Run effect with tracking
        let prev_effect = self.current_effect;
        self.current_effect = Some(effect_id);

        if let Some(callback) = self.effect_callbacks[effect_id].as_mut() {
            callback();
        }

        self.current_effect = prev_effect;
    }

    pub fn flush_effects(&mut self) {
        while !self.pending_effects.is_empty() {
            let effects: Vec<_> = self.pending_effects.drain().collect();
            for effect_id in effects {
                self.run_effect(effect_id);
            }
        }
    }

    pub fn batch<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.batch_depth += 1;
        let result = f();
        self.batch_depth -= 1;

        if self.batch_depth == 0 {
            self.flush_effects();
        }

        result
    }

    pub fn dispose_effect(&mut self, effect_id: EffectId) {
        // Clear dependencies
        let deps = std::mem::take(&mut self.effect_dependencies[effect_id]);
        for signal_id in deps {
            if signal_id < self.signal_subscribers.len() {
                self.signal_subscribers[signal_id].remove(&effect_id);
            }
        }
        self.effect_callbacks[effect_id] = None;
        self.pending_effects.remove(&effect_id);
    }
}

pub fn with_runtime<F, R>(f: F) -> R
where
    F: FnOnce(&mut Runtime) -> R,
{
    RUNTIME.with(|rt| f(&mut rt.borrow_mut()))
}

/// Try to access the runtime. This is safe to call from any thread.
/// On the main thread, runs the callback. On other threads, does nothing.
/// This enables signals to be updated from background threads without panicking.
pub fn try_with_runtime<F>(f: F)
where
    F: FnOnce(&mut Runtime),
{
    RUNTIME.with(|rt| {
        if let Ok(mut runtime) = rt.try_borrow_mut() {
            f(&mut runtime);
        }
        // If borrow fails (already borrowed), skip - this can happen during effect execution
    });
}

pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    with_runtime(|rt| rt.batch(f))
}
