//! A component can be generic over its props' types.
//!
//! `#[component]` used to drop the function's generics, so a generic component
//! failed to compile with `cannot find type P in this scope`, pointing at the
//! parameter rather than at the macro. That this file compiles is half of the
//! proof; the other half is that each instantiation draws what its own `P`
//! says, side by side in one tree.

use guido::prelude::*;

mod common;
use common::Harness;

/// How wide a bar a value asks for, in units.
trait Span {
    fn span(&self) -> f32;
}

impl Span for u32 {
    fn span(&self) -> f32 {
        *self as f32
    }
}

impl Span for String {
    fn span(&self) -> f32 {
        self.len() as f32
    }
}

/// How many pixels a unit is. No prop has this type: only the body names it.
trait Unit {
    const PX: f32;
}

struct One;
impl Unit for One {
    const PX: f32 = 1.0;
}

struct Ten;
impl Unit for Ten {
    const PX: f32 = 10.0;
}

/// A red bar as wide as its value's span: a bound in the parameter list, one
/// in a `where` clause, a prop of type `M`, a type `I` that only the body
/// uses, which the struct the macro generates still has to carry, and children
/// it is handed.
///
/// `M` and `I` are the names the macro's own `child` and `children` would
/// take for their generics, and a method's generic may not reuse its impl's:
/// a caller's names are theirs to pick, so the macro's have to stay out of
/// the way.
#[component]
fn bar<M: Span + Clone + PartialEq + Default + 'static, I>(
    value: M,
    #[prop(children)] children: (),
) -> impl Widget
where
    I: Unit + 'static,
{
    container()
        .width(move || value.get().span() * I::PX)
        .height(10.0)
        .background(Color::rgb(1.0, 0.0, 0.0))
        .children(children)
}

/// A callback prop's setter takes a closure under a generic of its own, which
/// must not be spelled `F` either.
#[component]
fn tappable<F: Clone + PartialEq + Default + 'static>(
    value: F,
    #[prop(callback)] on_tap: (),
) -> impl Widget {
    container()
        .on_click(on_tap)
        .child(text(move || (value.get() == F::default()).to_string()))
}

/// Two instantiations, one above the other: a `u32` bar counted in pixels and
/// a `String` one counted in tens of them.
fn two_bars() -> impl Widget {
    container()
        .layout(Flex::column())
        .child(bar::<u32, One>().value(30u32))
        .child(bar::<String, Ten>().value("abcd".to_string()))
}

#[test]
fn two_instantiations_draw_what_their_own_type_says() {
    let mut h = Harness::laid_out(two_bars(), 200.0, 200.0);

    let widths: Vec<f32> = h.painted_rects().iter().map(|r| r.width).collect();
    assert_eq!(
        widths,
        vec![30.0, 40.0],
        "the u32 bar is as wide as its number, the String bar ten per byte"
    );
}

#[test]
fn a_callback_prop_beside_a_type_parameter_named_f() {
    let mut h = Harness::laid_out(tappable::<u8>().value(0u8).on_tap(|| {}), 200.0, 200.0);
    assert_eq!(h.painted_texts(), vec!["true".to_string()]);
}

/// Through a real loop, onto pixels.
#[cfg(feature = "testing")]
#[test]
fn a_generic_component_renders_under_headless() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        two_bars,
    );
    app.configure(surface, 100, 20, 1.0);
    app.step();

    let red_at = |x: u32, y: u32| {
        let [r, g, b, _] = app.read_pixel(surface, x, y);
        r > 128 && g < 100 && b < 100
    };
    assert!(red_at(25, 5) && !red_at(35, 5), "the u32 bar ends at 30");
    assert!(
        red_at(35, 15) && !red_at(45, 15),
        "the String bar, below it, ends at 40"
    );
}
