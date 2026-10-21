//! A prop marked `#[prop(required)]` is an argument of the constructor.
//!
//! A prop without a default used to start at `Default::default()`, so leaving
//! one out compiled and drew an empty component. A required prop is taken the
//! way `text(content)` takes its content: `swatch(30.0)`, in declaration order,
//! and leaving it out is an arity error. That error is the doc test on
//! `guido::component`; this file is the other half, that a prop given there
//! reaches the body like any other.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;

mod common;
use common::Harness;

const RED: Color = Color::rgb(1.0, 0.0, 0.0);

/// A bar as wide as its required prop, in the colour of an ordinary one.
#[component]
fn swatch(
    #[prop(required)] width: f32,
    #[prop(default = 10.0)] height: f32,
    #[prop(default = RED)] color: Color,
) -> impl Widget {
    container().width(width).height(height).background(color)
}

/// Two required props, taken in the order they are declared, between them an
/// ordinary one.
#[component]
fn labelled(
    #[prop(required)] prefix: String,
    suffix: String,
    #[prop(required)] count: u32,
) -> impl Widget {
    text(move || format!("{} {} {}", prefix.get(), count.get(), suffix.get()))
}

/// A required callback is taken like a callback setter's argument.
#[component]
fn tap_target(#[prop(required, callback)] on_tap: ()) -> impl Widget {
    container().width(20.0).height(20.0).on_click(on_tap)
}

/// A required render prop is always given, so the body calls it as it is,
/// with no `Option` in the way.
#[component]
fn titled(
    #[prop(required)] title: String,
    #[prop(required, callback)] render: fn(String) -> impl Widget,
) -> impl Widget {
    container().child(render.run(title.get_untracked()))
}

/// A generic component whose required prop is of its type parameter.
#[component]
fn shown<P: Clone + std::fmt::Display + 'static>(#[prop(required)] value: P) -> impl Widget {
    text(move || value.get().to_string())
}

#[test]
fn a_required_prop_is_the_constructors_argument() {
    let mut h = Harness::laid_out(swatch(30.0).height(12.0), 200.0, 200.0);
    let sizes: Vec<_> = h
        .painted_rects()
        .iter()
        .map(|r| (r.width, r.height))
        .collect();
    assert_eq!(sizes, vec![(30.0, 12.0)]);
}

#[test]
fn required_props_are_taken_in_declaration_order() {
    let mut h = Harness::laid_out(
        labelled("seen", 3u32).suffix("times".to_string()),
        200.0,
        200.0,
    );
    assert_eq!(h.painted_texts(), vec!["seen 3 times".to_string()]);
}

#[test]
fn a_required_callback_is_called() {
    let taps = Rc::new(Cell::new(0));
    let counted = taps.clone();
    let mut h = Harness::laid_out(
        tap_target(move || counted.set(counted.get() + 1)),
        200.0,
        200.0,
    );
    h.click(10.0, 10.0);
    assert_eq!(taps.get(), 1);
}

#[test]
fn a_required_render_prop_is_called_as_it_is() {
    let mut h = Harness::laid_out(
        titled("home", |title| text(format!("page: {title}"))),
        200.0,
        200.0,
    );
    assert_eq!(h.painted_texts(), vec!["page: home".to_string()]);
}

#[test]
fn a_generic_component_with_a_required_prop_is_named_on_its_struct() {
    let value = create_signal(42u32);
    let mut h = Harness::laid_out(Shown::<u32>::new(value), 200.0, 200.0);
    assert_eq!(h.painted_texts(), vec!["42".to_string()]);
}

/// Through a real loop, onto pixels — and given a signal rather than a value,
/// which the bar follows as a setter's prop would.
#[cfg(feature = "testing")]
#[test]
fn a_required_prop_renders_under_headless_and_follows_its_signal() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let width = create_signal(30.0f32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || swatch(width),
    );
    app.configure(surface, 100, 20, 1.0);
    app.step();

    let red_at = |app: &mut guido::testing::Headless, x: u32| {
        let [r, g, b, _] = app.read_pixel(surface, x, 5);
        r > 128 && g < 100 && b < 100
    };
    assert!(
        red_at(&mut app, 25) && !red_at(&mut app, 35),
        "the bar ends at 30"
    );

    width.set(45.0);
    app.step();
    assert!(
        red_at(&mut app, 40) && !red_at(&mut app, 50),
        "the bar follows its signal to 45"
    );
}
