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
use guido::reactive::clipboard::{
    SelectionKind, clipboard_copy, clipboard_has_content, clipboard_paste, primary_copy,
    primary_paste,
};
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

/// A bar anchored to every edge, so that the compositor sizes it and nothing
/// is ever drawn at a size it did not give (#909). Anchored to both edges of an
/// axis, a positive zone counts for nothing unless the zone names its edge.
fn stretched_bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
        .exclusive_zone(52)
        .exclusive_edge(Edge::Bottom)
}

/// The zone and the edge it applies to are declared together, at creation.
#[test]
fn a_stretched_bar_reserves_its_zone_at_the_edge_it_names() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let bar = app.surface(stretched_bar(), move || self::bar(clicks));
    app.configure(bar, 1440, 892, 1.0);
    app.step();

    assert_eq!(app.exclusive_zones_asked(bar), [52]);
    assert_eq!(app.exclusive_edges_asked(bar), [Some(Edge::Bottom)]);
}

/// `Auto` reserves the extent on the edge's axis, and on a stretched axis that
/// extent is the screen's: nothing a bar means to keep for itself.
#[test]
fn auto_reserves_nothing_on_an_axis_the_compositor_stretches() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let bar = app.surface(
        stretched_bar().exclusive_zone(ExclusiveZone::Auto),
        move || self::bar(clicks),
    );
    app.configure(bar, 1440, 892, 1.0);
    app.step();

    assert_eq!(app.exclusive_zones_asked(bar), [0]);
}

/// `Auto` with an edge on an axis the surface sizes itself follows that axis,
/// as it would anchored to the edge alone: a corner bar reserving its height.
#[test]
fn auto_follows_the_named_edge_in_a_corner() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let corner = SurfaceConfig::new()
        .width(300)
        .height(40)
        .anchor(Anchor::TOP | Anchor::LEFT)
        .exclusive_zone(ExclusiveZone::Auto)
        .exclusive_edge(Edge::Top);
    let bar = app.surface(corner, move || self::bar(clicks));
    app.configure(bar, 300, 40, 1.0);
    app.step();

    assert_eq!(app.exclusive_zones_asked(bar), [40]);
    assert_eq!(app.exclusive_edges_asked(bar), [Some(Edge::Top)]);
}

/// A bar along one edge and both beside it keeps its height along that edge:
/// the axis the compositor stretches is the other one.
#[test]
fn auto_follows_the_named_edge_across_the_stretched_axis() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let top_bar = fixed_bar()
        .exclusive_zone(ExclusiveZone::Auto)
        .exclusive_edge(Edge::Top);
    let bar = app.surface(top_bar, move || self::bar(clicks));

    assert_eq!(app.exclusive_zones_asked(bar), [50]);
}

/// The compositor refuses an edge the surface is not anchored to as a protocol
/// error, so it is never sent: neither at creation, nor left standing by a
/// re-anchoring that leaves it, which withdraws it in the same group as the
/// anchor.
#[test]
fn an_edge_the_surface_is_not_anchored_to_is_never_sent() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let top_bar = fixed_bar().exclusive_zone(52).exclusive_edge(Edge::Bottom);
    let bar = app.surface(top_bar, move || self::bar(clicks));
    assert_eq!(app.exclusive_edges_asked(bar), [None], "not anchored to it");

    let stretched = app.surface(stretched_bar(), move || self::bar(clicks));
    app.configure(stretched, 1440, 892, 1.0);
    app.step();
    surface_handle(stretched).set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    app.step();

    assert_eq!(
        app.exclusive_edges_asked(stretched),
        [Some(Edge::Bottom), None],
        "the re-anchoring left the edge, and took it back"
    );
}

/// Set at runtime, like the zone, and sent with it.
#[test]
fn a_handle_moves_the_edge_the_zone_applies_to() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let bar = app.surface(stretched_bar(), move || self::bar(clicks));
    app.configure(bar, 1440, 892, 1.0);
    app.step();

    surface_handle(bar).set_exclusive_edge(Some(Edge::Top));
    app.step();

    assert_eq!(app.exclusive_zones_asked(bar), [52, 52]);
    assert_eq!(
        app.exclusive_edges_asked(bar),
        [Some(Edge::Bottom), Some(Edge::Top)]
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

/// Work queued for a surface that is waiting on the compositor cannot run until
/// the callback arrives, and that arrival wakes the loop by itself. So the loop
/// sleeps through the wait rather than polling for the work at frame rate —
/// which, with an output switched off, it would have done for as long as the
/// output stayed off.
#[test]
fn work_waiting_on_a_frame_callback_does_not_keep_the_loop_polling() {
    let Some(mut app) = headless() else { return };
    let height = create_signal(20.0);
    let surface = app.surface(fixed_bar(), move || filling(height));
    app.configure(surface, 200, 50, 1.0);
    app.step();
    app.step();
    assert_eq!(app.next_wait(), None, "an idle application blocks");

    app.hold_frame_callbacks(surface);
    height.set(30.0);
    app.step();
    let drawn = app.frames_presented(surface);
    height.set(40.0);
    app.step();
    assert_eq!(
        app.frames_presented(surface),
        drawn,
        "the frame waits for the one before it to be shown"
    );
    assert_eq!(
        app.next_wait(),
        None,
        "and the loop waits with it, rather than polling for work it cannot run"
    );

    app.release_frame_callbacks(surface);
    app.step();
    assert_eq!(
        app.frames_presented(surface),
        drawn + 1,
        "the callback is what runs it"
    );
}

/// The other side of the test above: a surface with nothing on screen yet, or a
/// change nothing is waiting on, keeps the loop polling. A loop that slept
/// through either would leave it undrawn until something else woke it.
#[test]
fn work_the_loop_can_run_keeps_it_polling() {
    let Some(mut app) = headless() else { return };
    let height = create_signal(20.0);
    let surface = app.surface(fixed_bar(), move || filling(height));
    app.configure(surface, 200, 50, 1.0);
    assert_eq!(
        app.next_wait(),
        Some(std::time::Duration::from_millis(16)),
        "a surface with nothing presented yet"
    );

    app.step();
    app.step();
    assert_eq!(app.next_wait(), None, "an idle application blocks");

    height.set(30.0);
    assert_eq!(
        app.next_wait(),
        Some(std::time::Duration::from_millis(16)),
        "a change nothing is waiting on"
    );
}

/// A surface whose output is off never hears a frame callback, and on a
/// compositor without fractional scaling at scale 1 it is never told a scale
/// either. Neither is a reason to keep repainting it: once it has presented,
/// it has a callback to pace against, and a scale that arrives later is a
/// scale change, which opens the gate on its own. Before, it was redrawn in
/// full every 16 ms for as long as either stayed unsaid.
#[test]
fn a_surface_that_never_hears_a_frame_callback_stops_repainting_after_its_first_present() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || container().width(fill()).height(fill()));
    app.hold_frame_callbacks(surface);
    app.configure(surface, 200, 50, 1.0);
    app.step();
    assert_eq!(
        app.frames_presented(surface),
        1,
        "the first frame waits for nothing"
    );

    for _ in 0..3 {
        app.step();
    }
    assert_eq!(
        app.frames_presented(surface),
        1,
        "nothing changed, and nothing it drew has been shown"
    );
    assert_eq!(app.next_wait(), None, "so the loop blocks");
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

/// A panel that reads its own surface where it is built and resizes that
/// surface when clicked — the handle reached through the scope, not handed in
/// from outside after the factory has already been taken.
fn self_resizing_panel(seen: Rc<RefCell<Vec<SurfaceId>>>) -> impl FnOnce() -> Container {
    move || {
        let me = expect_context::<SurfaceHandle>();
        seen.borrow_mut().push(me.id());
        container()
            .width(fill())
            .height(fill())
            .on_click(move || me.set_size(300, 60))
    }
}

/// A fixed size on a corner anchor, so both axes are the surface's to ask for.
fn corner_panel() -> SurfaceConfig {
    SurfaceConfig::new()
        .width(200)
        .height(50)
        .anchor(Anchor::TOP | Anchor::LEFT)
}

#[test]
fn every_surface_tree_holds_the_handle_of_its_own_surface() {
    let Some(mut app) = headless() else { return };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let first = app.surface(corner_panel(), self_resizing_panel(seen.clone()));
    let second = app.surface(corner_panel(), self_resizing_panel(seen.clone()));
    let spawned = spawn_surface(corner_panel(), self_resizing_panel(seen.clone()));
    app.step();
    let popup = spawn_popup(
        first,
        PopupConfig::new(80).height(40),
        self_resizing_panel(seen.clone()),
    );
    app.step();

    assert_eq!(
        *seen.borrow(),
        [first, second, spawned.id(), popup.id()],
        "each factory found its own surface, the popup's its own and not its parent's"
    );

    for id in [first, second, spawned.id()] {
        app.configure(id, 200, 50, 1.0);
    }
    app.step();
    let first_asks = app.sizes_asked(first).len();
    let spawned_asks = app.sizes_asked(spawned.id()).len();

    app.click(second, 10.0, 10.0);
    app.step();
    // The click queues the request after this pass has drained its commands,
    // so the next pass is the one that sends it.
    app.step();

    assert_eq!(
        app.sizes_asked(second).last(),
        Some(&(300, 60)),
        "the click resized the surface it was in"
    );
    assert_eq!(app.sizes_asked(first).len(), first_asks, "and not another");
    assert_eq!(app.sizes_asked(spawned.id()).len(), spawned_asks);
}

/// A lock cover is a surface like any other, built once per output from one
/// factory: each build reads the cover it is building.
#[test]
fn every_lock_cover_holds_the_handle_of_its_own_surface() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");
    let seen = Rc::new(RefCell::new(HashMap::new()));
    let built = seen.clone();
    lock_session(move |output: OutputInfo| {
        built
            .borrow_mut()
            .insert(output.id, expect_context::<SurfaceHandle>().id());
        container()
    });
    app.step();
    app.grant_lock();
    app.step();

    assert_eq!(seen.borrow()[&laptop], cover_of(&app, laptop));
    assert_eq!(seen.borrow()[&external], cover_of(&app, external));
}

/// A lock with one cover, whose handle comes back for the test to call.
fn lock_keeping_the_cover_handle(app: &mut Headless) -> (OutputId, SurfaceHandle) {
    let laptop = app.connect_output("eDP-1");
    let handle = Rc::new(RefCell::new(None));
    let kept = handle.clone();
    lock_session(move |_: OutputInfo| {
        *kept.borrow_mut() = Some(expect_context::<SurfaceHandle>());
        container()
    });
    app.step();
    app.grant_lock();
    assert_eq!(app.step(), None, "the grant is not the end of the app");
    let handle = handle.borrow_mut().take().expect("the cover was built");
    (laptop, handle)
}

/// #804: a lock screen that closes its own cover — a dismiss button, or a
/// component written for ordinary surfaces — leaves it where it is. A cover
/// lasts as long as the lock; `unlock_session` is the way out.
///
/// Closing it used to count as the last surface closing, so the application
/// quit with the session still locked, and ext-session-lock-v1 keeps a session
/// locked when the client holding it dies.
#[test]
fn closing_a_lock_cover_leaves_it_covering_its_output() {
    let Some(mut app) = headless() else { return };
    let (laptop, handle) = lock_keeping_the_cover_handle(&mut app);
    let cover = cover_of(&app, laptop);

    handle.close();

    assert_eq!(app.step(), None, "the application keeps running");
    assert!(app.is_locked(), "the session is still locked");
    assert_eq!(lock_state().get_untracked(), LockState::Locked);
    assert_eq!(app.surfaces_live(), [cover], "the output keeps its cover");
    assert!(
        app.surfaces_destroyed().is_empty(),
        "and the compositor was told to destroy nothing"
    );
}

/// A cover whose tree closes it while it is still being built — a component
/// that dismisses itself on its first run — is a cover already, and stays.
#[test]
fn a_lock_cover_closed_while_it_is_built_still_covers_its_output() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");
    lock_session(|_: OutputInfo| {
        expect_context::<SurfaceHandle>().close();
        container()
    });
    app.step();
    app.grant_lock();

    assert_eq!(app.step(), None, "the application keeps running");
    assert!(app.is_locked());
    assert_eq!(app.surfaces_live(), [cover_of(&app, laptop)]);
}

/// A close that reaches a cover the unlock already took away — from a handle
/// that outlived it — is a close of nothing. An application whose only
/// surfaces were covers idles after unlocking, as it would without the close.
#[test]
fn closing_a_lock_cover_after_the_unlock_is_harmless() {
    let Some(mut app) = headless() else { return };
    let (laptop, handle) = lock_keeping_the_cover_handle(&mut app);
    let cover = cover_of(&app, laptop);
    unlock_session();
    assert_eq!(app.step(), None, "unlocking is not the end of the app");
    assert_eq!(
        app.surfaces_destroyed(),
        [cover],
        "the unlock took the cover"
    );

    handle.close();

    assert_eq!(app.step(), None, "the application keeps running");
    assert!(!app.is_locked());
    assert_eq!(
        app.surfaces_destroyed(),
        [cover],
        "and the compositor was told about it once"
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Marker(u32);

/// What each closure read, by the name of the closure that read it.
type Reads = Rc<RefCell<Vec<(&'static str, Option<Marker>)>>>;

/// Note under `name` what `Marker` the current scope reads.
fn note(reads: &Reads, name: &'static str) {
    reads.borrow_mut().push((name, use_context::<Marker>()));
}

/// #786: a dynamic child's closure runs when the tree is laid out, long after
/// the factory that wrote it returned. It reads from the scope that was
/// current where it was written — the surface's — not from whatever is current
/// when the frame gets round to it. Writing `rerun` runs it again from the
/// frame's jobs rather than its layout, and that run reads the same.
#[test]
fn a_dynamic_child_reads_what_its_surface_declared() {
    let Some(mut app) = headless() else { return };
    let reads: Reads = Rc::default();
    let log = reads.clone();
    let rerun = create_signal(0u32);
    let surface = app.surface(SurfaceConfig::new().width(100).height(100), move || {
        provide_context(Marker(7));
        note(&log, "factory");
        container().child(move || {
            rerun.get();
            note(&log, "dynamic child");
            let log = log.clone();
            container().child(move || {
                note(&log, "nested dynamic child");
                container()
            })
        })
    });
    app.configure(surface, 100, 100, 1.0);
    app.step();
    app.step();
    rerun.set(1);
    app.step();

    assert_eq!(
        *reads.borrow(),
        [
            ("factory", Some(Marker(7))),
            ("dynamic child", Some(Marker(7))),
            ("nested dynamic child", Some(Marker(7))),
            ("dynamic child", Some(Marker(7))),
            ("nested dynamic child", Some(Marker(7))),
        ]
    );
}

/// #877: a dynamic child depends on what its latest run read, as an effect
/// does. Once `flag` sends it down the branch that does not read `other`, a
/// write to `other` leaves it alone — where it used to run again, rebuilding
/// what it returned and everything under it.
#[test]
fn a_dynamic_child_forgets_what_its_earlier_runs_read() {
    let Some(mut app) = headless() else { return };
    let runs = Rc::new(Cell::new(0u32));
    let flag = create_signal(false);
    let other = create_signal(0u32);
    let counted = runs.clone();
    let surface = app.surface(SurfaceConfig::new().width(100).height(100), move || {
        let runs = counted.clone();
        container().child(move || {
            runs.set(runs.get() + 1);
            if flag.get() {
                return container();
            }
            other.get();
            container()
        })
    });
    app.configure(surface, 100, 100, 1.0);
    app.step();
    assert_eq!(runs.get(), 1);

    other.set(1);
    app.step();
    assert_eq!(runs.get(), 2, "the first branch reads `other`");

    flag.set(true);
    app.step();
    assert_eq!(runs.get(), 3);

    other.set(2);
    app.step();
    assert_eq!(runs.get(), 3, "the latest run did not read `other`");
}

/// Forgetting is per segment: a dynamic child that re-runs drops its own
/// reads, not those of the dynamic child beside it in the same container.
#[test]
fn a_dynamic_child_forgets_only_its_own_reads() {
    let Some(mut app) = headless() else { return };
    let runs = Rc::new(Cell::new((0u32, 0u32)));
    let first = create_signal(0u32);
    let second = create_signal(0u32);
    let counted = runs.clone();
    let surface = app.surface(SurfaceConfig::new().width(100).height(100), move || {
        let (a, b) = (counted.clone(), counted.clone());
        container()
            .child(move || {
                first.get();
                a.set((a.get().0 + 1, a.get().1));
                container()
            })
            .child(move || {
                second.get();
                b.set((b.get().0, b.get().1 + 1));
                container()
            })
    });
    app.configure(surface, 100, 100, 1.0);
    app.step();
    assert_eq!(runs.get(), (1, 1));

    first.set(1);
    app.step();
    assert_eq!(runs.get(), (2, 1));

    second.set(1);
    app.step();
    assert_eq!(runs.get(), (2, 2), "the second child still hears `second`");
}

/// The rows of a keyed list are built by the same reconciler, so they read the
/// same scope — and a list inside a row reads it through the row.
#[test]
fn a_keyed_row_reads_what_its_surface_declared() {
    let Some(mut app) = headless() else { return };
    let reads: Reads = Rc::default();
    let log = reads.clone();
    let surface = app.surface(SurfaceConfig::new().width(100).height(100), move || {
        provide_context(Marker(7));
        container().children(keyed(
            || vec![1u32, 2],
            |k| *k,
            move |_| {
                note(&log, "row");
                let log = log.clone();
                container().children(keyed(
                    || vec![1u32],
                    |k| *k,
                    move |_| {
                        note(&log, "row in a row");
                        container()
                    },
                ))
            },
        ))
    });
    app.configure(surface, 100, 100, 1.0);
    app.step();

    assert_eq!(
        *reads.borrow(),
        [
            ("row", Some(Marker(7))),
            ("row", Some(Marker(7))),
            ("row in a row", Some(Marker(7))),
            ("row in a row", Some(Marker(7))),
        ]
    );
}

/// A list inside a row reads what the row declared, and goes on reading it
/// when a signal only the inner list read is written — a run the frame's jobs
/// make, with no row laying out around it.
#[test]
fn a_list_inside_a_row_reads_what_the_row_declared_when_it_runs_again() {
    let Some(mut app) = headless() else { return };
    let reads: Reads = Rc::default();
    let log = reads.clone();
    let rerun = create_signal(0u32);
    let surface = app.surface(SurfaceConfig::new().width(100).height(100), move || {
        provide_context(Marker(7));
        container().children(keyed(
            || vec![3u32],
            |k| *k,
            move |k| {
                provide_context(Marker(k));
                let log = log.clone();
                container().child(move || {
                    rerun.get();
                    note(&log, "list in a row");
                    container()
                })
            },
        ))
    });
    app.configure(surface, 100, 100, 1.0);
    app.step();
    rerun.set(1);
    app.step();

    assert_eq!(
        *reads.borrow(),
        [
            ("list in a row", Some(Marker(3))),
            ("list in a row", Some(Marker(3))),
        ]
    );
}

/// A popup is a surface of its own, and its dynamic children read what *it*
/// declared — over the bar it hangs from, and with its own handle.
#[test]
fn a_popup_s_dynamic_child_reads_what_the_popup_declared() {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), || {
        provide_context(Marker(7));
        measuring_24()
    });
    app.configure(parent, 200, 24, 1.0);
    app.step();

    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let popup = spawn_popup(parent, PopupConfig::new(80).height(40), move || {
        provide_context(Marker(9));
        container().child(move || {
            log.borrow_mut().push((
                use_context::<Marker>(),
                use_context::<SurfaceHandle>().map(|me| me.id()),
            ));
            container()
        })
    });
    app.step();
    app.configure(popup.id(), 80, 40, 1.0);
    app.step();

    assert_eq!(*seen.borrow(), [(Some(Marker(9)), Some(popup.id()))]);
}

/// What each `probe` body read, under the tag it was given: the `Marker` and
/// the id of the surface whose handle it found. A thread's own, so the tests
/// running beside one another each see only theirs.
type ProbeReads = Vec<(String, Option<Marker>, Option<SurfaceId>)>;

thread_local!(static PROBED: RefCell<ProbeReads> = RefCell::default());

#[component]
fn probe(tag: String) -> impl Widget {
    PROBED.with(|seen| {
        seen.borrow_mut().push((
            tag.get_untracked(),
            use_context::<Marker>(),
            use_context::<SurfaceHandle>().map(|me| me.id()),
        ))
    });
    container().width(10.0).height(10.0)
}

#[component]
fn declaring(marker: u32) -> impl Widget {
    provide_context(Marker(marker.get_untracked()));
    container().child(probe().tag("in a component"))
}

/// #860: a component's body runs when the tree first asks it for something,
/// after the factory that made it returned. It reads from the scope that was
/// current where it was made, as a dynamic child's closure does (#786) —
/// whether it was placed as a plain child, in a dynamic one or in a keyed row.
#[test]
fn a_component_reads_what_the_scope_it_was_made_in_declared() {
    let Some(mut app) = headless() else { return };
    let shown = create_signal(false);
    let surface = app.surface(SurfaceConfig::new().width(200).height(100), move || {
        provide_context(Marker(7));
        container()
            .child(probe().tag("static"))
            .child(move || shown.get().then(|| probe().tag("dynamic")))
            .children(keyed(
                move || if shown.get() { vec![1u32] } else { vec![] },
                |k| *k,
                |_| probe().tag("keyed"),
            ))
    });
    app.configure(surface, 200, 100, 1.0);
    app.step();
    shown.set(true);
    app.step();

    let mut seen = PROBED.take();
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    let read = |tag: &str| (tag.to_string(), Some(Marker(7)), Some(surface));
    assert_eq!(seen, [read("dynamic"), read("keyed"), read("static")]);
}

/// A component placed as a plain child of another component reads what the
/// outer one declared in its body: the outer body is the scope the inner one
/// was made in.
#[test]
fn a_component_inside_a_component_reads_what_the_outer_one_declared() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(SurfaceConfig::new().width(200).height(100), || {
        provide_context(Marker(7));
        container().child(declaring().marker(3u32))
    });
    app.configure(surface, 200, 100, 1.0);
    app.step();

    assert_eq!(
        PROBED.take(),
        [("in a component".to_string(), Some(Marker(3)), Some(surface))]
    );
}

#[test]
fn replacing_the_last_surface_in_one_step_keeps_the_loop_running() {
    let Some(mut app) = headless() else { return };
    let first = app.surface(content_bar(), measuring_24);
    app.configure(first, 200, 24, 1.0);
    app.step();

    surface_handle(first).close();
    let replacement = spawn_surface(content_bar(), measuring_24);

    assert_eq!(app.step(), None);
    assert_eq!(app.surfaces_destroyed(), [first]);
    assert_eq!(app.surfaces_live(), [replacement.id()]);
}

/// A surface whose root counts the actions it takes and closes itself on the
/// first one, as a dialog does on Enter or on its button. `alive` is held by
/// the widget, so its strong count says whether the widget is still there.
struct ClosingDialog {
    actions: Rc<Cell<u32>>,
    alive: Rc<()>,
}

impl ClosingDialog {
    fn new() -> Self {
        Self {
            actions: Rc::new(Cell::new(0)),
            alive: Rc::new(()),
        }
    }

    /// Open it as a surface of its own. What closes it is its own handle, so
    /// the id is handed to the widget once the surface has one.
    fn open(&self, app: &mut Headless) -> SurfaceId {
        let actions = self.actions.clone();
        let alive = self.alive.clone();
        let own: Rc<Cell<Option<SurfaceId>>> = Rc::new(Cell::new(None));
        let handed = own.clone();
        let id = app.surface(SurfaceConfig::new().height(40), move || {
            let act = Rc::new(move || {
                let _held = &alive;
                actions.set(actions.get() + 1);
                surface_handle(own.get().expect("opened")).close();
            });
            let on_key = act.clone();
            container()
                .width(160.0)
                .height(40.0)
                .on_click(move || act())
                .on_key_down(move |key, _, repeat| {
                    if key == Key::Enter && !repeat {
                        on_key();
                        EventResponse::Handled
                    } else {
                        EventResponse::Ignored
                    }
                })
        });
        handed.set(Some(id));
        app.configure(id, 160, 40, 1.0);
        app.step();
        id
    }

    fn widget_alive(&self) -> bool {
        Rc::strong_count(&self.alive) > 1
    }
}

fn enter_down() -> Event {
    key_down(Key::Enter)
}

fn enter_up() -> Event {
    Event::KeyUp {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    }
}

/// A key press that closes its surface is the last input that surface's
/// widgets see: the second Enter, already queued in the same batch, would be a
/// second action from a dialog that has been dismissed (#675). Destruction
/// still waits for the command pass, as it did.
#[test]
fn a_surface_that_asked_to_close_takes_no_more_of_its_queued_keys() {
    let Some(mut app) = headless() else { return };
    let dialog = ClosingDialog::new();
    let id = dialog.open(&mut app);

    let now = Instant::now();
    for event in [enter_down(), enter_up(), enter_down()] {
        app.event_at(id, event, now);
    }
    app.step();
    assert_eq!(dialog.actions.get(), 1, "one action, then nothing");

    app.step();
    assert_eq!(app.surfaces_destroyed(), [id], "and the close went through");
    assert!(!dialog.widget_alive(), "taking its widget with it");
}

/// The same for the pointer: a double click on a button that closes its
/// surface acts once.
#[test]
fn a_surface_that_asked_to_close_takes_no_more_of_its_queued_clicks() {
    let Some(mut app) = headless() else { return };
    let dialog = ClosingDialog::new();
    let id = dialog.open(&mut app);

    app.click(id, 20.0, 20.0);
    app.click(id, 20.0, 20.0);
    app.step();
    assert_eq!(dialog.actions.get(), 1, "one action, then nothing");

    app.step();
    assert_eq!(app.surfaces_destroyed(), [id]);
    assert!(!dialog.widget_alive());
}

/// What stops is the closing surface's input, not the batch's: a second
/// dialog, clicked twice in the same step, acts once as well rather than not
/// at all.
///
/// Both close themselves because the order the loop visits surfaces in is not
/// the test's to choose. Whichever runs second is the one a close that stopped
/// every surface's input would have silenced.
#[test]
fn a_surface_closing_does_not_stop_the_input_of_another() {
    let Some(mut app) = headless() else { return };
    let first = ClosingDialog::new();
    let second = ClosingDialog::new();
    let a = first.open(&mut app);
    let b = second.open(&mut app);

    for _ in 0..2 {
        app.click(a, 20.0, 20.0);
        app.click(b, 20.0, 20.0);
    }
    app.step();

    assert_eq!(first.actions.get(), 1);
    assert_eq!(second.actions.get(), 1, "each took its own first click");
}

/// An item in a submenu closes the whole menu, which is how a menu is
/// dismissed: the close names the menu, and the submenu goes because it hangs
/// from it. So the submenu is closing too, and the second click queued on its
/// item is not a second pick.
#[test]
fn a_popup_whose_menu_asked_to_close_takes_no_more_of_its_queued_clicks() {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), measuring_24);
    app.configure(parent, 200, 24, 1.0);
    app.step();

    let menu = spawn_popup(parent, PopupConfig::new(80).height(40), measuring_24);
    app.step();
    let picks = create_signal(0u32);
    let submenu = spawn_popup(menu.id(), PopupConfig::new(60).height(30), move || {
        container().width(60.0).height(30.0).on_click(move || {
            picks.update(|n| *n += 1);
            menu.close();
        })
    });
    app.step();
    app.configure(submenu.id(), 60, 30, 1.0);
    app.step();

    app.click(submenu.id(), 10.0, 10.0);
    app.click(submenu.id(), 10.0, 10.0);
    app.step();

    assert_eq!(picks.get(), 1, "one pick, then nothing");
    app.step();
    assert_eq!(app.surfaces_live(), [parent], "and the whole menu went");
}

/// A resident program — a polkit agent, an OSD — shows a surface when asked
/// and closes it when done, and is still there to be asked again.
///
/// Closed through its handle, which is the path the compositor's own close of
/// a layer surface takes too.
#[test]
fn an_app_that_outlives_its_last_surface_can_show_another() {
    let Some(mut app) = headless() else { return };
    app.quit_on_last_surface(false);

    let asked = create_signal(true);
    let shown: Rc<RefCell<Option<SurfaceHandle>>> = Rc::new(RefCell::new(None));
    let kept = shown.clone();
    create_effect(move || {
        if asked.get() {
            *kept.borrow_mut() = Some(spawn_surface(content_bar(), measuring_24));
        } else if let Some(dialog) = kept.borrow_mut().take() {
            dialog.close();
        }
    });
    let shown = move || shown.borrow().as_ref().map(SurfaceHandle::id);
    app.step();
    assert!(shown().is_some(), "the effect spawned one");

    asked.set(false);
    assert_eq!(
        app.step(),
        None,
        "the last surface closed and the loop runs on"
    );
    assert_eq!(app.surfaces_live(), []);
    assert_eq!(app.step(), None, "and idles, with nothing to draw");

    asked.set(true);
    app.step();
    let second = shown().expect("asked again, and it answered");
    app.configure(second, 200, 24, 1.0);
    app.step();
    assert_eq!(app.frames_presented(second), 1, "the second one drew");
}

/// A bar per monitor, set not to quit, survives every monitor going and
/// puts a bar on the one that comes back.
#[test]
fn a_bar_per_output_outlives_its_last_monitor() {
    let Some(mut app) = headless() else { return };
    app.quit_on_last_surface(false);
    let bars = a_bar_per_output();

    let laptop = app.connect_output("eDP-1");
    app.step();
    app.disconnect_output(laptop);
    assert_eq!(app.step(), None, "no monitor, no bar, and the loop runs on");
    assert_eq!(app.surfaces_live(), []);

    let back = app.connect_output("eDP-1");
    app.step();
    assert_eq!(
        app.surfaces_live(),
        [bars.borrow()[&back].id()],
        "the monitor came back and so did its bar"
    );
}

/// An application between windows holds no device: the renderer and the
/// context outlive the last surface by a grace period, so a dialog opened
/// again straight away finds them, and go once it has passed.
#[test]
fn the_device_outlives_the_last_surface_by_a_grace_period_and_no_more() {
    let Some(mut app) = headless() else { return };
    app.quit_on_last_surface(false);
    app.own_gpu();
    assert!(!app.holds_gpu(), "nothing asked for one yet");

    let t0 = Instant::now();
    let show = |app: &mut Headless, at: Instant| {
        let dialog = spawn_surface(content_bar(), measuring_24);
        app.step_at(at);
        app.configure(dialog.id(), 200, 24, 1.0);
        app.step_at(at);
        assert_eq!(app.frames_presented(dialog.id()), 1);
        dialog
    };

    show(&mut app, t0).close();
    app.step_at(t0);
    assert!(
        app.holds_gpu(),
        "closed a moment ago: kept for the next one"
    );

    let again = t0 + Duration::from_secs(10);
    show(&mut app, again).close();
    app.step_at(again);
    app.step_at(again + Duration::from_secs(29));
    assert!(
        app.holds_gpu(),
        "the grace period counts from the last close, not the first"
    );

    app.step_at(again + Duration::from_secs(30));
    assert!(!app.holds_gpu(), "idle for the whole grace period: let go");

    let later = again + Duration::from_secs(60);
    show(&mut app, later);
    assert!(app.holds_gpu(), "and made again for the next surface");
}

/// A lock the application has nothing to draw with is never asked for.
///
/// ext-session-lock-v1 keeps a session locked when the client holding the
/// lock dies. A lock screen that asked first and found no GPU afterwards would
/// leave the user locked out behind a process that is gone.
#[test]
fn a_lock_without_a_gpu_is_refused_before_it_is_asked_for() {
    let Some(mut app) = headless() else { return };
    app.without_gpu();
    app.connect_output("eDP-1");

    lock_session(lock_screen);
    assert_eq!(app.step(), None, "a refused lock is not the end of the app");

    assert!(!app.lock_asked(), "the compositor never heard of it");
    assert_eq!(lock_state().get_untracked(), LockState::Unlocked);
    assert!(app.lock_surfaces_created().is_empty());
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

/// Sets its flag when it is dropped, so a test can tell whether the widget that
/// holds it is gone.
struct DropFlag(Rc<Cell<bool>>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

/// A popup the platform refuses, spawned on a bar closed in the same turn: its
/// dynamic child reads `poke` from outside and a signal of its own. Once `poke`
/// is written, the child must not have run again and the content must be gone.
fn a_refused_popup_is_taken_down(config: PopupConfig) {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), measuring_24);
    app.configure(parent, 200, 30, 1.0);
    app.step();

    let poke = create_signal(0u32);
    let runs = Rc::new(Cell::new(0u32));
    let dropped = Rc::new(Cell::new(false));
    let count = runs.clone();
    let flag = DropFlag(dropped.clone());
    surface_handle(parent).close();
    let popup = spawn_popup(parent, config, move || {
        let own = create_signal(0u32);
        container().child(move || {
            let _ = &flag; // moved into the child, so it drops with the widget
            count.set(count.get() + 1);
            poke.get();
            own.get();
            container().height(24.0)
        })
    });
    app.step();
    assert!(popup.dismissed(), "the platform refused the popup");

    let before = runs.get();
    poke.set(1);
    app.step();
    assert_eq!(runs.get(), before, "the refused popup's child ran again");
    assert!(
        dropped.get(),
        "the refused popup's content was never dropped"
    );
}

/// A popup the platform refuses is taken down as a closed one is (#856). An
/// auto-height popup was measured, so its dynamic child had run and subscribed
/// to `poke`; dropping the surface without unregistering the widgets left it
/// subscribed, and the next write reran it against a signal its owner had
/// already disposed.
#[test]
fn a_refused_popup_measured_for_its_height_runs_nothing_again() {
    a_refused_popup_is_taken_down(PopupConfig::new(80));
}

/// The same with a fixed height: nothing was measured, so nothing ran, but the
/// widgets stayed in the tree all the same.
#[test]
fn a_refused_popup_of_a_fixed_height_leaves_the_tree() {
    a_refused_popup_is_taken_down(PopupConfig::new(80).height(40));
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

/// A surface gets back the widget it had focused when the keyboard comes back
/// to it (#726).
///
/// A suggestion list opened from a search field, as a popup that grabs the
/// keyboard: the bar loses the keyboard to it, the popup's own field takes the
/// focus, and the popup closes, taking that field with it. Nothing is focused
/// then, and only the bar remembering its field puts the next key there — it
/// is what a `GtkWindow`'s focus widget, and a browser's `activeElement`
/// across a window blur, are for.
#[test]
fn a_field_has_the_focus_back_when_a_grabbing_popup_closes() {
    let Some(mut app) = headless() else { return };
    let field = create_widget_ref();
    let value = create_signal(String::new());
    let bar = app.surface(fixed_bar(), move || bar_with_a_field(field, value));
    app.configure(bar, 200, 50, 1.0);
    app.step();
    app.click(bar, 100.0, 8.0);
    app.step();
    assert!(field.is_focused(), "a click into the field takes the focus");

    let suggestion = create_widget_ref();
    let popup = spawn_popup(bar, PopupConfig::new(80).height(40).grab(), move || {
        container()
            .width(fill())
            .height(fill())
            .child(text_input(create_signal(String::new())).widget_ref(suggestion))
    });
    app.step();
    app.configure(popup.id(), 80, 40, 1.0);
    app.step();
    // The grab: the compositor takes the keyboard from the bar and hands it to
    // the popup, whose field the user then clicks into.
    let now = Instant::now();
    app.event_at(bar, Event::FocusOut, now);
    app.event_at(popup.id(), Event::FocusIn, now);
    app.step();
    app.click(popup.id(), 40.0, 8.0);
    app.step();
    assert!(suggestion.is_focused(), "the popup's field took the focus");

    popup.close();
    app.step();
    app.event_at(bar, Event::FocusIn, Instant::now());
    app.step();
    app.event_at(bar, key_down(Key::Char('x')), Instant::now());
    app.step();

    assert!(
        field.is_focused(),
        "the keyboard back on the bar, its field has the focus"
    );
    assert_eq!(value.get_untracked(), "x", "and the key is typed into it");
}

/// A bar whose field has the focus and the keyboard, and a grabbing popup
/// opened from it whose field asks for `autofocus`, spawned and not yet
/// configured. The field is in the popup while `shown` says so.
struct AutofocusPopup {
    bar: SurfaceId,
    field: WidgetRef,
    value: RwSignal<String>,
    popup: PopupHandle,
    suggestion: WidgetRef,
    typed: RwSignal<String>,
}

fn an_autofocus_popup_over_a_focused_field(
    app: &mut Headless,
    shown: RwSignal<bool>,
) -> AutofocusPopup {
    let field = create_widget_ref();
    let value = create_signal(String::new());
    let bar = app.surface(fixed_bar(), move || bar_with_a_field(field, value));
    app.configure(bar, 200, 50, 1.0);
    app.step();
    app.event_at(bar, Event::FocusIn, Instant::now());
    app.click(bar, 100.0, 8.0);
    app.step();
    assert!(field.is_focused(), "a click into the field takes the focus");

    let suggestion = create_widget_ref();
    let typed = create_signal(String::new());
    let popup = spawn_popup(bar, PopupConfig::new(80).height(40).grab(), move || {
        container().width(fill()).height(fill()).children(move || {
            shown
                .get()
                .then(|| text_input(typed).widget_ref(suggestion).autofocus())
        })
    });
    app.step();
    AutofocusPopup {
        bar,
        field,
        value,
        popup,
        suggestion,
        typed,
    }
}

/// A popup's `autofocus` field takes the focus from a field on the bar that
/// opened it once the keyboard reaches the popup, and the bar has its field
/// back when the popup closes (#731).
///
/// The popup is laid out while the keyboard is still on the bar, as it is on a
/// real compositor: its field cannot take the focus then, or the keys still
/// going to the bar would reach no field. It waits as the popup's kept focus,
/// and the enter gives it the focus. The bar's leave comes after the popup's
/// enter here, which a compositor may also do.
#[test]
fn a_popups_autofocus_field_takes_the_focus_from_the_bar_when_the_keyboard_reaches_it() {
    let Some(mut app) = headless() else { return };
    let AutofocusPopup {
        bar,
        field,
        value,
        popup,
        suggestion,
        typed,
    } = an_autofocus_popup_over_a_focused_field(&mut app, create_signal(true));
    app.configure(popup.id(), 80, 40, 1.0);
    app.step();
    assert!(
        field.is_focused(),
        "laid out before the keyboard moved, the popup's field leaves the focus where the keys go"
    );

    let now = Instant::now();
    app.event_at(popup.id(), Event::FocusIn, now);
    app.event_at(bar, Event::FocusOut, now);
    app.step();
    app.event_at(popup.id(), key_down(Key::Char('a')), Instant::now());
    app.step();
    assert!(
        suggestion.is_focused(),
        "the keyboard on the popup, its field has the focus"
    );
    assert_eq!(typed.get_untracked(), "a", "and the key is typed into it");

    popup.close();
    app.step();
    app.event_at(bar, Event::FocusIn, Instant::now());
    app.step();
    app.event_at(bar, key_down(Key::Char('x')), Instant::now());
    app.step();
    assert!(
        field.is_focused(),
        "the keyboard back on the bar, its field has the focus"
    );
    assert_eq!(value.get_untracked(), "x", "and the key is typed into it");
}

/// An `autofocus` field that appears on a popup the keyboard is already on
/// takes the focus there and then, from the bar's field (#731).
///
/// The keyboard entering a surface cannot come before the surface's own first
/// layout: a compositor enters only a mapped surface. A field shown later can,
/// and the enter it would wait for has already happened.
#[test]
fn an_autofocus_field_shown_on_a_popup_holding_the_keyboard_takes_the_focus() {
    let Some(mut app) = headless() else { return };
    let shown = create_signal(false);
    let AutofocusPopup {
        bar,
        field,
        popup,
        suggestion,
        typed,
        ..
    } = an_autofocus_popup_over_a_focused_field(&mut app, shown);
    app.configure(popup.id(), 80, 40, 1.0);
    app.step();
    let now = Instant::now();
    app.event_at(bar, Event::FocusOut, now);
    app.event_at(popup.id(), Event::FocusIn, now);
    app.step();
    assert!(
        field.is_focused(),
        "the popup has nothing to focus, so the bar's field keeps the focus"
    );

    shown.set(true);
    app.step();
    assert!(
        suggestion.is_focused(),
        "the field appeared where the keyboard is"
    );

    app.event_at(popup.id(), key_down(Key::Char('a')), Instant::now());
    app.step();
    assert_eq!(typed.get_untracked(), "a");
}

/// A lock over two monitors, each cover with an `autofocus` field: whichever
/// cover the keyboard enters has its own field focused, and the keys typed
/// there land in it (#731).
#[test]
fn a_lock_over_two_monitors_focuses_the_field_on_the_one_with_the_keyboard() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");
    let external = app.connect_output("DP-2");
    let fields = lock_with_an_autofocus_field_per_output();
    app.step();
    app.grant_lock();
    app.step();
    let (on_laptop, on_external) = (cover_of(&app, laptop), cover_of(&app, external));
    app.configure(on_laptop, 200, 50, 1.0);
    app.configure(on_external, 200, 50, 1.0);
    app.step();

    let now = Instant::now();
    app.event_at(on_external, Event::FocusIn, now);
    app.event_at(on_external, key_down(Key::Char('a')), now);
    app.step();
    let (external_field, external_value) = fields.borrow()[&external];
    assert!(
        external_field.is_focused(),
        "the keyboard on the second cover, its field has the focus"
    );
    assert_eq!(external_value.get_untracked(), "a");
}

/// A cover's field, and the text typed into it.
type LockField = (WidgetRef, RwSignal<String>);

/// Lock the session with a cover per output holding an `autofocus` field,
/// and hand back each output's field and its text.
fn lock_with_an_autofocus_field_per_output() -> Rc<RefCell<HashMap<OutputId, LockField>>> {
    let fields: Rc<RefCell<HashMap<OutputId, LockField>>> = Rc::default();
    let built = fields.clone();
    lock_session(move |output: OutputInfo| {
        let (field, value) = (create_widget_ref(), create_signal(String::new()));
        built.borrow_mut().insert(output.id, (field, value));
        container()
            .width(fill())
            .height(fill())
            .child(text_input(value).widget_ref(field).autofocus())
    });
    fields
}

/// A monitor plugged in while the session is locked gets a cover whose
/// `autofocus` field does not take the focus from the password being typed on
/// the other one: the keyboard stays there, and the keys must too. The new
/// cover's field has the focus when the keyboard comes to it (#731).
#[test]
fn a_lock_cover_built_mid_lock_does_not_take_the_focus_from_the_one_being_typed_into() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");
    let fields = lock_with_an_autofocus_field_per_output();
    app.step();
    app.grant_lock();
    app.step();
    let on_laptop = cover_of(&app, laptop);
    app.configure(on_laptop, 200, 50, 1.0);
    app.step();
    app.event_at(on_laptop, Event::FocusIn, Instant::now());
    app.event_at(on_laptop, key_down(Key::Char('a')), Instant::now());
    app.step();
    let (laptop_field, laptop_value) = fields.borrow()[&laptop];
    assert!(laptop_field.is_focused());
    assert_eq!(laptop_value.get_untracked(), "a");

    let external = app.connect_output("DP-2");
    app.step();
    let on_external = cover_of(&app, external);
    app.configure(on_external, 200, 50, 1.0);
    app.step();
    app.event_at(on_laptop, key_down(Key::Char('b')), Instant::now());
    app.step();
    let (external_field, external_value) = fields.borrow()[&external];
    assert!(
        laptop_field.is_focused(),
        "the cover built for the new monitor leaves the focus where the keyboard is"
    );
    assert_eq!(
        laptop_value.get_untracked(),
        "ab",
        "and the keys keep landing there"
    );

    let now = Instant::now();
    app.event_at(on_laptop, Event::FocusOut, now);
    app.event_at(on_external, Event::FocusIn, now);
    app.event_at(on_external, key_down(Key::Char('c')), now);
    app.step();
    assert!(
        external_field.is_focused(),
        "the keyboard on the new cover, its field has the focus"
    );
    assert_eq!(external_value.get_untracked(), "c");
}

/// A focused field on a bar, built by `input` around its signal, with `typed`
/// already in it — each key stepped on its own.
fn a_focused_field_holding(
    app: &mut Headless,
    typed: &str,
    input: impl Fn(RwSignal<String>) -> TextInput + 'static,
) -> (SurfaceId, RwSignal<String>) {
    let value = create_signal(String::new());
    let surface = app.surface(fixed_bar(), move || {
        container()
            .width(fill())
            .height(fill())
            .child(input(value).autofocus())
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();
    for c in typed.chars() {
        app.event_at(surface, key_down(Key::Char(c)), Instant::now());
        app.step();
    }
    (surface, value)
}

fn key_down(key: Key) -> Event {
    Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
        repeat: false,
    }
}

/// Every key in one batch, and one step: the loop hands a surface all of
/// them before it lays anything out.
fn queue_keys(app: &mut Headless, surface: SurfaceId, keys: impl IntoIterator<Item = Key>) {
    let now = Instant::now();
    for key in keys {
        app.event_at(surface, key_down(key), now);
    }
    app.step();
}

/// A key queued behind the Enter whose `on_submit` cleared the field edits the
/// cleared field, not the text that was submitted (#725).
#[test]
fn a_key_queued_behind_a_submit_that_clears_the_field_edits_the_cleared_field() {
    let Some(mut app) = headless() else { return };
    let (surface, value) = a_focused_field_holding(&mut app, "hi", |value| {
        text_input(value).on_submit(move |_| value.set(String::new()))
    });
    assert_eq!(value.get_untracked(), "hi");

    queue_keys(&mut app, surface, [Key::Enter, Key::Char('x')]);
    assert_eq!(value.get_untracked(), "x");
}

/// The same for `on_change`: a tag input that takes its word at a comma, given
/// the whole word and the next letter in one batch, keeps only the letter.
#[test]
fn a_key_queued_behind_an_on_change_that_clears_the_field_edits_the_cleared_field() {
    let Some(mut app) = headless() else { return };
    let (surface, value) = a_focused_field_holding(&mut app, "", |value| {
        text_input(value).on_change(move |text| {
            if text.ends_with(',') {
                value.set(String::new());
            }
        })
    });

    queue_keys(&mut app, surface, "foo,b".chars().map(Key::Char));
    assert_eq!(value.get_untracked(), "b");
}

/// Another application's copy is not read when it is offered, only when the
/// field pastes it: one read, and the text lands in the field that asked.
#[test]
fn a_clipboard_offer_is_read_only_when_the_field_pastes() {
    let Some(mut app) = headless() else { return };
    let field = create_widget_ref();
    let value = create_signal(String::new());
    let surface = app.surface(fixed_bar(), move || bar_with_a_field(field, value));
    app.configure(surface, 200, 50, 1.0);
    app.step();
    app.click(surface, 100.0, 8.0);
    app.step();

    assert!(!clipboard_has_content(), "nothing is offered yet");
    app.offer_selection(SelectionKind::Clipboard, "from a password manager");
    app.step();
    assert!(clipboard_has_content(), "the offer is known about");
    assert!(
        app.selection_reads().is_empty(),
        "an offer arriving is not a paste"
    );

    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    app.event_at(
        surface,
        Event::KeyDown {
            key: Key::Char('v'),
            modifiers: ctrl,
            repeat: false,
        },
        Instant::now(),
    );
    app.step();
    app.step();
    assert_eq!(app.selection_reads(), [SelectionKind::Clipboard]);
    assert_eq!(value.get(), "from a password manager");
}

/// What the application copies is offered back to it, as a compositor offers a
/// new selection to every client, so a paste after a copy brings the copy.
#[test]
fn a_copy_is_what_the_next_paste_brings() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), container);
    app.configure(surface, 200, 50, 1.0);
    app.step();

    clipboard_copy("copied here");
    primary_copy("selected here");
    app.step();

    let got = Rc::new(RefCell::new(Vec::new()));
    let sink = got.clone();
    clipboard_paste(move |text| sink.borrow_mut().push(text));
    let sink = got.clone();
    primary_paste(move |text| sink.borrow_mut().push(text));
    app.step();
    app.step();

    assert_eq!(*got.borrow(), ["copied here", "selected here"]);
}

/// A paste from application code is answered through its callback, and two
/// asked for in one iteration share one read.
#[test]
fn pastes_asked_together_share_one_read() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), container);
    app.configure(surface, 200, 50, 1.0);
    app.step();
    app.offer_selection(SelectionKind::Primary, "selected elsewhere");

    let got = Rc::new(RefCell::new(Vec::new()));
    for _ in 0..2 {
        let got = got.clone();
        primary_paste(move |text| got.borrow_mut().push(text));
    }
    app.step();
    app.step();

    assert_eq!(app.selection_reads(), [SelectionKind::Primary]);
    assert_eq!(*got.borrow(), ["selected elsewhere", "selected elsewhere"]);
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

/// A widget that writes down the count of each press it is handed.
struct PressCounts(Rc<RefCell<Vec<u32>>>);

impl Widget for PressCounts {
    fn layout(&mut self, _ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        Size::new(constraints.max_width, constraints.max_height)
    }

    fn paint(&self, _ctx: &mut PaintContext) {}

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, event: &Event) -> EventResponse {
        if let Event::MouseDown { count, .. } = event {
            self.0.borrow_mut().push(*count);
        }
        EventResponse::Handled
    }
}

/// The counts the presses among `events` reach a widget with, each event
/// queued the given number of milliseconds after the first.
fn press_counts(events: impl IntoIterator<Item = (u64, Event)>) -> Option<Vec<u32>> {
    let mut app = headless()?;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let spy = seen.clone();
    let surface = app.surface(fixed_bar(), move || PressCounts(spy));
    app.configure(surface, 200, 50, 1.0);
    app.step();

    queue(&mut app, surface, Instant::now(), events);
    app.step();
    let counts = seen.borrow().clone();
    Some(counts)
}

/// Queue `events`, each the given number of milliseconds after `start`.
fn queue(
    app: &mut Headless,
    surface: SurfaceId,
    start: Instant,
    events: impl IntoIterator<Item = (u64, Event)>,
) {
    for (ms, event) in events {
        app.event_at(surface, event, start + Duration::from_millis(ms));
    }
}

/// A press and its release 20 ms later, at `x`.
fn click_at(ms: u64, x: f32, button: MouseButton) -> [(u64, Event); 2] {
    [
        (ms, Event::mouse_down(x, 10.0, button)),
        (ms + 20, Event::mouse_up(x, 10.0, button)),
    ]
}

/// Two left clicks, the second `apart` milliseconds after the first and
/// `moved` to the right of it.
fn two_clicks(apart: u64, moved: f32) -> Option<Vec<u32>> {
    press_counts(
        click_at(0, 10.0, MouseButton::Left)
            .into_iter()
            .chain(click_at(apart, 10.0 + moved, MouseButton::Left)),
    )
}

/// The count is the platform's, so what the recorder is handed has none: a
/// press reaches the widget numbered by when and where it landed against the
/// press before it.
#[test]
fn two_quick_presses_in_one_place_count_one_and_two() {
    let Some(counts) = two_clicks(100, 0.0) else {
        return;
    };
    assert_eq!(counts, [1, 2]);
}

#[test]
fn two_presses_too_far_apart_in_time_count_one_and_one() {
    let Some(counts) = two_clicks(700, 0.0) else {
        return;
    };
    assert_eq!(counts, [1, 1]);
}

#[test]
fn two_presses_too_far_apart_in_space_count_one_and_one() {
    let Some(counts) = two_clicks(100, 20.0) else {
        return;
    };
    assert_eq!(counts, [1, 1]);
}

/// Pressed, dragged away and back, released where it began: the next press
/// lands in time and in place, and starts a run of its own.
#[test]
fn a_press_after_a_drag_counts_one() {
    let dragged = [
        (0, Event::mouse_down(10.0, 10.0, MouseButton::Left)),
        (20, Event::mouse_move(60.0, 10.0)),
        (40, Event::mouse_move(10.0, 10.0)),
        (60, Event::mouse_up(10.0, 10.0, MouseButton::Left)),
    ];
    let Some(counts) = press_counts(dragged.into_iter().chain(click_at(
        100,
        10.0,
        MouseButton::Left,
    ))) else {
        return;
    };
    assert_eq!(counts, [1, 1]);
}

#[test]
fn a_right_click_after_a_left_click_counts_one() {
    let Some(counts) = press_counts(
        click_at(0, 10.0, MouseButton::Left)
            .into_iter()
            .chain(click_at(100, 10.0, MouseButton::Right)),
    ) else {
        return;
    };
    assert_eq!(counts, [1, 1]);
}

/// `on_double_click` fires on the release of the second click, as the web's
/// `dblclick` does, and only on the second: a third click in the run is a
/// click and nothing more. `on_click` fires for every one of them.
#[test]
fn a_double_click_fires_on_the_second_release_and_every_click_still_clicks() {
    let Some(mut app) = headless() else { return };
    let clicks = create_signal(0u32);
    let doubles = create_signal(0u32);
    let surface = app.surface(fixed_bar(), move || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(20.0)
                .on_click(move || clicks.update(|c| *c += 1))
                .on_double_click(move || doubles.update(|c| *c += 1)),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let start = Instant::now();
    let [first_down, first_up] = click_at(0, 10.0, MouseButton::Left);
    let [second_down, second_up] = click_at(100, 10.0, MouseButton::Left);
    queue(
        &mut app,
        surface,
        start,
        [first_down, first_up, second_down],
    );
    app.step();
    assert_eq!(
        (clicks.get(), doubles.get()),
        (1, 0),
        "not before the release"
    );

    queue(&mut app, surface, start, [second_up]);
    app.step();
    assert_eq!((clicks.get(), doubles.get()), (2, 1));

    queue(
        &mut app,
        surface,
        start,
        click_at(200, 10.0, MouseButton::Left),
    );
    app.step();
    assert_eq!(
        (clicks.get(), doubles.get()),
        (3, 1),
        "a triple is not two doubles"
    );
}

/// A container with nothing but `on_double_click` claims the press, as one
/// with an `on_click` does, so the release that makes the double click
/// reaches it.
#[test]
fn a_double_click_alone_is_heard() {
    let Some(mut app) = headless() else { return };
    let doubles = create_signal(0u32);
    let surface = app.surface(fixed_bar(), move || {
        container().width(fill()).height(fill()).child(
            container()
                .width(80.0)
                .height(20.0)
                .on_double_click(move || doubles.update(|c| *c += 1)),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    let clicks = click_at(0, 10.0, MouseButton::Left)
        .into_iter()
        .chain(click_at(100, 10.0, MouseButton::Left));
    queue(&mut app, surface, Instant::now(), clicks);
    app.step();
    assert_eq!(doubles.get(), 1);
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

/// A handle grab stops the queued glide; later application frames used to move the content.
#[test]
fn a_scrollbar_grab_stops_the_glide_in_the_application() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || {
        scroll_over_an_edge().scroll(Scroll::vertical())
    });
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
    assert_eq!(scrolled(&app, surface), 60.0);
    app.event_at(
        surface,
        Event::mouse_down(95.0, 100.0, MouseButton::Left),
        at,
    );
    app.step_at(at);
    for _ in 0..4 {
        at += Duration::from_millis(16);
        app.step_at(at);
        assert_eq!(scrolled(&app, surface), 60.0, "held handle drifted");
    }
    app.event_at(surface, Event::mouse_up(95.0, 100.0, MouseButton::Left), at);
    for _ in 0..4 {
        at += Duration::from_millis(16);
        app.step_at(at);
        assert_eq!(scrolled(&app, surface), 60.0, "release revived the glide");
    }
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
        .child(pressable(container().width(fill()).height(height), clicks))
}

/// `shape`, blue at rest and green while it is pressed, counting what it
/// activates — the colours [`pressed`] tells apart.
fn pressable(shape: Container, clicks: RwSignal<u32>) -> Container {
    shape
        .background(Color::rgb(0.0, 0.0, 1.0))
        .when_pressed(|s: StateStyle| s.background(Color::rgb(0.0, 1.0, 0.0)))
        .on_click(move || clicks.update(|c| *c += 1))
}

/// Whether the [`pressable`] under (50, 10) is drawing its pressed state this
/// frame — green rather than blue. Both scenes that read it keep one there,
/// whatever the scroll offset.
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
/// the list is already holding — and the list goes on following it.
///
/// The page stopped watching the press the moment the list took it, which is
/// the half of that ruling nothing else would notice: the page clips, and a
/// positioned event outside it is not offered to what it holds by position, so
/// a page that kept its watch would find the list out of the way and claim a
/// gesture that has been somebody else's for a dozen frames.
///
/// The list is not out of the way, though: the press landed on it, and a press
/// is owed every move until its release, wherever the finger goes (#587). It
/// used to stop at the page's edge.
#[test]
fn a_drag_that_leaves_the_page_does_not_become_the_pages() {
    let Some(mut app) = headless() else { return };
    let (surface, mut at) = scroller(&mut app, || a_list_inside_a_page(800.0));

    drag_up(&mut app, surface, &mut at, 480.0);

    // Out of the surface sideways, which the compositor goes on reporting
    // while the finger is down, and 20px further up than the drag had got.
    at += Duration::from_millis(8);
    app.event_at(surface, Event::finger_move(150.0, 360.0), at);
    app.step_at(at);

    assert_eq!(
        page_and_list(&app, surface),
        (0.0, 102.0),
        "the list kept the gesture it won and followed the finger out of the \
         page, and the page claimed nothing"
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

/// Two merged pills that ask the compositor to blur behind them publish
/// their shapes and a box over the neck that joins them (#847).
///
/// The region used to be each pill's shape alone, so the desktop behind the
/// neck stayed sharp under the merged fill. A region cannot follow the neck's
/// curve; the box spans the gap, as tall as the two pills side by side, and
/// nothing past them.
#[test]
fn merged_pills_publish_a_box_over_their_neck() {
    let Some(mut app) = headless() else { return };
    let pill = Color::rgba(0.1, 0.1, 0.12, 0.7);
    let surface = app.surface(fixed_bar(), move || {
        container().width(fill()).height(fill()).padding(8.0).child(
            container()
                .layout(Flex::row())
                .merge_children(18.0)
                .child(
                    container()
                        .width(120.0)
                        .height(34.0)
                        .corners(17.0)
                        .background(pill)
                        .backdrop_blur(24.0),
                )
                .child(
                    container()
                        .width(34.0)
                        .height(34.0)
                        .corners(17.0)
                        .background(pill)
                        .backdrop_blur(24.0)
                        .translate((8.0, 0.0)),
                ),
        )
    });
    app.configure(surface, 220, 50, 1.0);
    app.step();

    let published = app
        .blur_regions_asked(surface)
        .last()
        .expect("the frame that blurs publishes a region")
        .clone();
    let covers = |x: f32, y: f32| {
        published
            .iter()
            .any(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)
    };
    // The long pill runs from 8 to 128, the round one from 136 to 170.
    assert!(covers(60.0, 25.0), "the long pill is blurred");
    assert!(covers(150.0, 25.0), "and the round one");
    assert!(
        covers(132.0, 25.0),
        "the neck between them is blurred too: {published:?}"
    );
    assert!(
        !covers(132.0, 4.0) && !covers(175.0, 25.0),
        "nothing past the pills and their neck is: {published:?}"
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

/// A box 80 tall while `tall` holds and 24 when it does not, easing between
/// the two over 100 ms.
fn eases_80_to_24(tall: RwSignal<bool>) -> impl Fn() -> Container + 'static {
    move || {
        container().child(
            container().height(
                (move || if tall.get() { 80.0 } else { 24.0 })
                    .transition(Transition::new(100.0, TimingFunction::Linear)),
            ),
        )
    }
}

/// A content-sized surface eased from 80 to 24 over 100 ms, configured at 80.
/// Returns the app, the surface, the signal that opens it, and the instant the
/// close starts at: what the zones asked after that instant are compared from.
fn closing_from_80() -> Option<(Headless, SurfaceId, RwSignal<bool>, Instant)> {
    let mut app = headless()?;
    let tall = create_signal(true);
    let bar = app.surface(content_bar(), eases_80_to_24(tall));
    app.configure(bar, 200, 80, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);
    tall.set(false);
    Some((app, bar, tall, t0))
}

/// A surface whose content animates smaller keeps its size while the content
/// is on its way down, and asks once for where it went when it gets there.
///
/// The measure reads where the animation is going, which is what lets a growth
/// resize once, up front. The same read on a close shrinks the surface in the
/// close's first frame and cuts off everything still on its way (#893).
#[test]
fn a_shrinking_child_keeps_the_surface_until_it_has_settled() {
    let Some((mut app, bar, _, t0)) = closing_from_80() else {
        return;
    };
    holds_then_shrinks_to_24(&mut app, bar, t0);
}

/// Steps a surface through a 100 ms shrink that started at `t0`: nothing new
/// is reserved while it plays, and 24 once, when it is over.
fn holds_then_shrinks_to_24(app: &mut Headless, bar: SurfaceId, t0: Instant) {
    holds_then_asks_once_for_24(app, bar, t0, Headless::exclusive_zones_asked);
}

/// The same, for whatever `asked` reads off the surface: nothing new while
/// the shrink plays, and 24 once, when it is over.
fn holds_then_asks_once_for_24<T: PartialEq + std::fmt::Debug + From<u8>>(
    app: &mut Headless,
    id: SurfaceId,
    t0: Instant,
    asked: fn(&Headless, SurfaceId) -> &[T],
) {
    let before = asked(app, id).len();
    for ms in [0, 20, 40, 60, 80] {
        app.step_at(t0 + Duration::from_millis(ms));
        assert_eq!(
            asked(app, id)[before..],
            [],
            "{ms} ms into a 100 ms shrink, the surface already shrank"
        );
    }
    for ms in [100, 120, 140] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    assert_eq!(
        asked(app, id)[before..],
        [T::from(24)],
        "settled, it asks once for where the content went"
    );
}

/// An axis pinned with `set_size` and given back to `content()` while the
/// content is closing holds the shrink behind the size the pin gave it, not
/// behind what the content measured before the pin.
///
/// A bar that pins itself to the output's height while a panel is open, and
/// hands the height back as the panel closes, is the shape: the content
/// measure does not run while the axis is pinned, so what it last asked for
/// is the strip from before the panel opened (#896).
#[test]
fn a_content_axis_given_back_mid_close_holds_the_pinned_size() {
    let Some(mut app) = headless() else { return };
    let tall = create_signal(false);
    let bar = app.surface(content_bar(), eases_80_to_24(tall));
    app.configure(bar, 200, 24, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);

    surface_handle(bar).set_size(400, 200);
    app.step_at(t0);
    app.configure(bar, 200, 200, 1.0);
    tall.set(true);
    let t1 = t0 + Duration::from_millis(1000);
    for ms in [0, 200] {
        app.step_at(t1 + Duration::from_millis(ms));
    }

    let t2 = t1 + Duration::from_millis(1000);
    let sizes = app.sizes_asked(bar).len();
    tall.set(false);
    surface_handle(bar).set_size(400, content());
    holds_then_shrinks_to_24(&mut app, bar, t2);
    let asked = &app.sizes_asked(bar)[sizes..];
    let (_, held) = asked.split_last().expect("the shrink was asked for");
    assert!(
        held.iter().all(|&(_, height)| height >= 200),
        "only the last size asked is smaller than the pin: {asked:?}"
    );
}

/// A bar's root: a stack that fills the surface, holding a backdrop that fills
/// it too and a strip whose height eases to `strip` over 100 ms, at the bottom.
fn filling_root(strip: RwSignal<f32>, placed: WidgetRef) -> Container {
    container()
        .height(fill())
        .layout(ZStack::new().align(Alignment::BOTTOM_START))
        .child(container().width(fill()).height(fill()))
        .child(
            container()
                .width(fill())
                .height(
                    (move || strip.get())
                        .transition(Transition::new(100.0, TimingFunction::Linear)),
                )
                .widget_ref(placed),
        )
}

/// A root that fills a content axis is measured at its content, not at the
/// screen: with no surface size yet there is no space to fill, so `fill()`
/// sizes to what it holds, as it does on a scroller's axis (#898).
#[test]
fn a_filling_root_measures_to_its_content() {
    let Some(mut app) = headless() else { return };
    let strip = create_signal(24.0f32);
    let bar = app.surface(content_bar(), move || {
        filling_root(strip, create_widget_ref())
    });
    app.configure(bar, 200, 24, 1.0);
    app.step();
    app.step();

    assert_eq!(
        app.exclusive_zones_asked(bar).last(),
        Some(&24),
        "the surface follows the strip, not the screen: {:?}",
        app.exclusive_zones_asked(bar)
    );
}

/// While a shrink is held the surface is taller than its content, and a
/// filling root keeps what it aligns to the bottom at the bottom: the bar
/// stays on its edge until the surface has shrunk.
#[test]
fn a_filling_root_keeps_a_bottom_strip_at_the_bottom_while_a_shrink_is_held() {
    let Some(mut app) = headless() else { return };
    let strip = create_signal(80.0f32);
    let placed = create_widget_ref();
    let bar = app.surface(content_bar(), move || filling_root(strip, placed));
    app.configure(bar, 200, 80, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);

    let before = app.exclusive_zones_asked(bar).len();
    strip.set(24.0);
    for ms in [0, 20, 40] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    let rect = placed.rect().get_untracked();
    assert!(
        rect.height < 80.0 && rect.height > 24.0,
        "the strip is on its way down: {rect:?}"
    );
    assert_eq!(
        rect.y + rect.height,
        80.0,
        "and its bottom is the surface's: {rect:?}"
    );
    assert_eq!(
        app.exclusive_zones_asked(bar)[before..],
        [],
        "the shrink is held"
    );

    // The same instants again, which moves nothing, and on to the end.
    holds_then_shrinks_to_24(&mut app, bar, t0);
}

/// A text too long for the screen still wraps when its surface is measured:
/// with no maximum it answers with one line, so past the cap its width is
/// measured again at the cap and the height counts the lines it wraps to.
#[test]
fn a_text_wider_than_the_screen_measures_the_height_it_wraps_to() {
    let Some(mut app) = headless() else { return };
    let toast = || {
        SurfaceConfig::new()
            .width(content())
            .height(content())
            .anchor(Anchor::TOP)
    };
    let short = app.surface(toast(), || container().child(text("one line")));
    let long = app.surface(toast(), || {
        container().child(text("a line far too long to fit on any screen ".repeat(40)))
    });
    app.configure(short, 1, 1, 1.0);
    app.configure(long, 1, 1, 1.0);
    app.step();
    app.step();

    let (_, one_line) = *app.sizes_asked(short).last().expect("measured");
    let (width, height) = *app.sizes_asked(long).last().expect("measured");
    assert!(
        (700..=800).contains(&width),
        "wrapped at the cap, 800 with no output, so as wide as its longest line \
         there: {width}"
    );
    assert!(
        height >= 3 * one_line,
        "and as tall as the lines it wraps to there, not one: {height} against \
         {one_line} for one line"
    );
}

/// A height that follows a text with a transition heads for the height the
/// text wraps to, when the text grows past the screen.
///
/// The measure past the cap is the second of the frame, and a size that
/// follows its content keeps the target the frame's first measure gave it.
/// That one is the measure with no maximum, where the text is one line — so
/// unless the second measure starts the frame's targets over, the box eases to
/// one line while the text wraps to several.
#[test]
fn a_transitioned_height_follows_a_text_that_grows_past_the_screen() {
    let Some(mut app) = headless() else { return };
    let words = create_signal(String::from("one line"));
    let toast = app.surface(
        SurfaceConfig::new()
            .width(content())
            .height(content())
            .anchor(Anchor::TOP),
        move || {
            container()
                .height(
                    Length::default().transition(Transition::new(100.0, TimingFunction::Linear)),
                )
                .child(text(move || words.get()))
        },
    );
    app.configure(toast, 1, 1, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);
    app.step_at(t0);
    let (_, one_line) = *app.sizes_asked(toast).last().expect("measured");

    words.set("a line far too long to fit on any screen ".repeat(40));
    for ms in [0, 50, 100, 200] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    let (_, height) = *app.sizes_asked(toast).last().expect("measured");
    assert!(
        height >= 3 * one_line,
        "the box heads for the lines the text wraps to: {height} against \
         {one_line} for one line"
    );
}

/// Every size `bar` has committed since commit `from` came with a buffer of
/// that size, on the axes it asked for: none moved the surface's box around
/// the buffer it already had.
#[track_caller]
fn every_size_rode_a_buffer_of_it(app: &Headless, bar: SurfaceId, from: usize) {
    let commits = &app.size_commits(bar)[from..];
    assert!(!commits.is_empty(), "nothing asked for a size");
    for &((width, height), buffer) in commits {
        let Some((drawn_w, drawn_h)) = buffer else {
            panic!("{width}x{height} was committed over the buffer it had: {commits:?}");
        };
        assert!(
            (width == 0 || width == drawn_w) && (height == 0 || height == drawn_h),
            "{width}x{height} was committed with a {drawn_w}x{drawn_h} buffer: {commits:?}"
        );
    }
}

/// A content-sized surface resized from `from` to `to` commits the new size
/// with a buffer drawn at it, in the frame that asked.
fn resizes_with_a_buffer_of_its_size(from: f32, to: f32) {
    let Some(mut app) = headless() else { return };
    let height = create_signal(from);
    let bar = app.surface(content_bar(), move || {
        container().height(move || height.get())
    });
    app.configure(bar, 200, from as u32, 1.0);
    app.step();
    let commits = app.size_commits(bar).len();

    height.set(to);
    app.step();

    every_size_rode_a_buffer_of_it(&app, bar, commits);
    assert_eq!(
        app.physical_size(bar),
        (200, to as u32),
        "and that frame is drawn at it, before any configure"
    );
}

/// Committed on its own, a size reaches the compositor a round trip before a
/// buffer of it does, and the compositor places the box for the new size
/// around the old buffer: a bar on the bottom edge drawn for a frame as far up
/// as it grew (#900).
#[test]
fn a_growing_surface_commits_its_size_with_a_buffer_of_it() {
    resizes_with_a_buffer_of_its_size(24.0, 80.0);
}

/// The same for a shrink, once nothing holds it.
#[test]
fn a_shrinking_surface_commits_its_size_with_a_buffer_of_it() {
    resizes_with_a_buffer_of_its_size(80.0, 24.0);
}

/// A width on a surface anchored to the right edge, where the box grows to the
/// left and the old buffer would be drawn from its new left edge.
#[test]
fn a_right_anchored_surface_commits_its_width_with_a_buffer_of_it() {
    let Some(mut app) = headless() else { return };
    let width = create_signal(100.0f32);
    let dock = app.surface(
        SurfaceConfig::new()
            .width(content())
            .height(30)
            .anchor(Anchor::TOP | Anchor::RIGHT),
        move || container().width(move || width.get()).height(30.0),
    );
    app.configure(dock, 100, 30, 1.0);
    app.step();
    let commits = app.size_commits(dock).len();

    width.set(300.0);
    app.step();

    every_size_rode_a_buffer_of_it(&app, dock, commits);
    assert_eq!(app.physical_size(dock), (300, 30));
}

/// The compositor has the last word: a configure that does not give the size
/// asked for — niri clamping a surface to the area it may take — is the size
/// the next frame is drawn at.
#[test]
fn a_configure_that_clamps_the_size_asked_for_is_drawn_at() {
    let Some(mut app) = headless() else { return };
    let height = create_signal(24.0f32);
    let bar = app.surface(content_bar(), move || {
        container().height(move || height.get())
    });
    app.configure(bar, 200, 24, 1.0);
    app.step();

    height.set(80.0);
    app.step();
    assert_eq!(app.physical_size(bar), (200, 80), "drawn at what it asked");
    app.configure(bar, 200, 70, 1.0);
    app.step();
    assert_eq!(
        app.physical_size(bar),
        (200, 70),
        "and then at what it was given"
    );
}

/// And for a size an application sets, pinning a content axis and handing it
/// back: the bar that covers the output while a panel is open.
#[test]
fn a_pinned_size_and_its_release_commit_with_a_buffer_of_them() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(content_bar(), || container().height(24.0));
    app.configure(bar, 200, 24, 1.0);
    app.step();
    let from = app.size_commits(bar).len();

    surface_handle(bar).set_size(400, 300);
    app.step();
    assert_eq!(app.physical_size(bar), (200, 300), "drawn at the pin");
    surface_handle(bar).set_size(400, content());
    app.step();
    app.step();

    every_size_rode_a_buffer_of_it(&app, bar, from);
    assert_eq!(app.physical_size(bar), (200, 24), "and back at its content");
}

/// An auto-height popup whose content eases from 80 to 24 over 100 ms, opened
/// on a bar. Returns the app, the popup, the signal that opens it, and the
/// instant the close starts at.
fn popup_closing_from_80() -> Option<(Headless, SurfaceId, RwSignal<bool>, Instant)> {
    let mut app = headless()?;
    let bar = app.surface(content_bar(), measuring_24);
    app.configure(bar, 200, 24, 1.0);
    app.step();
    let tall = create_signal(true);
    let popup = spawn_popup(bar, PopupConfig::new(80), eases_80_to_24(tall));
    let t0 = Instant::now();
    app.step_at(t0);
    app.step_at(t0);
    tall.set(false);
    Some((app, popup.id(), tall, t0))
}

/// An auto-height popup whose content animates smaller keeps its height while
/// the content is on its way down, and asks once for where it went: the hold
/// a content-sized layer surface has (#893), on the popup's own measure (#894).
#[test]
fn a_shrinking_popup_keeps_its_height_until_it_has_settled() {
    let Some((mut app, popup, _, t0)) = popup_closing_from_80() else {
        return;
    };
    holds_then_asks_once_for_24(&mut app, popup, t0, Headless::popup_heights_asked);
}

/// A popup's close reopened half way asks for nothing.
#[test]
fn a_popup_close_reopened_half_way_never_repositions_it() {
    let Some((mut app, popup, tall, t0)) = popup_closing_from_80() else {
        return;
    };
    let before = app.popup_heights_asked(popup).len();

    for ms in [0, 20, 40] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    tall.set(true);
    for ms in [40, 60, 80, 100, 120, 160, 200] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    assert_eq!(app.popup_heights_asked(popup)[before..], []);
}

/// A close interrupted by a reopen asks for nothing: the surface never shrank,
/// so there is nothing to grow back to.
#[test]
fn a_close_reopened_half_way_never_resizes_the_surface() {
    let Some((mut app, bar, tall, t0)) = closing_from_80() else {
        return;
    };
    let zones = app.exclusive_zones_asked(bar).len();
    let sizes = app.sizes_asked(bar).len();

    for ms in [0, 20, 40] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    tall.set(true);
    for ms in [40, 60, 80, 100, 120, 140, 160, 200] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    assert_eq!(
        app.exclusive_zones_asked(bar)[zones..],
        [],
        "no reservation"
    );
    assert_eq!(app.sizes_asked(bar)[sizes..], [], "and no size");
}

/// A keyed row that leaves by collapsing holds the surface open for its exit,
/// and the surface shrinks once the row is gone.
#[test]
fn a_collapsing_row_keeps_the_surface_until_it_is_gone() {
    let Some(mut app) = headless() else { return };
    let rows = create_signal(vec![0u32, 1]);
    let bar = app.surface(content_bar(), move || {
        container().layout(Flex::column()).children(keyed(
            move || rows.get(),
            |key| *key,
            |_| {
                container().height(
                    Length::exact(24.0)
                        .transition(Transition::new(100.0, TimingFunction::Linear))
                        .exiting_to(Length::exact(0.0)),
                )
            },
        ))
    });
    app.configure(bar, 200, 48, 1.0);
    let t0 = Instant::now();
    app.step_at(t0);
    rows.set(vec![0]);
    holds_then_shrinks_to_24(&mut app, bar, t0);
}

/// The sizes a surface asked for, and the zones it reserved.
type Asked = (Vec<(u32, u32)>, Vec<i32>);

/// A 300x32 surface on the top edge, reserving what it is, holding a 240x32
/// container — then turned content-wide and given `height` in one call.
/// Returns the sizes it asked for and the zones it reserved, after one step.
fn resize_to_content_wide(height: u32) -> Option<Asked> {
    let mut app = headless()?;
    let bar = app.surface(
        SurfaceConfig::new()
            .width(300)
            .height(32)
            .anchor(Anchor::TOP)
            .exclusive_zone(ExclusiveZone::Auto),
        || container().width(240.0).height(32.0),
    );
    app.configure(bar, 300, 32, 1.0);
    app.step();

    surface_handle(bar).set_size(content(), height);
    app.step();
    Some((
        app.sizes_asked(bar).to_vec(),
        app.exclusive_zones_asked(bar).to_vec(),
    ))
}

/// A fixed axis given a new value in the call that turns the other one to
/// content keeps that value: the measure that follows pins it to what was just
/// asked for, not to what the compositor last confirmed — which would ask for
/// the old height last while reserving the new one.
#[test]
fn a_surface_turned_content_wide_keeps_the_height_it_was_just_given() {
    let Some((sizes, zones)) = resize_to_content_wide(48) else {
        return;
    };
    assert_eq!(
        sizes,
        [(300, 32), (300, 48), (240, 48)],
        "the last size asked carries the new height"
    );
    assert_eq!(zones, [32, 48], "and the reservation follows it");
}

/// The control: a fixed axis that did not change asks what it asked before.
#[test]
fn a_surface_turned_content_wide_at_its_own_height_asks_as_before() {
    let Some((sizes, zones)) = resize_to_content_wide(32) else {
        return;
    };
    assert_eq!(sizes, [(300, 32), (300, 32), (240, 32)]);
    assert_eq!(zones, [32]);
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
    let bar = app.surface(content_bar(), eases_80_to_24(tall));
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

/// `rect()` is the box a widget is drawn in, and not what it paints outside
/// that box: how far a shadow reaches is the damage tracker's concern. It used
/// to grow `rect()` by its shadow's reach, moving the corner and the size, so a
/// popup anchored to the widget opened beside it.
#[test]
fn a_widget_ref_rect_leaves_out_what_its_widget_paints_outside_it() {
    let Some(mut app) = headless() else { return };
    let card = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container().padding(10.0).child(
            container()
                .width(60.0)
                .height(20.0)
                .background(Color::WHITE)
                .shadow(Shadow::new(
                    (0.0, 4.0),
                    6.0,
                    2.0,
                    Color::rgba(0.0, 0.0, 0.0, 0.4),
                ))
                .widget_ref(card),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert_eq!(
        card.rect().get_untracked(),
        Rect::new(10.0, 10.0, 60.0, 20.0),
        "the card laid out 60 by 20 at the padding's corner"
    );
}

/// A widget ref's rect is where the row is drawn in a scrolled list, frame by
/// frame as the content coasts.
///
/// The row is laid out 500 down. A flick sets the content coasting, which
/// moves it on every frame, and the rect has to follow: on each frame it is
/// 500 less what the frame shows has scrolled, to the pixel the frame can say
/// it to.
#[test]
fn a_widget_ref_rect_follows_the_scroll_its_widget_is_drawn_under() {
    let Some(mut app) = headless() else { return };
    let row = create_widget_ref();
    let (surface, mut at) = scroller(&mut app, move || {
        container()
            .width(fill())
            .height(fill())
            .scroll(hidden_scroll())
            .child(
                container()
                    .layout(Flex::column())
                    .width(fill())
                    .child(block(500.0, Color::rgb(1.0, 0.0, 0.0)))
                    .child(block(1500.0, Color::rgb(0.0, 0.0, 1.0)).widget_ref(row)),
            )
    });
    assert_eq!(row.rect().get_untracked().y, 500.0, "laid out 500 down");

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

    let mut seen = Vec::new();
    for frame in 0..30 {
        let y = row.rect().get_untracked().y;
        assert_eq!(
            row.layout_rect().get_untracked().y,
            y,
            "frame {frame}: the layout box moves with the scroll too"
        );
        let drawn = 500.0 - scrolled(&app, surface);
        assert!(
            (y - drawn).abs() <= 1.0,
            "frame {frame}: the rect says {y}, the row is drawn at {drawn}"
        );
        seen.push(y);
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    seen.dedup();
    assert!(
        seen.len() > 5 && seen[0] - seen[seen.len() - 1] > 60.0,
        "the content coasted and the rect followed it: {seen:?}"
    );
}

/// A widget ref's rect follows an ancestor's translate as it animates, frame
/// by frame, and rests where the translate does.
#[test]
fn a_widget_ref_rect_follows_an_ancestor_translate_as_it_animates() {
    let Some(mut app) = headless() else { return };
    let moved = create_signal(false);
    let card = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container()
            .translate(
                (move || {
                    if moved.get() {
                        Translate::new(20.0, 0.0)
                    } else {
                        Translate::NONE
                    }
                })
                .transition(160.0),
            )
            .child(container().width(60.0).height(20.0).widget_ref(card))
    });
    app.configure(surface, 200, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    assert_eq!(card.rect().get_untracked(), Rect::new(0.0, 0.0, 60.0, 20.0));

    moved.set(true);
    let mut xs = Vec::new();
    for _ in 0..20 {
        at += Duration::from_millis(16);
        app.step_at(at);
        xs.push(card.rect().get_untracked().x);
    }
    assert!(
        xs.iter().any(|&x| x > 0.5 && x < 19.5),
        "the rect passed through the translate's frames, not only its ends: {xs:?}"
    );
    assert!(
        xs.windows(2).all(|pair| pair[1] >= pair[0]),
        "and moved one way: {xs:?}"
    );
    assert_eq!(
        card.rect().get_untracked(),
        Rect::new(20.0, 0.0, 60.0, 20.0),
        "and rests where the translate does"
    );
}

/// A widget ref's rect is the box its own transform draws: scaled twice about
/// its centre, a 60 by 20 card laid out at (10, 10) is drawn 120 by 40 around
/// the same centre.
#[test]
fn a_widget_ref_rect_is_the_box_its_own_scale_draws() {
    let Some(mut app) = headless() else { return };
    let card = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container().padding(10.0).child(
            container()
                .width(60.0)
                .height(20.0)
                .scale(2.0)
                .widget_ref(card),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    assert_eq!(
        card.rect().get_untracked(),
        Rect::new(-20.0, 0.0, 120.0, 40.0)
    );
}

/// A 60 by 20 card, translated 30 across inside a parent scaled twice, at
/// the padding's corner — or the same card with neither transform.
fn a_translated_card_in_a_scaled_parent(card: WidgetRef, transformed: bool) -> Container {
    let (scale, across) = if transformed { (2.0, 30.0) } else { (1.0, 0.0) };
    container().padding(10.0).child(
        container().scale(scale).child(
            container()
                .width(60.0)
                .height(20.0)
                .translate(Translate::new(across, 0.0))
                .widget_ref(card),
        ),
    )
}

/// `layout_rect()` is `rect()` with every transform taken out: the card's
/// own translate and its parent's scale both move `rect()`, and neither moves
/// `layout_rect()`, which is where the same card is drawn with no transform
/// at all.
#[test]
fn a_widget_ref_layout_rect_is_its_rect_with_the_transforms_taken_out() {
    let Some(mut app) = headless() else { return };
    let card = create_widget_ref();
    let plain = create_widget_ref();
    let one = app.surface(fixed_bar(), move || {
        a_translated_card_in_a_scaled_parent(card, true)
    });
    let two = app.surface(fixed_bar(), move || {
        a_translated_card_in_a_scaled_parent(plain, false)
    });
    app.configure(one, 200, 50, 1.0);
    app.configure(two, 200, 50, 1.0);
    app.step();

    let untransformed = plain.rect().get_untracked();
    assert_eq!(untransformed, Rect::new(10.0, 10.0, 60.0, 20.0));
    assert_eq!(
        card.rect().get_untracked(),
        Rect::new(40.0, 0.0, 120.0, 40.0),
        "drawn translated, then doubled about the parent's centre"
    );
    assert_eq!(card.layout_rect().get_untracked(), untransformed);
}

/// How many times `signal` is written, counted by an effect that reads it —
/// its first run included.
fn writes_of(signal: Signal<Rect>) -> Rc<Cell<usize>> {
    let writes = Rc::new(Cell::new(0));
    let counted = writes.clone();
    create_effect(move || {
        signal.get();
        counted.set(counted.get() + 1);
    });
    writes
}

/// A translate animating across frames moves `rect()` on each of them and
/// never writes `layout_rect()`: nothing about the layout moved.
#[test]
fn an_animating_translate_never_writes_a_widget_ref_layout_rect() {
    let Some(mut app) = headless() else { return };
    let moved = create_signal(false);
    let card = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container().padding(10.0).child(
            container()
                .width(60.0)
                .height(20.0)
                .translate(
                    (move || {
                        if moved.get() {
                            Translate::new(40.0, 0.0)
                        } else {
                            Translate::NONE
                        }
                    })
                    .transition(160.0),
                )
                .widget_ref(card),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    let writes = writes_of(card.layout_rect());
    let mut at = Instant::now();
    app.step_at(at);
    let laid_out = card.layout_rect().get_untracked();
    assert_eq!(laid_out, Rect::new(10.0, 10.0, 60.0, 20.0));
    let before = writes.get();

    moved.set(true);
    // The frame that starts the transition draws it at its start.
    at += Duration::from_millis(16);
    app.step_at(at);
    let mut xs = vec![card.rect().get_untracked().x];
    for _ in 0..8 {
        at += Duration::from_millis(16);
        app.step_at(at);
        xs.push(card.rect().get_untracked().x);
    }
    assert!(
        xs.windows(2).all(|pair| pair[1] > pair[0]),
        "rect() moved on every frame of the translate: {xs:?}"
    );
    assert_eq!(writes.get(), before, "and layout_rect() was never written");
    assert_eq!(card.layout_rect().get_untracked(), laid_out);
}

/// A layout change that moves the widget writes `layout_rect()` once, on the
/// frame that laid it out, and the frames after it write nothing.
#[test]
fn a_layout_move_writes_a_widget_ref_layout_rect_once() {
    let Some(mut app) = headless() else { return };
    let wide = create_signal(false);
    let card = create_widget_ref();
    let surface = app.surface(fixed_bar(), move || {
        container()
            .padding(move || if wide.get() { 10.0 } else { 5.0 })
            .child(container().width(60.0).height(20.0).widget_ref(card))
    });
    app.configure(surface, 200, 50, 1.0);
    let writes = writes_of(card.layout_rect());
    app.step();
    let before = writes.get();

    wide.set(true);
    for _ in 0..3 {
        app.step();
    }
    assert_eq!(
        card.layout_rect().get_untracked(),
        Rect::new(10.0, 10.0, 60.0, 20.0)
    );
    assert_eq!(writes.get(), before + 1, "written once, for the one move");
}

/// A layout root is restarted under its own constraints, not the surface's.
///
/// Partial layout re-enters the tree at a relayout boundary, which has no
/// parent above it to say what room it has. `Tree::placed_under` is that
/// memory: the constraints the boundary was last placed under, by whichever
/// pass placed it. Handed the whole surface instead, one inside a padded
/// parent grows to a width its parent never offered.
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

/// The lock surface covering `output`.
fn cover_of(app: &Headless, output: OutputId) -> SurfaceId {
    app.lock_surfaces_created()
        .into_iter()
        .find(|(_, on)| *on == output)
        .map(|(surface, _)| surface)
        .expect("the output is covered")
}

/// What a lock screen is built from: one per monitor, saying which.
fn lock_screen(output: OutputInfo) -> Text {
    text(format!("locked: {}", output.id.raw()))
}

/// A lock covers every connected monitor exactly once, as soon as it is asked
/// for, and unlocking takes every cover away.
///
/// The whole of `src/session_lock.rs` had nothing watching it: `Recorder` took
/// the trait's defaults for all four lock methods, so a test could call
/// `lock_session` and the loop would find a compositor that refuses.
///
/// The covers come before the grant: ext-session-lock-v1 wants them first,
/// niri holds `locked` until they have drawn and gives up after a second, and
/// Hyprland waits for them and sends `finished` after five. Asking only once
/// `Locked` arrived was a second of the compositor's no-frame colour on niri,
/// and a lock refused outright on Hyprland (#514).
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
    let (covers, screens): (Vec<SurfaceId>, Vec<OutputId>) =
        app.lock_surfaces_created().into_iter().unzip();
    assert_eq!(
        screens,
        [laptop, external],
        "every monitor is covered already, once"
    );

    app.grant_lock();
    app.step();

    assert!(app.is_locked(), "the compositor is holding a grant");
    assert_eq!(
        lock_state().get_untracked(),
        LockState::Locked,
        "and the application knows it"
    );
    assert_eq!(
        app.lock_surface_requests().len(),
        2,
        "the grant asks for no second set of covers"
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

/// A lock the compositor refuses takes down the covers made while it was
/// being asked for.
///
/// With the covers made during `Locking`, `finished` in place of `locked` is
/// the path that has them to tear down: left behind, they would be surfaces
/// the application goes on laying out and drawing with no lock under them.
#[test]
fn a_refused_lock_takes_down_the_covers_made_while_locking() {
    let Some(mut app) = headless() else { return };
    app.connect_output("eDP-1");
    app.connect_output("DP-2");

    lock_session(lock_screen);
    app.step();
    let (mut covers, _): (Vec<SurfaceId>, Vec<OutputId>) =
        app.lock_surfaces_created().into_iter().unzip();
    assert_eq!(covers.len(), 2, "both monitors covered while locking");

    app.finish_lock();
    app.step();

    assert_eq!(lock_state().get_untracked(), LockState::Unlocked);
    let mut taken = app.surfaces_destroyed().to_vec();
    taken.sort_by_key(|id| id.raw());
    covers.sort_by_key(|id| id.raw());
    assert_eq!(taken, covers, "every cover went with the refusal");
    assert!(
        app.surfaces_live().is_empty(),
        "and the application holds none of them"
    );
}

/// Unlocking before the compositor has answered takes down the covers made
/// while locking, as a refusal does.
#[test]
fn an_unlock_before_the_grant_takes_down_the_covers_made_while_locking() {
    let Some(mut app) = headless() else { return };
    app.connect_output("eDP-1");
    app.connect_output("DP-2");

    lock_session(lock_screen);
    app.step();
    let (mut covers, _): (Vec<SurfaceId>, Vec<OutputId>) =
        app.lock_surfaces_created().into_iter().unzip();
    assert_eq!(covers.len(), 2, "both monitors covered while locking");

    unlock_session();
    app.step();

    assert_eq!(lock_state().get_untracked(), LockState::Unlocked);
    let mut taken = app.surfaces_destroyed().to_vec();
    taken.sort_by_key(|id| id.raw());
    covers.sort_by_key(|id| id.raw());
    assert_eq!(taken, covers, "every cover went with the unlock");
    assert!(app.surfaces_live().is_empty());
}

/// A monitor plugged in while the lock is being asked for is covered too, and
/// so is one plugged in after it is granted.
#[test]
fn an_output_connected_while_locking_or_locked_is_covered() {
    let Some(mut app) = headless() else { return };
    let laptop = app.connect_output("eDP-1");

    lock_session(lock_screen);
    app.step();
    let external = app.connect_output("DP-2");
    app.step();

    assert_eq!(lock_state().get_untracked(), LockState::Locking);
    let screens = |app: &Headless| -> Vec<OutputId> {
        let (_, screens): (Vec<SurfaceId>, _) = app.lock_surfaces_created().into_iter().unzip();
        screens
    };
    assert_eq!(
        screens(&app),
        [laptop, external],
        "the monitor that arrived mid-request is covered before the grant"
    );

    app.grant_lock();
    app.step();
    let late = app.connect_output("HDMI-A-1");
    app.step();

    assert_eq!(
        screens(&app),
        [laptop, external, late],
        "and one that arrives after it is covered as well"
    );
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
    let going = cover_of(&app, external);

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
    let Some((mut app, _, shown, loading)) = a_cursor_closure_under_the_pointer() else {
        return;
    };

    // Gone from under the still pointer, the frame works out what is there
    // now, which declares nothing (#842).
    shown.set(false);
    app.step();
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Pointer, CursorIcon::Default]
    );
    loading.set(true);
    app.step();

    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Pointer, CursorIcon::Default],
        "nothing is asked for a declaration that is no longer there"
    );
}

/// The same, with a button held: the hover is not worked out again until the
/// release, so the declaration that went is still the one the pointer was
/// last over when the closure's signal is written.
#[test]
fn a_cursor_closure_whose_widget_went_under_a_held_button_is_not_read_again() {
    let Some((mut app, surface, shown, loading)) = a_cursor_closure_under_the_pointer() else {
        return;
    };
    app.event_at(
        surface,
        Event::mouse_down(10.0, 10.0, MouseButton::Right),
        Instant::now(),
    );
    app.step();
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

/// A widget filling a bar declares its cursor with a closure, and the
/// pointer rests on it: `shown` takes the widget away, `loading` is what the
/// closure reads.
fn a_cursor_closure_under_the_pointer()
-> Option<(Headless, SurfaceId, RwSignal<bool>, RwSignal<bool>)> {
    let mut app = headless()?;
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
    Some((app, surface, shown, loading))
}

// ---------------------------------------------------------------------------
// A surface without a cursor (#500)
// ---------------------------------------------------------------------------

/// Hidden is a shape like any other: the root declaring it is the whole
/// surface without a pointer, and each enter asks for it again, because the
/// compositor shows whatever it likes until a cursor is set against that
/// enter's serial.
#[test]
fn a_root_that_hides_the_cursor_asks_for_hidden_on_every_enter() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .cursor(CursorIcon::Hidden)
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    for _ in 0..2 {
        pointer_enters(&mut app, surface);
        app.step();
        app.event_at(surface, Event::MouseLeave, Instant::now());
        app.step();
    }

    assert_eq!(app.cursors_asked(), [CursorIcon::Hidden; 2]);
}

/// A widget inside a surface without a cursor can still claim one, and the
/// root's `Hidden` takes over again the moment the pointer leaves it.
#[test]
fn a_child_of_a_hidden_root_shows_its_own_cursor_and_hides_it_again_on_leaving() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        left_of_a_bar(container().cursor(CursorIcon::Pointer)).cursor(CursorIcon::Hidden)
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Pointer], "over the child");

    pointer_moves(&mut app, surface, 160.0, 10.0);
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Pointer, CursorIcon::Hidden],
        "over the root alone"
    );
}

// ---------------------------------------------------------------------------
// A text input says which cursor it shows (#502)
// ---------------------------------------------------------------------------

/// A lock screen's password field: the surface has no pointer, and neither
/// does the field. The press is what the input handles itself, and a claim
/// made only on a move would give the I-beam back on every click.
#[test]
fn a_text_input_that_hides_the_cursor_claims_hidden_on_moves_and_on_presses() {
    let Some(mut app) = headless() else { return };
    let value = create_password();
    let surface = app.surface(fixed_bar(), move || {
        left_of_a_bar(
            container().child(password_input(value).no_caret().cursor(CursorIcon::Hidden)),
        )
        .cursor(CursorIcon::Pointer)
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Hidden], "over the field");

    app.click(surface, 10.0, 10.0);
    app.step();
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Hidden],
        "a click in the field asks for nothing else"
    );

    pointer_moves(&mut app, surface, 160.0, 10.0);
    assert_eq!(
        app.cursors_asked(),
        [CursorIcon::Hidden, CursorIcon::Pointer],
        "and the root's shape comes back beside it"
    );
}

#[test]
fn a_text_input_that_declares_no_cursor_still_shows_the_i_beam() {
    let Some(mut app) = headless() else { return };
    let value = create_signal(String::new());
    let surface = app.surface(fixed_bar(), move || {
        left_of_a_bar(container().child(text_input(value))).cursor(CursorIcon::Hidden)
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    app.click(surface, 10.0, 10.0);
    app.step();

    assert_eq!(app.cursors_asked(), [CursorIcon::Text]);
}

#[test]
fn a_text_input_cursor_given_by_a_signal_changes_while_the_pointer_stays_still() {
    let Some(mut app) = headless() else { return };
    let value = create_signal(String::new());
    let waiting = create_signal(false);
    let surface = app.surface(fixed_bar(), move || {
        left_of_a_bar(container().child(text_input(value).cursor(move || {
            if waiting.get() {
                CursorIcon::Wait
            } else {
                CursorIcon::Text
            }
        })))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();

    pointer_enters(&mut app, surface);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Text]);

    waiting.set(true);
    app.step();
    assert_eq!(app.cursors_asked(), [CursorIcon::Text, CursorIcon::Wait]);
}

/// The issue's sequence (#625), through the loop rather than a bare tree, in
/// 10px moves: a right-click pressed and released in the middle of a left drag
/// leaves the drag's moves and its release where they were going. On `main`
/// the click dropped the route while the pointer was two siblings away from
/// the drag's origin, which no container offers by position.
#[test]
fn a_right_click_two_siblings_from_a_left_drag_leaves_the_drag_alone() {
    let Some(mut app) = headless() else { return };
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let surface = app.surface(fixed_bar(), move || {
        let (down, moved, up) = (log.clone(), log.clone(), log.clone());
        container()
            .layout(Flex::row())
            .child(
                container()
                    .width(100.0)
                    .height(20.0)
                    .on_mouse_down(move |x, _| down.borrow_mut().push(format!("down{x}")))
                    .on_pointer_move(move |x, _| moved.borrow_mut().push(format!("move{x}")))
                    .on_mouse_up(move |x, _| up.borrow_mut().push(format!("up{x}"))),
            )
            .children((0..4).map(|_| container().width(100.0).height(20.0)))
    });
    app.configure(surface, 500, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    let drag = (2..=45).map(|step| Event::mouse_move(step as f32 * 10.0, 10.0));
    let events = [
        Event::mouse_down(10.0, 10.0, MouseButton::Left),
        Event::mouse_down(10.0, 10.0, MouseButton::Right),
    ]
    .into_iter()
    .chain(drag)
    .chain([
        Event::mouse_up(450.0, 10.0, MouseButton::Right),
        Event::mouse_move(460.0, 10.0),
        Event::mouse_up(460.0, 10.0, MouseButton::Left),
    ]);
    for event in events {
        at += Duration::from_millis(16);
        app.event_at(surface, event, at);
        app.step_at(at);
    }

    let seen = seen.borrow();
    assert_eq!(
        (
            seen.iter()
                .filter(|entry| entry.starts_with("move"))
                .count(),
            seen.last().map(String::as_str),
        ),
        (45, Some("up460")),
        "every move and the release reach the drag's origin: {seen:?}"
    );
}

/// A surface closed while a button is held is never sent the release, so the
/// button it held must not turn the next surface's press into part of a chord
/// that no longer exists.
#[test]
fn a_press_held_on_a_surface_that_closes_does_not_hold_the_next_one() {
    let Some(mut app) = headless() else { return };
    let first = app.surface(fixed_bar(), measuring_24);
    app.configure(first, 200, 50, 1.0);
    let clicks = create_signal(0u32);
    let second = app.surface(fixed_bar(), move || bar(clicks));
    app.configure(second, 200, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    at += Duration::from_millis(16);
    app.event_at(first, Event::mouse_down(10.0, 10.0, MouseButton::Left), at);
    app.step_at(at);
    surface_handle(first).close();
    app.step_at(at);

    app.click(second, 10.0, 10.0);
    app.step();
    assert_eq!(
        clicks.get(),
        1,
        "the press on the second surface is hit-tested where it lands"
    );
}

/// A press inside a chord is not hit-tested, so it says nothing about what is
/// under the pointer — and must not take away the cursor the last move asked
/// for (#625).
#[test]
fn a_right_press_during_a_drag_keeps_the_cursor_under_the_pointer() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || {
        container()
            .layout(Flex::row())
            .child(container().width(100.0).height(20.0).on_mouse_up(|_, _| {}))
            // Clear of the window the press at x=10 was offered, which is the
            // container under it and one either side.
            .children((0..2).map(|_| container().width(100.0).height(20.0)))
            .child(
                container()
                    .width(100.0)
                    .height(20.0)
                    .cursor(CursorIcon::Pointer),
            )
    });
    app.configure(surface, 500, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    pointer_enters(&mut app, surface);

    for event in [
        Event::mouse_down(10.0, 10.0, MouseButton::Left),
        Event::mouse_move(350.0, 10.0),
        Event::mouse_down(350.0, 10.0, MouseButton::Right),
    ] {
        at += Duration::from_millis(16);
        app.event_at(surface, event, at);
        app.step_at(at);
    }

    assert_eq!(
        app.cursors_asked().last(),
        Some(&CursorIcon::Pointer),
        "the press went along the drag's route, and the cursor stays what the \
         move over the fourth container asked for: {:?}",
        app.cursors_asked()
    );
}

/// What hiding the panel is tested with: a spinner, and a label whose colour
/// pulses for ever, each inside the panel rather than the panel itself.
fn spinner_and_pulse() -> (Container, Text) {
    let spinner = container()
        .width(20.0)
        .height(20.0)
        .background(Color::WHITE)
        .rotate(
            0.0.timeline(
                Keyframes::new(1000.0)
                    .at(0.0, 0.0)
                    .at(1.0, 360.0)
                    .repeat(Repeat::Forever),
            ),
        );
    let pulse = text("busy").color(
        Color::WHITE.timeline(
            Keyframes::new(1000.0)
                .at(0.0, Color::WHITE)
                .at(1.0, Color::BLACK)
                .repeat(Repeat::Forever),
        ),
    );
    (spinner, pulse)
}

/// Hiding a panel stops the animations inside it, not only its own (#666):
/// what it holds is not on screen, so nothing should be presented for it.
/// Showing the panel again starts them again.
#[test]
fn what_a_hidden_panel_holds_lets_the_surface_go_idle() {
    for which in ["spinner", "pulse"] {
        let Some(mut app) = headless() else { return };
        let busy = create_signal(true);
        let surface = app.surface(fixed_bar(), move || {
            let (spinner, pulse) = spinner_and_pulse();
            let panel = container().visible(busy);
            container().width(fill()).height(fill()).child(match which {
                "spinner" => panel.child(spinner),
                _ => panel.child(pulse),
            })
        });
        app.configure(surface, 200, 50, 1.0);
        let t0 = Instant::now();
        let at = |i: u64| t0 + Duration::from_millis(16 * i);
        for i in 0..5 {
            app.step_at(at(i));
        }
        assert_eq!(app.frames_presented(surface), 5, "{which}: running");

        busy.set(false);
        for i in 5..8 {
            app.step_at(at(i));
        }
        let settled = app.frames_presented(surface);
        for i in 8..28 {
            app.step_at(at(i));
        }
        assert_eq!(
            app.frames_presented(surface),
            settled,
            "{which}: a hidden panel's contents presented frames of nothing"
        );

        busy.set(true);
        for i in 28..32 {
            app.step_at(at(i));
        }
        assert!(
            app.frames_presented(surface) >= settled + 3,
            "{which}: shown again, it runs again"
        );
    }
}

/// Inside two panels, the outer one hidden: showing it again starts the
/// spinner again, though the inner panel never changed and the spinner's last
/// read stopped at the outer one.
#[test]
fn a_spinner_two_panels_deep_runs_again_when_the_outer_one_is_shown() {
    let Some(mut app) = headless() else { return };
    let outer = create_signal(true);
    let inner = create_signal(true);
    let surface = app.surface(fixed_bar(), move || {
        let (spinner, _) = spinner_and_pulse();
        container().width(fill()).height(fill()).child(
            container()
                .visible(outer)
                .child(container().visible(inner).child(spinner)),
        )
    });
    app.configure(surface, 200, 50, 1.0);
    let t0 = Instant::now();
    let at = |i: u64| t0 + Duration::from_millis(16 * i);
    for i in 0..5 {
        app.step_at(at(i));
    }
    outer.set(false);
    for i in 5..8 {
        app.step_at(at(i));
    }
    let settled = app.frames_presented(surface);
    for i in 8..28 {
        app.step_at(at(i));
    }
    assert_eq!(app.frames_presented(surface), settled, "hidden two deep");

    outer.set(true);
    for i in 28..32 {
        app.step_at(at(i));
    }
    assert!(
        app.frames_presented(surface) >= settled + 3,
        "shown again, it runs again"
    );
}

/// A widget from outside the crate that animates by reading a signal under
/// its Animation job: inside a hidden panel a change of that signal advances
/// nothing, and shown again it does (#666). The check is the loop's, so it is
/// one a widget cannot forget to make.
#[test]
fn a_widget_from_outside_the_crate_does_not_animate_inside_a_hidden_panel() {
    struct Ticker {
        tick: Signal<u32>,
        advanced: Rc<Cell<u32>>,
    }
    impl Widget for Ticker {
        fn layout(&mut self, ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
            let id = ctx.id();
            with_signal_tracking(id, JobType::Animation, || self.tick.get());
            Size::new(10.0, 10.0)
        }
        fn advance_animations(&mut self, _tree: &mut Tree, id: WidgetId) -> bool {
            self.advanced.set(self.advanced.get() + 1);
            with_signal_tracking(id, JobType::Animation, || self.tick.get());
            false
        }
        fn paint(&self, _ctx: &mut PaintContext) {}
    }

    let Some(mut app) = headless() else { return };
    let shown = create_signal(true);
    let tick = create_signal(0u32);
    let advanced = Rc::new(Cell::new(0));
    let counted = advanced.clone();
    let surface = app.surface(fixed_bar(), move || {
        container().child(container().visible(shown).child(Ticker {
            tick: tick.into(),
            advanced: counted.clone(),
        }))
    });
    app.configure(surface, 200, 50, 1.0);
    app.step();
    tick.set(1);
    app.step();
    assert_eq!(advanced.get(), 1, "a tick advances it");

    shown.set(false);
    app.step();
    tick.set(2);
    app.step();
    assert_eq!(
        advanced.get(),
        1,
        "inside a hidden panel a tick advances nothing"
    );

    shown.set(true);
    app.step();
    assert!(advanced.get() > 1, "shown again, it advances");
}

/// A surface configured before the compositor has sent a scale is drawn at 1,
/// the scale Wayland defines it to have until it is told another (#643), and
/// redrawn only when something changes. sctk names a scale only when it differs
/// from the 1 it assumed, so at scale 1 without fractional scaling none ever
/// comes: waiting on one repainted the surface in full at frame rate for good.
/// A scale that does come is a change, which the test below draws.
#[test]
fn a_surface_never_told_its_scale_is_built_at_one_and_redrawn_only_on_change() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), container);
    app.configure_size(surface, 200, 50);
    app.step();
    assert_eq!(
        app.targets_built(surface),
        [(200, 50)],
        "built at a scale of 1"
    );

    let drawn = app.frames_presented(surface);
    app.step();
    app.step();
    assert_eq!(
        app.frames_presented(surface),
        drawn,
        "a pass that changes nothing draws nothing, scale or no scale"
    );
}

/// A scale that arrives after the first frame resizes the surface's buffer to
/// it, and once a frame is up at that scale the surface stops being rendered
/// on passes where nothing changed.
#[test]
fn a_late_scale_resizes_the_buffer_and_ends_the_forced_render() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), container);
    app.configure_size(surface, 200, 50);
    app.step();
    assert_eq!(app.physical_size(surface), (200, 50));

    app.send_scale(surface, 2.0);
    app.step();
    assert_eq!(app.physical_size(surface), (400, 100), "resized to 2");

    let drawn = app.frames_presented(surface);
    app.step();
    app.step();
    assert_eq!(
        app.frames_presented(surface),
        drawn,
        "with a frame up at its scale, a pass that changes nothing draws nothing"
    );
}

/// A scale sent before the surface is configured is the one its first buffer
/// is built at: one target, at 2, and never one at 1 grown to it.
#[test]
fn a_scale_sent_before_configure_is_the_first_buffers() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), container);
    app.send_scale(surface, 2.0);
    app.configure_size(surface, 200, 50);
    app.step();
    assert_eq!(app.targets_built(surface), [(400, 100)], "built at 2");
    assert_eq!(app.physical_size(surface), (400, 100));
}

/// A lock surface given its size and no scale is built at 1, as every surface
/// is until the compositor sends one: one target, at its logical size.
#[test]
fn a_lock_surface_with_no_scale_yet_is_built_at_one() {
    let Some(mut app) = headless() else { return };
    app.connect_output("eDP-1");
    lock_session(lock_screen);
    app.step();
    let (cover, _) = app.lock_surfaces_created()[0];

    app.configure_size(cover, 300, 200);
    app.step();
    assert_eq!(app.targets_built(cover), [(300, 200)]);
}

/// A popup arrives with its scale, so once it has drawn, a pass that changes
/// nothing does not draw it again.
#[test]
fn a_drawn_popup_is_not_drawn_again_for_nothing() {
    let Some(mut app) = headless() else { return };
    let parent = app.surface(content_bar(), measuring_24);
    app.configure(parent, 200, 24, 1.0);
    app.step();
    let popup = spawn_popup(parent, PopupConfig::new(80).height(40), measuring_24);
    app.step();
    app.step();

    let drawn = app.frames_presented(popup.id());
    assert!(drawn > 0, "the popup drew");
    app.step();
    app.step();
    assert_eq!(app.frames_presented(popup.id()), drawn);
}

/// The row #670 measured, on a surface wider than it: a title that takes what
/// is left, and a badge of a fixed size whose width and visibility are signals.
///
/// A fixed width and height make the badge a relayout boundary, and so does
/// being hidden. A boundary absorbs what changes *inside* it; these change the
/// badge itself, and the row has to place its children again around it.
fn title_and_badge(app: &mut Headless, width: f32, shown: bool) -> BadgeRow {
    let (badge_width, badge_shown) = (create_signal(width), create_signal(shown));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container()
                .width(300.0)
                .layout(Flex::row().spacing(8.0))
                .child(block(20.0, Color::rgb(1.0, 0.0, 0.0)))
                .child(
                    container()
                        .visible(move || badge_shown.get())
                        .width(move || badge_width.get())
                        .height(20.0)
                        .background(Color::rgb(0.0, 0.0, 1.0)),
                )
        },
    );
    app.configure(surface, 400, 20, 1.0);
    app.step();
    BadgeRow {
        surface,
        width: badge_width,
        shown: badge_shown,
    }
}

struct BadgeRow {
    surface: SurfaceId,
    width: RwSignal<f32>,
    shown: RwSignal<bool>,
}

impl BadgeRow {
    /// Where the title and the badge are on the last frame, read across row
    /// 10: the red run and the blue run, each as `(x, width)`, `None` for one
    /// that is not drawn.
    fn spans(&self, app: &Headless) -> [Option<(u32, u32)>; 2] {
        let (width, _) = app.physical_size(self.surface);
        let frame = app.read_frame(self.surface);
        let row = &frame[(10 * width * 4) as usize..(11 * width * 4) as usize];
        let span = |channel: usize| {
            let lit: Vec<u32> = (0..width)
                .filter(|&x| {
                    let px = &row[(x * 4) as usize..(x * 4 + 3) as usize];
                    (0..3).all(|c| (px[c] > 127) == (c == channel))
                })
                .collect();
            let (&first, &last) = (lit.first()?, lit.last()?);
            assert_eq!(lit.len() as u32, last - first + 1, "one run of {channel}");
            Some((first, last - first + 1))
        };
        [span(0), span(2)]
    }
}

/// A badge that grows pushes the title back rather than out of the row.
#[test]
fn a_badge_that_grows_lays_its_row_out_again() {
    let Some(mut app) = headless() else { return };
    let row = title_and_badge(&mut app, 40.0, true);
    assert_eq!(row.spans(&app), [Some((0, 252)), Some((260, 40))]);

    row.width.set(80.0);
    app.step();
    assert_eq!(
        row.spans(&app),
        [Some((0, 212)), Some((220, 80))],
        "the badge grew past the row's edge and the title never moved"
    );
}

/// A badge that is hidden gives the title its place, spacing and all.
#[test]
fn a_badge_that_is_hidden_lays_its_row_out_again() {
    let Some(mut app) = headless() else { return };
    let row = title_and_badge(&mut app, 40.0, true);

    row.shown.set(false);
    app.step();
    assert_eq!(
        row.spans(&app),
        [Some((0, 300)), None],
        "the badge left a hole where it was"
    );
}

/// A badge built hidden and then shown takes its place beside the title, though
/// a hidden container is a relayout boundary whatever its size says.
#[test]
fn a_badge_that_is_shown_lays_its_row_out_again() {
    let Some(mut app) = headless() else { return };
    let row = title_and_badge(&mut app, 40.0, false);
    assert_eq!(row.spans(&app), [Some((0, 300)), None]);

    row.shown.set(true);
    app.step();
    assert_eq!(
        row.spans(&app),
        [Some((0, 252)), Some((260, 40))],
        "the badge was drawn over the title's end"
    );
}

// ---------------------------------------------------------------------------
// Switching a container off ends the hover and the press of it and of what is
// inside it (#712, #716)
// ---------------------------------------------------------------------------

/// The two ways a container stops taking the pointer. Both return from `event`
/// before its children are asked, so both owe them the leave.
#[derive(Clone, Copy, Debug)]
enum SwitchedOff {
    Hidden,
    Disabled,
}

/// Each way of switching off, on the container that holds the hover or the
/// press itself (#712) and on its parent (#716).
const SWITCHED_OFF: [(SwitchedOff, bool); 4] = [
    (SwitchedOff::Hidden, false),
    (SwitchedOff::Hidden, true),
    (SwitchedOff::Disabled, false),
    (SwitchedOff::Disabled, true),
];

impl SwitchedOff {
    /// `target`, `width` by `height`, switched off while `on` is false —
    /// itself, or through a parent of that size when `nested`.
    fn around(
        self,
        target: Container,
        nested: bool,
        on: RwSignal<bool>,
        width: f32,
        height: Length,
    ) -> Container {
        let gated = |container: Container| match self {
            SwitchedOff::Hidden => container.visible(on),
            SwitchedOff::Disabled => container.enabled(on),
        };
        if nested {
            gated(container().width(width).height(height))
                .child(target.width(fill()).height(fill()))
        } else {
            gated(target.width(width).height(height))
        }
    }
}

/// The pointer on a container that is then switched off, or whose parent is,
/// and then leaving the surface: whoever asked to hear about the hover hears
/// that it ended.
#[test]
fn a_container_switched_off_under_the_pointer_hears_it_leave() {
    for (gate, nested) in SWITCHED_OFF {
        let Some(mut app) = headless() else { return };
        let on = create_signal(true);
        let calls = Rc::new(RefCell::new(Vec::new()));
        let record = calls.clone();
        let surface = app.surface(fixed_bar(), move || {
            let record = record.clone();
            let target = container().on_hover(move |hovered| record.borrow_mut().push(hovered));
            container().width(fill()).height(fill()).child(gate.around(
                target,
                nested,
                on,
                80.0,
                20.0.into(),
            ))
        });
        app.configure(surface, 200, 50, 1.0);
        app.step();

        let case = format!("{gate:?}, nested: {nested}");
        pointer_moves(&mut app, surface, 10.0, 10.0);
        assert_eq!(*calls.borrow(), [true], "{case}: the pointer is on it");

        on.set(false);
        app.step();
        app.event_at(surface, Event::MouseLeave, Instant::now());
        app.step();
        assert_eq!(
            *calls.borrow(),
            [true, false],
            "{case}: and then it was switched off"
        );
    }
}

/// Switched off between press and release, the container misses the release;
/// on again, it is not still pressed — neither drawn so, nor ready to take a
/// release that belongs to a press somewhere else.
#[test]
fn a_container_switched_off_mid_press_is_not_pressed_when_on_again() {
    for (gate, nested) in SWITCHED_OFF {
        let Some(mut app) = headless() else { return };
        let on = create_signal(true);
        let clicks = create_signal(0u32);
        let surface = app.surface(fixed_bar(), move || {
            container()
                .layout(Flex::row())
                .width(fill())
                .height(fill())
                .child(gate.around(pressable(container(), clicks), nested, on, 80.0, fill()))
                .child(container().width(80.0).height(fill()).on_click(|| {}))
        });
        app.configure(surface, 200, 50, 1.0);
        app.step();

        let case = format!("{gate:?}, nested: {nested}");
        let now = Instant::now();
        app.event_at(
            surface,
            Event::mouse_down(10.0, 10.0, MouseButton::Left),
            now,
        );
        app.step();
        assert!(pressed(&app, surface), "{case}: the press landed on it");

        on.set(false);
        app.step();
        app.event_at(surface, Event::mouse_up(10.0, 10.0, MouseButton::Left), now);
        app.event_at(surface, Event::mouse_move(120.0, 10.0), now);
        app.step();
        on.set(true);
        app.step();
        assert!(
            !pressed(&app, surface),
            "{case}: the button is up, so it is not drawn down"
        );

        app.event_at(
            surface,
            Event::mouse_down(120.0, 10.0, MouseButton::Left),
            now,
        );
        app.event_at(surface, Event::mouse_move(10.0, 10.0), now);
        app.event_at(surface, Event::mouse_up(10.0, 10.0, MouseButton::Left), now);
        app.step();
        assert_eq!(
            clicks.get(),
            0,
            "{case}: a press on the sibling released over it activated nothing"
        );
    }
}

/// 160ms, linear: so the middle of a move is half of it.
fn sliding() -> Transition {
    Transition::new(160.0, TimingFunction::Linear)
}

/// A column of rows 20 tall, in the order `rows` holds their keys, each
/// sliding to where layout moves it and reported through `refs[key]`. A click on one records its key in `clicked`.
fn reorderable(
    rows: RwSignal<Vec<u32>>,
    refs: [WidgetRef; 2],
    clicked: Rc<Cell<Option<u32>>>,
) -> Container {
    container().layout(Flex::column()).children(keyed(
        move || rows.get(),
        |key| *key,
        move |key| {
            let clicked = clicked.clone();
            container()
                .width(60.0)
                .height(20.0)
                .animate_position(sliding())
                .on_click(move || clicked.set(Some(key)))
                .widget_ref(refs[key as usize])
        },
    ))
}

/// A row moved by a reorder is drawn where it was on the first frame after
/// the change, between the two half way through, and where it now is once
/// the transition ends. Its layout box is the new one from the first frame,
/// and a click lands where it is drawn.
#[test]
fn a_row_moved_by_a_reorder_slides_from_where_it_was() {
    let Some(mut app) = headless() else { return };
    let (a, b) = (create_widget_ref(), create_widget_ref());
    let rows = create_signal(vec![0, 1]);
    let clicked = Rc::new(Cell::new(None));
    let seen = clicked.clone();
    let surface = app.surface(fixed_bar(), move || reorderable(rows, [a, b], seen.clone()));
    app.configure(surface, 200, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    assert_eq!(
        (a.rect().get_untracked().y, b.rect().get_untracked().y),
        (0.0, 20.0),
        "the first layout places the rows without sliding them there"
    );

    rows.set(vec![1, 0]);
    at += Duration::from_millis(16);
    app.step_at(at);
    assert_eq!(a.rect().get_untracked().y, 0.0, "drawn where it was");
    assert_eq!(b.rect().get_untracked().y, 20.0);
    assert_eq!(
        a.layout_rect().get_untracked().y,
        20.0,
        "laid out where it is"
    );
    assert_eq!(b.layout_rect().get_untracked().y, 0.0);

    // The click arrives with the next frame, where row 0 is still drawn at
    // the top, and lands on it rather than on the row laid out there.
    app.event_at(surface, Event::mouse_down(10.0, 5.0, MouseButton::Left), at);
    app.event_at(surface, Event::mouse_up(10.0, 5.0, MouseButton::Left), at);
    at += Duration::from_millis(80);
    app.step_at(at);
    assert_eq!(
        clicked.get(),
        Some(0),
        "the click lands on the row drawn there"
    );
    let (mid_a, mid_b) = (a.rect().get_untracked().y, b.rect().get_untracked().y);
    assert!(
        mid_a > 2.0 && mid_a < 18.0 && mid_b > 2.0 && mid_b < 18.0,
        "half way through, both are between their two places: {mid_a}, {mid_b}"
    );
    assert_eq!(a.layout_rect().get_untracked().y, 20.0);

    for _ in 0..10 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    assert_eq!(a.rect().get_untracked(), a.layout_rect().get_untracked());
    assert_eq!(a.rect().get_untracked().y, 20.0, "and settles where it is");
    assert_eq!(b.rect().get_untracked().y, 0.0);
}

/// Scrolling moves the content, not a row inside it: no row's origin in its
/// parent changes, so nothing slides, and every row is drawn where it is laid
/// out on every frame of the scroll.
#[test]
fn scrolling_an_ancestor_slides_nothing() {
    let Some(mut app) = headless() else { return };
    let row = create_widget_ref();
    let (surface, mut at) = scroller(&mut app, move || {
        container()
            .width(fill())
            .height(fill())
            .scroll(hidden_scroll())
            .child(
                container()
                    .layout(Flex::column())
                    .width(fill())
                    .child(block(500.0, Color::rgb(1.0, 0.0, 0.0)).animate_position(sliding()))
                    .child(
                        block(1500.0, Color::rgb(0.0, 0.0, 1.0))
                            .animate_position(sliding())
                            .widget_ref(row),
                    ),
            )
    });
    for _ in 0..6 {
        app.event_at(
            surface,
            Event::scroll(50.0, 50.0, 0.0, 10.0, ScrollSource::Finger),
            at,
        );
        at += Duration::from_millis(8);
    }
    app.event_at(surface, Event::scroll_end(50.0, 50.0), at);
    let mut ys = Vec::new();
    for frame in 0..20 {
        app.step_at(at);
        let (drawn, laid_out) = (
            row.rect().get_untracked(),
            row.layout_rect().get_untracked(),
        );
        assert_eq!(drawn, laid_out, "frame {frame}: drawn where it is laid out");
        ys.push(drawn.y);
        at += Duration::from_millis(16);
    }
    assert!(
        ys[0] - ys[ys.len() - 1] > 60.0,
        "and the content did scroll: {ys:?}"
    );
}

/// A parent that moves carries its children, and only its own move slides:
/// the child's origin inside it never changed, so the child is drawn at the
/// same place in the parent on every frame — at once where the parent does
/// not slide, and riding the parent's slide where it does.
#[test]
fn a_parents_move_does_not_slide_its_children() {
    let Some(mut app) = headless() else { return };
    let moved = create_signal(false);
    let (still, sliding_parent, carried) = (
        create_widget_ref(),
        create_widget_ref(),
        create_widget_ref(),
    );
    let surface = app.surface(fixed_bar(), move || {
        let pad = move || if moved.get() { [0.0, 80.0] } else { [0.0, 0.0] };
        container()
            .layout(Flex::column())
            .child(
                container().padding(pad).child(
                    container().child(
                        container()
                            .width(20.0)
                            .height(20.0)
                            .animate_position(sliding())
                            .widget_ref(still),
                    ),
                ),
            )
            .child(
                container().padding(pad).child(
                    container()
                        .animate_position(sliding())
                        .widget_ref(sliding_parent)
                        .child(
                            container()
                                .width(20.0)
                                .height(20.0)
                                .animate_position(sliding())
                                .widget_ref(carried),
                        ),
                ),
            )
    });
    app.configure(surface, 200, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    moved.set(true);
    let mut parent_xs = Vec::new();
    for frame in 0..14 {
        at += Duration::from_millis(16);
        app.step_at(at);
        assert_eq!(
            still.rect().get_untracked(),
            still.layout_rect().get_untracked(),
            "frame {frame}: a child whose parent does not slide is where its parent put it"
        );
        let (parent, child) = (
            sliding_parent.rect().get_untracked(),
            carried.rect().get_untracked(),
        );
        assert_eq!(
            (child.x, child.y),
            (parent.x, parent.y),
            "frame {frame}: and one whose parent slides rides it without a slide of its own"
        );
        parent_xs.push(parent.x);
    }
    assert_eq!(still.rect().get_untracked().x, 80.0);
    assert!(
        parent_xs.iter().any(|&x| x > 1.0 && x < 79.0),
        "the parent did slide: {parent_xs:?}"
    );
    assert_eq!(parent_xs.last(), Some(&80.0));
}

/// A card slid across by its parent's padding, with a spring.
fn a_springing_card(card: WidgetRef, across: RwSignal<f32>, wide: RwSignal<bool>) -> Container {
    container().padding(move || [0.0, across.get()]).child(
        container()
            .width((move || if wide.get() { 80.0 } else { 20.0 }).transition(sliding()))
            .height(20.0)
            .animate_position(SpringConfig::SNAPPY)
            .widget_ref(card),
    )
}

/// A move during a move starts from where the card is drawn and keeps going
/// the way it was going: no frame jumps further than the spring was already
/// moving, and it settles at the second target.
#[test]
fn a_move_during_a_move_continues_from_where_it_is_drawn() {
    let Some(mut app) = headless() else { return };
    let card = create_widget_ref();
    let across = create_signal(0.0);
    let wide = create_signal(false);
    let surface = app.surface(fixed_bar(), move || a_springing_card(card, across, wide));
    app.configure(surface, 400, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    across.set(200.0);
    let mut xs = Vec::new();
    for _ in 0..5 {
        at += Duration::from_millis(16);
        app.step_at(at);
        xs.push(card.rect().get_untracked().x);
    }
    let before = *xs.last().expect("five frames");
    assert!(
        before > 1.0 && before < 199.0,
        "mid-spring when the second move comes: {xs:?}"
    );
    let fastest = xs
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);

    across.set(100.0);
    at += Duration::from_millis(16);
    app.step_at(at);
    let after = card.rect().get_untracked().x;
    assert!(
        (after - before).abs() <= fastest + 1.0,
        "the frame after the retarget moved {} from {before}, \
         and the spring was moving at most {fastest} a frame: {xs:?} then {after}",
        after - before
    );
    assert!(
        after >= before,
        "still moving the way it was: {before} then {after}"
    );
    assert_eq!(card.layout_rect().get_untracked().x, 100.0);

    for _ in 0..120 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    assert_eq!(
        card.rect().get_untracked().x,
        100.0,
        "and settles at the second"
    );
}

/// Sliding to a new place and growing to a new width, together: the width
/// transition and the slide each reach their own target.
#[test]
fn a_card_that_moves_and_grows_reaches_both_targets() {
    let Some(mut app) = headless() else { return };
    let card = create_widget_ref();
    let across = create_signal(0.0);
    let wide = create_signal(false);
    let surface = app.surface(fixed_bar(), move || a_springing_card(card, across, wide));
    app.configure(surface, 400, 50, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    across.set(150.0);
    wide.set(true);
    let mut seen = Vec::new();
    for _ in 0..120 {
        at += Duration::from_millis(16);
        app.step_at(at);
        let drawn = card.rect().get_untracked();
        seen.push((drawn.x, drawn.width));
    }
    assert!(
        seen.iter()
            .any(|&(x, w)| x > 1.0 && x < 149.0 && w > 21.0 && w < 79.0),
        "on the way, both were between their ends: {seen:?}"
    );
    assert_eq!(
        card.rect().get_untracked(),
        Rect::new(150.0, 0.0, 80.0, 20.0),
        "and both arrived"
    );
}

/// A row sliding out of view from a place in it is still painted on its way
/// out. Its layout box is far below the viewport from the first frame, and a
/// scroller paints only the children whose box can reach what it shows — so
/// the slide has to reach that far, or the row vanishes on the frame it
/// starts to move.
#[test]
fn a_row_sliding_out_of_view_is_painted_on_its_way() {
    let Some(mut app) = headless() else { return };
    let pushed = create_signal(false);
    let red = |[r, g, b, _]: [u8; 4]| r > 200 && g < 50 && b < 50;
    let (surface, mut at) = scroller(&mut app, move || {
        container()
            .width(fill())
            .height(fill())
            .scroll(hidden_scroll())
            .child(
                container()
                    .layout(Flex::column())
                    .width(fill())
                    .child(
                        container()
                            .width(fill())
                            .height(move || if pushed.get() { 1000.0 } else { 0.0 }),
                    )
                    .child(block(20.0, Color::rgb(1.0, 0.0, 0.0)).animate_position(sliding())),
            )
    });
    assert!(
        red(app.read_pixel(surface, 50, 10)),
        "the row is at the top"
    );

    pushed.set(true);
    at += Duration::from_millis(16);
    app.step_at(at);
    assert!(
        red(app.read_pixel(surface, 50, 10)),
        "laid out 1000 down, and still drawn where it was"
    );

    at += Duration::from_millis(64);
    app.step_at(at);
    let y = (0..600).find(|&y| red(app.read_pixel(surface, 50, y)));
    assert!(
        y.is_some_and(|y| y > 100 && y < 590),
        "part way down the viewport, on its way out: {y:?}"
    );
}

/// Two views swapped on one signal, as a panel that collapses to a pill does:
/// both fill the surface, and the pill springs its scale down while `squished`
/// says so.
fn a_panel_and_its_pill(
    pill: WidgetRef,
    collapsed: RwSignal<bool>,
    squished: RwSignal<bool>,
) -> Container {
    container()
        .layout(ZStack::new())
        .width(fill())
        .height(fill())
        .child(
            container()
                .width(fill())
                .height(fill())
                .background(Color::rgb(0.2, 0.2, 0.8))
                .visible(move || !collapsed.get()),
        )
        .child(
            container()
                .width(fill())
                .height(fill())
                .background(Color::rgb(0.1, 0.1, 0.1))
                .scale(
                    (move || {
                        if squished.get() {
                            Scale::uniform(0.5)
                        } else {
                            Scale::NONE
                        }
                    })
                    .transition(Transition::spring(SpringConfig::SNAPPY)),
                )
                .visible(collapsed)
                .widget_ref(pill),
        )
}

/// A 400 by 300 surface showing the panel, its pill hidden — mid-spring when
/// `spring` says so — and then the pill shown in the frame the surface is
/// configured to 100 by 40, as it is when the configure lands while the frame
/// that shows it is waiting on its callback. The surface, what the pill was
/// laid out at, and how many damage rectangles were reported before that frame.
fn a_pill_shown_as_the_surface_shrinks(
    app: &mut Headless,
    spring: bool,
) -> (SurfaceId, Rect, usize) {
    let pill = create_widget_ref();
    let collapsed = create_signal(true);
    let squished = create_signal(false);
    let surface = app.surface(SurfaceConfig::new().width(400).height(300), move || {
        a_panel_and_its_pill(pill, collapsed, squished)
    });
    app.configure(surface, 400, 300, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    if spring {
        squished.set(true);
        for _ in 0..3 {
            at += Duration::from_millis(16);
            app.step_at(at);
        }
    }
    collapsed.set(false);
    at += Duration::from_millis(16);
    app.step_at(at);

    let before = app.damage_reported(surface).len();
    collapsed.set(true);
    app.configure(surface, 100, 40, 1.0);
    for _ in 0..20 {
        at += Duration::from_millis(16);
        app.step_at(at);
    }
    (surface, pill.rect().get_untracked(), before)
}

/// A container shown again in the frame its surface shrank is laid out against
/// the surface it is in now, and the animation it resumes damages nothing
/// outside it (#775). It used to keep the 400 by 300 layout, and every frame of
/// the spring damaged — and drew — a pill four times the surface.
#[test]
fn a_container_shown_as_its_surface_shrinks_resumes_its_animation_inside_it() {
    let Some(mut app) = headless() else { return };
    let (surface, pill, before) = a_pill_shown_as_the_surface_shrinks(&mut app, true);
    let damage = &app.damage_reported(surface)[before..];

    assert_eq!(app.root_size(surface), (100.0, 40.0));
    assert!(
        pill.width < 100.0 && pill.height < 40.0,
        "the 100 by 40 pill, scaled down by its spring: {pill:?}"
    );
    assert!(damage.len() > 1, "the spring went on drawing: {damage:?}");
    assert!(
        damage
            .iter()
            .all(|&(x, y, width, height)| x >= 0 && y >= 0 && x + width <= 100 && y + height <= 40),
        "damage past the 100 by 40 surface: {damage:?}"
    );
}

/// The same with nothing animating: the layout the show asks for and the resize
/// arrive in one frame, and the resize is not lost to it. Nothing repaints
/// after that frame, so the stale layout was only seen by the next thing that
/// did.
#[test]
fn a_layout_pending_in_the_frame_a_surface_is_resized_does_not_keep_the_old_size() {
    let Some(mut app) = headless() else { return };
    let (surface, pill, _) = a_pill_shown_as_the_surface_shrinks(&mut app, false);

    assert_eq!(app.root_size(surface), (100.0, 40.0));
    assert_eq!(pill, Rect::new(0.0, 0.0, 100.0, 40.0));
}

/// A surface whose logical size changes while its buffer stays the same is
/// laid out at the new size: 2560 by 32 at 1.5 and 3840 by 48 at 1 are one
/// 3840 by 48 buffer, and only one of them is 3840 wide.
#[test]
fn a_resize_that_keeps_the_buffer_still_lays_the_surface_out_again() {
    let Some(mut app) = headless() else { return };
    let surface = app.surface(fixed_bar(), || container().width(fill()).height(fill()));
    app.configure(surface, 2560, 32, 1.5);
    app.step();
    app.configure(surface, 3840, 48, 1.0);
    app.step();

    assert_eq!(app.physical_size(surface), (3840, 48), "the same buffer");
    assert_eq!(app.root_size(surface), (3840.0, 48.0));
}

/// A 40 square card holding a 40 square child whose shadow spreads 20, the
/// child's colour following `tint`, under a spacer `gap` tall in a padded
/// column; `moves` says how the card moves. The child is reported through
/// `child`.
fn a_card_whose_child_repaints(
    gap: RwSignal<f32>,
    tint: RwSignal<bool>,
    child: WidgetRef,
    moves: impl FnOnce(Container) -> Container,
) -> Container {
    let shadow = Shadow::new((0.0, 0.0), 0.0, 20.0, Color::WHITE);
    container()
        .width(fill())
        .height(fill())
        .padding(50.0)
        .layout(Flex::column())
        .child(container().width(40.0).height(gap))
        .child(
            moves(container().width(40.0).height(40.0)).child(
                container()
                    .width(40.0)
                    .height(40.0)
                    .shadow(shadow)
                    .background(move || {
                        if tint.get() {
                            Color::rgb(1.0, 0.0, 0.0)
                        } else {
                            Color::rgb(0.0, 0.0, 1.0)
                        }
                    })
                    .widget_ref(child),
            ),
        )
}

/// Frame by frame through a move and a second after it, the child repainting
/// in every frame: each frame's damage covers the child and its shadow where
/// that frame draws them (#830). While the card moves, its animation advances
/// before its Paint job republishes where it draws, so the child is marked
/// against the frame before, and the card's own damage covers it. Once the
/// card rests away from its place, nothing but the child repaints, and only
/// the child's own damage can name where it is drawn.
fn a_repainting_child_is_damaged_where_each_frame_draws_it(
    start: impl FnOnce(RwSignal<f32>, RwSignal<f32>),
    moves: impl FnOnce(Container, RwSignal<f32>) -> Container + 'static,
    case: &str,
) {
    let Some(mut app) = headless() else { return };
    let (gap, offset, tint) = (
        create_signal(0.0f32),
        create_signal(0.0f32),
        create_signal(false),
    );
    let child = create_widget_ref();
    let surface = app.surface(SurfaceConfig::new().width(400).height(400), move || {
        a_card_whose_child_repaints(gap, tint, child, |card| moves(card, offset))
    });
    app.configure(surface, 400, 400, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    start(gap, offset);
    let mut moved = 0;
    for frame in 0..60 {
        tint.set(frame % 2 == 0);
        let before = app.damage_reported(surface).len();
        at += Duration::from_millis(16);
        app.step_at(at);
        let drawn = child.rect().get_untracked();
        if drawn != child.layout_rect().get_untracked() {
            moved += 1;
        }
        let shadowed = (
            (drawn.x - 20.0).floor() as i32,
            (drawn.y - 20.0).floor() as i32,
            (drawn.x + drawn.width + 20.0).ceil() as i32,
            (drawn.y + drawn.height + 20.0).ceil() as i32,
        );
        let damage = &app.damage_reported(surface)[before..];
        assert!(!damage.is_empty(), "{case}, frame {frame}: nothing damaged");
        let covered = damage.iter().fold(
            (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            |(x0, y0, x1, y1), &(x, y, w, h)| (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)),
        );
        assert!(
            covered.0 <= shadowed.0
                && covered.1 <= shadowed.1
                && covered.2 >= shadowed.2
                && covered.3 >= shadowed.3,
            "{case}, frame {frame}: damaged {damage:?}, the child and its shadow \
             drawn at {shadowed:?}"
        );
    }
    assert!(
        moved > 3,
        "{case}: the setup: the child moved in {moved} frames"
    );
}

#[test]
fn a_child_repainting_in_a_sliding_card_is_damaged_where_each_frame_draws_it() {
    a_repainting_child_is_damaged_where_each_frame_draws_it(
        |gap, _| gap.set(200.0),
        |card, _| card.animate_position(Transition::new(160.0, TimingFunction::Linear)),
        "a slide",
    );
}

#[test]
fn a_child_repainting_in_a_card_translated_by_a_transition_is_damaged_where_each_frame_draws_it() {
    a_repainting_child_is_damaged_where_each_frame_draws_it(
        |_, offset| offset.set(200.0),
        |card, offset| {
            card.translate(
                (move || Translate::new(offset.get(), 0.0))
                    .transition(Transition::new(160.0, TimingFunction::Linear)),
            )
        },
        "a translate",
    );
}

#[test]
fn a_child_repainting_in_a_card_translated_by_a_spring_is_damaged_where_each_frame_draws_it() {
    a_repainting_child_is_damaged_where_each_frame_draws_it(
        |_, offset| offset.set(200.0),
        |card, offset| {
            card.translate(
                (move || Translate::new(offset.get(), 0.0))
                    .transition(Transition::spring(SpringConfig::BOUNCY)),
            )
        },
        "a spring",
    );
}

/// Frame by frame through a wheel, a flick and the glide after it, then a
/// second at rest, a row of a scrolled list repainting in every frame: each
/// frame's damage covers what of the row the viewport shows, where that frame
/// draws it (#833). While the list moves the scroller damages its viewport,
/// which covers the row however late its offset is published; once it rests,
/// nothing but the row repaints, and only the row's own damage, carried
/// through the scroll offset, can name where it is drawn.
#[test]
fn a_row_repainting_in_a_scrolled_list_is_damaged_where_each_frame_draws_it() {
    let Some(mut app) = headless() else { return };
    let tint = create_signal(false);
    let row = create_widget_ref();
    let surface =
        app.surface(SurfaceConfig::new().width(400).height(400), move || {
            container().padding(8.0).child(
                container()
                    .width(200.0)
                    .height(200.0)
                    .scroll(Scroll::vertical())
                    .child(container().layout(Flex::column()).children((0..10).map(
                        move |index| {
                            let rows = container().width(200.0).height(80.0);
                            if index != 3 {
                                return rows.background(Color::rgb(0.3, 0.3, 0.3));
                            }
                            rows.background(move || {
                                if tint.get() {
                                    Color::rgb(1.0, 0.0, 0.0)
                                } else {
                                    Color::rgb(0.0, 0.0, 1.0)
                                }
                            })
                            .widget_ref(row)
                        },
                    ))),
            )
        });
    app.configure(surface, 400, 400, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    let viewport = Rect::new(8.0, 8.0, 200.0, 200.0);
    let (mut scrolled_frames, mut resting_away) = (0, 0);
    let mut was = row.rect().get_untracked();
    for frame in 0..80 {
        tint.set(frame % 2 == 0);
        match frame {
            0..6 => app.event_at(
                surface,
                Event::scroll(100.0, 100.0, 0.0, 10.0, ScrollSource::Wheel),
                at,
            ),
            6..12 => app.event_at(
                surface,
                Event::scroll(100.0, 100.0, 0.0, 5.0, ScrollSource::Finger),
                at,
            ),
            12 => app.event_at(surface, Event::scroll_end(100.0, 100.0), at),
            _ => {}
        }
        let before = app.damage_reported(surface).len();
        at += Duration::from_millis(16);
        app.step_at(at);
        let drawn = row.rect().get_untracked();
        // Laid out at 8 + 3 · 80.
        if drawn.y != 248.0 {
            scrolled_frames += 1;
            if drawn == was {
                resting_away += 1;
            }
        }
        was = drawn;
        let Some(shown) = drawn.intersection(&viewport) else {
            continue;
        };
        let shown = (
            shown.x.floor() as i32,
            shown.y.floor() as i32,
            (shown.x + shown.width).ceil() as i32,
            (shown.y + shown.height).ceil() as i32,
        );
        let damage = &app.damage_reported(surface)[before..];
        assert!(!damage.is_empty(), "frame {frame}: nothing damaged");
        let covered = damage.iter().fold(
            (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            |(x0, y0, x1, y1), &(x, y, w, h)| (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)),
        );
        assert!(
            covered.0 <= shown.0
                && covered.1 <= shown.1
                && covered.2 >= shown.2
                && covered.3 >= shown.3,
            "frame {frame}: damaged {damage:?}, the row shown at {shown:?}"
        );
    }
    assert!(
        scrolled_frames > 40,
        "the setup: the row was drawn off its place in {scrolled_frames} frames"
    );
    assert!(
        resting_away > 10,
        "the setup: the list rested away from its start in {resting_away} \
         frames, the only ones where the row's own damage has to say where it is"
    );
}

/// A shape that repaints damages the ring its antialiased edge is drawn in,
/// past its box: up to a device pixel out, the furthest the shader's fade
/// reaches (#836). Short of it, the compositor keeps that ring from the frame
/// before, and a box that changed colour keeps a fringe of the old one.
#[test]
fn a_repainting_shape_damages_the_fringe_of_its_edge() {
    for scale in [1.0f32, 2.0] {
        let Some(mut app) = headless() else { return };
        let tint = create_signal(false);
        let surface = app.surface(SurfaceConfig::new().width(200).height(200), move || {
            container()
                .width(fill())
                .height(fill())
                .padding(Padding::all(50.0).with_left(50.3).with_top(50.6))
                .child(
                    container()
                        .width(40.0)
                        .height(40.0)
                        .corners(8.0)
                        .background(move || {
                            if tint.get() {
                                Color::rgb(1.0, 0.0, 0.0)
                            } else {
                                Color::rgb(0.0, 0.0, 1.0)
                            }
                        }),
                )
        });
        app.configure(surface, 200, 200, scale);
        app.step();

        let before = app.damage_reported(surface).len();
        tint.set(true);
        app.step();
        let damage = &app.damage_reported(surface)[before..];
        assert!(!damage.is_empty(), "at {scale}x: nothing damaged");
        let covered = damage.iter().fold(
            (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
            |(x0, y0, x1, y1), &(x, y, w, h)| (x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)),
        );
        let fringe = (
            (50.3 * scale - 1.0).floor() as i32,
            (50.6 * scale - 1.0).floor() as i32,
            (90.3 * scale + 1.0).ceil() as i32,
            (90.6 * scale + 1.0).ceil() as i32,
        );
        assert!(
            covered.0 <= fringe.0
                && covered.1 <= fringe.1
                && covered.2 >= fringe.2
                && covered.3 >= fringe.3,
            "at {scale}x: damaged {damage:?}, the box and its fringe drawn over {fringe:?}"
        );
    }
}
