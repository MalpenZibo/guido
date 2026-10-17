//! A column of cards a timer keeps reordering, each sliding to its new place.
//!
//! What a reordered list or a moving highlight is made of: a keyed column whose
//! rows declare `animate_position`, so a reorder slides each one from where it
//! was drawn to where layout put it. Every [`TICK`] the last card moves to the
//! top — every other card slides one row down, and that one slides the whole
//! column up — and nothing else on the surface moves, so the damage the frames
//! report is the slide's alone.
//!
//! Each card casts a shadow offset downward, which is the other directional
//! overflow a damage rect has to cover.

use std::time::Duration;

use guido::prelude::*;

/// The surface, in logical pixels: much wider than the column, so a damage
/// rect that grows sideways has somewhere to grow into.
pub const VIEWPORT: (u32, u32) = (600, 400);

const CARD: (f32, f32) = (200.0, 40.0);
const MARGIN: f32 = 20.0;

/// How often the column turns over: twenty frames of the script, so each slide
/// settles before the next starts.
pub const TICK: Duration = Duration::from_millis(320);

const SLIDE_MS: f32 = 200.0;

const SHADOW: Shadow = Shadow::new((0.0, 4.0), 6.0, 0.0, Color::rgba(0.0, 0.0, 0.0, 0.3));

/// `cards` cards in a column at the surface's left edge.
pub fn scene(cards: usize) -> Container {
    let order = create_signal((0..cards).collect::<Vec<_>>());
    // The handle is dropped and the timer kept: it lives as long as the owner
    // building the surface, which is the run.
    let _ = set_interval(TICK, move || order.update(|o| o.rotate_right(1)));
    container()
        .width(VIEWPORT.0 as f32)
        .height(VIEWPORT.1 as f32)
        .padding(MARGIN)
        .background(Color::rgb(0.10, 0.10, 0.14))
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .children(keyed(move || order.get(), |card| *card, card)),
        )
}

fn card(index: usize) -> Container {
    let shade = 0.3 + 0.05 * (index % 8) as f32;
    container()
        .width(CARD.0)
        .height(CARD.1)
        .corners(8.0)
        .background(Color::rgb(shade, 0.45, 0.75))
        .shadow(SHADOW)
        .animate_position(Transition::new(SLIDE_MS, TimingFunction::Linear))
}
