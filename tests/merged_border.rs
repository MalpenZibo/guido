//! A merging container's border is drawn along the merged shape, never round
//! the container's own box, so its box keeps none of it (#902). The layout and
//! the clip are in the `a_merged_border_takes_no_room_in_its_box` snapshot;
//! this is the hit test, which has to agree with them.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;

mod common;
use common::Harness;

/// A click in the band the border would have kept reaches the child drawn
/// there: the clip the hit test asks about is the box's, as the paint's is.
#[test]
fn a_click_inside_a_merging_box_reaches_the_child_at_its_edge() {
    let clicked = Rc::new(Cell::new(0));
    let counted = clicked.clone();
    let mut h = Harness::laid_out(
        container().child(
            container()
                .layout(ZStack::new())
                .merge_children(8.0)
                .border(2.0, Color::WHITE)
                .overflow(Overflow::Hidden)
                .child(
                    container()
                        .width(40.0)
                        .height(40.0)
                        .background(Color::BLACK)
                        .on_click(move || counted.set(counted.get() + 1)),
                ),
        ),
        200.0,
        200.0,
    );

    h.click(1.0, 1.0);

    assert_eq!(
        clicked.get(),
        1,
        "the child is drawn there, and answers there"
    );
}
