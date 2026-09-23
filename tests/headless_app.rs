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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;
use guido::widget_prelude::*;

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

/// The buffer and the viewport destination that says what it stands for are
/// never committed out of step.
///
/// A viewport is how a fractionally-scaled surface declares its logical size,
/// and the compositor reads the buffer through it: a destination the buffer
/// does not match is the surface drawn at the wrong size, and with a source
/// rectangle it is `wp_viewport: error 2, source rectangle out of buffer
/// bounds` — fatal, the compositor drops the client (libsdl-org/SDL#9283).
///
/// The sequence is built so neither half can be inferred from the other. The
/// second configure moves the buffer while the logical size stays; the fourth
/// moves the logical size while the buffer stays — 2560x32 at 1.5 and 3840x48
/// at 1.0 are the same 3840x48 buffer. A destination published only when the
/// swapchain resizes survives the first three and declares 3840x48 over a
/// 2560x32 surface on the fourth.
#[test]
fn a_buffer_and_the_destination_that_declares_it_never_disagree() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || container().width(fill()).height(fill()));

    // The logical size and scale the compositor confirms, the buffer the
    // protocol's rounding asks for, and every destination declared by then —
    // written out rather than derived, so the expectation is a statement and
    // not a second copy of the rule under test.
    let configures = [
        ((2560, 32), 1.5, (3840, 48), &[(2560, 32)][..]),
        ((2560, 32), 2.0, (5120, 64), &[(2560, 32)][..]),
        ((3840, 48), 1.0, (3840, 48), &[(2560, 32), (3840, 48)][..]),
        (
            (2560, 32),
            1.5,
            (3840, 48),
            &[(2560, 32), (3840, 48), (2560, 32)][..],
        ),
    ];

    for ((width, height), scale, buffer, declared) in configures {
        app.configure(surface, width, height, scale);
        app.step();
        // A second frame with nothing new to say must not re-declare anything.
        app.step();

        assert_eq!(
            app.physical_size(surface),
            buffer,
            "the buffer for {width}x{height} at {scale}"
        );
        assert_eq!(
            app.viewport_destinations(surface),
            declared,
            "every destination declared, after {width}x{height} at {scale}"
        );
    }
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

/// A content-sized surface is measured on every frame with layout to do, and a
/// reservation the compositor already has is not sent again for it.
#[test]
fn a_reservation_that_did_not_move_is_not_sent_again() {
    let Some(mut app) = headless() else { return };
    let width = create_signal(40.0f32);
    let bar = app.surface(content_bar(), move || {
        container()
            .height(24.0)
            .child(container().width(move || width.get()).height(24.0))
    });
    app.configure(bar, 200, 24, 1.0);
    app.step();
    assert_eq!(app.exclusive_zones_asked(bar), [1, 24]);

    for w in [60.0, 80.0, 100.0] {
        width.set(w);
        app.step();
    }

    assert_eq!(
        app.exclusive_zones_asked(bar),
        [1, 24],
        "three frames re-measured a bar that stayed 24 tall, and resent 24 each time"
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

/// A new grab closes the grab chain it cannot nest under, deepest first.
///
/// xdg-shell lets one grab chain exist at a time and a new grab must nest under
/// the current holder. Opening one somewhere else means tearing the old chain
/// down first, and in the same order as any other teardown — the compositor
/// applies `not_the_topmost_popup` to these destroys like the rest.
///
/// This path had no sensor at all: the recorder used to discard the
/// `PopupConfig` it was handed, so it could not say which popups held a grab
/// and answered `conflicting_grab_popups` with the trait's empty default.
#[test]
fn a_new_grab_tears_down_the_chain_it_cannot_nest_under() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(content_bar(), measuring_24);
    app.configure(bar, 200, 24, 1.0);
    app.step();

    let menu = spawn_popup(bar, PopupConfig::new(80).height(40).grab(), measuring_24);
    app.step();
    let submenu = spawn_popup(
        menu.id(),
        PopupConfig::new(60).height(30).grab(),
        measuring_24,
    );
    app.step();
    assert_eq!(app.surfaces_created(), [bar, menu.id(), submenu.id()]);

    // A second menu on the bar itself: it cannot nest under the chain above, so
    // that chain has to go before this one opens.
    let other = spawn_popup(bar, PopupConfig::new(80).height(40).grab(), measuring_24);
    app.step();

    assert_eq!(
        app.surfaces_destroyed(),
        [submenu.id(), menu.id()],
        "the chain comes down deepest first, before the new grab opens"
    );
    assert_eq!(
        app.surfaces_created(),
        [bar, menu.id(), submenu.id(), other.id()],
        "and the new grab did open"
    );
}

/// A bar with one field on it and nothing else: the shape #206 was filed
/// against, less the second field it did not need.
fn bar_with_a_field(field: WidgetRef, value: RwSignal<String>) -> Container {
    container()
        .width(fill())
        .height(fill())
        .child(text_input(value).widget_ref(field))
}

/// A press nobody claimed takes the keyboard with it. The decision is the
/// dispatcher's, and the unit tests beside it pin the decision; this is the
/// loop actually taking it — a surface configured, a frame open, and a press
/// routed the way the compositor's would be.
#[test]
fn a_click_outside_the_field_takes_the_keyboard_off_it() {
    let Some(mut app) = headless() else { return };
    let field = create_widget_ref();
    let value = create_signal(String::new());
    let surface = app.surface(fixed_bar(), move || bar_with_a_field(field, value));
    app.configure(surface, 200, 50, 1.0);
    app.step();

    app.click(surface, 100.0, 8.0);
    app.step();
    assert!(
        field.is_focused(),
        "a click into the field takes the keyboard"
    );

    app.click(surface, 100.0, 40.0);
    app.step();
    assert!(
        !field.is_focused(),
        "and a click on the bar behind it gives it back"
    );
}

// ---------------------------------------------------------------------------
// Input regions, declared on the tree and derived from the frame (#360)
// ---------------------------------------------------------------------------

/// A surface that lets everything through, with one island that does not.
fn island(w: f32, h: f32) -> Container {
    container()
        .width(fill())
        .height(fill())
        .child(container().width(w).height(h).takes_input(true))
}

#[test]
fn an_island_is_where_input_reaches_a_click_through_surface() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || island(80.0, 20.0));
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert!(app.input_reaches(surface, 40.0, 10.0), "inside the island");
    assert!(
        !app.input_reaches(surface, 150.0, 40.0),
        "the surface around it passes clicks to whatever is below"
    );
}

#[test]
fn a_hole_passes_clicks_through_a_surface_that_otherwise_takes_them() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .child(container().width(80.0).height(20.0).takes_input(false))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert!(
        !app.input_reaches(surface, 40.0, 10.0),
        "the hole is where the desktop gets the click"
    );
    assert!(app.input_reaches(surface, 150.0, 40.0), "the bar around it");
}

/// A turned island takes clicks where it is, not over the square it sits in.
///
/// An 80×80 island turned 45° has a 113×113 bounding box: read as that box it
/// claims 56% more area than it drew, and every click in the four triangles
/// lands on a panel that is not there while the desktop below never hears it.
/// The region is tessellated from the placed shape now, so the corners of the
/// box belong to whatever is underneath again.
#[test]
fn a_turned_island_takes_input_where_it_is_drawn() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(80.0)
                .rotate(45.0)
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 120, 1.0);
    app.step();

    // The island is laid out at the top-left and turned about its own centre,
    // so it is a diamond with its points at the edges of an 80×80 box.
    assert!(
        app.input_reaches(surface, 40.0, 40.0),
        "the middle of the diamond is the island"
    );
    assert!(
        app.input_reaches(surface, 40.0, 10.0),
        "and so is the space under its top point"
    );
    assert!(
        !app.input_reaches(surface, 5.0, 5.0),
        "but the box's top-left corner is 28 pixels of nothing, and used to \
         swallow the click"
    );
    assert!(
        !app.input_reaches(surface, 75.0, 75.0),
        "as was its bottom-right"
    );
}

/// A bevelled island takes input inside the cut, not out to the square corner.
///
/// `Corners::bevel` is a straight diagonal across the corner — K = 0, and
/// strictly inside the circle of the same radius. The region was cut as a
/// circle whatever the container drew, so a bevelled one took clicks in the
/// sliver between the two, on ground it had not painted.
///
/// The curvature has been on the draw command all along and only the shader
/// read it; the input region had nowhere to put it until a clip and a shape
/// became one type.
#[test]
fn a_bevelled_island_takes_input_inside_its_cut() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(80.0)
                .corners(Corners::bevel(40.0))
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 120, 1.0);
    app.step();

    assert!(
        app.input_reaches(surface, 40.0, 40.0),
        "the middle of the island is the island"
    );
    // The top-left corner's chord runs from (0, 40) to (40, 0). A point 12 in
    // and 12 down is inside a circle of 40 and outside that line.
    assert!(
        !app.input_reaches(surface, 12.0, 12.0),
        "and the bevel has already cut this pixel away, though a circle of the \
         same radius would still reach it"
    );
}

/// A squircled island takes input out to the curve it drew, not in to the
/// circle inside it.
///
/// The mirror of the bevel above, and the other direction of the same error.
/// `Corners::squircle` is K = 2, and a squircle is *larger* than the circle of
/// the same radius — so cutting the region as a circle gave up a band along
/// each corner diagonal, 7.6 pixels of it at a radius of 40. Inside that band
/// the island is painted and the surface does not claim it, so the compositor
/// sends the click to whatever is behind and nothing happens (#400).
///
/// Between them the two tests pin the region to the curve from both sides: a
/// bevel must not reach past its cut, a squircle must reach out to its own.
#[test]
fn a_squircled_island_takes_input_out_to_its_curve() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(80.0)
                .corners(Corners::squircle(40.0))
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 120, 1.0);
    app.step();

    assert!(
        app.input_reaches(surface, 40.0, 40.0),
        "the middle of the island is the island"
    );
    // 9 in and 9 down: 2·31^4 is 1,847,042 against 40^4 of 2,560,000, so it is
    // inside the squircle — and 31·√2 is 43.8 against a radius of 40, so it is
    // outside the circle the region used to publish.
    assert!(
        app.input_reaches(surface, 9.0, 9.0),
        "the squircle is drawn here, so the surface has to claim it — the \
         circle inscribed in it does not reach this pixel"
    );
}

/// A scoop keeps the corner the round gives away, and the region has to keep
/// it too.
///
/// The scoop is the one curvature that bends *inward*: the shape is the box
/// minus a disc at each corner, so it reaches all the way along both edges to
/// within a radius of the corner and is bitten away only around the diagonal.
/// The tessellator could not sample it — chords of a concave curve fall
/// outside the shape — so it published the chord tangent to the bite instead.
/// That chord runs from `r·√2` on one edge to `r·√2` on the other and cuts off
/// everything nearer the corner than itself, which is 17% of the whole shape.
/// On the input region 17% of a widget that does not take clicks looks, from
/// inside the application, like nothing happening (#410).
///
/// The gap is widest where the chord meets an edge: the shape reaches to 40
/// from the corner there and the chord stopped at 56.6. (42, 6) is 42.4 from
/// the corner, so the scoop is drawn on it, and `42 + 6` is 48 against the
/// chord's 56.6, so the old region did not claim it — eight pixels of margin
/// either way.
#[test]
fn a_scooped_island_keeps_the_horns_the_bite_leaves_it() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(120.0)
                .height(120.0)
                .corners(Corners::scoop(40.0))
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 160, 1.0);
    app.step();

    assert!(
        app.input_reaches(surface, 60.0, 60.0),
        "the middle of the island is the island"
    );
    for (x, y) in [(42.0, 6.0), (6.0, 42.0)] {
        assert!(
            app.input_reaches(surface, x, y),
            "({x}, {y}) is {:.1} from the bite's centre against its radius of \
             40, so the scoop is drawn there and the surface has to claim it",
            (x * x + y * y).sqrt()
        );
    }
    // And the bite itself stays nobody's: the corner the disc took out.
    assert!(
        !app.input_reaches(surface, 10.0, 10.0),
        "(10, 10) is 14 from the corner against a radius of 40, so it is \
         inside the disc the scoop removes and nothing was drawn there"
    );
}

/// The region is the shape that was *drawn*. A `WidgetRef` reports the box a
/// widget was laid out in, which is why the region could never follow one.
#[test]
fn a_transformed_island_declares_where_it_draws_not_where_it_was_laid_out() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().click_through(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(20.0)
                .scale(2.0)
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert!(
        app.input_reaches(surface, 100.0, 25.0),
        "outside the laid-out box, inside the shape on screen"
    );
}

/// Every way a container can stop painting is a way a registry would have gone
/// stale. The frame is the register.
#[test]
fn an_island_that_stops_painting_stops_taking_input() {
    let Some(mut app) = headless() else { return };
    let shown = create_signal(true);
    let surface = app.surface(fixed_bar().click_through(), move || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(20.0)
                .visible(shown)
                .takes_input(true),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();
    assert!(app.input_reaches(surface, 40.0, 10.0));

    shown.set(false);
    app.step();
    assert!(
        !app.input_reaches(surface, 40.0, 10.0),
        "a hidden island leaves no region behind it"
    );
}

/// The same discipline `sync_blur_region` keeps: a frame that repaints for
/// some other reason, with the region unchanged, says nothing about it.
///
/// A frame that does not repaint at all proves nothing here — it never reaches
/// the comparison — so this one paints, twice, and watches the count stand
/// still while the frames go up.
#[test]
fn a_frame_that_repaints_without_moving_the_region_publishes_nothing() {
    let Some(mut app) = headless() else { return };
    let shade = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(fixed_bar().click_through(), move || {
        container()
            .width(fill())
            .height(fill())
            .background(shade)
            .child(container().width(80.0).height(20.0).takes_input(true))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let (frames, published) = (
        app.frames_presented(surface),
        app.input_regions_published(surface),
    );

    shade.set(Color::rgb(0.2, 0.2, 0.2));
    app.step();
    shade.set(Color::rgb(0.3, 0.3, 0.3));
    app.step();

    assert!(
        app.frames_presented(surface) > frames,
        "the frames have to have run for this to be asking anything"
    );
    assert_eq!(
        app.input_regions_published(surface),
        published,
        "and the region did not move, so the compositor was not told twice"
    );
}

/// The rectangle the surface itself can name, for a region that belongs to no
/// widget.
const PILL: Rect = Rect {
    x: 120.0,
    y: 10.0,
    width: 40.0,
    height: 20.0,
};

/// The escape hatch is still there and still watched: a region the surface
/// declares reaches the compositor when it is created, without a frame and
/// without anything in the tree saying a word.
#[test]
fn a_surface_declared_with_a_region_asks_for_it_at_birth() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar().input_region([PILL]), container);
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert_eq!(app.input_regions_asked(surface), [Some(vec![PILL])]);
}

/// The two ways of saying it compose rather than erasing each other: what a
/// handle sets is the area the surface takes, and the declarations in the tree
/// are applied on top of it.
#[test]
fn a_region_set_by_hand_is_the_base_the_tree_declares_against() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || island(80.0, 20.0));
    app.configure(surface, 200, 50, 1.0);
    app.step();

    surface_handle(surface).set_input_region(Some(vec![PILL]));
    app.step();

    assert!(
        app.input_reaches(surface, 130.0, 15.0),
        "the rectangle the handle named"
    );
    assert!(
        app.input_reaches(surface, 40.0, 10.0),
        "and the island the tree declared, on top of it"
    );
    assert!(
        !app.input_reaches(surface, 40.0, 45.0),
        "and nothing else, where the bar used to take everything"
    );
}

/// A declaration inside a declaration is a smaller area declared later, which
/// is the whole of the nesting rule: an island inside a hole takes input, the
/// hole around it does not, and the bar around that still does.
#[test]
fn a_declaration_inside_a_hole_is_an_island_in_it() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(40.0)
                .takes_input(false)
                .child(container().width(20.0).height(10.0).takes_input(true)),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert!(app.input_reaches(surface, 10.0, 5.0), "the island");
    assert!(
        !app.input_reaches(surface, 60.0, 30.0),
        "the hole around it"
    );
    assert!(
        app.input_reaches(surface, 150.0, 40.0),
        "the bar around that"
    );
}

/// A widget that writes down the moment each event it is handed happened, as
/// the tree declared it.
struct Stamps(Rc<RefCell<Vec<Instant>>>);

impl Widget for Stamps {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        Size::new(constraints.max_width, constraints.max_height)
    }

    fn paint(&self, _ctx: &mut PaintContext) {}

    fn event(&mut self, tree: &mut Tree, _id: WidgetId, _event: &Event) -> EventResponse {
        self.0.borrow_mut().push(tree.event_instant().into_inner());
        EventResponse::Handled
    }
}

/// Two events queued at two moments arrive in one frame, each carrying its
/// own. Both are in the past, so no clock the frame could read would answer
/// with either: the widget can only have been told.
#[test]
fn each_queued_event_arrives_at_the_moment_it_was_queued_for() {
    let Some(mut app) = headless() else { return };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let spy = seen.clone();
    let surface = app.surface(fixed_bar(), move || Stamps(spy));
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let now = Instant::now();
    let pressed = now - Duration::from_secs(2);
    let released = now - Duration::from_secs(1);
    app.event_at(
        surface,
        Event::mouse_down(10.0, 10.0, MouseButton::Left),
        pressed,
    );
    app.event_at(
        surface,
        Event::mouse_up(10.0, 10.0, MouseButton::Left),
        released,
    );
    app.step();

    assert_eq!(*seen.borrow(), [pressed, released]);
}

/// A block of one colour, which is what these scroll scenes are made of: the
/// frame says where the content is by where one gives way to the next.
fn block(height: f32, color: Color) -> Container {
    container().width(fill()).height(height).background(color)
}

/// Scrolling on the vertical axis with no scrollbar drawn over it.
fn hidden_scroll() -> Scroll {
    Scroll::vertical().visibility(ScrollbarVisibility::Hidden)
}

/// A scroll area as tall as its surface, over content whose top 500px are one
/// colour and the rest another — so where the edge between them is drawn says
/// how far the content has moved. `over` is laid on the first block for the
/// one test that needs something in the way.
fn scroll_over_an_edge_under(over: Option<EveryMoveIsMine>) -> Container {
    container()
        .width(fill())
        .height(fill())
        .scroll(hidden_scroll())
        .child(
            container()
                .layout(Flex::column())
                .width(fill())
                .child(block(500.0, Color::rgb(1.0, 0.0, 0.0)).child(over))
                .child(block(1500.0, Color::rgb(0.0, 0.0, 1.0))),
        )
}

fn scroll_over_an_edge() -> Container {
    scroll_over_an_edge_under(None)
}

/// How far the content has scrolled, read off the frame: 500 less the first
/// row drawn more blue than red. To the pixel, which is as fine as a frame
/// can say it.
fn scrolled(app: &Headless, surface: SurfaceId) -> f32 {
    500.0 - first_row(app, surface, 0, |[r, _, b, _]| b > r, "edge") as f32
}

/// A finger landing at `from` and dragging 100px up the surface in five steps
/// a frame apart, which is 18 of slop and 82 of content.
fn drag_up(app: &mut Headless, surface: SurfaceId, at: &mut Instant, from: f32) {
    app.event_at(surface, Event::finger_down(50.0, from), *at);
    for step in 1..=5 {
        *at += Duration::from_millis(8);
        app.event_at(
            surface,
            Event::finger_move(50.0, from - 20.0 * step as f32),
            *at,
        );
    }
}

/// A surface 100x600 over `view`, stepped once so the first frame has run.
fn scroller(app: &mut Headless, view: impl Fn() -> Container + 'static) -> (SurfaceId, Instant) {
    let surface = app.surface(fixed_bar().height(600), view);
    app.configure(surface, 100, 600, 1.0);
    let at = Instant::now();
    app.step_at(at);
    (surface, at)
}

/// A second of frames at sixty a second, and how far the content had got by
/// the end of it. `at` comes back where the last frame left it.
fn coast(app: &mut Headless, surface: SurfaceId, at: &mut Instant) -> f32 {
    for _ in 0..60 {
        *at += Duration::from_millis(16);
        app.step_at(*at);
    }
    scrolled(app, surface)
}

/// A flick played through the application: six samples eight milliseconds
/// apart, the finger lifted, then frames at sixty a second. The same shape
/// `tests/scroll_momentum.rs` asserts on a tree, one layer up — through the
/// routing, the frame's phases and the paint.
#[test]
fn a_flick_played_through_the_application_coasts_past_its_last_sample() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    for _ in 0..6 {
        app.event_at(
            surface,
            Event::scroll(50.0, 50.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    app.event_at(surface, Event::scroll_end(50.0, 50.0), at);
    app.step_at(at);
    let at_lift = scrolled(&app, surface);
    assert_eq!(at_lift, 60.0, "the samples themselves moved the content");

    let coasted = coast(&mut app, surface, &mut at) - at_lift;
    assert!(
        coasted > 10.0,
        "the finger lifted after a 60px flick and the content coasted {coasted}px"
    );
}

/// A scroller as tall as its surface, over one pressable block taller than it.
///
/// The block lights green while it is pressed and counts what it activates, so
/// a frame says whether the press is still live and `clicks` says whether it
/// ever fired.
fn scroll_over_a_button(clicks: RwSignal<u32>, height: f32) -> Container {
    container()
        .width(fill())
        .height(fill())
        .scroll(hidden_scroll())
        .child(
            block(height, Color::rgb(0.0, 0.0, 1.0))
                .when_pressed(|s: StateStyle| s.background(Color::rgb(0.0, 1.0, 0.0)))
                .on_click(move || clicks.update(|c| *c += 1)),
        )
}

/// Whether the block is drawing its pressed state this frame — green rather
/// than blue, read where the block covers the surface whatever the offset.
fn pressed(app: &Headless, surface: SurfaceId) -> bool {
    let [_, g, b, _] = app.read_pixel(surface, 50, 10);
    g > b
}

/// A finger on the content is the gesture a touch interface actually uses, and
/// until #429 the only one guido answered was grabbing the scrollbar.
///
/// The slop is spent once and is all that is spent: the move that crosses it
/// scrolls the part of itself beyond it, and every move after it scrolls
/// whole. Flutter starts the drag where it was won rather than where the
/// finger landed, so the content never jumps by the threshold, and Android
/// subtracts the slop from that first delta rather than discarding it — which
/// matters here because a compositor coalesces motion to one event a frame, so
/// a fast swipe can cross the slop and a hundred pixels at once.
#[test]
fn a_finger_dragging_the_content_scrolls_it() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    drag_up(&mut app, surface, &mut at, 300.0);
    app.step_at(at);

    assert_eq!(
        scrolled(&app, surface),
        82.0,
        "the finger travelled 100px up, 18 of which bought the drag"
    );
}

/// And a finger lifted while it is still moving hands off to the momentum
/// `a_flick_played_through_the_application_coasts_past_its_last_sample` already
/// plays through a `Scroll` gesture. Same physics, fed from the drag.
///
/// The lift is the pair the fold synthesizes — a release, then the leave that
/// says nothing hovers after a finger — because a momentum that the second of
/// those cancelled would coast for exactly no frames.
#[test]
fn a_finger_lifted_mid_drag_hands_the_content_to_its_momentum() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    app.event_at(surface, Event::finger_down(50.0, 500.0), at);
    for y in [490.0, 480.0, 470.0, 460.0, 450.0, 440.0, 430.0] {
        at += Duration::from_millis(8);
        app.event_at(surface, Event::finger_move(50.0, y), at);
    }
    app.event_at(surface, Event::mouse_up(50.0, 430.0, MouseButton::Left), at);
    app.event_at(surface, Event::MouseLeave, at);
    app.step_at(at);

    let at_lift = scrolled(&app, surface);
    assert_eq!(
        at_lift, 52.0,
        "the drag itself moved the content: seven 10px steps, 18 of which \
         bought the drag"
    );

    let coasted = coast(&mut app, surface, &mut at) - at_lift;
    assert!(
        coasted > 10.0,
        "the finger lifted mid-drag and the content coasted {coasted}px"
    );
}

/// A drag's momentum is built from that drag, not from whatever scrolled here
/// last.
///
/// `last_scroll_time` and the sample count outlive a gesture, so a drag that
/// followed one without resetting them would measure its first sample against
/// a timestamp seconds old and smooth the previous gesture's velocity into its
/// own — a list flung the way it was going before, by a finger that dragged it
/// the other way.
///
/// The stale state here is a touchpad gesture that simply stopped reporting:
/// the protocol only promises an `axis_stop` for a finger, so a `Continuous`
/// source leaving a velocity behind is the ordinary case rather than a
/// contrived one.
#[test]
fn a_drag_after_another_gesture_is_flung_by_its_own_speed() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    // Scrolled downward, fast, and never terminated.
    for _ in 0..6 {
        app.event_at(
            surface,
            Event::scroll(50.0, 50.0, 0.0, 10.0, ScrollSource::Continuous),
            at,
        );
        at += Duration::from_millis(8);
    }
    app.step_at(at);
    assert_eq!(
        scrolled(&app, surface),
        60.0,
        "the samples moved the content"
    );

    // Seconds later, a finger drags the other way and lifts at once.
    at += Duration::from_secs(1);
    app.event_at(surface, Event::finger_down(50.0, 300.0), at);
    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(50.0, 320.0), at);
    app.event_at(surface, Event::mouse_up(50.0, 320.0, MouseButton::Left), at);
    app.event_at(surface, Event::MouseLeave, at);
    app.step_at(at);
    let at_lift = scrolled(&app, surface);

    let after = coast(&mut app, surface, &mut at);
    assert_eq!(
        after, at_lift,
        "one move is one sample and a sample is not a speed, so nothing was \
         thrown — least of all downward, which is where the gesture before \
         this one was going"
    );
}

/// A finger on a coasting list stops it.
///
/// `ScrollView` treats a touch during a fling as a drag already in progress,
/// so the glide ends at the press rather than 18px later — waiting for the
/// slop would slide the content out from under the finger first, and a tap
/// below the slop would never stop it at all.
#[test]
fn a_finger_on_a_coasting_list_stops_it() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    for _ in 0..6 {
        app.event_at(
            surface,
            Event::scroll(50.0, 50.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    app.event_at(surface, Event::scroll_end(50.0, 50.0), at);
    app.step_at(at);

    // Two frames of glide, so the content is demonstrably still moving.
    for _ in 0..2 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    let gliding = scrolled(&app, surface);
    assert!(gliding > 60.0, "the flick is still running at {gliding}px");

    app.event_at(surface, Event::finger_down(50.0, 300.0), at);
    app.step_at(at);
    let at_press = scrolled(&app, surface);

    assert_eq!(
        coast(&mut app, surface, &mut at),
        at_press,
        "a second of frames after the finger landed, and the content stayed \
         where it was"
    );
}

/// And stopping it is not tapping it: the press is the scroller's from the
/// first event, so nothing under the finger lights up and nothing fires.
#[test]
fn a_finger_that_stops_a_coasting_list_activates_nothing() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let (surface, mut at) = scroller(&mut app, move || scroll_over_a_button(clicks, 2000.0));

    for _ in 0..6 {
        app.event_at(
            surface,
            Event::scroll(50.0, 50.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    app.event_at(surface, Event::scroll_end(50.0, 50.0), at);
    app.step_at(at);
    at += Duration::from_millis(16);
    app.step_at(at);

    app.event_at(surface, Event::finger_down(50.0, 100.0), at);
    app.step_at(at);
    assert!(
        !pressed(&app, surface),
        "the finger stopped the list, so nothing under it is held down"
    );

    app.event_at(surface, Event::mouse_up(50.0, 100.0, MouseButton::Left), at);
    app.event_at(surface, Event::MouseLeave, at);
    app.step_at(at);
    assert_eq!(clicks.get(), 0, "and the lift activated nothing");
}

/// Below the slop the press is still the child's: a finger that wobbles on a
/// button has pressed the button, which is the whole reason the threshold is
/// 18px rather than one.
#[test]
fn a_finger_that_moves_less_than_the_slop_still_clicks_what_it_pressed() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let (surface, mut at) = scroller(&mut app, move || scroll_over_a_button(clicks, 2000.0));

    app.event_at(surface, Event::finger_down(50.0, 100.0), at);
    app.step_at(at);
    assert!(pressed(&app, surface), "the finger landed on the block");

    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(50.0, 90.0), at);
    app.step_at(at);
    assert!(
        pressed(&app, surface),
        "10px is under the slop, so the press is still the block's"
    );

    app.event_at(surface, Event::mouse_up(50.0, 90.0, MouseButton::Left), at);
    app.step_at(at);
    assert_eq!(clicks.get(), 1, "and the lift activated it");
}

/// Above it the press is taken back. The child saw a `MouseDown` and lit up;
/// crossing the slop sends it the `MouseLeave` that ends a press without
/// activating anything, so the state layer clears and the eventual release
/// fires nothing — `pointer_left` and the `is_pressed` guard on the `MouseUp`
/// arm, which is the mechanism #429's comment found already here.
#[test]
fn a_drag_that_crosses_the_slop_takes_the_press_back_from_the_child() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let (surface, mut at) = scroller(&mut app, move || scroll_over_a_button(clicks, 2000.0));

    app.event_at(surface, Event::finger_down(50.0, 100.0), at);
    app.step_at(at);
    assert!(pressed(&app, surface), "the finger landed on the block");

    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(50.0, 60.0), at);
    app.step_at(at);
    assert!(
        !pressed(&app, surface),
        "40px is a scroll, and the press it took is void"
    );

    app.event_at(surface, Event::mouse_up(50.0, 60.0, MouseButton::Left), at);
    app.step_at(at);
    assert_eq!(clicks.get(), 0, "so the release activated nothing");
}

/// A scroller with nothing to scroll claims nothing. A declaration is not an
/// overflow: the same list is short today and long tomorrow, and the short one
/// must not swallow the tap that the long one is right to take.
///
/// The wheel path says this by consuming only what `apply_scroll` moved; the
/// drag has to say it up front, because by the time the slop is crossed the
/// child's press has already been cancelled.
#[test]
fn a_list_that_fits_does_not_take_the_tap_it_cannot_scroll() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let (surface, mut at) = scroller(&mut app, move || scroll_over_a_button(clicks, 100.0));

    app.event_at(surface, Event::finger_down(50.0, 20.0), at);
    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(50.0, 60.0), at);
    app.step_at(at);
    assert!(
        pressed(&app, surface),
        "40px over content that cannot move is still a press on the block"
    );

    app.event_at(surface, Event::mouse_up(50.0, 60.0, MouseButton::Left), at);
    app.step_at(at);
    assert_eq!(clicks.get(), 1, "and the lift activated it");
}

/// The same drag with a mouse scrolls nothing, which is the ruling on #429's
/// second open question: a pointer that hovers is a pointer that can select,
/// and drag-select inside a scrollable is what a content drag-scroll would
/// cost. A mouse scrolls by the wheel and by the scrollbar, as it always has.
#[test]
fn a_mouse_dragging_the_same_content_does_not_scroll_it() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, scroll_over_an_edge);

    app.event_at(
        surface,
        Event::mouse_down(50.0, 300.0, MouseButton::Left),
        at,
    );
    for y in [280.0, 260.0, 240.0, 220.0, 200.0] {
        at += Duration::from_millis(8);
        app.event_at(surface, Event::mouse_move(50.0, y), at);
    }
    app.step_at(at);

    assert_eq!(
        scrolled(&app, surface),
        0.0,
        "a mouse dragging the content moves nothing"
    );
}

/// A leaf that takes every move it is given and counts them, the way a text
/// input extending a selection takes them. It draws nothing, so the block
/// under it still reads off the frame.
struct EveryMoveIsMine(Rc<Cell<u32>>);

impl Widget for EveryMoveIsMine {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(100.0, 500.0)
    }

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, event: &Event) -> EventResponse {
        match event {
            Event::MouseMove { .. } => {
                self.0.set(self.0.get() + 1);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

/// `scroll_over_an_edge` with one of those laid over the first block, and the
/// count of the moves it was given.
fn scroll_under_a_greedy_child(app: &mut Headless) -> (SurfaceId, Instant, Rc<Cell<u32>>) {
    let moves = Rc::new(Cell::new(0));
    let counted = moves.clone();
    let (surface, at) = scroller(app, move || {
        scroll_over_an_edge_under(Some(EveryMoveIsMine(counted.clone())))
    });
    (surface, at, moves)
}

/// A child taking the move is not a scroller taking it.
///
/// The claim a scroller makes after its children is made whether or not one of
/// them answered `Handled`, because the only thing that means the gesture is
/// spoken for is a *scroller* below saying so. Read the child's answer as the
/// ruling instead and a finger could not scroll a list past anything that
/// tracks a drag of its own — a text input being the one in the crate.
#[test]
fn a_child_that_takes_every_move_does_not_take_the_drag() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at, _) = scroll_under_a_greedy_child(&mut app);

    drag_up(&mut app, surface, &mut at, 300.0);
    app.step_at(at);

    assert_eq!(
        scrolled(&app, surface),
        82.0,
        "the leaf under the finger consumed every move, and the list it sits \
         in scrolled by all of them but the slop"
    );
}

/// And a gesture the list has won stops being offered to what is under it.
///
/// Claimed is claimed: nothing below can take a drag back, so the scroller
/// answers the rest of it before the children rather than after, and the
/// subtree it is scrolling is not walked once a frame for an answer that
/// cannot change anything.
#[test]
fn a_drag_the_list_has_won_is_not_offered_to_the_list_again() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at, moves) = scroll_under_a_greedy_child(&mut app);

    drag_up(&mut app, surface, &mut at, 300.0);
    app.step_at(at);

    assert_eq!(
        moves.get(),
        1,
        "the first move was still anybody's and the leaf saw it; the four \
         after it were the list's"
    );
}

/// A list inside a page, both scrolling vertically, and the page's own last
/// block below the list.
///
/// Down the middle the surface reads red to where the page has scrolled the
/// list into view, green to where the list has scrolled its own content, and
/// blue from there on — the list's remainder and the page's last block are
/// both blue, so neither edge stops being the one below it as the two offsets
/// move.
///
/// `list_content` is what the list holds: 800 in a 200-tall viewport has
/// somewhere to go, 200 has not.
fn a_list_inside_a_page(list_content: f32) -> Container {
    container()
        .width(fill())
        .height(fill())
        .scroll(hidden_scroll())
        .child(
            container()
                .layout(Flex::column())
                .width(fill())
                .child(block(300.0, Color::rgb(1.0, 0.0, 0.0)))
                .child(
                    container()
                        .width(fill())
                        .height(200.0)
                        .scroll(hidden_scroll())
                        .child(
                            container()
                                .layout(Flex::column())
                                .width(fill())
                                .child(block(150.0, Color::rgb(0.0, 1.0, 0.0)))
                                .child(block(list_content - 150.0, Color::rgb(0.0, 0.0, 1.0))),
                        ),
                )
                .child(block(500.0, Color::rgb(0.0, 0.0, 1.0))),
        )
}

/// The first row at or below `from` that `is` answers for, halved rather than
/// walked because every read copies the frame back from the device.
fn first_row(
    app: &Headless,
    surface: SurfaceId,
    from: u32,
    is: impl Fn([u8; 4]) -> bool,
    edge: &str,
) -> u32 {
    assert!(
        !is(app.read_pixel(surface, 50, from)),
        "the {edge} is above row {from}, where the search starts"
    );
    assert!(
        is(app.read_pixel(surface, 50, 599)),
        "the {edge} has left the bottom of the surface"
    );
    let (mut above, mut below) = (from, 599);
    while below - above > 1 {
        let mid = (above + below) / 2;
        if is(app.read_pixel(surface, 50, mid)) {
            below = mid
        } else {
            above = mid
        }
    }
    below
}

/// How far the page and the list have each scrolled, in that order.
fn page_and_list(app: &Headless, surface: SurfaceId) -> (f32, f32) {
    let green = first_row(app, surface, 0, |[r, g, b, _]| g > r || b > r, "red edge");
    let blue = first_row(app, surface, green, |[_, g, b, _]| b > g, "green edge");
    (300.0 - green as f32, 150.0 - (blue - green) as f32)
}

/// A finger in a list inside a page drags the list, and leaves the page where
/// it was.
///
/// Both watch the press, because both have somewhere to go; the innermost one
/// that can move is whose the gesture is. Flutter's arena gives it to the
/// innermost `Scrollable`, Android's `ScrollView` declines to intercept while
/// a nested scroller holds the sequence, and GTK bubbles from the innermost
/// widget out. None of the three resolves it by running the outer one first —
/// and neither does the wheel here, which the innermost scroller takes because
/// its `Handled` ends the dispatch before an ancestor is asked.
#[test]
fn a_finger_in_a_list_inside_a_page_drags_the_list() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(800.0));
    assert_eq!(
        page_and_list(&app, surface),
        (0.0, 0.0),
        "neither has moved on the first frame"
    );

    // The list is the 200 rows below 300, and the finger stays inside it.
    drag_up(&mut app, surface, &mut at, 480.0);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (0.0, 82.0),
        "the finger travelled 100px up inside the list, 18 of which bought \
         the drag, and the page around it stayed where it was"
    );
}

/// A finger that wanders out of the page's box does not hand it the gesture
/// the list is already holding.
///
/// The page stopped watching the press the moment the list took it, which is
/// the half of that ruling nothing else would notice: a container that clips
/// does not dispatch a positioned event that falls outside it, so a page that
/// kept its watch would find the list out of the way and claim a gesture that
/// has been somebody else's for a dozen frames.
#[test]
fn a_drag_that_leaves_the_page_does_not_become_the_pages() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(800.0));

    drag_up(&mut app, surface, &mut at, 480.0);

    // Out of the surface sideways, which the compositor goes on reporting
    // while the finger is down, and 180px up from where it landed.
    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(150.0, 300.0), at);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (0.0, 82.0),
        "the list kept the gesture it won, and the page claimed nothing"
    );
}

/// A finger on the page outside the list drags the page, which is what makes
/// the rule innermost-*under-the-finger* rather than innermost-ever.
#[test]
fn a_finger_on_the_page_outside_the_list_drags_the_page() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(800.0));

    // 300 rows of the page above the list, and the finger stays in them.
    drag_up(&mut app, surface, &mut at, 200.0);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (82.0, 0.0),
        "the page took the drag it was under, and the list it carries did not"
    );
}

/// A list that runs out mid-drag keeps the gesture rather than passing what is
/// left of it to the page.
///
/// The fork CSS calls scroll chaining, and the three toolkits do not agree on
/// it: Flutter's nested scrollables keep the drag and overscroll instead,
/// Android's `NestedScrollView` passes the remainder up, and the web chains by
/// default and offers `overscroll-behavior` to say otherwise. Keeping it is
/// what follows from the claim being a claim — and the one of the three that
/// needs no vocabulary to express, which is what an unasked question should
/// cost.
#[test]
fn a_list_that_runs_out_mid_drag_does_not_hand_the_rest_to_the_page() {
    let Some(mut app) = headless() else { return };
    // 50 rows of room in the list, against 400 in the page under it.
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(250.0));

    drag_up(&mut app, surface, &mut at, 480.0);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (0.0, 50.0),
        "the list took as much of the 82px as it had room for, and the 32 \
         it could not went nowhere"
    );
}

/// And the page takes a drag over a list that has nowhere to go. A list that
/// fits is not a claim on the gesture — the same answer `has_room_to_scroll`
/// already gives a tap, now that an ancestor depends on it.
#[test]
fn a_finger_in_a_list_that_fits_drags_the_page_around_it() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(200.0));

    drag_up(&mut app, surface, &mut at, 480.0);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (82.0, 0.0),
        "the list under the finger cannot move, so the page did"
    );
}

/// A child that asks the compositor to blur behind it while `on` says so, over
/// a background that can be changed to make a frame repaint for another reason.
fn blurring(on: RwSignal<bool>, tint: RwSignal<Color>) -> Container {
    container()
        .width(fill())
        .height(fill())
        .background(tint)
        .child(container().width(80.0).height(20.0).backdrop_blur(move || {
            let sources = if on.get() {
                BackdropSources::COMPOSITOR
            } else {
                BackdropSources::empty()
            };
            BackdropBlur::new(0.0).sources(sources)
        }))
}

/// A turned card publishes the shape it drew, not the box around it.
///
/// This is the half of #198 the issue's own title is about, and it is the half
/// that had no end-to-end test: the tessellator is covered in `region.rs` and
/// the *input* region got a headless twin, but nothing asked what the
/// compositor is actually handed for a rotated `backdrop_blur`. Wiring the
/// wrong transform into `blur.rs` would have left every test green.
///
/// A 100×100 card turned 45° is a diamond with a 141×141 box. The rectangles
/// published must cover its middle and leave the box's corners to the desktop.
#[test]
fn a_turned_compositor_blur_publishes_its_shape() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container().width(fill()).height(fill()).child(
            container()
                .width(100.0)
                .height(100.0)
                .rotate(45.0)
                .backdrop_blur(BackdropBlur::new(8.0).sources(BackdropSources::COMPOSITOR)),
        )
    });
    app.configure(surface, 220, 220, 1.0);
    app.step();

    let published = app
        .blur_regions_asked(surface)
        .last()
        .expect("the frame that blurs publishes a region")
        .clone();
    assert!(!published.is_empty());

    let covers = |x: f32, y: f32| {
        published
            .iter()
            .any(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)
    };

    assert!(covers(50.0, 50.0), "the middle of the diamond is blurred");
    assert!(
        covers(50.0, 12.0),
        "and the space under its top point is too"
    );
    // The box runs from about -20 to 120 on each axis; its corners are ~28
    // pixels of desktop the card never covered.
    for (x, y) in [
        (-12.0, -12.0),
        (112.0, -12.0),
        (-12.0, 112.0),
        (112.0, 112.0),
    ] {
        assert!(
            !covers(x, y),
            "({x}, {y}) is a corner of the bounding box, not of the card"
        );
    }
}

/// The region is handed to the compositor, not only computed — every test of
/// what a region contains stops at the command list. And a blur that goes away
/// is withdrawn on the frame it went, with an empty region, and then nothing
/// more is said however often the surface repaints.
#[test]
fn a_compositor_blur_is_published_and_withdrawn_once_it_goes() {
    let Some(mut app) = headless() else { return };
    let on = create_signal(true);
    let tint = create_signal(Color::BLACK);
    let surface = app.surface(fixed_bar(), move || blurring(on, tint));
    app.configure(surface, 200, 50, 1.0);
    app.step();
    assert_eq!(
        app.blur_regions_asked(surface),
        [vec![Rect::new(0.0, 0.0, 80.0, 20.0)]],
        "the frame that blurs has to publish the region"
    );

    on.set(false);
    app.step();
    let asked = app.blur_regions_asked(surface);
    assert_eq!(
        asked.last(),
        Some(&vec![]),
        "the frame the blur went has to withdraw it"
    );
    let withdrawn = asked.len();

    tint.set(Color::WHITE);
    app.step();
    assert_eq!(
        app.blur_regions_asked(surface).len(),
        withdrawn,
        "a surface with nothing to blur and nothing published says nothing"
    );
}

/// A content-sized surface follows its content when the content grows.
///
/// The measure that sizes such a surface is asked the same constraints every
/// frame — they come from the output, not from the content — so a cache that
/// answers it from an entry written before the content changed says the
/// surface is the size it used to be, for ever. A toast that grows, an OSD
/// that gains a line, a popup whose list fills: none of them would ever ask
/// the compositor for the new height.
#[test]
fn a_content_sized_surface_follows_content_that_grows() {
    let Some(mut app) = headless() else { return };
    let height = create_signal(24.0f32);
    let bar = app.surface(content_bar(), move || {
        container().height(move || height.get())
    });
    app.configure(bar, 200, 24, 1.0);
    app.step();

    height.set(80.0);
    app.step();
    app.step();

    assert!(
        app.exclusive_zones_asked(bar).contains(&80),
        "the content grew to 80 and the surface never asked for it: {:?}",
        app.exclusive_zones_asked(bar)
    );
}

/// A surface whose height follows an animating child asks for where the
/// animation is *going*, never for a size it is passing through.
///
/// The measure that configures a content-sized surface reads targets for this
/// reason: asking the compositor to resize on every frame of an animation is
/// one round trip per frame, and the surface would follow the animation a
/// frame behind all the way up. The trap is the cache: a subtree in flight is
/// laid out and then, later in the same frame, answered from that layout —
/// and if what it said about itself is dropped at that second asking, the
/// measure reads the in-flight layout back and configures the surface to it.
#[test]
fn a_growing_child_configures_the_surface_once_to_where_it_is_going() {
    let Some(mut app) = headless() else { return };
    let tall = create_signal(false);
    let bar = app.surface(content_bar(), move || {
        container().child(
            container().height(
                (move || if tall.get() { 80.0 } else { 24.0 })
                    .transition(Transition::new(100.0, TimingFunction::Linear)),
            ),
        )
    });
    app.configure(bar, 200, 24, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);

    tall.set(true);
    for ms in [0, 20, 40, 60, 80, 100, 120] {
        app.step_at(t0 + Duration::from_millis(ms));
    }

    let asked = app.exclusive_zones_asked(bar);
    assert!(
        asked
            .iter()
            .all(|&zone| zone == 1 || zone == 24 || zone == 80),
        "the surface was configured to a size the animation was passing \
         through: {asked:?}"
    );
    assert!(
        asked.contains(&80),
        "and it has to arrive at the one the animation is going to: {asked:?}"
    );
}

/// A surface's first layout is a pass, and it dates from the moment the driver
/// named rather than from the wall clock.
///
/// An enter animation is seeded at that first layout and advanced by the frame
/// that follows it. Both happen inside this one `step_at`, so the width must
/// still be where the enter starts: no time passed between them.
///
/// The instant is a minute from the wall clock on purpose. The two clocks tell
/// apart only where they disagree, and a minute is longer than any transition —
/// seeded from the wall clock, the animation reaches this frame already
/// finished, at its full 200.
#[test]
fn a_first_layout_dates_from_the_frame_that_asked_for_it() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container()
            .width(200.0.transition(1000.0).entering_from(0.0))
            .height(fill())
    });
    app.configure(surface, 200, 50, 1.0);
    let at = Instant::now() + Duration::from_secs(60);
    app.step_at(at);

    assert_eq!(
        app.root_size(surface).0,
        0.0,
        "seeded and advanced in one frame, the enter has not begun to travel"
    );

    // And it does travel, from there. Without this the assertion above is also
    // what an enter that never plays at all would produce. Half way is asserted
    // as a range because the easing curve is not this test's business; the ends
    // are what it is pinning.
    app.step_at(at + Duration::from_millis(500));
    let half = app.root_size(surface).0;
    assert!(
        half > 0.0 && half < 200.0,
        "half a transition in, the enter is between its ends: {half}"
    );

    app.step_at(at + Duration::from_millis(2000));
    assert_eq!(
        app.root_size(surface).0,
        200.0,
        "a transition and a half in, it has arrived"
    );
}

/// A subtree that skips does not erase what the pass has already been told.
///
/// A skip folds two things together: whatever the subtree around this one has
/// already said, and what this one said when it last ran. It has to carry both
/// out, or a parent that lays out an animation and then a settled sibling ends
/// the second one having forgotten the first, and caches its own answer as
/// settled. The measure that follows then asks the same question at the same
/// constraints, reads the in-flight size back off that entry instead of
/// descending to where the animation is going, and the compositor is asked for
/// a height the animation is merely passing through — the defect #368 was
/// about, arriving by another route.
///
/// The tree is that shape: an animated child, and a fixed one after it that
/// skips. `a_growing_child_configures_the_surface_once_to_where_it_is_going`
/// is the same scenario without the sibling, and cannot see this — its root is
/// asked different constraints by the two passes, so the measure descends
/// whatever the cached answer says. Here the root pins both axes it hands
/// down, `fill()` across and `at_most` down, so everything below it is asked
/// the same question by both passes and the cached flag is the only thing
/// deciding whether the measure descends.
#[test]
fn a_skipped_subtree_still_says_what_is_in_flight_below_it() {
    let Some(mut app) = headless() else { return };
    let tall = create_signal(false);
    let bar = app.surface(content_bar(), move || {
        container().width(fill()).height(at_most(24.0)).child(
            container()
                .child(
                    container().height(
                        (move || if tall.get() { 16.0 } else { 8.0 })
                            .transition(Transition::new(100.0, TimingFunction::Linear)),
                    ),
                )
                .child(container().height(4.0)),
        )
    });
    app.configure(bar, 200, 24, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);

    tall.set(true);
    for ms in [0, 20, 40] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    let part_way = app.root_size(bar).1;
    for ms in [60, 80, 100, 120] {
        app.step_at(t0 + Duration::from_millis(ms));
    }

    // Without this the assertion below is also what an animation that never
    // played at all would produce, and a skip has nothing to report about a
    // subtree that was never in flight.
    assert!(
        part_way > 12.0 && part_way < 20.0,
        "part way through, the layout is between the animation's ends: \
         {part_way}"
    );
    assert_eq!(
        app.exclusive_zones_asked(bar),
        [1, 12, 20],
        "the placeholder, the height it stood at, and the height it is going \
         to — and nothing in between, which is what the measure would report \
         if the skip had handed its parent a settled answer"
    );
}

/// A layout root is restarted under its own constraints, not the surface's.
///
/// Partial layout re-enters the tree at a relayout boundary, which has no
/// parent above it to say what room it has. `Tree::last_layout_constraints`
/// is that memory: the constraints the boundary was last really laid out
/// under. Without them the boundary is handed the whole surface, and one
/// inside a padded parent grows to a width its parent never offered.
///
/// The boundary here declares 150 inside a parent that offers 100, so the two
/// answers differ: its own constraints clamp it to 100, the surface's leave it
/// at 150. The height moves so the assertion cannot pass by the restart never
/// happening at all.
#[test]
fn a_restarted_layout_root_keeps_the_constraints_it_was_placed_under() {
    let Some(mut app) = headless() else { return };
    let tall = create_signal(false);
    let boundary = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container().padding([0.0, 50.0]).child(
            container()
                .width(150.0)
                .height(move || if tall.get() { 30.0 } else { 20.0 })
                .widget_ref(boundary),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let placed = boundary.rect().get_untracked();
    assert_eq!(
        (placed.width, placed.height),
        (100.0, 20.0),
        "declared 150 wide, clamped by the 100 its padded parent offers"
    );

    tall.set(true);
    app.step();

    let restarted = boundary.rect().get_untracked();
    assert_eq!(
        restarted.height, 30.0,
        "the layout really did re-run, and nothing resized, so it re-ran from \
         the boundary"
    );
    assert_eq!(
        restarted.width, 100.0,
        "and at its own constraints: the surface's would leave it at 150"
    );
}

/// One bar per monitor, the shape `examples/multi_output.rs` and `outputs()`'s
/// own documentation both show: an effect over the reactive list that spawns a
/// surface pinned to every output it has not seen, and closes the handle of one
/// that has gone.
///
/// The map is what the effect keeps between runs, and the test reads it to
/// learn which surface belongs to which monitor.
fn a_bar_per_output() -> Rc<RefCell<HashMap<OutputId, SurfaceHandle>>> {
    let bars: Rc<RefCell<HashMap<OutputId, SurfaceHandle>>> = Rc::new(RefCell::new(HashMap::new()));
    let kept = bars.clone();
    create_effect(move || {
        let current = outputs().get();
        let mut bars = kept.borrow_mut();
        bars.retain(|id, handle| {
            let alive = current.iter().any(|o| o.id == *id);
            if !alive {
                handle.close();
            }
            alive
        });
        for info in current {
            bars.entry(info.id)
                .or_insert_with(|| spawn_surface(fixed_bar().output(info.id), measuring_24));
        }
    });
    bars
}

/// A monitor is plugged in and its bar arrives; it is unplugged and the bar
/// goes with it.
///
/// Nothing above the output seam had a sensor before: every scenario here ran
/// with an empty output list, so the effect that spawns a surface per monitor,
/// and the teardown when one leaves, were watched by a person with a spare
/// screen.
#[test]
fn a_surface_spawned_per_output_goes_when_its_output_goes() {
    let Some(mut app) = headless() else { return };
    let bars = a_bar_per_output();

    let laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");
    app.step();

    assert_eq!(
        app.surfaces_created().len(),
        2,
        "one surface for each connected monitor"
    );
    let going = bars.borrow()[&external].id();
    app.enter_output(going, external);
    assert_eq!(
        surface_output(going),
        Some(external),
        "the compositor put it on the monitor it was pinned to"
    );

    app.disconnect_output(external);
    app.step();

    assert_eq!(
        app.surfaces_destroyed(),
        [going],
        "the bar of the monitor that left, and only it"
    );
    assert_eq!(
        surface_output(going),
        None,
        "and nothing is still saying which screen it was on"
    );
    let left = outputs().get();
    assert_eq!(
        left.iter().map(|o| o.id).collect::<Vec<_>>(),
        [laptop],
        "and the list holds what is still plugged in"
    );
}

/// The bar of a monitor that left is torn down on both sides — the compositor
/// is told to destroy it, and the application stops holding it — and it is
/// told once.
///
/// Abandoning it is the failure this watches: a surface the compositor has
/// destroyed but the `SurfaceManager` still holds is one the loop goes on
/// laying out and drawing into every frame. Re-spawning it is the other, and
/// the effect re-runs often enough for that to be a real way to be wrong.
#[test]
fn a_departed_outputs_surface_is_torn_down_rather_than_abandoned() {
    let Some(mut app) = headless() else { return };
    let bars = a_bar_per_output();

    let laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");
    app.step();

    let (kept, going) = (bars.borrow()[&laptop].id(), bars.borrow()[&external].id());
    app.configure(kept, 200, 50, 1.0);
    app.configure(going, 300, 50, 1.0);
    app.step();

    app.disconnect_output(external);
    app.step();
    for _ in 0..5 {
        app.step();
    }

    assert_eq!(
        app.surfaces_destroyed(),
        [going],
        "destroyed once, and never asked for again"
    );
    assert_eq!(
        app.surfaces_live(),
        [kept],
        "the application holds the one monitor that is left, and nothing else"
    );
}

/// What a lock screen is built from: one per monitor, saying which.
fn lock_screen(output: OutputInfo) -> Text {
    text(format!("locked: {}", output.id.raw()))
}

/// A lock covers every connected monitor exactly once, and unlocking takes
/// every cover away.
///
/// The whole of `src/session_lock.rs` had nothing watching it: `Recorder` took
/// the trait's defaults for all four lock methods, so a test could call
/// `lock_session` and the loop would find a compositor that refuses.
#[test]
fn a_lock_covers_each_connected_output_exactly_once() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");

    lock_session(lock_screen);
    app.step();

    assert_eq!(
        lock_state().get_untracked(),
        LockState::Locking,
        "asked for, and nothing has answered yet"
    );
    assert!(
        app.lock_surface_requests().is_empty(),
        "so no monitor has been asked to be covered"
    );

    app.grant_lock();
    app.step();

    assert!(app.is_locked(), "the compositor is holding a grant");
    assert_eq!(
        lock_state().get_untracked(),
        LockState::Locked,
        "and the application knows it"
    );
    let (covers, screens): (Vec<SurfaceId>, Vec<OutputId>) =
        app.lock_surfaces_created().into_iter().unzip();
    assert_eq!(
        screens,
        [laptop, external],
        "one cover per monitor, and only one"
    );

    unlock_session();
    app.step();

    assert!(!app.is_locked(), "the grant was handed back");
    assert_eq!(lock_state().get_untracked(), LockState::Unlocked);
    // Sorted, not in the order they went: the teardown drains a map, and
    // nothing in the protocol cares which cover is destroyed first — unlike a
    // popup chain, where the order is the assertion.
    let mut taken = app.surfaces_destroyed().to_vec();
    taken.sort_by_key(|id| id.raw());
    assert_eq!(taken, covers, "both covers were taken away");
}

/// A monitor unplugged while the session is locked is asked for nothing
/// further.
///
/// This is #422 as the application sees it: a lock surface is asked for once
/// per output per iteration until one exists, so an output that is gone but
/// still listed is asked sixty times a second for ever — 104 refusals in five
/// seconds on the screen it was found on. The count standing still over thirty
/// frames is the assertion that would have caught it.
#[test]
fn an_output_that_leaves_mid_lock_is_asked_for_nothing_further() {
    let Some(mut app) = headless() else { return };
    let _laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");

    lock_session(lock_screen);
    app.step();
    app.grant_lock();
    app.step();

    assert_eq!(app.lock_surface_requests().len(), 2, "one ask per monitor");
    let going = app
        .lock_surfaces_created()
        .into_iter()
        .find(|(_, output)| *output == external)
        .map(|(id, _)| id)
        .expect("the external monitor was covered");

    app.disconnect_output(external);
    for _ in 0..30 {
        app.step();
    }

    assert_eq!(
        app.lock_surface_requests().len(),
        2,
        "a monitor that has gone is not asked for a cover again"
    );
    assert_eq!(
        app.surfaces_destroyed(),
        [going],
        "its cover was taken away instead, and the one that stayed kept its own"
    );
}

/// The pointer arriving over a surface at a point, as `wl_pointer.enter` says
/// it: an enter, then the move that places it.
fn pointer_enters(app: &mut Headless, surface: SurfaceId) {
    let at = Instant::now();
    let point = Point::new(10.0, 10.0);
    app.event_at(surface, Event::MouseEnter { at: Some(point) }, at);
    app.event_at(surface, Event::mouse_move(point.x, point.y), at);
}

/// A shape is bound to the enter it was set against, and the compositor draws
/// nothing until one is. A surface that never asks for a cursor still owes the
/// pointer the arrow: `Default` is a shape like any other, not the absence of
/// one — which is what dropping it as "no change" made it (#479).
#[test]
fn a_surface_that_never_sets_a_cursor_asks_for_the_arrow_on_enter() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || container().width(fill()).height(fill()));
    app.configure(surface, 200, 50, 1.0);
    app.step();
    assert_eq!(app.cursors_asked(), [], "no pointer has entered yet");

    pointer_enters(&mut app, surface);
    app.step();

    assert_eq!(app.cursors_asked(), [CursorIcon::Default]);
}

/// Each enter is a new serial and a cursor left undefined until it is set
/// against that one — so going to another surface and back asks again every
/// time, even though the shape itself never changed.
#[test]
fn every_enter_asks_for_the_shape_again() {
    let Some(mut app) = headless() else { return };
    let left = app.surface(fixed_bar(), || container().width(fill()).height(fill()));
    let right = app.surface(fixed_bar(), || container().width(fill()).height(fill()));
    app.configure(left, 200, 50, 1.0);
    app.configure(right, 200, 50, 1.0);
    app.step();

    for surface in [left, right, left] {
        pointer_enters(&mut app, surface);
        app.step();
        app.event_at(surface, Event::MouseLeave, Instant::now());
        app.step();
    }

    assert_eq!(app.cursors_asked(), [CursorIcon::Default; 3]);
}

// ---------------------------------------------------------------------------
// The cursor, resolved from the point after every pointer event (#496)
// ---------------------------------------------------------------------------

/// A move to a point on a surface, now.
fn pointer_moves(app: &mut Headless, surface: SurfaceId, x: f32, y: f32) {
    app.event_at(surface, Event::mouse_move(x, y), Instant::now());
    app.step();
}

/// A bar whose left 120 pixels are `left`, and whose rest declares nothing.
fn left_of_a_bar(left: Container) -> Container {
    container()
        .width(fill())
        .height(fill())
        .layout(Flex::row())
        .child(left.width(120.0).height(fill()))
}

#[test]
fn hovering_a_container_that_declares_a_cursor_asks_for_it_and_leaving_asks_for_the_arrow() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        left_of_a_bar(container().cursor(CursorIcon::Pointer))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Pointer]);

    pointer_moves(&mut app, surface, 160.0, 10.0);
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Pointer, CursorIcon::Default]
    );
}

/// The innermost declaration under the point wins, and the one around it
/// takes over again the moment the pointer is only over that — which is what
/// an imperative `set_cursor(Default)` on the field's leave could never say.
#[test]
fn a_field_inside_a_clickable_row_shows_its_own_cursor_and_hands_the_row_its_back() {
    let Some(mut app) = headless() else { return };
    let value = create_signal(String::new());
    let surface = app.surface(fixed_bar(), move || {
        left_of_a_bar(
            container()
                .cursor(CursorIcon::Pointer)
                .child(text_input(value)),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Text], "over the field");

    pointer_moves(&mut app, surface, 10.0, 40.0);
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Text, CursorIcon::Pointer],
        "over the row, below the field"
    );

    pointer_moves(&mut app, surface, 160.0, 40.0);
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Text, CursorIcon::Pointer, CursorIcon::Default],
        "outside the row"
    );
}

#[test]
fn a_cursor_given_by_a_signal_changes_while_the_pointer_stays_still() {
    let Some(mut app) = headless() else { return };
    let busy = create_signal(false);
    let surface = app.surface(fixed_bar(), move || {
        container().width(fill()).height(fill()).cursor(move || {
            if busy.get() {
                CursorIcon::Wait
            } else {
                CursorIcon::Default
            }
        })
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Default]);

    busy.set(true);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Default, CursorIcon::Wait]);

    busy.set(false);
    app.step();
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Default, CursorIcon::Wait, CursorIcon::Default]
    );
}

/// A declaration on a hole in the surface's input is a declaration over a
/// place the pointer is not being given: it claims nothing, and the container
/// beneath it answers.
#[test]
fn a_container_that_takes_no_input_leaves_the_cursor_to_the_one_beneath_it() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .cursor(CursorIcon::Pointer)
            .child(
                container()
                    .width(120.0)
                    .height(fill())
                    .takes_input(false)
                    .cursor(CursorIcon::Crosshair),
            )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Pointer]);
}

/// The declaration under a still pointer can go away with its widget, and
/// what its closure read does not go with it. A write to that afterwards is
/// not a shape to show — it used to reach the closure's corpse and panic.
#[test]
fn a_cursor_closure_whose_widget_is_gone_is_not_read_again() {
    let Some(mut app) = headless() else { return };
    let shown = create_signal(true);
    let loading = create_signal(false);
    let surface = app.surface(fixed_bar(), move || {
        container().width(fill()).height(fill()).child(move || {
            shown.get().then(|| {
                container().width(fill()).height(fill()).cursor(move || {
                    if loading.get() {
                        CursorIcon::Wait
                    } else {
                        CursorIcon::Pointer
                    }
                })
            })
        })
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Pointer]);

    shown.set(false);
    app.step();
    loading.set(true);
    app.step();

    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Pointer],
        "nothing is asked for a declaration that is no longer there"
    );
}
