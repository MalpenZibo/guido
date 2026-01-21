use std::sync::Arc;

use super::signal::{ReadSignal, Signal};

/// A value that can be either static or dynamic (reactive).
/// This allows widget properties to accept both plain values and signals.
pub enum MaybeDyn<T: 'static> {
    Static(T),
    Dynamic(Arc<dyn Fn() -> T + Send + Sync>),
}

impl<T: Clone + 'static> MaybeDyn<T> {
    /// Get the current value. If dynamic, this will track the signal read.
    pub fn get(&self) -> T {
        match self {
            MaybeDyn::Static(v) => v.clone(),
            MaybeDyn::Dynamic(f) => f(),
        }
    }

    /// Create a static MaybeDyn
    pub fn fixed(value: T) -> Self {
        MaybeDyn::Static(value)
    }

    /// Create a dynamic MaybeDyn from a closure
    pub fn dynamic<F: Fn() -> T + Send + Sync + 'static>(f: F) -> Self {
        MaybeDyn::Dynamic(Arc::new(f))
    }
}

impl<T: Clone + 'static> Clone for MaybeDyn<T> {
    fn clone(&self) -> Self {
        match self {
            MaybeDyn::Static(v) => MaybeDyn::Static(v.clone()),
            MaybeDyn::Dynamic(f) => MaybeDyn::Dynamic(f.clone()),
        }
    }
}

// MaybeDyn is Send + Sync when T is Send + Sync
unsafe impl<T: Send + Sync + 'static> Send for MaybeDyn<T> {}
unsafe impl<T: Send + Sync + 'static> Sync for MaybeDyn<T> {}

/// Trait for types that can be converted into MaybeDyn<T>
pub trait IntoMaybeDyn<T: Clone + 'static> {
    fn into_maybe_dyn(self) -> MaybeDyn<T>;
}

// ============================================================================
// Static value implementations for specific types
// (We can't use a blanket impl because it would conflict with the closure impl)
// ============================================================================

impl IntoMaybeDyn<String> for String {
    fn into_maybe_dyn(self) -> MaybeDyn<String> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<String> for &str {
    fn into_maybe_dyn(self) -> MaybeDyn<String> {
        MaybeDyn::Static(self.to_string())
    }
}

impl IntoMaybeDyn<f32> for f32 {
    fn into_maybe_dyn(self) -> MaybeDyn<f32> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<f64> for f64 {
    fn into_maybe_dyn(self) -> MaybeDyn<f64> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<i32> for i32 {
    fn into_maybe_dyn(self) -> MaybeDyn<i32> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<u32> for u32 {
    fn into_maybe_dyn(self) -> MaybeDyn<u32> {
        MaybeDyn::Static(self)
    }
}

impl IntoMaybeDyn<bool> for bool {
    fn into_maybe_dyn(self) -> MaybeDyn<bool> {
        MaybeDyn::Static(self)
    }
}

// ============================================================================
// Closure implementation - works for any Fn() -> T
// ============================================================================

impl<T, F> IntoMaybeDyn<T> for F
where
    T: Clone + Send + Sync + 'static,
    F: Fn() -> T + Send + Sync + 'static,
{
    fn into_maybe_dyn(self) -> MaybeDyn<T> {
        MaybeDyn::Dynamic(Arc::new(self))
    }
}

// ============================================================================
// Signal implementations
// ============================================================================

impl<T: Clone + Send + Sync + 'static> IntoMaybeDyn<T> for Signal<T> {
    fn into_maybe_dyn(self) -> MaybeDyn<T> {
        MaybeDyn::Dynamic(Arc::new(move || self.get()))
    }
}

impl<T: Clone + Send + Sync + 'static> IntoMaybeDyn<T> for ReadSignal<T> {
    fn into_maybe_dyn(self) -> MaybeDyn<T> {
        MaybeDyn::Dynamic(Arc::new(move || self.get()))
    }
}

// ============================================================================
// Already a MaybeDyn
// ============================================================================

impl<T: Clone + 'static> IntoMaybeDyn<T> for MaybeDyn<T> {
    fn into_maybe_dyn(self) -> MaybeDyn<T> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal::create_signal;

    #[test]
    fn test_fixed_returns_static_value() {
        let value = MaybeDyn::fixed(42);
        assert_eq!(value.get(), 42);
        assert_eq!(value.get(), 42); // Multiple gets return same value
    }

    #[test]
    fn test_dynamic_calls_closure() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let counter_clone = counter.clone();
        let value = MaybeDyn::dynamic(move || {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
        });
        // Each get() calls the closure
        assert_eq!(value.get(), 1);
        assert_eq!(value.get(), 2);
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_into_maybe_dyn_for_string() {
        let value: MaybeDyn<String> = "hello".into_maybe_dyn();
        assert_eq!(value.get(), "hello");

        let value: MaybeDyn<String> = String::from("world").into_maybe_dyn();
        assert_eq!(value.get(), "world");
    }

    #[test]
    fn test_into_maybe_dyn_for_f32() {
        let value: MaybeDyn<f32> = 2.5f32.into_maybe_dyn();
        assert_eq!(value.get(), 2.5);
    }

    #[test]
    fn test_into_maybe_dyn_for_bool() {
        let value: MaybeDyn<bool> = true.into_maybe_dyn();
        assert!(value.get());

        let value: MaybeDyn<bool> = false.into_maybe_dyn();
        assert!(!value.get());
    }

    #[test]
    fn test_into_maybe_dyn_for_closures() {
        let signal = create_signal(10);
        let value: MaybeDyn<i32> = (move || signal.get()).into_maybe_dyn();
        assert_eq!(value.get(), 10);

        signal.set(20);
        assert_eq!(value.get(), 20);
    }

    #[test]
    fn test_clone_static() {
        let value1 = MaybeDyn::fixed(100);
        let value2 = value1.clone();
        assert_eq!(value1.get(), 100);
        assert_eq!(value2.get(), 100);
    }

    #[test]
    fn test_clone_dynamic() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
        let counter_clone = counter.clone();
        let value1 = MaybeDyn::dynamic(move || {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        });
        let value2 = value1.clone();

        // Both share the same closure (Arc)
        value1.get();
        value2.get();
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn test_signal_into_maybe_dyn() {
        let signal = create_signal(42);
        let value: MaybeDyn<i32> = signal.into_maybe_dyn();

        assert_eq!(value.get(), 42);
        signal.set(100);
        assert_eq!(value.get(), 100);
    }

    #[test]
    fn test_maybe_dyn_into_maybe_dyn() {
        let value1 = MaybeDyn::fixed(7);
        let value2: MaybeDyn<i32> = value1.into_maybe_dyn();
        assert_eq!(value2.get(), 7);
    }
}
