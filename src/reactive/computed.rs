use std::sync::Arc;

use super::effect::Effect;
use super::signal::Signal;

struct ComputedInner<T> {
    signal: Signal<T>,
    _effect: Effect,
}

/// A computed value that derives from other signals.
///
/// Computed values automatically update when their dependencies change.
/// They are read-only - you cannot directly set their value.
#[derive(Clone)]
pub struct Computed<T> {
    inner: Arc<ComputedInner<T>>,
}

// Computed is Send + Sync when T is Send + Sync
// Note: Effect is not Send, so Computed itself cannot be sent across threads
// but the underlying signal value can still be read from any thread

impl<T: Clone + 'static> Computed<T> {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> T + 'static,
    {
        let initial = f();
        let signal = Signal::new(initial);
        let signal_clone = signal.clone();

        let effect = Effect::new(move || {
            let value = f();
            signal_clone.set(value);
        });

        Self {
            inner: Arc::new(ComputedInner {
                signal,
                _effect: effect,
            }),
        }
    }

    pub fn get(&self) -> T {
        self.inner.signal.get()
    }

    pub fn get_untracked(&self) -> T {
        self.inner.signal.get_untracked()
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        self.inner.signal.with(f)
    }
}

pub fn create_computed<T, F>(f: F) -> Computed<T>
where
    T: Clone + 'static,
    F: Fn() -> T + 'static,
{
    Computed::new(f)
}
