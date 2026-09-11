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
