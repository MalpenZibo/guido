//! What a screen of small images costs a frame.
//!
//! ```bash
//! cargo bench --bench icon_grid --features testing,render-stats -- 1100
//! ```
//!
//! The third of the scripted benchmarks, and the one #546 asked for. It plays
//! the same gesture as `benches/scroll_list` over a grid of icons instead of a
//! list of boxes, so its `draw.*` and `quad.*` counts say how many calls a
//! frame of images issues and how many GPU objects it makes to issue them, and
//! its phases what that costs.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/icon_grid_benchmark_is_repeatable.rs`.
//!
//! The one argument is the rows of the grid. The gesture travels 37440 pixels
//! before it turns round, and at 36 pixels a row the default of 1100 rows is
//! enough that it never pins.

mod scene;
#[path = "../scroll_list/scripted.rs"]
mod scripted;

use guido::testing::Headless;

/// The heap column has to be asked for here. A `#[global_allocator]` may only
/// be set by the binary that links the program, so guido ships the allocator
/// and installs nothing.
#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

/// As many frames per phase as `scroll_list` plays, so the tables are read the
/// same way.
const FRAMES_PER_PHASE: usize = 60;

const DEFAULT_ROWS: usize = 1100;

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
    let run = scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(rows),
        &script,
    );

    print!("{}", scripted::report(&run, rows));
}
