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
