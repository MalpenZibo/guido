use super::signal::{RwSignal, Signal, create_derived, create_stored};

// ============================================================================
// Marker types for IntoSignal disambiguation
// ============================================================================

#[doc(hidden)]
pub struct ValueMarker;
#[doc(hidden)]
pub struct LossyMarker;
#[doc(hidden)]
pub struct ClosureMarker;
#[doc(hidden)]
pub struct SignalMarker;
#[doc(hidden)]
pub struct RwSignalMarker;
#[doc(hidden)]
pub struct MemoMarker;

/// Trait for types that can be converted into `Signal<T>`
///
/// The marker generic `M` disambiguates blanket impls so that static values,
/// closures, signals, and memos each use a distinct marker.
pub trait IntoSignal<T: Clone + 'static, M = ValueMarker> {
    fn into_signal(self) -> Signal<T>;
}

// ============================================================================
// IntoVal - conversion trait for closure return types
// ============================================================================

/// Trait that enables closures returning different types to work with `IntoSignal`.
///
/// For example, `|| 8` (returns `i32`) can be used where `Signal<f32>` is expected,
/// because `IntoVal<f32>` is implemented for `i32`.
pub trait IntoVal<T> {
    fn into_val(self) -> T;
}

// Identity: any T converts to itself
impl<T> IntoVal<T> for T {
    fn into_val(self) -> T {
        self
    }
}

// Lossy integer → f32 conversions (no std From impl)
impl IntoVal<f32> for i32 {
    fn into_val(self) -> f32 {
        self as f32
    }
}

impl IntoVal<f32> for u32 {
    fn into_val(self) -> f32 {
        self as f32
    }
}

impl IntoVal<f32> for u16 {
    fn into_val(self) -> f32 {
        self as f32
    }
}

// ============================================================================
// Blanket IntoSignal impls with distinct markers
// ============================================================================

// 1. Static values: any I where Into<T> exists (identity + all From impls)
impl<T: Clone + 'static, I: Into<T>> IntoSignal<T, ValueMarker> for I {
    fn into_signal(self) -> Signal<T> {
        create_stored(self.into())
    }
}

// 2. Lossy i32/u32 → f32 static conversions (no std From, can't use Into blanket)
impl IntoSignal<f32, LossyMarker> for i32 {
    fn into_signal(self) -> Signal<f32> {
        create_stored(self as f32)
    }
}

impl IntoSignal<f32, LossyMarker> for u32 {
    fn into_signal(self) -> Signal<f32> {
        create_stored(self as f32)
    }
}

// 3. Closures: Fn() -> R where R: IntoVal<T>
impl<T, R, F> IntoSignal<T, ClosureMarker> for F
where
    T: Clone + 'static,
    R: IntoVal<T> + 'static,
    F: Fn() -> R + 'static,
{
    fn into_signal(self) -> Signal<T> {
        create_derived(move || self().into_val())
    }
}

// 4. Signal<T> passthrough
impl<T: Clone + 'static> IntoSignal<T, SignalMarker> for Signal<T> {
    fn into_signal(self) -> Signal<T> {
        self
    }
}

// 5. RwSignal<T> → Signal<T> via read_only()
impl<T: Clone + 'static> IntoSignal<T, RwSignalMarker> for RwSignal<T> {
    fn into_signal(self) -> Signal<T> {
        self.read_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal::create_signal;

    #[test]
    fn test_into_signal_for_string() {
        let sig: Signal<String> = "hello".into_signal();
        assert_eq!(sig.get(), "hello");

        let sig: Signal<String> = String::from("world").into_signal();
        assert_eq!(sig.get(), "world");
    }

    #[test]
    fn test_into_signal_for_f32() {
        let sig: Signal<f32> = 2.5f32.into_signal();
        assert_eq!(sig.get(), 2.5);
    }

    #[test]
    fn test_into_signal_for_bool() {
        let sig: Signal<bool> = true.into_signal();
        assert!(sig.get());

        let sig: Signal<bool> = false.into_signal();
        assert!(!sig.get());
    }

    #[test]
    fn test_into_signal_for_closures() {
        let signal = create_signal(10);
        let sig: Signal<i32> = (move || signal.get()).into_signal();
        assert_eq!(sig.get(), 10);

        signal.set(20);
        assert_eq!(sig.get(), 20);
    }

    #[test]
    fn test_signal_into_signal() {
        let rw = create_signal(42);
        let sig: Signal<i32> = rw.into_signal();

        assert_eq!(sig.get(), 42);
        rw.set(100);
        assert_eq!(sig.get(), 100);
    }

    #[test]
    fn test_rw_signal_into_signal() {
        let rw = create_signal(42);
        let sig: Signal<i32> = rw.into_signal();
        assert_eq!(sig.get(), 42);
    }

    #[test]
    fn test_closure_lossy_conversion() {
        // Closure returning i32 used where Signal<f32> is expected
        let sig: Signal<f32> = (|| 8i32).into_signal();
        assert_eq!(sig.get(), 8.0);
    }

    #[test]
    fn test_stored_is_copy() {
        let sig = create_stored(42);
        let sig2 = sig; // Copy
        assert_eq!(sig.get(), 42);
        assert_eq!(sig2.get(), 42);
    }

    #[test]
    fn test_derived_is_copy() {
        let count = create_signal(5);
        let derived = create_derived(move || count.get() * 2);
        let derived2 = derived; // Copy
        assert_eq!(derived.get(), 10);
        assert_eq!(derived2.get(), 10);

        count.set(10);
        assert_eq!(derived.get(), 20);
    }
}
