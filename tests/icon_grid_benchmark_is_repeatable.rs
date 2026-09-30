#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches `benches/icon_grid`.
//!
//! The benchmark exists to say what a screen of small images costs a frame —
//! the draw calls, the GPU objects made to issue them, and the phases around
//! them — and it is only worth pointing at two revisions if one revision gives
//! the same counts twice. The other two benchmarks paint no images at all, so
//! nothing else says the image columns repeat, or that the grid reaches the
//! image pipeline in the first place.
//!
//! The file compiles the benchmark's own modules rather than a copy of them, as
//! the other two repeatability tests do.

mod common;
#[path = "../benches/icon_grid/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

/// Enough rows that the gesture stays inside the grid, and few enough that
/// several plays fit inside a test run.
const ROWS: usize = 200;

const FRAMES_PER_PHASE: usize = 6;

#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

fn play(rows: usize) -> Option<scripted::Run> {
    let _alone = common::one_play_at_a_time();
    let mut app = common::headless()?;
    let script = scripted::script(FRAMES_PER_PHASE);
    Some(scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(rows),
        &script,
    ))
}

/// The property the benchmark is for, over the columns only it exercises.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    let Some(first) = play(ROWS) else {
        return;
    };
    let second = play(ROWS).expect("the first run had an adapter");

    assert_eq!(
        first.counts, second.counts,
        "the same script over the same grid did two different amounts of work"
    );
    assert_eq!(
        counts_table(&scripted::report(&first, ROWS)),
        counts_table(&scripted::report(&second, ROWS)),
        "a column under the header that promises to repeat did not"
    );
}

/// A grid whose icons never reached the image pipeline would repeat perfectly
/// and measure nothing: every painted frame draws images, and the gesture never
/// runs off the end of the grid.
#[test]
fn every_painted_frame_draws_icons_and_the_gesture_stays_inside_the_grid() {
    let Some(run) = play(ROWS) else {
        return;
    };

    assert!(run.counts.frames_painted > 0, "the script painted nothing");
    assert!(
        run.counts.draw_calls.images >= run.counts.frames_painted,
        "{} image draw calls over {} painted frames: a frame drew no icon",
        run.counts.draw_calls.images,
        run.counts.frames_painted
    );
    assert_eq!(
        run.counts.frames_pinned, 0,
        "the gesture ran off the end of the grid"
    );
}

/// The lines of the report between the counts header and the phases after it.
fn counts_table(report: &str) -> Vec<&str> {
    report
        .lines()
        .skip_while(|line| !line.starts_with("counts ("))
        .take_while(|line| !line.starts_with("cpu phases"))
        .collect()
}
