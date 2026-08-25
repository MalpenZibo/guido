use super::memo::Memo;
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
pub struct ConvertedSignalMarker;
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

// A closure returning a bare value where an optional one is expected. The
// constant path already accepts it, through std's `From<T> for Option<T>`, so
// without this the *same expression* compiles as a value and not as a closure —
// exactly the asymmetry `IntoSignal` exists to remove. `container().gradient`
// is where that showed: `Some(g)` and `move || Some(g)` both worked, `g` worked
// and `move || g` did not.
impl<T> IntoVal<Option<T>> for T {
    fn into_val(self) -> Option<T> {
        Some(self)
    }
}

// Lossy f64 → f32: bare float literals in closures default to f64, and
// accepting them avoids the deprecated f32 inference fallback
// (rust-lang/rust#154024)
impl IntoVal<f32> for f64 {
    fn into_val(self) -> f32 {
        self as f32
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

// 2. Lossy f64/i32/u32 → f32 static conversions (no std From, can't use the
// Into blanket). f64 matters most: bare float literals default to f64, so
// accepting them avoids the deprecated f32 inference fallback.
impl IntoSignal<f32, LossyMarker> for f64 {
    fn into_signal(self) -> Signal<f32> {
        create_stored(self as f32)
    }
}

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

// 6. A signal whose value converts to the property's type.
//
// Without these a signal is the one form of `IntoSignal` that does not take
// what the others take: `width(100.0)` and `width(move || w.get())` both
// compile, and `width(w)` on a `Signal<f32>` did not, because `Length` is a
// different type and so a different key. The rule these restore is that **a
// signal accepts what a closure returning the same type accepts** — which is
// exactly the `IntoVal` relation, so that is what they are written over.
//
// Written out per source type rather than as a blanket `S: IntoVal<T>`, which
// cannot work: `IntoVal` is reflexive, so a blanket impl also covers
// `Signal<Length> -> Length`, collides with the passthrough below, and leaves
// the marker undecidable. Naming the source excludes the reflexive case by
// construction, so a signal already holding the property's own type still
// arrives by identity rather than through a derived signal.
//
// The cost of that is a list: a new conversion gets the value and closure
// forms from one `IntoVal` impl and the signal form only if someone adds it
// here too. See #226.
macro_rules! converting_signals {
    ($($from:ty => $to:ty),* $(,)?) => {$(
        impl $crate::reactive::IntoSignal<$to, $crate::reactive::ConvertedSignalMarker>
            for $crate::reactive::Signal<$from>
        {
            fn into_signal(self) -> $crate::reactive::Signal<$to> {
                $crate::reactive::create_derived(move || {
                    $crate::reactive::IntoVal::<$to>::into_val(self.get())
                })
            }
        }
        impl $crate::reactive::IntoSignal<$to, $crate::reactive::ConvertedSignalMarker>
            for $crate::reactive::RwSignal<$from>
        {
            fn into_signal(self) -> $crate::reactive::Signal<$to> {
                let read = self.read_only();
                $crate::reactive::create_derived(move || {
                    $crate::reactive::IntoVal::<$to>::into_val(read.get())
                })
            }
        }
        impl $crate::reactive::IntoSignal<$to, $crate::reactive::ConvertedSignalMarker>
            for $crate::reactive::Memo<$from>
        {
            fn into_signal(self) -> $crate::reactive::Signal<$to> {
                $crate::reactive::create_derived(move || {
                    $crate::reactive::IntoVal::<$to>::into_val(self.get())
                })
            }
        }
    )*};
}
pub(crate) use converting_signals;

// The optional case generalises where the others cannot. `IntoVal<Option<T>>
// for T` already lets a closure return a bare value where an optional one is
// expected; this is the same for a signal. No reflexive collision: reaching
// `Option<X>` from `Signal<Option<X>>` is the passthrough, and this impl is
// only ever selected for a `Signal<X>`.
impl<T: Clone + 'static> IntoSignal<Option<T>, ConvertedSignalMarker> for Signal<T> {
    fn into_signal(self) -> Signal<Option<T>> {
        create_derived(move || Some(self.get()))
    }
}

impl<T: Clone + 'static> IntoSignal<Option<T>, ConvertedSignalMarker> for RwSignal<T> {
    fn into_signal(self) -> Signal<Option<T>> {
        let read = self.read_only();
        create_derived(move || Some(read.get()))
    }
}

impl<T: Clone + PartialEq + Send + 'static> IntoSignal<Option<T>, ConvertedSignalMarker>
    for Memo<T>
{
    fn into_signal(self) -> Signal<Option<T>> {
        create_derived(move || Some(self.get()))
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
    fn test_into_signal_for_f64_literal() {
        let sig: Signal<f32> = 2.5.into_signal();
        assert_eq!(sig.get(), 2.5);
    }

    #[test]
    fn test_into_signal_for_f64_runtime_value() {
        let value: f64 = 2.5;
        let sig: Signal<f32> = value.into_signal();
        assert_eq!(sig.get(), 2.5);
    }

    #[test]
    fn test_into_signal_f64_narrows_to_f32() {
        let sig: Signal<f32> = 0.1_f64.into_signal();
        assert_eq!(sig.get(), 0.1_f32);
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
    fn test_closure_f64_conversion() {
        let sig: Signal<f32> = (|| 8.5).into_signal();
        assert_eq!(sig.get(), 8.5);
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
