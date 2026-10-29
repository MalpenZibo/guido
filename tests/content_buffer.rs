#![cfg(feature = "testing")]
//! A surface the compositor stretches over the screen, drawing only what it
//! shows (#911).
//!
//! The surface is as large as the compositor allows; its buffer is as large as
//! what is drawn, and placed inside it. What is asserted is what the surface
//! asks for — where its buffer goes and how big it is — and what lands in it.
//! What a compositor makes of a subsurface is the hand check.

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;

mod common;
use common::headless;

const RED: Color = Color::rgb(1.0, 0.0, 0.0);
const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);

/// A bar on the bottom edge of a surface the compositor stretches over the
/// whole screen, transparent wherever it draws nothing.
fn stretched_bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
        .exclusive_zone(20)
        .exclusive_edge(Edge::Bottom)
        .background_color(Color::TRANSPARENT)
        .buffer(BufferExtent::Content)
}

/// A red bar 20 tall at the bottom of the surface, and above it a blue panel
/// that eases from nothing to 100 tall over 100 ms while `open` holds.
fn bar_and_panel(open: RwSignal<bool>) -> impl Fn() -> Container + 'static {
    move || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column().main_alignment(MainAlignment::End))
            .child(
                container().width(fill()).background(BLUE).height(
                    (move || if open.get() { 100.0 } else { 0.0 })
                        .transition(Transition::new(100.0, TimingFunction::Linear)),
                ),
            )
            .child(container().width(fill()).height(20.0).background(RED))
    }
}

fn opened_at_rest(open: bool) -> Option<(Headless, SurfaceId, RwSignal<bool>)> {
    let mut app = headless()?;
    let open = create_signal(open);
    let bar = app.surface(stretched_bar(), bar_and_panel(open));
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();
    Some((app, bar, open))
}

/// The buffer is the bar, at the bottom of the surface, and the bar is drawn
/// into it from its first row.
#[test]
fn a_content_buffer_covers_only_what_is_drawn() {
    let Some((app, bar, _)) = opened_at_rest(false) else {
        return;
    };

    assert_eq!(app.physical_size(bar), (400, 20));
    assert_eq!(app.buffer_origins(bar).last(), Some(&(0, 280)));
    assert_eq!(app.read_pixel(bar, 10, 0), [255, 0, 0, 255]);
    assert_eq!(app.read_pixel(bar, 10, 19), [255, 0, 0, 255]);
}

/// The layer surface takes the space and the compositor sizes it: the content
/// never asks for a size.
#[test]
fn the_surface_is_never_resized_by_its_content() {
    let Some((mut app, bar, open)) = opened_at_rest(false) else {
        return;
    };
    let asked = app.sizes_asked(bar).len();
    open.set(true);
    let t0 = Instant::now();
    for ms in [0, 50, 100, 150] {
        app.step_at(t0 + Duration::from_millis(ms));
    }

    assert_eq!(app.sizes_asked(bar)[asked..], []);
    assert_eq!(app.root_size(bar), (400.0, 300.0));
}

/// An animated growth is covered from its first frame: the buffer takes the
/// size and place the panel ends at at once, and keeps them while it plays,
/// so nothing is resized frame by frame and nothing is cut.
#[test]
fn a_growth_is_covered_from_its_first_frame() {
    let Some((mut app, bar, open)) = opened_at_rest(false) else {
        return;
    };
    let placed = app.buffer_origins(bar).len();
    open.set(true);
    let t0 = Instant::now();
    for ms in [0, 20, 50, 80] {
        app.step_at(t0 + Duration::from_millis(ms));
        assert_eq!(
            app.physical_size(bar),
            (400, 120),
            "{ms} ms into a 100 ms growth, the buffer is not the size it ends at"
        );
    }

    assert_eq!(app.buffer_origins(bar)[placed..], [(0, 180)]);
    // Halfway, the panel is 50 tall: the top of the buffer is still empty,
    // and the bar is where it always was, at the bottom.
    assert_eq!(app.read_pixel(bar, 10, 10)[3], 0);
    assert_eq!(app.read_pixel(bar, 10, 110), [255, 0, 0, 255]);
}

/// A shrink waits until the content has settled, as a `content()` size does:
/// the panel on its way down is drawn whole, and the buffer goes back to the
/// bar once, when it gets there.
#[test]
fn a_shrink_waits_until_the_content_has_settled() {
    let Some((mut app, bar, open)) = opened_at_rest(true) else {
        return;
    };
    assert_eq!(app.physical_size(bar), (400, 120));
    let placed = app.buffer_origins(bar).len();
    open.set(false);
    let t0 = Instant::now();
    for ms in [0, 20, 50, 80] {
        app.step_at(t0 + Duration::from_millis(ms));
        assert_eq!(
            app.physical_size(bar),
            (400, 120),
            "{ms} ms into a 100 ms shrink, the buffer already shrank"
        );
    }
    for ms in [100, 120, 140] {
        app.step_at(t0 + Duration::from_millis(ms));
    }

    assert_eq!(app.physical_size(bar), (400, 20));
    assert_eq!(app.buffer_origins(bar)[placed..], [(0, 280)]);
}

/// Input is the surface's: a region declared over the bar is published where
/// the bar is on the surface, not where it is in the buffer.
#[test]
fn the_input_region_is_in_the_surfaces_coordinates() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(stretched_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .takes_input(false)
            .layout(Flex::column().main_alignment(MainAlignment::End))
            .child(
                container()
                    .width(fill())
                    .height(20.0)
                    .background(RED)
                    .takes_input(true),
            )
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    assert!(app.input_reaches(bar, 10.0, 290.0));
    assert!(!app.input_reaches(bar, 10.0, 10.0));
}

/// A compositor without subsurfaces gets the whole surface, drawn as it would
/// be with the declaration left out.
#[test]
fn without_subsurfaces_the_whole_surface_is_drawn() {
    let Some(mut app) = headless() else { return };
    app.without_subsurfaces();
    let open = create_signal(false);
    let bar = app.surface(stretched_bar(), bar_and_panel(open));
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    assert_eq!(app.physical_size(bar), (400, 300));
    assert_eq!(app.buffer_origins(bar), []);
    assert_eq!(app.read_pixel(bar, 10, 290), [255, 0, 0, 255]);
}

/// The colour at `(x, y)` in the surface's coordinates, read out of its buffer
/// wherever that was placed.
fn surface_pixel(app: &Headless, id: SurfaceId, x: u32, y: u32) -> [u8; 4] {
    let (bx, by) = app.buffer_origins(id).last().copied().unwrap_or((0, 0));
    let (width, height) = app.physical_size(id);
    let (x, y) = (x as i32 - bx, y as i32 - by);
    assert!(
        (0..width as i32).contains(&x) && (0..height as i32).contains(&y),
        "({x}, {y}) is outside a {width}x{height} buffer placed at ({bx}, {by})"
    );
    app.read_pixel(id, x as u32, y as u32)
}

/// A shadow is part of what is drawn: the buffer reaches as far as it does,
/// rather than cutting it at the bar's edge.
#[test]
fn a_shadow_is_drawn_whole() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(stretched_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column().main_alignment(MainAlignment::End))
            .child(
                container()
                    .width(fill())
                    .height(20.0)
                    .background(RED)
                    .shadow(Shadow::new((0.0, 0.0), 10.0, 0.0, BLUE)),
            )
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    let (_, top) = app.buffer_origins(bar).last().copied().unwrap();
    assert!(
        top <= 270,
        "the shadow 10 above the bar is cut: buffer at {top}"
    );
    assert!(
        surface_pixel(&app, bar, 200, 277)[3] > 0,
        "nothing drawn above the bar"
    );
}

/// A panel above the bar, on a box that draws nothing itself and moves it by
/// `translate`.
fn bar_and_moved_panel<M>(moved: impl IntoAnimated<Translate, M>) -> Container {
    container()
        .width(fill())
        .height(fill())
        .layout(Flex::column().main_alignment(MainAlignment::End))
        .child(
            container()
                .width(fill())
                .translate(moved)
                .child(container().width(fill()).height(100.0).background(BLUE)),
        )
        .child(container().width(fill()).height(20.0).background(RED))
}

/// A transform moves no layout: what a box that draws nothing moves is its
/// children, and the buffer goes where they are drawn rather than where they
/// were laid out.
#[test]
fn a_moved_child_is_drawn_where_it_is_moved_to() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(stretched_bar(), || {
        bar_and_moved_panel(|| Translate::new(0.0, -180.0))
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    // Laid out at 180, drawn 180 above, at the top of the surface.
    assert_eq!(surface_pixel(&app, bar, 200, 5), [0, 0, 255, 255]);
}

/// A panel sliding up from below the screen into place, as a translate eased
/// over 100 ms, is drawn whole on its way.
#[test]
fn a_panel_sliding_in_is_drawn_whole() {
    let Some(mut app) = headless() else { return };
    let open = create_signal(false);
    let bar = app.surface(stretched_bar(), move || {
        bar_and_moved_panel(
            (move || Translate::new(0.0, if open.get() { 0.0 } else { 200.0 }))
                .transition(Transition::new(100.0, TimingFunction::Linear)),
        )
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();
    open.set(true);
    let t0 = Instant::now();
    for ms in [0, 25, 50, 75] {
        app.step_at(t0 + Duration::from_millis(ms));
    }

    // Three quarters in, 50 short of its place: its top edge is at 230.
    assert_eq!(surface_pixel(&app, bar, 200, 235), [0, 0, 255, 255]);
    for ms in [100, 120] {
        app.step_at(t0 + Duration::from_millis(ms));
    }
    assert_eq!(surface_pixel(&app, bar, 200, 185), [0, 0, 255, 255]);
}

/// A backdrop the size of the surface that only takes the click closing the
/// panel draws nothing: the buffer is still the bar, and the backdrop takes the
/// click anywhere on the surface.
#[test]
fn a_backdrop_that_only_takes_clicks_draws_nothing() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(stretched_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column())
            .child(container().width(fill()).height(fill()).takes_input(true))
            .child(container().width(fill()).height(20.0).background(RED))
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    assert_eq!(app.physical_size(bar), (400, 20));
    assert!(app.input_reaches(bar, 10.0, 10.0));
}

/// A surface over the whole screen that blurs nothing says so: a compositor
/// rule blurring the layer surfaces that declare no region of their own would
/// otherwise blur the screen behind it (niri's `layer-rule` does).
#[test]
fn a_surface_that_blurs_nothing_declares_an_empty_blur_region() {
    let Some((app, bar, _)) = opened_at_rest(false) else {
        return;
    };

    assert_eq!(app.blur_regions_asked(bar).last(), Some(&vec![]));
}

/// A transform changed after the content has settled moves no layout, and the
/// buffer follows it all the same: the frame says what it drew.
#[test]
fn a_transform_that_changes_alone_is_followed() {
    let Some(mut app) = headless() else { return };
    let lifted = create_signal(false);
    let bar = app.surface(stretched_bar(), move || {
        bar_and_moved_panel(move || Translate::new(0.0, if lifted.get() { -180.0 } else { 0.0 }))
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();
    assert_eq!(app.buffer_origins(bar).last(), Some(&(0, 180)));

    lifted.set(true);
    app.step();

    assert_eq!(surface_pixel(&app, bar, 200, 5), [0, 0, 255, 255]);
}

/// A backdrop that starts to draw — dimming the screen behind an open panel —
/// changes a colour and no layout, and is drawn over the whole surface.
#[test]
fn a_backdrop_that_starts_to_draw_is_drawn_whole() {
    let Some(mut app) = headless() else { return };
    let dim = create_signal(false);
    let bar = app.surface(stretched_bar(), move || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column())
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .background(move || if dim.get() { BLUE } else { Color::TRANSPARENT }),
            )
            .child(container().width(fill()).height(20.0).background(RED))
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();
    assert_eq!(app.physical_size(bar), (400, 20));

    dim.set(true);
    app.step();

    assert_eq!(app.physical_size(bar), (400, 300));
    assert_eq!(surface_pixel(&app, bar, 200, 5), [0, 0, 255, 255]);
}

/// At scale 2 the buffer is the bar's physical size, placed in logical
/// coordinates, and the bar fills it from its first row.
#[test]
fn a_content_buffer_at_scale_two_is_placed_in_logical_pixels() {
    let Some(mut app) = headless() else { return };
    let open = create_signal(false);
    let bar = app.surface(stretched_bar(), bar_and_panel(open));
    app.configure(bar, 400, 300, 2.0);
    app.step();
    app.step();

    assert_eq!(app.physical_size(bar), (800, 40));
    assert_eq!(app.buffer_origins(bar).last(), Some(&(0, 280)));
    assert_eq!(app.read_pixel(bar, 10, 0), [255, 0, 0, 255]);
    assert_eq!(app.read_pixel(bar, 10, 39), [255, 0, 0, 255]);
}

/// A subtree faded as one picture — two draws on the same pixels, so it is
/// drawn into a texture and composited once — lands where the subtree is, in a
/// buffer that has been moved: half a red, not the three quarters two draws
/// faded one by one would make.
#[test]
fn a_layer_lands_where_it_is_in_a_moved_buffer() {
    let Some(mut app) = headless() else { return };
    let bar = app.surface(stretched_bar(), || {
        container()
            .width(fill())
            .height(fill())
            .layout(Flex::column().main_alignment(MainAlignment::End))
            .child(
                container()
                    .width(fill())
                    .height(20.0)
                    .background(RED)
                    .opacity(0.5)
                    .child(container().width(fill()).height(20.0).background(RED)),
            )
    });
    app.configure(bar, 400, 300, 1.0);
    app.step();
    app.step();

    // Moved to the bar, give or take the transparent edge the texture keeps.
    let (_, top) = app.buffer_origins(bar).last().copied().unwrap();
    assert!((270..=280).contains(&top), "the buffer is at {top}");
    let [r, g, b, a] = surface_pixel(&app, bar, 200, 290);
    assert!(
        (120..=136).contains(&a) && r > 100 && g == 0 && b == 0,
        "half a red bar, composited at the bottom of the surface: {:?}",
        [r, g, b, a]
    );
}
