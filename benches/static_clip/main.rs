//! What a static clipped subtree costs when it is walked again every frame.
//!
//! ```bash
//! cargo bench --bench static_clip --features testing,render-stats -- 400
//! ```
//!
//! The sibling of `benches/scroll_list`, and the answer to what that one cannot
//! ask. A scroller moves everything under its clip, so it shows that reaching
//! the flatten cache from inside a clip costs nothing; it cannot show what the
//! cache is for. This plays the same script over a surface where the only thing
//! that moves is a small ticker, and the panels beside it are clipped and
//! still.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/static_clip_benchmark_is_repeatable.rs`.
//!
//! The one argument is the rows in each panel.

mod scene;
#[path = "../scroll_list/scripted.rs"]
mod scripted;

use guido::testing::Headless;

/// As many frames per phase as `scroll_list` plays, so the two tables are read
/// the same way.
const FRAMES_PER_PHASE: usize = 60;

const DEFAULT_ROWS: usize = 400;

fn main() {
    let rows = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .and_then(|a| a.parse().ok())
        .unwrap_or(DEFAULT_ROWS);

    let script = scripted::script(FRAMES_PER_PHASE);
    for panels in scene::Panel::ALL {
        // One application per scenario, rather than one surface after another
        // in the same one. A surface the loop still holds is a surface that
        // still paints, and the per-frame accounting in `scripted` counts both
        // — it says so rather than averaging them, which is how this was found.
        let Some(mut app) = Headless::new() else {
            eprintln!("no GPU adapter; a frame has to land somewhere");
            std::process::exit(1);
        };
        let run = scripted::play(
            &mut app,
            scene::VIEWPORT,
            move || scene::scene(rows, panels),
            &script,
        );
        println!("=== panels: {} ===", panels.name());
        print!("{}", scripted::report(&run, rows));
        println!();
    }
}
