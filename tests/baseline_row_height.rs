//! A row aligned on the baseline is tall enough for what the alignment moved.
//!
//! A box has no baseline and sits on its bottom edge, so a box taller than a
//! label's ascent pushes the label down, and the label's descent hangs below
//! the box. The row is as deep as the deepest baseline plus the greatest depth
//! any child has below its own, which is how CSS sizes a line box.
//!
//! Measured with the vendored font, so the label's metrics are DejaVu Sans
//! Mono's and not whatever the machine has installed.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::widgets::Rect;

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");

/// The row is a box 24 tall beside "Ag" at 14px, which is ascent 13.0 and
/// 16.3 tall: the label lands 11 down and ends 3.3 below the box. Under the
/// row, in a column, a sibling that should start below the label.
fn laid_out(align: CrossAlignment) -> (Rect, Rect, Rect) {
    guido::load_font(FONT.to_vec());
    let h = Harness::laid_out(
        container()
            .layout(Flex::column())
            .child(
                container()
                    .layout(Flex::row().cross_alignment(align))
                    .child(container().width(20.0).height(24.0))
                    .child(
                        text("Ag")
                            .font_size(14.0)
                            .font_family(FontFamily::name("DejaVu Sans Mono")),
                    ),
            )
            .child(container().width(20.0).height(10.0)),
        400.0,
        400.0,
    );
    let bounds = |id| h.tree.get_bounds(id).expect("laid out");
    let column = h.tree.get_children(h.root).to_vec();
    let row = column[0];
    let label = h.tree.get_children(row)[1];
    (bounds(row), bounds(label), bounds(column[1]))
}

#[test]
fn a_baseline_row_holds_the_descent_it_moved_down() {
    let (row, label, below) = laid_out(CrossAlignment::Baseline);
    assert!(
        label.y > 0.0,
        "the box pushes the label down to its baseline, got {label:?}"
    );
    assert!(
        row.y + row.height >= label.y + label.height - 0.01,
        "the row ends at {} and the label at {}",
        row.y + row.height,
        label.y + label.height
    );
    assert!(
        below.y >= label.y + label.height - 0.01,
        "the sibling below starts at {} and the label ends at {}",
        below.y,
        label.y + label.height
    );
}

#[test]
fn a_row_aligned_to_the_start_keeps_its_height() {
    let (row, _, below) = laid_out(CrossAlignment::Start);
    assert_eq!(row.height, 24.0, "the tallest child, which is the box");
    assert_eq!(below.y, 24.0, "the sibling starts where the row ends");
}

/// A label trimmed to its capitals keeps its glyphs on the row's baseline:
/// the baseline it reports is the one in the box the trim left, so the
/// trimmed label and an untrimmed one beside it stand on the same line, and
/// the trimmed one's box starts where its capitals do.
#[test]
fn a_trimmed_label_stands_on_the_same_baseline() {
    guido::load_font(FONT.to_vec());
    let label = || {
        text("Ag")
            .font_size(14.0)
            .font_family(FontFamily::name("DejaVu Sans Mono"))
    };
    let h = Harness::laid_out(
        container()
            .layout(Flex::row().cross_alignment(CrossAlignment::Baseline))
            .child(label())
            .child(label().text_box(TextEdge::Cap, TextEdge::Alphabetic)),
        400.0,
        400.0,
    );
    let row = h.tree.get_children(h.root).to_vec();
    let line = |id| {
        let bounds = h.tree.get_bounds(id).expect("laid out");
        (
            bounds,
            bounds.y + h.tree.baseline(id).expect("a text has one"),
        )
    };
    let ((plain, plain_line), (trimmed, trimmed_line)) = (line(row[0]), line(row[1]));
    assert!(
        (plain_line - trimmed_line).abs() < 0.01,
        "the untrimmed label's baseline is at {plain_line} and the trimmed one's at {trimmed_line}"
    );
    assert!(
        (trimmed.y + trimmed.height - trimmed_line).abs() < 0.01,
        "trimmed to the baseline, the box ends on it: {trimmed:?} against {trimmed_line}"
    );
    assert!(
        trimmed.y > plain.y && trimmed.height < plain.height,
        "the trimmed box is inside the line box: {trimmed:?} within {plain:?}"
    );
}
