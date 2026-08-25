//! A signal accepts what a closure returning the same type accepts.
//!
//! `IntoSignal` has three forms — a value, a closure, a signal — and the
//! conversions have to line up across all three or the same expression compiles
//! in one position and not another. The value and closure forms share the
//! `From`/`IntoVal` impls beside each type; the signal form needs its own, and
//! for a long time did not have them, so `width(100.0)` and
//! `width(move || w.get())` compiled while `width(w)` on a `Signal<f32>` did
//! not.
//!
//! A test that only has to build. It is a list rather than a rule because the
//! impls are a list — see `reactive::into_signal` for why they cannot be one
//! blanket impl, and #226 for the drift that leaves.

use guido::prelude::*;

#[test]
fn a_signal_reaches_a_property_it_can_convert_into() {
    let f = create_signal(8.0f32);
    let i = create_signal(4i32);
    let pair = create_signal((2.0f32, 0.5f32));

    let _ = container()
        .width(f)
        .height(i)
        .padding(f)
        .corners(f)
        .backdrop_blur(f)
        .elevation(i)
        .scale(f)
        .scale(pair)
        .translate(pair)
        .rotate(i)
        .layout(Flex::row().spacing(i));
}

/// A bare value where an optional one is expected, which the closure form has
/// always accepted.
#[test]
fn a_signal_of_a_value_reaches_an_optional_property() {
    let g = create_signal(LinearGradient::horizontal(Color::RED, Color::BLUE));
    let _ = container().gradient(g);
}

/// And a signal already holding the property's own type still arrives by
/// identity, not through a derived signal — the two impls carry different
/// markers, so nothing is ambiguous.
#[test]
fn a_signal_of_the_property_type_is_unaffected() {
    let c = create_signal(Corners::squircle(8.0));
    let s = create_signal(Scale::uniform(1.2));
    let p = create_signal(Padding::all(4.0));
    let _ = container().corners(c).scale(s).padding(p);
}
