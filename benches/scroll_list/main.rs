//! The scrolling benchmark: a list of N rows, a scripted gesture, a table.
//!
//! ```bash
//! cargo bench --bench scroll_list --features testing,render-stats -- 5000
//! ```
//!
//! Run by hand, not by CI. A hosted runner's timings are noise on a machine
//! shared with whatever else is on it, and a number nobody can act on inside a
//! gate is a gate that gets ignored — this produces a number a person reads and
//! compares against the same command on another revision. What CI does watch is
//! `tests/scroll_benchmark_is_repeatable.rs`, which asserts the counts repeat;
//! the counts are the part a runner cannot spoil.
//!
//! The one argument is the row count. `cargo bench` passes flags of its own, so
//! anything beginning with `-` is ignored.

#[path = "../../examples/bench_list/list.rs"]
mod list;
mod scripted;

use guido::testing::Headless;

/// As many frames per phase as a gesture a person would make: four phases of
/// sixty is four seconds of scrolling at 60Hz.
const FRAMES_PER_PHASE: usize = 60;

const DEFAULT_ROWS: usize = 5000;

fn main() {
    let rows = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .and_then(|a| a.parse().ok())
        .unwrap_or(DEFAULT_ROWS);

    let Some(mut app) = Headless::new() else {
        eprintln!("no GPU adapter; a frame has to land somewhere");
        std::process::exit(1);
    };

    let script = scripted::script(FRAMES_PER_PHASE);
    let run = scripted::play(&mut app, list::VIEWPORT, move || list::list(rows), &script);

    print!("{}", scripted::report(&run, rows));
}
