//! What a frame costs when rounded clips are nested.
//!
//! ```bash
//! cargo bench --bench nested_clip --features testing,render-stats -- 240
//! ```
//!
//! The scripted benchmark #698 asked for. It plays the same gesture as
//! `benches/scroll_list` over a grid of rounded cards, each clipping a child
//! that clips again — some pairs one rounded rect describes exactly, and some
//! it does not — so the phases and the `draw.*` counts say what nesting a clip
//! costs shapes, text, text quads and images alike.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/nested_clip_benchmark_is_repeatable.rs`.
//!
//! The one argument is the rows of the grid. The gesture travels 37440 pixels
//! before it turns round, and at 172 pixels a row the default of 240 rows is
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

const DEFAULT_ROWS: usize = 240;

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
