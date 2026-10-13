//! A scroller full of rounded cards, each clipping a clipped child.
//!
//! Every clip below the scroller's is nested in another rounded one. The other
//! benchmarks clip only once beneath the viewport, so none of them reaches the
//! case where two rounded clips cannot be folded into one exactly.
//!
//! Four kinds of card, cycled, because merging two clips is exact for the
//! first and for none of the others:
//!
//! - [`Card::Filled`] — the inner clip is the card's own box, so the two
//!   intersect to the card exactly and can be merged.
//! - [`Card::Inset`] — a smaller rounded clip two pixels in, whose arcs
//!   cross the card's. No single rounded rect is their intersection.
//! - [`Card::Mixed`] — a circle holding a squircle: two curvatures, which one
//!   rounded rect cannot carry.
//! - [`Card::Turned`] — an inner clip rotated by a non-quarter turn, which no
//!   axis-aligned rect describes.
//!
//! Inside each, rows whose backgrounds run into the corners, with a label and
//! a small image, so shapes, glyphon text, transformed-text quads and image
//! quads are all drawn under the nested clips.

use std::sync::Arc;

use guido::prelude::*;

/// The surface, in logical pixels.
pub const VIEWPORT: (u32, u32) = (600, 800);

const CARD_WIDTH: f32 = 280.0;
const CARD_HEIGHT: f32 = 160.0;
const GAP: f32 = 12.0;
const PAD: f32 = 8.0;
/// Cards in a grid row: `2 * 280 + 12 + 2 * 8` is 588, inside the 600 the
/// surface is wide, with the scrollbar's strip to spare.
const PER_ROW: usize = 2;

const CORNER: f32 = 16.0;
const ROW_HEIGHT: f32 = 34.0;
const ROWS_PER_CARD: usize = 5;
const ICON: f32 = 20.0;
/// Distinct image sources, small enough to share one atlas page.
const DISTINCT: usize = 8;

/// The four ways a card's inner clip meets its own.
#[derive(Clone, Copy)]
enum Card {
    Filled,
    Inset,
    Mixed,
    Turned,
}

impl Card {
    const ALL: [Card; 4] = [Card::Filled, Card::Inset, Card::Mixed, Card::Turned];
}

/// A grid of `rows` rows of [`PER_ROW`] cards, in a scroller the size of the
/// surface. The scroller is the only thing the script moves, so everything
/// under every card's clip moves on every painted frame.
///
/// The inner clips are `Overflow::Hidden` rather than scrollers: the script
/// aims its wheel at the middle of the surface, and a scroller under the
/// pointer would take the gesture away from the one that moves the grid.
pub fn scene(rows: usize) -> Container {
    let sources: Arc<[ImageSource]> = (0..DISTINCT).map(source).collect();
    container()
        .width(VIEWPORT.0 as f32)
        .height(VIEWPORT.1 as f32)
        .scroll(Scroll::vertical())
        .background(Color::rgb(0.10, 0.10, 0.14))
        .child(
            container()
                .layout(Flex::column().spacing(GAP))
                .padding(PAD)
                .children((0..rows).map(move |row| {
                    let sources = sources.clone();
                    container()
                        .layout(Flex::row().spacing(GAP))
                        .children((0..PER_ROW).map(move |column| {
                            let index = row * PER_ROW + column;
                            card(Card::ALL[index % Card::ALL.len()], index, &sources)
                        }))
                })),
        )
}

fn card(kind: Card, index: usize, sources: &[ImageSource]) -> Container {
    let outer = container()
        .width(CARD_WIDTH)
        .height(CARD_HEIGHT)
        .background(Color::rgb(0.18, 0.20, 0.28))
        .overflow(Overflow::Hidden)
        .layout(ZStack::new());
    let inner = container()
        .width(fill())
        .height(fill())
        .overflow(Overflow::Hidden)
        .background(Color::rgb(0.22, 0.24, 0.34))
        .layout(Flex::column())
        .children((0..ROWS_PER_CARD).map(|line| {
            let n = index * ROWS_PER_CARD + line;
            row(n, sources[n % sources.len()].clone())
        }));
    match kind {
        Card::Filled => outer.corners(CORNER).child(inner),
        // Two pixels in with a radius of four: the inner arc runs outside the
        // card's sixteen-pixel one and the card's corner outside the inner
        // box, so neither clip contains the other at any corner.
        Card::Inset => outer
            .corners(CORNER)
            .padding(2.0)
            .child(inner.corners(Corners::rounded(4.0))),
        // A circle, as wide as the card is tall, holding a squircle.
        Card::Mixed => outer
            .width(CARD_HEIGHT)
            .corners(CARD_HEIGHT / 2.0)
            .padding(8.0)
            .child(inner.corners(Corners::squircle(24.0))),
        Card::Turned => outer
            .corners(CORNER)
            .padding(12.0)
            .child(inner.corners(Corners::rounded(12.0)).rotate(15.0)),
    }
}

/// A row whose background runs the full width of its clip, so the first and
/// last rows reach into its corners, with an icon and a label at its left end
/// — inside the corner box, where a rounded clip cuts a text differently from
/// the box around it.
fn row(n: usize, icon: ImageSource) -> Container {
    let shade = 0.26 + (n % 3) as f32 * 0.04;
    container()
        .width(fill())
        .height(ROW_HEIGHT)
        .background(Color::rgb(shade, shade, shade + 0.08))
        .padding([2.0, 4.0])
        .layout(
            Flex::row()
                .spacing(6.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(
            container()
                .width(ICON)
                .height(ICON)
                .child(image(icon).content_fit(ContentFit::Fill)),
        )
        .child(
            text(format!("Row {n}"))
                .font_size(13.0)
                .color(Color::WHITE)
                .nowrap(),
        )
}

/// The `index`th icon, raw RGBA in a colour of its own.
fn source(index: usize) -> ImageSource {
    let side = ICON as u32;
    let channel = |bit: usize| ((index >> bit) & 1) as u8 * 200 + 40;
    ImageSource::Rgba {
        width: side,
        height: side,
        pixels: [channel(0), channel(1), channel(2), 255]
            .repeat((side * side) as usize)
            .into(),
    }
}
