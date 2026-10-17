#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches `benches/slide_list`.
//!
//! The benchmark says how many buffer pixels a frame of a slide damages, and
//! it is only worth pointing at two revisions if one revision gives the same
//! counts twice — the damaged pixels included, which a timer and a transition
//! drive. It is also the only benchmark whose damage is not a scroller's
//! viewport, so nothing else says `damage.pixels` measures anything smaller.
//!
//! The file compiles the benchmark's own modules rather than a copy of them, as
//! the other repeatability tests do.

mod common;
#[path = "../benches/slide_list/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
// Its scroll gesture is the one part this does not play: nothing here scrolls.
#[allow(dead_code)]
mod scripted;

const CARDS: usize = 8;

/// Three turns of the column.
const FRAMES: usize = 60;

#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

fn play() -> Option<scripted::Run> {
    let _alone = common::one_play_at_a_time();
    let mut app = common::headless()?;
    Some(scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(CARDS),
        &[0.0; FRAMES],
    ))
}

/// The property the benchmark is for.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    let Some(first) = play() else {
        return;
    };
    let second = play().expect("the first run had an adapter");

    assert_eq!(
        first.counts, second.counts,
        "the same script over the same cards did two different amounts of work"
    );
    assert_eq!(
        common::counts_table(&scripted::report(&first, CARDS)),
        common::counts_table(&scripted::report(&second, CARDS)),
        "a column under the header that promises to repeat did not"
    );
}

/// A column that never turned, or a slide whose frames damaged the whole
/// surface, would repeat perfectly and measure nothing: the slides paint, each
/// painted frame names some damage, and none of it is the whole surface.
#[test]
fn the_slides_paint_and_damage_less_than_the_surface() {
    let Some(run) = play() else {
        return;
    };

    let painted = run.counts.frames_painted;
    let surface = u64::from(scene::VIEWPORT.0 * scene::VIEWPORT.1);
    assert!(
        painted > 0,
        "the timer turned the column and nothing painted"
    );
    assert_eq!(
        run.counts.damage_full, 0,
        "a slide damaged the whole surface"
    );
    assert!(
        run.counts.damage_pixels > 0,
        "{painted} painted frames named no damage"
    );
    assert!(
        run.counts.damage_pixels < painted * surface,
        "{} pixels over {painted} frames is the whole surface every frame",
        run.counts.damage_pixels
    );
}
