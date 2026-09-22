#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches the benchmark.
//!
//! `benches/scroll_list` exists because two hand-scrolled runs of
//! `examples/bench_list` on the same machine polled 136714 and 83209 frames, so
//! their per-frame averages described different workloads and could not be
//! compared. Replacing the hand with a script is only worth anything if the
//! script really does run the same way twice, and that is not something a
//! benchmark can claim about itself — it is an assertion, and this is where it
//! is made.
//!
//! What is asked. That two runs of one script agree on every count, and that
//! the table which says so about itself holds to it. That the
//! script's deltas reach the scroller at all. That a workload which differs is
//! *reported* as differing, because a number that never moves proves nothing by
//! repeating. That a gesture longer than the list says so rather than quietly
//! averaging a stationary one. And that the GPU line names the adapter that
//! produced it.
//!
//! Only counts, never microseconds — `scripted::Counts` says which is which.
//! The heap figures are a third thing, and `scripted::Heap` says what about
//! them holds: the counts repeat to within the handful the graphics driver's
//! own threads add, and the live-byte levels are a level rather than a total.
//!
//! The file compiles the benchmark's own modules rather than a copy of them. A
//! harness that watched a second implementation of the workload would be
//! watching the wrong one the first time the two drifted.

mod common;
#[path = "../examples/bench_list/list.rs"]
mod list;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

/// Enough rows that the scroller has far more content than viewport and the
/// paint window has something to narrow, and few enough that several plays of
/// the script fit inside a test run.
const ROWS: usize = 200;

/// Short, because the property is the script's determinism and not its length.
const FRAMES_PER_PHASE: usize = 6;

/// What a binary has to install for any of that to be counted at all. The
/// library may not: a `#[global_allocator]` is the final binary's to choose.
#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

/// `None` on a machine with no adapter, which is a skip unless the job pointed
/// at lavapipe says a skip is a failure — `common::headless` keeps that
/// contract for every test that needs one.
fn play(rows: usize) -> Option<scripted::Run> {
    let _alone = common::one_play_at_a_time();
    let mut app = common::headless()?;
    let script = scripted::script(FRAMES_PER_PHASE);
    Some(scripted::play(
        &mut app,
        list::VIEWPORT,
        move || list::list(rows),
        &script,
    ))
}

/// The property the hand-scrolled runs lacked.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    let Some(first) = play(ROWS) else {
        return;
    };
    let second = play(ROWS).expect("the first run had an adapter");

    assert_eq!(
        first.counts, second.counts,
        "the same script over the same tree did two different amounts of work"
    );
}

/// And the same of what the frames cost the heap, which is the number the
/// benchmark grew a fourth section for.
///
/// It is asserted apart from the counts above because it is not one: the
/// counter is the whole process's, so the graphics driver's worker threads are
/// in it and two plays agree within a margin rather than exactly. How wide a
/// margin, and why it is not the same for a count of requests as for a count of
/// bytes, is on `common::COUNTS_AGREE_TO` and `common::BYTES_AGREE_TO`.
///
/// A play is thrown away first, for the reason `scripted::play` throws away its
/// first frame: the first play in a process pays for everything the process
/// builds lazily — font caches, glyph atlases, the driver's own tables — and
/// that was 39 allocations of 12238 here, which is not a measurement of the
/// script. Every other test in this file compares counts, which those do not
/// touch.
#[test]
fn two_runs_of_the_same_script_allocate_the_same_way() {
    let Some(_warm) = play(ROWS) else {
        return;
    };
    let first = play(ROWS).expect("the first run had an adapter");
    let second = play(ROWS).expect("the first run had an adapter");

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
    // Both windows, because the section prints both under one header that says
    // they repeat.
    common::heap_figures_agree(
        first.heap.play_allocations,
        second.heap.play_allocations,
        "allocations over the whole play",
        100,
    );
    common::heap_figures_agree(
        first.heap.play_bytes,
        second.heap.play_bytes,
        "bytes over the whole play",
        common::BYTES_AGREE_TO,
    );
}

/// The script's own arithmetic, asserted where a reader can see it: every frame
/// with a delta paints, and every frame without one does not. It is what says
/// the wheel reached the scroller at all — a benchmark whose events landed
/// nowhere would report a perfectly repeatable idle list, and every other
/// assertion here would still pass.
#[test]
fn every_scripted_delta_paints_a_frame_and_no_pause_does() {
    let Some(run) = play(ROWS) else {
        return;
    };
    let script = scripted::script(FRAMES_PER_PHASE);
    let moved = script.iter().filter(|d| **d != 0.0).count() as u64;

    assert_eq!(run.counts.frames_painted, moved);
    assert_eq!(
        run.counts.frames_not_painted,
        script.len() as u64 - moved,
        "a frame that was neither painted nor still"
    );
}

/// The honest check that it measures anything at all. Two workloads that differ
/// have to be reported as differing — the benchmark's whole use is being
/// pointed at two revisions, and a report that came back the same either way
/// would do that without anyone noticing.
///
/// Rows, because that is the difference a test can make from the outside. The
/// direction is asserted rather than the mere inequality: a longer list offers
/// the paint window more children to narrow, every frame, and a benchmark that
/// reported otherwise would be reporting something else.
#[test]
fn a_longer_list_is_reported_as_more_work() {
    let Some(short) = play(ROWS) else {
        return;
    };
    let long = play(ROWS * 4).expect("the first run had an adapter");

    assert_ne!(short.counts, long.counts);
    assert!(
        long.counts.window_children_total > short.counts.window_children_total,
        "four times the rows offered the paint window {} children against {}",
        long.counts.window_children_total,
        short.counts.window_children_total
    );
    // And the heap, on the same pair of runs: a figure that came back the same
    // for four times the rows would satisfy every assertion above about
    // repeating and be measuring nothing.
    //
    // The peak rather than the count, and that is the finding. Four times the
    // rows is four times the widgets held at once, so the peak moves; the
    // *count* barely does, because a scroller paints what its viewport shows
    // and a longer list below it costs the frames nothing. 12238 allocations
    // at 200 rows against 12200 at 800.
    assert!(
        long.heap.peak_live_bytes > short.heap.peak_live_bytes,
        "four times the rows held {} bytes at their peak against {}",
        long.heap.peak_live_bytes,
        short.heap.peak_live_bytes
    );
}

/// The table headed *"identical on every run of this revision"* makes a claim
/// about itself, and a number printed under it that nothing covers is the one a
/// reader would trust wrongly. So the section is compared as text rather than
/// field by field: a column added to it tomorrow is covered the day it is
/// added, without anybody remembering to say so here.
#[test]
fn the_table_that_says_it_repeats_repeats() {
    let Some(first) = play(ROWS) else {
        return;
    };
    let second = play(ROWS).expect("the first run had an adapter");

    assert_eq!(
        counts_table(&scripted::report(&first, ROWS)),
        counts_table(&scripted::report(&second, ROWS)),
        "a column under the header that promises to repeat did not"
    );
}

/// The heap section is not under that header, and the tests above read the
/// fields rather than the printed lines — so nothing said the printed lines
/// carry the figures they are labelled with. A report that put the whole play's
/// number under `heap.frame_allocations` would leave every assertion in this
/// file green and give a person comparing two revisions the wrong number, which
/// is exactly what the adapter test exists to stop for the GPU line.
///
/// Six names and six values, against the run they were taken from. `report` is
/// one function and `static_clip` prints the same one, so once is enough.
#[test]
fn the_heap_section_prints_the_figure_each_of_its_names_promises() {
    let Some(run) = play(ROWS) else {
        return;
    };
    let report = scripted::report(&run, ROWS);
    let heap = &run.heap;

    for (name, value) in [
        ("heap.frame_allocations", heap.frame_allocations as i64),
        ("heap.frame_bytes", heap.frame_bytes as i64),
        ("heap.play_allocations", heap.play_allocations as i64),
        ("heap.play_bytes", heap.play_bytes as i64),
        ("heap.peak_live_bytes", heap.peak_live_bytes as i64),
        ("heap.retained_bytes", heap.retained_bytes),
    ] {
        let line = report
            .lines()
            .find(|line| line.trim_start().starts_with(name))
            .unwrap_or_else(|| panic!("the report never names {name}"));
        assert_eq!(
            line.split_whitespace().nth(1),
            Some(value.to_string().as_str()),
            "{name} is printed as {line:?} and the run says {value}"
        );
    }
}

/// The lines of the report between the counts header and the phases that
/// follow it.
fn counts_table(report: &str) -> Vec<&str> {
    report
        .lines()
        .skip_while(|line| !line.starts_with("counts ("))
        .take_while(|line| !line.starts_with("cpu phases"))
        .collect()
}

/// A gesture longer than the list is a gesture that spends most of itself
/// against the bottom, measuring a stationary list — repeatable, and not the
/// workload anybody asked for. The report has to say so, because the frames it
/// averages over are the only other sign and a person comparing two tables
/// would not see it.
#[test]
fn a_gesture_that_runs_off_the_end_of_the_list_says_so() {
    let Some(inside) = play(ROWS) else {
        return;
    };
    assert_eq!(
        inside.counts.frames_pinned, 0,
        "the script was supposed to stay inside a list this long"
    );
    assert!(!scripted::report(&inside, ROWS).contains("moved nothing"));

    // Four rows against a gesture that travels thousands of pixels.
    let pinned = play(4).expect("the first run had an adapter");
    assert!(
        pinned.counts.frames_pinned > 0,
        "a four-row list absorbed the whole gesture"
    );
    assert!(
        scripted::report(&pinned, 4).contains("moved nothing"),
        "the report never mentions the deltas that moved nothing"
    );
}

/// A GPU number that does not say what produced it is a claim about no machine
/// in particular, which is the one thing this benchmark must not print.
#[test]
fn the_report_names_the_adapter_behind_its_gpu_number() {
    let Some(run) = play(ROWS) else {
        return;
    };
    let report = scripted::report(&run, ROWS);

    assert!(!run.adapter.is_empty(), "the adapter had no name");
    let gpu_line = report
        .lines()
        .find(|line| line.contains("gpu phase"))
        .expect("no gpu section in the report");
    assert!(
        gpu_line.contains(&run.adapter),
        "the gpu section says {gpu_line:?} and never names {}",
        run.adapter
    );
}
