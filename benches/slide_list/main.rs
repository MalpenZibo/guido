//! What a frame of a slide damages.
//!
//! ```bash
//! cargo bench --bench slide_list --features testing,render-stats -- 8
//! ```
//!
//! The benchmark #757 asked for. A column of cards is reordered by a timer and
//! every card slides to its new place, with nothing else on the surface
//! moving, so `damage.pixels` — the buffer pixels the frames' damage named —
//! is what the slides cost the compositor. The script is still frames only:
//! there is nothing here to scroll, and the timer is what paints.
//!
//! Run by hand, not by CI, for the reason `scroll_list` gives. What CI watches
//! is `tests/slide_list_benchmark_is_repeatable.rs`.
//!
//! The one argument is the cards. The one that moves from the bottom to the
//! top slides past all the others, so more cards is a longer slide.

mod scene;
#[path = "../scroll_list/scripted.rs"]
// Its scroll gesture is the one part this does not play: nothing here scrolls.
#[allow(dead_code)]
mod scripted;

use guido::testing::Headless;

/// The heap column has to be asked for here. A `#[global_allocator]` may only
/// be set by the binary that links the program, so guido ships the allocator
/// and installs nothing.
#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

/// Twelve turns of the column, at twenty frames a turn.
const FRAMES: usize = 240;

const DEFAULT_CARDS: usize = 8;

fn main() {
    let cards = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .and_then(|a| a.parse().ok())
        .unwrap_or(DEFAULT_CARDS);

    let Some(mut app) = Headless::new() else {
        eprintln!("no GPU adapter; a frame has to land somewhere");
        std::process::exit(1);
    };

    let run = scripted::play(
        &mut app,
        scene::VIEWPORT,
        move || scene::scene(cards),
        &[0.0; FRAMES],
    );

    print!("{}", scripted::report(&run, cards));
}
