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
//! What is asked. That two runs of one script agree on every count. That the
//! script's deltas reach the scroller at all. That a workload which differs is
//! *reported* as differing, because a number that never moves proves nothing by
//! repeating. That a gesture longer than the list says so rather than quietly
//! averaging a stationary one. And that the GPU line names the adapter that
//! produced it.
//!
//! Only counts, never microseconds — `scripted::Counts` says which is which.
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

/// `None` on a machine with no adapter, which is a skip unless the job pointed
/// at lavapipe says a skip is a failure — `common::headless` keeps that
/// contract for every test that needs one.
fn play(rows: usize) -> Option<scripted::Run> {
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
