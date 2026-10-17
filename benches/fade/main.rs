//! What a fade costs, frame by frame.
//!
//! ```bash
//! cargo bench --bench fade --features testing,render-stats -- 12
//! ```
//!
//! The sibling of `benches/scroll_list`, `benches/static_clip`,
//! `benches/icon_grid` and `benches/nested_clip`, for the one thing none of
//! them draws: a subtree composited as a layer. Cards whose label overlaps
//! their background fade by the scripted wheel, as a group, draw by draw, and
//! under a moving blur — and the `layer.*` rows say what the layers did: drawn
//! into their textures, laid down from one an earlier frame drew, blurred,
//! and how many textures that took.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/fade_benchmark_is_repeatable.rs`.
//!
//! The one argument is the number of cards.

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

const DEFAULT_CARDS: usize = 12;

fn main() {
    let cards = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .and_then(|a| a.parse().ok())
        .unwrap_or(DEFAULT_CARDS);

    let script = scripted::script(FRAMES_PER_PHASE);
    for fade in scene::Fade::ALL {
        // One application per scenario, for the reason `static_clip` gives.
        let Some(mut app) = Headless::new() else {
            eprintln!("no GPU adapter; a frame has to land somewhere");
            std::process::exit(1);
        };
        let run = scripted::play(
            &mut app,
            scene::VIEWPORT,
            move || scene::scene(cards, fade),
            &script,
        );
        println!("=== fade: {} ===", fade.name());
        print!("{}", scripted::report(&run, cards));
        println!();
    }
}
