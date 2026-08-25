//! Every spelling the documentation promises, compiled against the public
//! prelude.
//!
//! A test that only has to build. `IntoSignal` accepts a value, a closure, a
//! signal or a memo, and the conversions have to line up so that the same
//! expression works in each position — the asymmetry where `1.5` compiled and
//! `move || 1.5` did not is the one this catches.
//!
//! Written as an integration test on purpose: it exercises what `guido::prelude`
//! actually exports, which a unit test inside the crate cannot.

use guido::prelude::*;

#[test]
fn every_spelling_the_documentation_promises_compiles() {
    let sig = create_signal(1.5f32);
    let _ = container()
        // integers, as the book claims
        .rotate(45)
        .scale(2)
        // floats
        .rotate(45.0)
        .scale(1.5)
        // pairs and arrays
        .scale((2.0, 0.5))
        .scale([2.0, 0.5])
        .translate((10.0, 20.0))
        .translate([10.0, 20.0])
        .translate((10, 20))
        // named values
        .translate(Translate::new(1.0, 2.0))
        .scale(Scale::uniform(1.2))
        .scale(Scale::NONE)
        // closures — must accept exactly what the constant form accepts
        .rotate(move || sig.get())
        .scale(move || 1.5)
        .scale(move || (2.0, 0.5))
        .scale(move || sig.get())
        .translate(move || (1.0, 2.0))
        .translate(move || Translate::NONE)
        // signals
        .rotate(sig)
        .pivot(Pivot::TOP_LEFT);

    // state layer
    let _ = container().when_pressed(|s| s.scale(0.98).rotate(2.0).translate((1.0, 0.0)));
}
