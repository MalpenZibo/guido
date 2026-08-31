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

/// A bar of a fixed height, on an anchor that hands the width to the
/// compositor. Four tests share it; the two that reserve chain
/// `.exclusive_zone(ExclusiveZone::Auto)` onto it.
fn fixed_bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(50)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
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
    let surface = app.surface(fixed_bar().exclusive_zone(ExclusiveZone::Auto), move || {
        bar(clicks)
    });
    app.configure(surface, 200, 50, 2.0);
    app.step();

    assert_eq!(
        app.root_size(surface),
        (200.0, 50.0),
        "logical, what layout measures"
    );
    assert_eq!(
        app.physical_size(surface),
        (400, 100),
        "physical, what the buffer is: the scale the compositor confirmed"
    );
}

/// The event reached the widget under it, and only that one.
#[test]
fn a_click_inside_runs_the_handler_and_one_outside_does_not() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let surface = app.surface(fixed_bar(), move || bar(clicks));
    app.configure(surface, 200, 50, 2.0);
    app.step();

    app.click(surface, 10.0, 10.0);
    app.step();
    assert_eq!(clicks.get(), 1, "a click inside the child");

    app.click(surface, 150.0, 40.0);
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
    let surface = app.surface(fixed_bar().exclusive_zone(ExclusiveZone::Auto), move || {
        bar(clicks)
    });
    app.configure(surface, 200, 50, 2.0);
    app.step();

    assert_eq!(
        app.exclusive_zones_asked(surface),
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
    let bar = app.surface(content_bar(), measuring_24);

    assert_eq!(
        app.exclusive_zones_asked(bar),
        [1],
        "born reserving the placeholder, which is wrong until a frame has run"
    );

    app.configure(bar, 200, 24, 1.0);
    app.step();

    assert_eq!(
        app.exclusive_zones_asked(bar),
        [1, 24],
        "the frame measured the content and the reservation followed"
    );
    assert_eq!(
        app.sizes_asked(bar),
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
    let bar = app.surface(content_bar(), measuring_24);

    assert_eq!(
        app.sizes_asked(bar),
        [(0, 1)],
        "born asking for the placeholder, having measured nothing"
    );

    app.configure(bar, 200, 50, 1.0);
    app.step();

    assert_eq!(
        app.sizes_asked(bar),
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
    let surface = app.surface(
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.25, 0.5, 0.75)),
        container,
    );
    app.configure(surface, 100, 32, 1.0);
    app.step();

    assert_eq!(app.read_pixel(surface, 1, 1), [64, 128, 191, 255]);
    assert_eq!(app.read_pixel(surface, 98, 30), [64, 128, 191, 255]);
}

/// A widget that fills its surface and holds one whose height is a signal —
/// the shape a test needs to watch a write reach more than one surface.
fn filling(height: RwSignal<f32>) -> Container {
    container()
        .width(fill())
        .height(fill())
        .child(container().height(height))
}

/// Two surfaces at once, which is what the loop is for: one `SurfaceManager`,
/// one tree, one renderer, and a signal that does not know how many surfaces
/// are reading it.
///
/// Each is configured at its own size and lays out at it — the driver keeping a
/// map rather than one id is the whole of what makes this expressible.
#[test]
fn two_surfaces_lay_out_at_their_own_sizes_and_one_signal_reaches_both() {
    let Some(mut app) = headless() else { return };
    let height = create_signal(20.0);

    let top = app.surface(fixed_bar(), move || filling(height));
    let bottom = app.surface(fixed_bar(), move || filling(height));

    app.configure(top, 200, 50, 1.0);
    app.configure(bottom, 300, 40, 1.0);
    app.step();

    assert_eq!(app.root_size(top), (200.0, 50.0));
    assert_eq!(app.root_size(bottom), (300.0, 40.0), "each at its own size");

    let before = (app.frames_presented(top), app.frames_presented(bottom));
    height.set(35.0);
    app.step();

    assert_eq!(
        (app.frames_presented(top), app.frames_presented(bottom)),
        (before.0 + 1, before.1 + 1),
        "one write, and both surfaces drew again"
    );
}

/// `spawn_surface` is guido's own API, not the harness's: a test asks for a
/// surface exactly as an application does, and the step that follows is the
/// loop draining the command it queued.
#[test]
fn a_surface_spawned_at_runtime_reaches_the_compositor_and_the_last_close_ends_the_loop() {
    let Some(mut app) = headless() else { return };
    let first = app.surface(content_bar(), measuring_24);
    app.configure(first, 200, 24, 1.0);
    app.step();

    assert_eq!(app.surfaces_created(), [first]);

    let second = spawn_surface(content_bar(), measuring_24);
    app.step();

    assert_eq!(
        app.surfaces_created(),
        [first, second.id()],
        "the command reached `Platform::create_surface`"
    );
    second.close();
    assert_eq!(app.step(), None, "one surface left, so the loop runs on");
    assert_eq!(app.surfaces_destroyed(), [second.id()]);

    surface_handle(first).close();
    assert_eq!(
        app.step(),
        Some(ExitReason::Quit),
        "nothing left to draw on, so the loop ends"
    );
}

/// A popup is torn down before the surface it hangs from, deepest first.
///
/// Getting this wrong is not a cosmetic bug: destroying a popup that still has
/// a live child raises `not_the_topmost_popup` and the compositor kills the
/// connection. Until now that ordering was a comment in `process_surface_commands`
/// and an example somebody ran.
///
/// Two popups deep rather than one, so that the order is something a list can
/// get wrong: with a single popup every rule agrees, and this case could not
/// tell a teardown order from its reverse.
#[test]
fn a_popup_is_destroyed_before_the_surface_it_hangs_from() {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), measuring_24);
    app.configure(parent, 200, 24, 1.0);
    app.step();

    let popup = spawn_popup(parent, PopupConfig::new(80).height(40), measuring_24);
    app.step();
    let nested = spawn_popup(popup.id(), PopupConfig::new(60).height(30), measuring_24);
    app.step();

    assert_eq!(app.surfaces_created(), [parent, popup.id(), nested.id()]);

    surface_handle(parent).close();
    app.step();

    assert_eq!(
        app.surfaces_destroyed(),
        [nested.id(), popup.id(), parent],
        "the child goes first, or the compositor kills the connection"
    );
}

/// A branching popup tree comes down topmost first, and "topmost" is a decided
/// order rather than whatever a map iterated.
///
/// xdg-shell keeps the live popups in a stack and a client must destroy them
/// from the top down; the one raised last is the one on top. A `SurfaceId` is a
/// monotonic counter, so descending id *is* that stack, and it gives
/// child-before-parent for free — a popup cannot be created on a parent that
/// does not exist yet.
///
/// The chain in `a_popup_is_destroyed_before_the_surface_it_hangs_from` has one
/// child at every level, so every order agrees on it. This one branches, which
/// is where they stop agreeing: a frontier reversed, a recursion, and a hash
/// iteration are three different answers, and only one of them is the stack.
#[test]
fn a_branching_popup_tree_comes_down_newest_first() {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), measuring_24);
    app.configure(parent, 200, 24, 1.0);
    app.step();

    let first = spawn_popup(parent, PopupConfig::new(80).height(40), measuring_24);
    app.step();
    let nested = spawn_popup(first.id(), PopupConfig::new(60).height(30), measuring_24);
    app.step();
    // A sibling of `first`, raised after the whole of `first`'s own chain.
    let last = spawn_popup(parent, PopupConfig::new(80).height(40), measuring_24);
    app.step();

    assert_eq!(
        app.surfaces_created(),
        [parent, first.id(), nested.id(), last.id()],
        "the four surfaces exist, in the order they were asked for"
    );

    surface_handle(parent).close();
    app.step();

    assert_eq!(
        app.surfaces_destroyed(),
        [last.id(), nested.id(), first.id(), parent],
        "the stack comes down from the top: the newest popup, then the deepest \
         branch, then its root, then the surface they all hang from"
    );
}
