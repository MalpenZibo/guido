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

/// A memo is the fourth form `IntoSignal` accepts, and it converts too.
#[test]
fn a_memo_reaches_a_property_it_can_convert_into() {
    let n = create_signal(4.0f32);
    let doubled = create_memo(move || n.get() * 2.0);
    let _ = container().width(doubled).scale(doubled).corners(doubled);
}

/// The pair and array forms, in every numeric type the value form takes. These
/// are the ones the first list of `converting_signals!` left out — the drift
/// the rule in `.claude/skills/widgets` exists to stop, committed in the same
/// change that wrote the rule.
#[test]
fn the_pair_and_array_forms_convert_in_every_numeric_type() {
    let ints = create_signal((10i32, 20i32));
    let arr = create_signal([8.0f32, 16.0f32]);
    let quad = create_signal([1.0f32, 2.0f32, 3.0f32, 4.0f32]);
    // `CornerRadii` reaches `Corners` through a blanket `From`, which is easy
    // to miss when mirroring the list by hand — and was.
    let radii = create_signal(CornerRadii::from(4.0));
    let _ = container()
        .translate(ints)
        .scale(ints)
        .padding(arr)
        .corners(quad)
        .corners(radii)
        .corners(move || CornerRadii::from(4.0));
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

/// The properties that stopped being read once, in all three spellings.
///
/// The point of the change was that these take a signal at all; the point of
/// this test is that they still take the other two. A setter widened to
/// `impl IntoSignal<T, M>` accepts the value form through the blanket `Into`
/// impl and the closure form through `IntoVal`'s reflexive one, so nothing has
/// to be added to `converting_signals!` for a property whose declared type is
/// the type it is given — but nothing checks that until somebody writes it
/// down, which is what this is.
#[test]
fn the_values_that_became_signals_take_all_three_forms() {
    let masked = create_signal(true);
    let mask = create_signal('*');
    let fit = create_signal(ContentFit::Cover);
    let wraps = create_signal(false);
    let axis = create_signal(Axis::Vertical);
    let hot = create_signal(Color::RED);

    let _ = text("value").wrap(false);
    let _ = text("closure").wrap(move || wraps.get());
    let _ = text("signal").wrap(wraps);

    let _ = image("x.png").content_fit(ContentFit::Cover);
    let _ = image("x.png").content_fit(move || fit.get());
    let _ = image("x.png").content_fit(fit);

    let _ = text_input(create_signal(String::new()))
        .password(true)
        .mask_char('*')
        .caret(false);
    let _ = text_input(create_signal(String::new()))
        .password(move || masked.get())
        .mask_char(move || mask.get())
        .caret(move || wraps.get());
    let _ = text_input(create_signal(String::new()))
        .password(masked)
        .mask_char(mask)
        .caret(wraps);

    let _ = container().layout(Flex::new(Axis::Horizontal));
    let _ = container().layout(Flex::new(move || axis.get()));
    let _ = container().layout(Flex::new(axis));

    let _ = container().when_pressed(|s| s.ripple_with_color(Color::RED));
    let _ = container().when_pressed(move |s| s.ripple_with_color(move || hot.get()));
    let _ = container().when_pressed(move |s| s.ripple_with_color(hot));

    // `shadow` takes a `Shadow` and nothing convertible into one, so there is no
    // entry in `converting_signals!` for it — which is exactly the case this
    // test exists to cover: the three forms have to arrive through the blanket
    // impls, and nothing checks that they do until it is written down. On the
    // state layer too, where the override is a value and never a motion.
    let lift = create_signal(Shadow::new((0.0, 6.0), 10.0, 0.0, Color::BLACK));
    let _ = container().shadow(Shadow::none());
    let _ = container().shadow(move || lift.get());
    let _ = container().shadow(lift);
    let _ = container().when_hovered(|s| s.shadow(Shadow::none()));
    let _ = container().when_hovered(move |s| s.shadow(move || lift.get()));
    let _ = container().when_hovered(move |s| s.shadow(lift));
}
