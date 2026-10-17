//! A rounded card that clips, holding a child that clips too, still cuts what
//! is inside that child round (#698).
//!
//! The child's clip meets the card's in `ClipTree::cut`, and every command
//! under the child names the result. The unit tests beside `merge_exact` say
//! what that result is; this says the arrangement an application actually
//! builds — a rounded popup holding a scroller that fills it — reaches it,
//! through paint and flatten, and that what comes out is the card's rounding
//! rather than the child's square corners.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::renderer::{
    CornerRadii, DrawCommand, FlattenScratch, NO_CHAIN, ResolvedClip, flatten_root_into,
};

const CARD: f32 = 100.0;
const RADIUS: f32 = 20.0;
const ROW: f32 = 30.0;
const ROW_FILL: Color = Color::rgb(0.9, 0.3, 0.2);

/// The card, its clipping child filling it, and one row reaching its top
/// corners.
fn card_holding(child: Container) -> Container {
    container()
        .width(CARD)
        .height(CARD)
        .corners(RADIUS)
        .overflow(Overflow::Hidden)
        .child(
            child
                .width(fill())
                .height(fill())
                .child(container().width(CARD).height(ROW).background(ROW_FILL)),
        )
}

/// The clip the row was drawn under, after paint and flatten.
fn row_clip(view: Container) -> ResolvedClip {
    let mut surface = Harness::laid_out(view, CARD, CARD);
    let root = surface.paint();
    let (mut commands, mut layers) = (Vec::new(), Vec::new());
    let _ = flatten_root_into(
        &root,
        1.0,
        &mut commands,
        &mut layers,
        &mut FlattenScratch::default(),
    );
    commands
        .iter()
        .find(|cmd| {
            matches!(&*cmd.command, DrawCommand::RoundedRect { color, .. } if *color == ROW_FILL)
        })
        .expect("the row was drawn")
        .clip
        .as_ref()
        .expect("the row was drawn under a clip")
        .resolved()
}

/// One rounded rect, the card's, with nothing left over to test.
fn assert_cut_round(clip: ResolvedClip, why: &str) {
    assert_eq!(clip.first.radii, CornerRadii::uniform(RADIUS), "{why}");
    assert_eq!(clip.rest, NO_CHAIN, "the two merge exactly: {why}");
}

#[test]
fn a_row_in_a_scroller_in_a_rounded_card_is_cut_round() {
    assert_cut_round(
        row_clip(card_holding(container().scroll(Scroll::vertical()))),
        "the scroller's square viewport turned the card's corners square, so \
         the row was drawn across them",
    );
}

#[test]
fn a_row_in_a_clipping_column_in_a_rounded_card_is_cut_round() {
    assert_cut_round(
        row_clip(card_holding(container().overflow(Overflow::Hidden))),
        "a square clip filling the card",
    );
}

/// The control: a child that does not clip leaves the card's clip alone, which
/// is what the two above have to come out the same as.
#[test]
fn a_row_in_a_plain_child_of_a_rounded_card_is_cut_round() {
    assert_cut_round(row_clip(card_holding(container())), "no child clip");
}
