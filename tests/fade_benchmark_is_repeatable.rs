#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches `benches/fade`.
//!
//! The benchmark exists to say what compositing a subtree as a layer costs a
//! frame, against the per-draw fade it replaces, and it is only worth pointing
//! at two revisions if one revision gives the same counts twice. It is also
//! the only benchmark that draws a layer at all, so nothing else says the
//! `layer.*` rows repeat, or that each scenario does the work its name says.
//!
//! The file compiles the benchmark's own modules rather than a copy of them, as
//! the other repeatability tests do.

mod common;
#[path = "../benches/fade/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

use scene::Fade;

/// A few cards: enough that a frame composites several layers, and few enough
/// that several plays fit inside a test run.
const CARDS: usize = 4;

const FRAMES_PER_PHASE: usize = 6;

#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

fn play(fade: Fade) -> Option<scripted::Run> {
    let _alone = common::one_play_at_a_time();
    let mut app = common::headless()?;
    let script = scripted::script(FRAMES_PER_PHASE);
    Some(scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(CARDS, fade),
        &script,
    ))
}

/// The property the benchmark is for, in every scenario.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    for fade in Fade::ALL {
        let Some(first) = play(fade) else {
            return;
        };
        let second = play(fade).expect("the first run had an adapter");

        assert_eq!(
            first.counts,
            second.counts,
            "the same script over the same {} cards did two different amounts \
             of work",
            fade.name()
        );
        assert_eq!(
            common::counts_table(&scripted::report(&first, CARDS)),
            common::counts_table(&scripted::report(&second, CARDS)),
            "a column under the header that promises to repeat did not, for {}",
            fade.name()
        );
    }
}

/// And what the frames cost the heap, which the benchmark prints beside them.
/// One scenario, for the reason `tests/static_clip_benchmark_is_repeatable.rs`
/// gives, after a play thrown away to pay for what the process builds lazily.
#[test]
fn two_runs_of_the_same_script_allocate_the_same_way() {
    let Some(_warm) = play(Fade::Group) else {
        return;
    };
    let first = play(Fade::Group).expect("the first run had an adapter");
    let second = play(Fade::Group).expect("the first run had an adapter");

    assert!(
        first.heap.frame_allocations > 0,
        "nothing was counted: this binary did not install guido::heap::CountingAllocator"
    );
    common::heap_figures_agree(
        first.heap.frame_allocations,
        second.heap.frame_allocations,
        "allocations",
        common::COUNTS_AGREE_TO,
    );
    common::heap_figures_agree(
        first.heap.frame_bytes,
        second.heap.frame_bytes,
        "bytes",
        common::BYTES_AGREE_TO,
    );
}

/// Each scenario does the work its name says, asserted rather than left to a
/// reader comparing tables — a scene that quietly stopped fading, or stopped
/// overlapping, would repeat perfectly and measure nothing.
///
/// Every scripted delta moves the fade, so every one paints. Faded as a group,
/// each card is a layer that keeps the texture the frame before the script
/// drew: laid down every frame, drawn into on none. Faded draw by draw, there
/// is no layer. Blurred, each card is a layer whose texture is blurred again
/// every frame and drawn into on none — and no frame allocates a texture,
/// because the blur's grow in steps and the first frame sized them.
#[test]
fn each_scenario_does_the_work_its_name_says() {
    let script = scripted::script(FRAMES_PER_PHASE);
    let moved = script.iter().filter(|delta| **delta != 0.0).count() as u64;
    let layers = CARDS as u64 * moved;
    for fade in Fade::ALL {
        let Some(run) = play(fade) else {
            return;
        };
        let counts = &run.counts;
        assert_eq!(
            counts.frames_painted,
            moved,
            "{}: a scripted delta that faded nothing",
            fade.name()
        );
        assert_eq!(counts.frames_pinned, 0, "{}", fade.name());
        let (drawn, kept, blurred) = match fade {
            Fade::Group => (0, layers, 0),
            Fade::Modulate => (0, 0, 0),
            Fade::Blur => (0, layers, layers),
        };
        assert_eq!(
            (
                counts.layers.drawn,
                counts.layers.kept,
                counts.layers.blurred
            ),
            (drawn, kept, blurred),
            "{}: layers drawn, kept and blurred",
            fade.name()
        );
    }
}

/// A GPU number is only comparable against the same adapter, so the line that
/// carries it names the one that produced it.
#[test]
fn the_gpu_line_names_the_adapter_that_produced_it() {
    let Some(run) = play(Fade::Group) else {
        return;
    };
    assert!(
        scripted::report(&run, CARDS).contains(&run.adapter),
        "the report quotes a GPU cost without saying which adapter paid it"
    );
}
