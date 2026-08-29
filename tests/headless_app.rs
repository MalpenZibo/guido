#![cfg(feature = "testing")]
//! An application, stepped without a compositor.
//!
//! Everything else in `tests/` reaches below the application: a `Tree` and a
//! widget, or the renderer and a texture. This drives what `App::run` drives —
//! a surface that configures, a frame that opens, input that routes, layout
//! that measures, a paint that lands — with a recorder where the compositor
//! would be. What it asserts is the half nothing could see before: not what the
//! frame drew, but what the surface *asked the compositor for*.
//!
//! Compiled only under the `testing` feature — without it the file is empty,
//! so a default `cargo test` still builds. With it, a machine that has no GPU
//! adapter skips, unless `GUIDO_GPU_REQUIRED` says a skip is a failure.

use guido::prelude::*;
use guido::testing::Headless;

/// A bar of the shape the acceptance criterion names: anchored to the top,
/// reserving automatically, with something clickable in it.
fn bar(clicks: RwSignal<u32>) -> Container {
    container().width(fill()).height(fill()).child(
        container()
            .width(80.0)
            .height(20.0)
            .on_click(move || clicks.update(|c| *c += 1)),
    )
}

/// A surface whose height is whatever it holds, on an anchor that hands the
/// width to the compositor. Two tests share it, and the only thing that differs
/// between them is the height it is then configured at.
fn content_bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(content())
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
        .exclusive_zone(ExclusiveZone::Auto)
}

fn measuring_24() -> Container {
    container().height(24.0).child(container().height(24.0))
}

fn headless() -> Option<Headless> {
    match Headless::new() {
        Some(app) => Some(app),
        None if std::env::var_os("GUIDO_GPU_REQUIRED").is_some() => {
            panic!("GUIDO_GPU_REQUIRED is set and no GPU adapter was found")
        }
        None => {
            eprintln!("no GPU adapter; skipping");
            None
        }
    }
}

/// The frame ran: the tree was measured against the size the compositor
/// confirmed, at the scale it confirmed.
#[test]
fn a_configured_surface_lays_out_at_the_size_it_was_given() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    app.surface(
        SurfaceConfig::new()
            .height(50)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .exclusive_zone(ExclusiveZone::Auto),
        move || bar(clicks),
    );
    app.configure(200, 50, 2.0);
    app.step();

    assert_eq!(
        app.root_size(),
        (200.0, 50.0),
        "logical, what layout measures"
    );
    assert_eq!(
        app.physical_size(),
        (400, 100),
        "physical, what the buffer is: the scale the compositor confirmed"
    );
}

/// The event reached the widget under it, and only that one.
#[test]
fn a_click_inside_runs_the_handler_and_one_outside_does_not() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    app.surface(
        SurfaceConfig::new()
            .height(50)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || bar(clicks),
    );
    app.configure(200, 50, 2.0);
    app.step();

    app.click(10.0, 10.0);
    app.step();
    assert_eq!(clicks.get(), 1, "a click inside the child");

    app.click(150.0, 40.0);
    app.step();
    assert_eq!(clicks.get(), 1, "a click outside it changes nothing");
}

/// The reservation a fixed-height bar declares, which it declares once — at
/// creation, the way `create_surface_with_id` does, and never again.
///
/// This passes without a frame ever running, and that is where the value comes
/// from rather than a weakness in the test: `layout_pass` resyncs a reservation
/// only for a surface whose size follows its content. The frame-path half is
/// the test below.
#[test]
fn a_bar_reserving_automatically_declares_its_height_when_it_is_created() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    app.surface(
        SurfaceConfig::new()
            .height(50)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .exclusive_zone(ExclusiveZone::Auto),
        move || bar(clicks),
    );
    app.configure(200, 50, 2.0);
    app.step();

    assert_eq!(
        app.exclusive_zones_asked(),
        [50],
        "once, at creation, and the frames after it say nothing"
    );
}

/// A surface whose height follows its content cannot resolve its reservation
/// until something has measured it — so this one *is* the frame path, and the
/// half the recorder exists for.
///
/// At creation there is nothing to resolve against: `requested_extent` on an
/// unmeasured content axis falls back to the 1px placeholder, so the surface is
/// born reserving one pixel. The measure runs inside `layout_pass` and the
/// reservation follows it.
#[test]
fn a_content_sized_surface_reserves_only_once_a_frame_has_measured_it() {
    let Some(mut app) = headless() else { return };
    app.surface(content_bar(), measuring_24);

    assert_eq!(
        app.exclusive_zones_asked(),
        [1],
        "born reserving the placeholder, which is wrong until a frame has run"
    );

    app.configure(200, 24, 1.0);
    app.step();

    assert_eq!(
        app.exclusive_zones_asked(),
        [1, 24],
        "the frame measured the content and the reservation followed"
    );
    assert_eq!(
        app.sizes_asked(),
        [(0, 1)],
        "and it asked for no new size, having measured back the one it has: the \
         resize is conditional where the resync is not. This is the only thing \
         watching that condition's false arm — see \
         a_surface_configured_taller_than_its_content_asks_to_shrink_to_it for \
         the true one"
    );
}

/// The other half of what a frame tells the compositor: not only what to
/// reserve, but what size to be.
///
/// A surface configured taller than its content measures asks to shrink, and
/// the request goes out from inside the measure pass — the one place the
/// measured number is known before the compositor knows it. The width stays 0
/// throughout, for the reason `surface::honour_owned_axes` gives.
#[test]
fn a_surface_configured_taller_than_its_content_asks_to_shrink_to_it() {
    let Some(mut app) = headless() else { return };
    app.surface(content_bar(), measuring_24);

    assert_eq!(
        app.sizes_asked(),
        [(0, 1)],
        "born asking for the placeholder, having measured nothing"
    );

    app.configure(200, 50, 1.0);
    app.step();

    assert_eq!(
        app.sizes_asked(),
        [(0, 1), (0, 24)],
        "the frame measured 24 against a surface configured at 50, and said so"
    );
}

/// And the pixels: what the compositor would have been handed.
///
/// The assertion is the surface's own background — the least interesting thing
/// on the frame, and the only one exactly predictable on any adapter, which is
/// why this needs an adapter rather than lavapipe the way a golden does.
///
/// Two pixels in different rows and columns, at a width whose rows need padding
/// (100 pixels is 400 bytes, which pads to 512), so neither the stride nor the
/// offset can be wrong and still land on the right bytes.
#[test]
fn the_frame_that_was_drawn_can_be_read_back() {
    let Some(mut app) = headless() else { return };
    app.surface(
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.25, 0.5, 0.75)),
        container,
    );
    app.configure(100, 32, 1.0);
    app.step();

    assert_eq!(app.read_pixel(1, 1), [64, 128, 191, 255]);
    assert_eq!(app.read_pixel(98, 30), [64, 128, 191, 255]);
}
