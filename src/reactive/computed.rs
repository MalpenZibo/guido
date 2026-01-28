use std::cell::Cell;
use std::sync::Arc;

use super::owner::register_effect;
use super::runtime::{EffectId, with_runtime};
use super::signal::{Signal, create_signal};

struct ComputedInner<T> {
    /// Cached value storage
    signal: Signal<T>,
    /// Effect ID for dependency tracking
    effect_id: EffectId,
    /// Dirty flag - set true when dependencies change
    dirty: Cell<bool>,
    /// The computation closure
    compute: Box<dyn Fn() -> T>,
}

/// A computed value that derives from other signals.
///
/// Computed values use lazy evaluation - they only recompute when:
/// 1. A dependency has changed (marked dirty)
/// 2. The value is actually read via `.get()`
///
/// This avoids unnecessary computation when dependencies change but
/// the computed value isn't being read.
#[derive(Clone)]
pub struct Computed<T> {
    inner: Arc<ComputedInner<T>>,
}

impl<T: Clone + PartialEq + Send + Sync + 'static> Computed<T> {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> T + 'static,
    {
        // Compute initial value (without tracking yet)
        let initial = f();
        let signal = create_signal(initial);

        // Allocate effect slot for dependency tracking
        // The callback just marks dirty - actual computation is lazy
        let effect_id = with_runtime(|rt| rt.allocate_effect(Box::new(|| {})));

        let inner = Arc::new(ComputedInner {
            signal,
            effect_id,
            dirty: Cell::new(false),
            compute: Box::new(f),
        });

        // Set up the effect callback to mark dirty
        let inner_weak = Arc::downgrade(&inner);
        with_runtime(|rt| {
            rt.set_effect_callback(
                effect_id,
                Box::new(move || {
                    if let Some(inner) = inner_weak.upgrade() {
                        inner.dirty.set(true);
                    }
                }),
            );

            // Run computation with tracking to establish dependencies
            // We already have the value, but we need to track which signals were read
            rt.run_with_tracking(effect_id, || {
                let _ = (inner.compute)();
            });
        });

        // Register with current owner for automatic cleanup
        register_effect(effect_id);

        Self { inner }
    }

    pub fn get(&self) -> T {
        // If dirty, recompute before returning
        if self.inner.dirty.get() {
            let value = with_runtime(|rt| {
                rt.run_with_tracking(self.inner.effect_id, || (self.inner.compute)())
            });
            self.inner.signal.set(value);
            self.inner.dirty.set(false);
        }
        self.inner.signal.get()
    }

    pub fn get_untracked(&self) -> T {
        // If dirty, recompute before returning
        if self.inner.dirty.get() {
            let value = with_runtime(|rt| {
                rt.run_with_tracking(self.inner.effect_id, || (self.inner.compute)())
            });
            self.inner.signal.set(value);
            self.inner.dirty.set(false);
        }
        self.inner.signal.get_untracked()
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        // If dirty, recompute before accessing
        if self.inner.dirty.get() {
            let value = with_runtime(|rt| {
                rt.run_with_tracking(self.inner.effect_id, || (self.inner.compute)())
            });
            self.inner.signal.set(value);
            self.inner.dirty.set(false);
        }
        self.inner.signal.with(f)
    }
}

pub fn create_computed<T, F>(f: F) -> Computed<T>
where
    T: Clone + PartialEq + Send + Sync + 'static,
    F: Fn() -> T + 'static,
{
    Computed::new(f)
}
