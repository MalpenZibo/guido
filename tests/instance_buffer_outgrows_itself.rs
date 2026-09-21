#![cfg(feature = "testing")]

//! More shapes than the instance buffer was made for.
//!
//! `Renderer::ensure_instance_capacity` grows the buffer when a frame carries
//! more instances than it holds — and since #398 it must rebuild the bind
//! group too, because that is what now names the buffer. Under the vertex
//! attribute layout it did not have to: a vertex buffer was named at draw
//! time, so replacing it could not leave a stale binding behind. A bind group
//! holds the buffer it was made from, so forgetting to rebuild one leaves the
//! shader reading the *old, smaller* buffer, and every instance past its end
//! reads whatever the clamp gives back.
//!
//! Nothing was exercising that. The buffer starts at 256 instances and no test
//! in the suite had ever drawn more, so `ensure_instance_capacity` could be
//! replaced with `()` — its whole body deleted — and the suite stayed green.
//! CI's mutation run is what said so.
//!
//! So: draw well past 256, and check the ones at the end are on screen. A
//! frame that overflows the buffer and does not grow it fails wgpu's own
//! validation on the write; one that grows it without rebuilding the bind
//! group draws the late shapes wrong.

mod common;

use guido::prelude::*;

/// Comfortably past the 256 the buffer starts at, and past the 512 one
/// doubling gives, so the growth runs more than once.
const SHAPES: usize = 600;

const CELL: f32 = 8.0;
const ACROSS: usize = 30;

/// A grid of small squares, the last of them a colour nothing else uses.
///
/// The marker is what says the *late* instances arrived: a stale binding
/// leaves the shapes past the old capacity reading another instance's data,
/// and the one thing no other instance can supply is this colour.
fn grid() -> Container {
    let mut rows = container().layout(Flex::column().spacing(0.0));
    for r in 0..SHAPES.div_ceil(ACROSS) {
        let mut row = container().layout(Flex::row().spacing(0.0));
        for c in 0..ACROSS {
            let i = r * ACROSS + c;
            if i >= SHAPES {
                break;
            }
            let last = i == SHAPES - 1;
            row = row.child(container().width(CELL).height(CELL).background(if last {
                Color::rgb(1.0, 0.0, 1.0)
            } else {
                Color::rgb(0.0, 0.3, 0.6)
            }));
        }
        rows = rows.child(row);
    }
    rows
}

#[test]
fn the_shapes_past_the_buffers_first_size_are_still_drawn() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let width = (ACROSS as f32 * CELL) as u32;
    let height = (SHAPES.div_ceil(ACROSS) as f32 * CELL) as u32;

    let config = SurfaceConfig::new()
        .height(height)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    let surface = app.surface(config, || {
        container().width(fill()).height(fill()).child(grid())
    });
    app.configure(surface, width, height, 1.0);
    app.step();

    // The middle of the very last cell.
    let last = SHAPES - 1;
    let x = ((last % ACROSS) as f32 * CELL + CELL / 2.0) as u32;
    let y = ((last / ACROSS) as f32 * CELL + CELL / 2.0) as u32;

    let [r, g, b, _] = app.read_pixel(surface, x, y);
    assert!(
        r > 200 && g < 60 && b > 200,
        "instance {last} of {SHAPES} is the magenta cell at ({x}, {y}) and it \
         reads as ({r}, {g}, {b}) — the instance buffer grew past its first \
         size and something is still reading the old one"
    );

    // And an early cell is untouched, so the test is not passing on a frame
    // that drew nothing but the marker.
    let [er, eg, eb, _] = app.read_pixel(surface, (CELL / 2.0) as u32, (CELL / 2.0) as u32);
    assert!(
        er < 60 && eg > 40 && eb > 100,
        "the first cell should still be the ordinary blue, got ({er}, {eg}, {eb})"
    );
}
