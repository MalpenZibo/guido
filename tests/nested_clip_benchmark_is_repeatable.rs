#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches `benches/nested_clip`.
//!
//! The benchmark exists to say what nested rounded clips cost a frame, and it
//! is only worth pointing at two revisions if one revision gives the same
//! counts twice. It is also the only benchmark that draws text quads and images
//! under two clips at once, so nothing else says those columns repeat there, or
//! that the cards reach every pipeline in the first place.
//!
//! The file compiles the benchmark's own modules rather than a copy of them, as
//! the other repeatability tests do.

mod common;
#[path = "../benches/nested_clip/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

/// Enough rows that the gesture stays inside the grid, and few enough that
/// several plays fit inside a test run.
const ROWS: usize = 40;

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
        "the same script over the same cards did two different amounts of work"
    );
    assert_eq!(
        common::counts_table(&scripted::report(&first, ROWS)),
        common::counts_table(&scripted::report(&second, ROWS)),
        "a column under the header that promises to repeat did not"
    );
}

/// Cards that reached only some of the pipelines would repeat perfectly and
/// measure only a part of what nesting costs: every painted frame draws shapes,
/// images, glyphon text and transformed-text quads under the nested clips, and
/// the gesture never runs off the end of the grid.
#[test]
fn every_painted_frame_reaches_every_pipeline_and_the_gesture_stays_inside_the_grid() {
    let Some(run) = play(ROWS) else {
        return;
    };

    let painted = run.counts.frames_painted;
    let calls = &run.counts.draw_calls;
    assert!(painted > 0, "the script painted nothing");
    for (pipeline, count) in [
        ("shape", calls.shapes),
        ("image", calls.images),
        ("text", calls.text),
        ("text quad", calls.text_quads),
    ] {
        assert!(
            count >= painted,
            "{count} {pipeline} draw calls over {painted} painted frames: a frame drew none"
        );
    }
    assert_eq!(
        run.counts.frames_pinned, 0,
        "the gesture ran off the end of the grid"
    );
}
