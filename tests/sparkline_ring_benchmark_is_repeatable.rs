#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches `benches/sparkline_ring`.
//!
//! The benchmark says what rings and charts that keep moving cost a frame, and
//! it is only worth pointing at two revisions if one revision gives the same
//! counts twice — the tessellations included, which a timer and an easing
//! drive. It is also the only benchmark that draws paths at all, so nothing
//! else says the path columns count anything.
//!
//! The file compiles the benchmark's own modules rather than a copy of them, as
//! the other repeatability tests do.

mod common;
#[path = "../benches/sparkline_ring/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

/// Enough rows that the gesture stays inside them — it travels 3744 pixels at
/// six frames a phase, 86 rows — and few enough that several plays fit inside
/// a test run.
const ROWS: usize = 110;

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

/// The property the benchmark is for.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    let Some(first) = play(ROWS) else {
        return;
    };
    let second = play(ROWS).expect("the first run had an adapter");

    assert_eq!(
        first.counts, second.counts,
        "the same script over the same rows did two different amounts of work"
    );
    assert_eq!(
        common::counts_table(&scripted::report(&first, ROWS)),
        common::counts_table(&scripted::report(&second, ROWS)),
        "a column under the header that promises to repeat did not"
    );
}

/// Rows that drew no path, or charts that never changed, would repeat
/// perfectly and measure nothing: every painted frame draws paths, the timer's
/// readings are tessellated, a chart that only scrolled is drawn from the
/// cache — more often than one is tessellated, since a reading arrives every
/// fourth frame — and the gesture never runs off the end of the rows.
#[test]
fn every_painted_frame_draws_paths_and_most_are_drawn_again() {
    let Some(run) = play(ROWS) else {
        return;
    };

    let painted = run.counts.frames_painted;
    let meshes = run.counts.path_meshes;
    assert!(painted > 0, "the script painted nothing");
    assert!(
        run.counts.draw_calls.paths >= painted,
        "{} path draw calls over {painted} painted frames: a frame drew none",
        run.counts.draw_calls.paths
    );
    assert!(
        run.counts.draw_calls.shapes >= painted,
        "the rings are shapes, and a frame drew none"
    );
    assert!(meshes.tessellated > 0, "no reading was ever tessellated");
    // The charts' own tessellations: the outline's, one a painted frame, are
    // not a path that only moved.
    let charts = meshes.tessellated - painted;
    assert!(
        meshes.reused > charts,
        "{} meshes drawn again against {charts} tessellated for the charts: a \
         path that only moved was tessellated again",
        meshes.reused,
    );
    assert_eq!(
        run.counts.frames_pinned, 0,
        "the gesture ran off the end of the rows"
    );
}
