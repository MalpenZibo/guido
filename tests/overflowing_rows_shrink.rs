//! A row whose children do not fit shares the overflow among those that can
//! give room up, and a row whose children fit is laid out as it always was.
//!
//! The overflow is shared in proportion to each child's natural size, and no
//! child is taken below its minimum: an exact length at any depth, a text's
//! full width when it neither wraps nor ellipsizes, the content of a container
//! that does not clip. A child that would go below its minimum is held there
//! and the rest share what is left, as CSS Flexbox §9.7 resolves flexible
//! lengths. Columns follow the same rule along their height (#700).
//!
//! Measured with the vendored font, so a glyph is DejaVu Sans Mono's 8.43 at
//! 14px and twenty of them are 168.57, whatever the machine has installed.

mod common;

use common::Harness;
use guido::prelude::*;
use guido::renderer::{DrawCommand, LineFit, RenderNode};
use guido::widgets::Rect;

const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");
const TWENTY: &str = "abcdefghijklmnopqrst";
/// Twenty glyphs at 14px.
const TWENTY_WIDE: f32 = 168.57;
/// One glyph at 14px: what an ellipsized text can come back narrower than it
/// was offered by, since it ends in whole glyphs.
const GLYPH: f32 = TWENTY_WIDE / 20.0;

fn label(content: &'static str) -> Text {
    text(content)
        .font_size(14.0)
        .font_family(FontFamily::name("DejaVu Sans Mono"))
        .nowrap()
}

fn ellipsized(content: &'static str) -> Text {
    label(content).overflow(TextOverflow::Ellipsis)
}

fn swatch() -> Container {
    container().width(60.0).height(10.0)
}

/// A panel that clips what it holds: a box `width` wide, which it can give up
/// entirely.
fn clipping(width: f32) -> Container {
    container()
        .overflow(Overflow::Hidden)
        .child(container().width(width).height(10.0))
}

fn laid_out(root: Container) -> Harness {
    guido::load_font(FONT.to_vec());
    Harness::laid_out(root, 400.0, 400.0)
}

fn row(width: f32) -> Container {
    container().width(width).layout(Flex::row())
}

fn bounds(h: &Harness, path: &[usize]) -> Rect {
    let mut id = h.root;
    let mut x = 0.0;
    let mut y = 0.0;
    for &i in path {
        id = h.tree.get_children(id)[i];
        let (ox, oy) = h.tree.get_origin(id).expect("placed");
        x += ox;
        y += oy;
    }
    let b = h.tree.get_bounds(id).expect("laid out");
    Rect::new(x, y, b.width, b.height)
}

/// The fit every text was painted with, in paint order.
fn fits(h: &mut Harness) -> Vec<Option<LineFit>> {
    fn collect(node: &RenderNode, out: &mut Vec<Option<LineFit>>) {
        for cmd in &node.commands {
            if let DrawCommand::Text { fit, .. } = &**cmd {
                out.push(*fit);
            }
        }
        for child in &node.children {
            collect(child, out);
        }
    }
    let mut out = Vec::new();
    collect(&h.paint(), &mut out);
    out
}

fn ends(r: Rect) -> f32 {
    r.x + r.width
}

/// Whether a text offered `share` took it, to within the glyph that may not
/// fit.
fn took(r: Rect, share: f32) -> bool {
    r.width <= share + 0.01 && r.width > share - GLYPH
}

/// Whether the text was painted cut to a width under its own, with a mark.
fn ellipsized_within(fit: Option<LineFit>, width: f32) -> bool {
    fit.is_some_and(|fit| {
        fit.overflow == TextOverflow::Ellipsis && fit.width.is_some_and(|w| w <= width + 0.01)
    })
}

#[test]
fn a_title_beside_a_box_ends_in_an_ellipsis_inside_the_row() {
    let mut h = laid_out(row(200.0).child(swatch()).child(ellipsized(TWENTY)));
    let title = bounds(&h, &[1]);
    assert!(
        ends(title) <= 200.01,
        "the title ends at {}, past the row's 200",
        ends(title)
    );
    assert_eq!(bounds(&h, &[0]).width, 60.0, "an exact length keeps it");
    assert!(
        took(title, 140.0),
        "the title takes what the box leaves, got {title:?}"
    );
    let fits = fits(&mut h);
    assert!(
        ellipsized_within(fits[0], 140.0),
        "the title was painted with {:?}, not cut at 140 with an ellipsis",
        fits[0]
    );
}

#[test]
fn a_box_after_a_title_stays_inside_the_row_at_its_width() {
    let h = laid_out(row(200.0).child(ellipsized(TWENTY)).child(swatch()));
    let swatch = bounds(&h, &[1]);
    assert_eq!(swatch.width, 60.0);
    assert!(
        ends(swatch) <= 200.01,
        "the box is pushed to {swatch:?}, past the row"
    );
}

#[test]
fn a_row_that_fits_is_laid_out_at_natural_sizes() {
    let h = laid_out(row(400.0).child(swatch()).child(ellipsized(TWENTY)));
    let title = bounds(&h, &[1]);
    assert_eq!(title.x, 60.0);
    assert!(
        (title.width - TWENTY_WIDE).abs() < 0.01,
        "a title with room keeps its width, got {title:?}"
    );
}

#[test]
fn a_title_inside_a_padded_container_shrinks_with_it() {
    let mut h = laid_out(
        row(200.0)
            .child(swatch())
            .child(container().padding(4.0).child(ellipsized(TWENTY))),
    );
    let wrapper = bounds(&h, &[1]);
    assert!(
        ends(wrapper) <= 200.01,
        "the wrapper ends at {}, past the row",
        ends(wrapper)
    );
    let fits = fits(&mut h);
    assert!(
        ellipsized_within(fits[0], 132.0),
        "the title inside was painted with {:?}",
        fits[0]
    );
}

#[test]
fn an_exact_length_inside_a_container_is_not_squashed() {
    // A button: padding round an icon of a declared width. The button's
    // minimum is the icon plus its padding, so the title gives up the room.
    let h = laid_out(
        row(200.0)
            .child(ellipsized(TWENTY))
            .child(container().padding(4.0).child(swatch())),
    );
    assert_eq!(bounds(&h, &[1, 0]).width, 60.0, "the icon keeps its width");
    assert_eq!(bounds(&h, &[1]).width, 68.0);
    assert!(ends(bounds(&h, &[1])) <= 200.01);
}

#[test]
fn a_container_that_does_not_clip_keeps_its_content() {
    // Ten glyphs that neither wrap nor ellipsize: a minimum of their full
    // width, which a container that does not clip carries up.
    let h = laid_out(
        row(200.0)
            .child(container().child(label("abcdefghij")))
            .child(ellipsized(TWENTY)),
    );
    let kept = bounds(&h, &[0]);
    assert!(
        (kept.width - TWENTY_WIDE / 2.0).abs() < 0.01,
        "the container went below its content: {kept:?}"
    );
    let title = bounds(&h, &[1]);
    assert!(
        ends(title) <= 200.01,
        "the title takes the rest, got {title:?}"
    );
}

#[test]
fn a_container_that_clips_gives_its_content_up() {
    let h = laid_out(
        row(200.0)
            .child(
                container()
                    .overflow(Overflow::Hidden)
                    .child(label("abcdefghij")),
            )
            .child(ellipsized(TWENTY)),
    );
    let clipped = bounds(&h, &[0]);
    let title = bounds(&h, &[1]);
    // 252.86 shared over 200: each keeps 200 / 252.86 of its natural size.
    let share = 200.0 / (TWENTY_WIDE * 1.5);
    assert!(
        (clipped.width - TWENTY_WIDE / 2.0 * share).abs() < 0.05,
        "the clipping container takes its proportional share, got {clipped:?}"
    );
    assert!(
        took(title, TWENTY_WIDE * share),
        "and so does the title, got {title:?}"
    );
}

#[test]
fn a_child_held_at_its_minimum_leaves_the_rest_to_the_others() {
    // Padding of 40 round a panel that clips: 100 wide, and never under the
    // 80 of its padding. Proportionally it would be offered 74.5, so it is
    // held at 80 and the title takes 120.
    let h = laid_out(
        row(200.0)
            .child(container().padding(40.0).child(clipping(20.0)))
            .child(ellipsized(TWENTY)),
    );
    assert_eq!(bounds(&h, &[0]).width, 80.0);
    let title = bounds(&h, &[1]);
    assert!(
        took(title, 120.0),
        "the title takes what the held box leaves, got {title:?}"
    );
}

#[test]
fn children_whose_minimums_do_not_fit_overflow_as_before() {
    let h = laid_out(
        row(200.0)
            .child(container().width(150.0).height(10.0))
            .child(container().width(150.0).height(10.0)),
    );
    assert_eq!(bounds(&h, &[0]).width, 150.0);
    assert_eq!(bounds(&h, &[1]), Rect::new(150.0, 0.0, 150.0, 10.0));
}

#[test]
fn a_column_shrinks_a_scroller_under_a_header() {
    // A header and a list that scrolls, in a column 100 tall: the list was
    // offered the whole column and ran 30 past it. A scroller's minimum is
    // nothing, so it takes what the header leaves.
    let h = laid_out(
        container()
            .height(100.0)
            .layout(Flex::column())
            .child(container().height(30.0))
            .child(
                container()
                    .scroll(Scroll::vertical())
                    .child(container().height(200.0)),
            ),
    );
    let list = bounds(&h, &[1]);
    assert_eq!(list.y, 30.0);
    assert!(
        (list.height - 70.0).abs() < 0.01,
        "the list ends inside the column, got {list:?}"
    );
}

#[test]
fn a_text_that_wraps_keeps_its_widest_word() {
    // Eleven glyphs that wrap, in a row 100 wide beside a box of 60: the
    // share left is 40, under the seven-glyph word, so the text is held at
    // that word and the row runs past its end rather than break inside it.
    let h = laid_out(
        row(100.0).child(swatch()).child(
            text("abc defghij")
                .font_size(14.0)
                .font_family(FontFamily::name("DejaVu Sans Mono")),
        ),
    );
    let wrapped = bounds(&h, &[1]);
    assert!(
        (wrapped.width - 7.0 * GLYPH).abs() < 0.01,
        "the text is as narrow as its widest word, got {wrapped:?}"
    );
}

#[test]
fn a_child_held_first_does_not_take_more_from_those_after_it() {
    // Three panels 100 wide in a row of 150, whose least is their padding:
    // 55, 30 and nothing. Proportionally each keeps 50, under the first one's
    // 55, so it is held there and the other two share the 95 left — 47.5
    // each, above the second one's 30. Holding the first must not shrink the shares of the two
    // after it in the same round.
    let padded = |pad: f32| container().padding(pad).child(clipping(100.0 - 2.0 * pad));
    let h = laid_out(
        row(150.0)
            .child(padded(27.5))
            .child(padded(15.0))
            .child(clipping(100.0)),
    );
    assert_eq!(bounds(&h, &[0]).width, 55.0);
    assert_eq!(bounds(&h, &[1]).width, 47.5);
    assert_eq!(bounds(&h, &[2]).width, 47.5);
}
