//! What a frame costs when rings and charts keep moving.
//!
//! ```bash
//! cargo bench --bench sparkline_ring --features testing,render-stats -- 900
//! ```
//!
//! The benchmark #736 asked for. It plays the same gesture as
//! `benches/scroll_list` over rows of status meters — a ring drawn as one SDF
//! shape whose progress eases every frame, and a line chart drawn as two
//! tessellated paths that a timer gives new readings — so `draw.paths` says
//! how many calls the paths cost a frame, and `path.tessellated` against
//! `path.reused` how much of that was tessellation and how much a mesh drawn
//! again. Above the rows, one dashed outline marches every frame, which is a
//! tessellation every frame — what #755 asked an animated dash to be measured
//! by.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/sparkline_ring_benchmark_is_repeatable.rs`.
//!
//! The one argument is the rows. The gesture travels 37440 pixels before it
//! turns round, and at 44 pixels a row the default of 900 rows is enough that
//! it never pins.

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

const DEFAULT_ROWS: usize = 900;

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
