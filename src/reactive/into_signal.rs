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

// ============================================================================
// Blanket IntoSignal impls with distinct markers
// ============================================================================

// 1. Static values: any I where Into<T> exists (identity + all From impls)
impl<T: Clone + 'static, I: Into<T>> IntoSignal<T, ValueMarker> for I {
    fn into_signal(self) -> Signal<T> {
        create_stored(self.into())
    }
}

// 2. Closures: Fn() -> R where R: IntoVal<T>
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

// 3. A conversion, in every form a property accepts.
//
// A property takes a value, a closure, a signal, a writable signal or a memo,
// and the same expression has to compile in all five or the property is only
// reactive by accident. This macro is where a pair is declared, once, and it
// emits every form the pair does not already have — so a pair cannot exist in
// one and not the others. It was two lists mirrored by hand, and they drifted.
//
// The `=>` shape is a pair std can convert: the body is `<To>::from`, which
// will not compile unless the `From` beside the type exists, so the value form
// is proved rather than mirrored. The `as` shape is a widening std has no
// `From` for, and there the value form is ours to emit too. A pair that has a
// `From` must not use `as`: both value impls would apply and the marker would
// be undecidable.
//
// The two shapes cannot be mixed in one invocation — a list is all `=>` or all
// `as` — and `@pair` is the internal rule they share, not a way in: it takes
// the conversion body directly, which is exactly the proof the `=>` arm buys.
//
// The signal impls name their source type rather than being one blanket
// `S: IntoVal<T>`. `IntoVal` is reflexive, so a blanket also covers
// `Signal<Length> -> Length` and collides with the passthrough impl, leaving
// the marker undecidable. Dropping the passthrough would make the blanket
// compile — the collision is not a language limit — but then every
// `.width(sig)` already holding the property's type would build a derived node
// to hand itself through, and that is the commonest call in the library. So
// the pairs stay a list, and this is the only way to write an entry in it.
macro_rules! converts {
    // One pair, in every reactive form. Both shapes below arrive here.
    (@pair $from:ty, $to:ty, |$value:ident| $conversion:expr) => {
        impl $crate::reactive::IntoVal<$to> for $from {
            fn into_val(self) -> $to {
                let $value = self;
                $conversion
            }
        }

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
    };

    // A pair std converts, which is every pair but three.
    ($($from:ty => $to:ty),* $(,)?) => {$(
        $crate::reactive::converts!(@pair $from, $to, |value| <$to>::from(value));
    )*};

    // A widening std has no `From` for, so the value form is emitted here.
    ($($from:ty as $to:ty),* $(,)?) => {$(
        $crate::reactive::converts!(@pair $from, $to, |value| value as $to);

        impl $crate::reactive::IntoSignal<$to, $crate::reactive::LossyMarker> for $from {
            fn into_signal(self) -> $crate::reactive::Signal<$to> {
                $crate::reactive::create_stored(self as $to)
            }
        }
    )*};
}
pub(crate) use converts;

// The numeric widenings. `f64` matters most: a bare float literal defaults to
// it, so accepting it avoids the deprecated f32 inference fallback
// (rust-lang/rust#154024).
converts!(f64 as f32, i32 as f32, u32 as f32);

// `u16` widens without loss, so std has the `From` and the value form arrives
// through the `Into` blanket above. Declaring it with `as` would emit a second
// value impl and make `4u16` ambiguous between the two markers.
converts!(u16 => f32);

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

#[cfg(test)]
mod one_declaration_covers_every_form {
    use super::*;
    use crate::reactive::{create_memo, create_signal};

    /// A type with no conversions of its own beyond the `From` every value
    /// needs, so the only thing that can carry it into a property is the
    /// declaration below.
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Millimetres(f32);

    impl From<f32> for Millimetres {
        fn from(size: f32) -> Self {
            Self(size)
        }
    }

    converts!(f32 => Millimetres);

    /// What a property setter does with what it is given.
    fn resolve<M>(source: impl IntoSignal<Millimetres, M>) -> Millimetres {
        source.into_signal().get()
    }

    /// One declaration, and the pair arrives in all five spellings a property
    /// accepts. Drop any impl the macro emits and this stops compiling — which
    /// is the whole point of the pair being declared once.
    #[test]
    fn a_pair_declared_once_arrives_in_every_form() {
        let count = create_signal(4.0f32);
        let read = count.read_only();
        let doubled = create_memo(move || count.get() * 2.0);

        assert_eq!(resolve(4.0f32), Millimetres(4.0), "a value");
        assert_eq!(resolve(move || 4.0f32), Millimetres(4.0), "a closure");
        assert_eq!(resolve(read), Millimetres(4.0), "a signal");
        assert_eq!(resolve(count), Millimetres(4.0), "a writable signal");
        assert_eq!(resolve(doubled), Millimetres(8.0), "a memo");
    }

    /// The other shape, where std has no `From` and so the value form is the
    /// macro's to emit as well. `i64` is a source nothing else in the library
    /// converts from, so these five impls are the declaration below and
    /// nothing else.
    mod a_widening_std_cannot_do {
        use super::*;

        converts!(i64 as f32);

        fn resolve<M>(source: impl IntoSignal<f32, M>) -> f32 {
            source.into_signal().get()
        }

        #[test]
        fn arrives_in_every_form_too() {
            let count = create_signal(4i64);
            let read = count.read_only();
            let doubled = create_memo(move || count.get() * 2);

            assert_eq!(resolve(4i64), 4.0, "a value");
            assert_eq!(resolve(move || 4i64), 4.0, "a closure");
            assert_eq!(resolve(read), 4.0, "a signal");
            assert_eq!(resolve(count), 4.0, "a writable signal");
            assert_eq!(resolve(doubled), 8.0, "a memo");
        }
    }
}
