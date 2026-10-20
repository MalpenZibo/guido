//! A component can take a closure that returns a value — a render prop.
//!
//! `#[prop(callback)] render: fn(P) -> Container` is a `Callback` that returns
//! what its closure returns, and `fn(P) -> impl Widget` takes a closure
//! returning any widget, erased to an `AnyWidget` by the setter. Each
//! test hands the component a closure that picks its page's colour by the
//! argument it is called with, and reads the page back off the surface.
#![cfg(feature = "testing")]

use guido::prelude::*;
mod common;
use common::headless;

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

/// Calls its render prop with `7` and shows the page it gets back.
#[component]
fn erased(#[prop(callback)] render: fn(u32) -> impl Widget) -> impl Widget {
    container()
        .width(fill())
        .height(fill())
        .child(render.map(|render| render.run(7)))
}

/// The same, with the page's type spelled out.
#[component]
fn concrete(#[prop(callback)] render: fn(u32) -> Container) -> impl Widget {
    container()
        .width(fill())
        .height(fill())
        .child(render.map(|render| render.run(7)))
}

/// Another component, for a render prop to return.
#[component]
fn swatch(color: Color) -> impl Widget {
    container().width(fill()).height(fill()).background(color)
}

fn page(color: Color) -> Container {
    container().width(fill()).height(fill()).background(color)
}

/// Red for the argument the component passes, blue for any other.
fn by_argument(v: u32) -> Color {
    if v == 7 { RED } else { BLUE }
}

/// The colour in the middle of a surface built from `view`.
fn painted<W: Widget + 'static>(view: impl Fn() -> W + 'static) -> Option<[u8; 4]> {
    let mut app = headless()?;
    let surface = app.surface(
        SurfaceConfig::new()
            .height(40)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        view,
    );
    app.configure(surface, 100, 40, 1.0);
    app.step();
    Some(app.read_pixel(surface, 50, 20))
}

fn is_red([r, g, b, _]: [u8; 4]) -> bool {
    r > 200 && g < 50 && b < 50
}

#[test]
fn an_erased_render_prop_builds_the_container_its_closure_returns() {
    let Some(pixel) = painted(|| erased().render(|v: u32| page(by_argument(v)))) else {
        return;
    };
    assert!(
        is_red(pixel),
        "the page the closure built for 7, got {pixel:?}"
    );
}

#[test]
fn an_erased_render_prop_builds_the_component_its_closure_returns() {
    let Some(pixel) = painted(|| erased().render(|v: u32| swatch().color(by_argument(v)))) else {
        return;
    };
    assert!(
        is_red(pixel),
        "the component the closure built for 7, got {pixel:?}"
    );
}

#[test]
fn a_concrete_render_prop_builds_the_container_its_closure_returns() {
    let Some(pixel) = painted(|| concrete().render(|v: u32| page(by_argument(v)))) else {
        return;
    };
    assert!(
        is_red(pixel),
        "the page the closure built for 7, got {pixel:?}"
    );
}

/// A render prop is forwarded as it is, as a callback prop is: the outer
/// component's `Option<Callback<..>>` goes straight into the inner builder.
#[component]
fn forwarding(#[prop(callback)] render: fn(u32) -> impl Widget) -> impl Widget {
    erased().render(render)
}

#[test]
fn an_erased_render_prop_is_forwarded_as_it_is() {
    let Some(pixel) = painted(|| forwarding().render(|v: u32| swatch().color(by_argument(v))))
    else {
        return;
    };
    assert!(is_red(pixel), "the outer caller's page, got {pixel:?}");
}
