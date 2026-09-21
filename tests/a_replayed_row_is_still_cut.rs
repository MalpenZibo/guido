//! The clip, in pixels, on the frame after the flatten cache replayed a row.
//!
//! `tests/paint_cache_across_frames.rs` watches the same thing in draw
//! commands, which is where a mistake would be made — but not where it would be
//! seen. Nothing else can watch it: a golden renders one frame from a tree
//! built for it, so no golden has ever replayed anything, and the snapshots are
//! the render tree rather than the commands the clip lives in.
//!
//! So this drives the real loop with a recorder where the compositor is,
//! scrolls it, and reads the surface back. A row is replayed rather than
//! re-flattened — the counter says so, when it is compiled in — and the pixel
//! where the scrolled row *would* be if its viewport had travelled with it is
//! still the background. That is the failure mode this whole area exists to
//! prevent: a clip that stops cutting, and a list that paints straight through
//! its scroller.

#![cfg(feature = "testing")]

mod common;

use guido::prelude::*;
use guido::surface::SurfaceId;
use guido::testing::Headless;
use std::time::{Duration, Instant};

const WIDTH: u32 = 100;
const HEIGHT: u32 = 200;
/// The gap above the scroller, which is the part of the surface the rows must
/// never reach.
const ABOVE: f32 = 50.0;
const VIEWPORT: f32 = 100.0;
const ROW: f32 = 40.0;
const ROWS: usize = 5;
/// Less than a row, so the first row is still under the viewport afterwards
/// and its top edge has crossed out of it.
const SCROLL: f32 = 20.0;

/// A strip of empty surface, then a scroller over rows taller than it.
fn scroller_below_a_gap() -> Container {
    container()
        .layout(guido::layout::Flex::column())
        .child(container().width(fill()).height(ABOVE))
        .child(
            container()
                .width(fill())
                .height(VIEWPORT)
                .scroll(Scroll::vertical())
                .child(container().layout(guido::layout::Flex::column()).children(
                    (0..ROWS).map(|_| container().width(fill()).height(ROW).background(Color::RED)),
                )),
        )
}

/// Whether what was drawn at `y` down the middle of the surface is a row.
fn shows_a_row(app: &Headless, surface: SurfaceId, y: u32) -> bool {
    let [r, g, b, _] = app.read_pixel(surface, WIDTH / 2, y);
    r > 128 && g < 100 && b < 100
}

#[test]
fn a_replayed_row_is_still_cut_by_the_scroller_it_left() {
    let Some(mut app) = common::headless() else {
        return;
    };

    let surface = app.surface(
        SurfaceConfig::new()
            .width(WIDTH)
            .height(HEIGHT)
            .background_color(Color::BLACK),
        scroller_below_a_gap,
    );
    app.configure(surface, WIDTH, HEIGHT, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    assert!(
        shows_a_row(&app, surface, ABOVE as u32 + 10),
        "the rows are not being drawn inside the scroller at all, so nothing \
         below this says anything"
    );
    assert!(
        !shows_a_row(&app, surface, ABOVE as u32 - 10),
        "a row was drawn above the scroller before anything scrolled"
    );

    guido::render_stats::reset_stats();
    at += Duration::from_millis(16);
    app.event_at(
        surface,
        Event::scroll(
            WIDTH as f32 / 2.0,
            ABOVE + VIEWPORT / 2.0,
            0.0,
            SCROLL,
            ScrollSource::Wheel,
        ),
        at,
    );
    app.step_at(at);

    // The rows have moved up by less than their height, so the first of them
    // now reaches `SCROLL` pixels above the viewport — and is cut there.
    assert!(
        shows_a_row(&app, surface, ABOVE as u32 + 10),
        "the scroller stopped drawing its rows"
    );
    assert!(
        !shows_a_row(&app, surface, (ABOVE - SCROLL / 2.0) as u32),
        "the scrolled row was drawn above the scroller: its clip travelled \
         with it instead of staying on the viewport, so it cuts nothing"
    );

    #[cfg(feature = "render-stats")]
    assert!(
        guido::render_stats::get_stats().flatten_nodes_cached > 0,
        "nothing was replayed on the scrolled frame, so the pixels above say \
         only that a full flatten cuts correctly — which was never in doubt"
    );
}
