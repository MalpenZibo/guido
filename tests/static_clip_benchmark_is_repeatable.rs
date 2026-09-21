#![cfg(all(feature = "testing", feature = "render-stats"))]
//! What watches the other benchmark.
//!
//! `benches/scroll_list` measures a workload where everything under the clip
//! moves, which is the case #442 must not make worse. `benches/static_clip`
//! measures the one it exists to make better: a clipped panel that is standing
//! still while something beside it repaints. The number that says so —
//! `flatten.nodes_cached` climbing off zero, and `nodes_flattened` falling — is
//! the only evidence of the benefit anywhere outside the unit tests, and a
//! benchmark nothing watches is a number that quietly stops meaning anything.
//!
//! What is asked here is what its sibling asks, plus the one thing particular
//! to this scenario: that the panels really are reused. Both halves are needed.
//! A scenario whose panels stopped being reached would report a perfectly
//! repeatable nothing, and every other assertion would still pass.
//!
//! Only counts, never microseconds — `scripted::Counts` says which is which.
//!
//! The file compiles the benchmark's own modules rather than a copy of them.

mod common;
#[path = "../benches/static_clip/scene.rs"]
mod scene;
#[path = "../benches/scroll_list/scripted.rs"]
mod scripted;

use scene::Panel;

/// Enough rows in each panel that a panel holds more than it can show, and few
/// enough that several plays fit inside a test run.
const ROWS: usize = 40;

/// Short, because the property is the script's determinism and not its length.
const FRAMES_PER_PHASE: usize = 6;

/// `None` on a machine with no adapter, which is a skip unless the job pointed
/// at lavapipe says a skip is a failure.
fn play(rows: usize, panels: Panel) -> Option<scripted::Run> {
    let mut app = common::headless()?;
    let script = scripted::script(FRAMES_PER_PHASE);
    Some(scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(rows, panels),
        &script,
    ))
}

/// The property the hand-run numbers lacked, for both scenarios.
#[test]
fn two_runs_of_the_same_script_agree_on_every_count() {
    for panels in Panel::ALL {
        let Some(first) = play(ROWS, panels) else {
            return;
        };
        let second = play(ROWS, panels).expect("the first run had an adapter");

        assert_eq!(
            first.counts,
            second.counts,
            "the same script over the same {} tree did two different amounts of work",
            panels.name()
        );
    }
}

/// What the scenario is for, asserted rather than left to a reader comparing
/// two tables: the panels stand still, so flatten replays them.
///
/// Zero here is what `main` reports and what this whole change is about, so a
/// scenario that quietly stopped reaching the panels — a layout that culled
/// them away, a ticker that stopped painting — would read exactly like the
/// revision before the fix. Both panels are clipped both ways, so both ways are
/// asserted.
#[test]
fn the_panels_standing_still_are_replayed_rather_than_flattened_again() {
    for panels in Panel::ALL {
        let Some(run) = play(ROWS, panels) else {
            return;
        };

        assert!(
            run.counts.flatten_nodes_cached > 0,
            "not one subtree was replayed across {} painted frames with {} \
             panels: on this revision a clipped panel that never moves is \
             supposed to be flattened once and replayed after that",
            run.counts.frames_painted,
            panels.name()
        );
    }
}

/// The script's own arithmetic, asserted where a reader can see it: every frame
/// with a delta paints, and every frame without one does not.
///
/// Without it the scenario could be vacuous in the way that matters most here.
/// The panels are only re-flattened on frames that paint, so a ticker whose
/// wheel events landed nowhere would leave nothing to save and *both*
/// revisions would report the same — a green comparison that measured an idle
/// surface.
#[test]
fn every_scripted_delta_paints_a_frame_and_no_pause_does() {
    for panels in Panel::ALL {
        let Some(run) = play(ROWS, panels) else {
            return;
        };
        let script = scripted::script(FRAMES_PER_PHASE);
        let moved = script.iter().filter(|d| **d != 0.0).count() as u64;

        assert_eq!(
            run.counts.frames_painted,
            moved,
            "{} panels: a scripted delta that painted nothing",
            panels.name()
        );
        assert_eq!(
            run.counts.frames_not_painted,
            script.len() as u64 - moved,
            "{} panels: a frame that was neither painted nor still",
            panels.name()
        );
    }
}

/// The two scenarios are two workloads, and the benchmark has to report them as
/// such — otherwise one of the two tables it prints is decoration.
///
/// The direction is asserted rather than the mere inequality. An
/// `Overflow::Hidden` panel does not cull, so it puts every row it holds
/// through flatten; a scroller culls to its viewport and puts through only what
/// is visible. That is the whole reason both are here.
#[test]
fn hiding_overflow_is_reported_as_more_work_than_resting_on_a_scroller() {
    let Some(resting) = play(ROWS, Panel::Resting) else {
        return;
    };
    let hidden = play(ROWS, Panel::Hidden).expect("the first run had an adapter");

    assert_ne!(resting.counts, hidden.counts);
    assert!(
        hidden.counts.paint_children_culled < resting.counts.paint_children_culled,
        "an `Overflow::Hidden` panel culled {} children against a resting \
         scroller's {}, so the two scenarios are not the two workloads this \
         benchmark prints two tables for",
        hidden.counts.paint_children_culled,
        resting.counts.paint_children_culled
    );
}

/// The table headed *"identical on every run of this revision"* makes a claim
/// about itself, and a number printed under it that nothing covers is the one a
/// reader would trust wrongly. So the section is compared as text rather than
/// field by field: a column added to it tomorrow is covered the day it is
/// added, without anybody remembering to say so here.
#[test]
fn the_table_that_says_it_repeats_repeats() {
    for panels in Panel::ALL {
        let Some(first) = play(ROWS, panels) else {
            return;
        };
        let second = play(ROWS, panels).expect("the first run had an adapter");

        assert_eq!(
            counts_table(&scripted::report(&first, ROWS)),
            counts_table(&scripted::report(&second, ROWS)),
            "a column under the header that promises to repeat did not, with {} panels",
            panels.name()
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

/// A GPU number is only comparable against the same adapter, so the line that
/// carries it has to name the one that produced it. The sibling benchmark
/// asserts this of its own report; this one prints the same report and would
/// otherwise be trusting that nobody ever prints it differently.
#[test]
fn the_gpu_line_names_the_adapter_that_produced_it() {
    let Some(run) = play(ROWS, Panel::Resting) else {
        return;
    };
    let report = scripted::report(&run, ROWS);

    assert!(
        report.contains(&run.adapter),
        "the report quotes a GPU cost without saying which adapter paid it"
    );
}
