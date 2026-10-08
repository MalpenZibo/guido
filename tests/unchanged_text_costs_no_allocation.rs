//! What a painted frame allocates for a label that did not change.
//!
//! A background behind a column of labels changes colour every frame, and
//! nothing about the labels does. Their buffers are cached across frames, so
//! nothing about them should have to be made again — yet each one used to cost
//! a few allocations every frame anyway: a copy of its string from the draw
//! command into the renderer's entry, and a fresh `Vec` to file its shaped
//! buffer under for the next frame to find.
//!
//! The figure asserted is the *difference* between two columns, one of
//! `FEW` labels and one of `MANY`, divided by the labels between them: what one
//! more unchanged label costs a frame. A fixed cost per frame — the driver's,
//! the shapes', the frame's own scratch — is in both columns and cancels.
//!
//! **No harness**, for the reason `frame_allocations_are_counted.rs` gives:
//! the counter is the process's, and libtest's own thread allocates into it.

mod common;

use guido::prelude::*;
use guido::render_stats;

#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

const FEW: usize = 8;
const MANY: usize = 40;

/// Frames measured per column, after the ones that warm every cache up.
const FRAMES: usize = 8;
const WARM_UP: usize = 4;

/// Allocations over `FRAMES` painted frames of a column of `labels` labels,
/// with only the colour behind them changing; `None` without an adapter.
fn allocations_of_a_column(labels: usize) -> Option<u64> {
    let mut app = common::headless()?;
    let backdrop = create_signal(Color::rgb(0.1, 0.1, 0.1));
    let surface = app.surface(
        SurfaceConfig::new()
            .height(800)
            .anchor(Anchor::TOP | Anchor::LEFT),
        move || {
            container()
                .layout(Flex::column())
                .background(backdrop)
                .children((0..labels).map(|i| text(format!("label number {i}")).font_size(12.0)))
        },
    );
    app.configure(surface, 200, 800, 1.0);

    let mut shade = 0.0;
    let mut frame = |app: &mut guido::testing::Headless| {
        shade = (shade + 0.01) % 1.0;
        backdrop.set(Color::rgb(shade, 0.1, 0.1));
        app.step();
    };
    for _ in 0..WARM_UP {
        frame(&mut app);
    }

    let mut total = 0;
    for _ in 0..FRAMES {
        render_stats::reset_stats();
        frame(&mut app);
        let stats = render_stats::get_stats();
        assert_eq!(
            stats.frames_painted, 1,
            "the frame under test was not painted"
        );
        assert!(stats.draw_calls.text > 0, "the frame drew no text");
        total += stats.allocations;
    }
    Some(total)
}

fn main() {
    let Some(few) = allocations_of_a_column(FEW) else {
        return;
    };
    let many = allocations_of_a_column(MANY).expect("the first column had an adapter");

    let per_label = many.saturating_sub(few) as f64 / ((MANY - FEW) * FRAMES) as f64;
    println!(
        "{few} allocations over {FRAMES} frames of {FEW} labels, {many} of {MANY}: \
         {per_label:.2} per unchanged label per frame"
    );
    // Not zero exactly: the counter is the process's, and a driver thread that
    // allocates during one column's frames and not the other's is in the
    // difference. Before the labels stopped costing anything, this was two
    // and more.
    assert!(
        per_label < 0.25,
        "an unchanged label costs {per_label:.2} allocations a frame ({few} for {FEW} labels, \
         {many} for {MANY}, over {FRAMES} frames)"
    );
}
