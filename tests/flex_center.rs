//! `Flex::center` is the four-line spelling of a centred flex, and nothing else.
//!
//! Each case lays the same children out twice, once under `.center()` and
//! once under the explicit `main_alignment` and `cross_alignment` pair, and
//! asks for the same bounds on every child. Children of different sizes, so
//! the cross axis has something to centre; one child and several, a row and
//! a column, with spacing, so the shorthand is seen to compose with it.

use guido::prelude::*;
use guido::widgets::Rect;

mod common;
use common::Harness;

fn boxes(count: usize) -> Vec<Container> {
    (0..count)
        .map(|i| {
            let side = 10.0 + 8.0 * i as f32;
            container().width(side).height(side * 1.5)
        })
        .collect()
}

fn child_bounds(layout: Flex, count: usize) -> Vec<Rect> {
    let h = Harness::laid_out(
        container()
            .width(300.0)
            .height(200.0)
            .layout(layout)
            .children(boxes(count)),
        400.0,
        400.0,
    );
    h.tree
        .get_children(h.root)
        .iter()
        .map(|&id| h.tree.get_bounds(id).expect("laid out"))
        .collect()
}

fn spelled_out(flex: Flex) -> Flex {
    flex.main_alignment(MainAlignment::Center)
        .cross_alignment(CrossAlignment::Center)
}

#[test]
fn a_centred_flex_is_the_two_alignments() {
    for (axis, flex) in [("row", Flex::row as fn() -> Flex), ("column", Flex::column)] {
        for count in [1, 3] {
            let long = child_bounds(spelled_out(flex().spacing(6.0)), count);
            assert_eq!(long.len(), count, "{axis} of {count}: every child laid out");
            assert_eq!(
                child_bounds(flex().spacing(6.0).center(), count),
                long,
                "{axis} of {count}: `.center()` places the children where the \
                 two alignments do"
            );
        }
    }
}

/// The comparison above says the two spellings agree; this says that what
/// they agree on is centred.
#[test]
fn a_centred_child_sits_in_the_middle_on_both_axes() {
    for layout in [Flex::row().center(), Flex::column().center()] {
        let only = child_bounds(layout, 1)[0];
        assert_eq!(
            (only.x + only.width / 2.0, only.y + only.height / 2.0),
            (150.0, 100.0),
            "a lone child's centre is the container's, got {only:?}"
        );
    }
}
