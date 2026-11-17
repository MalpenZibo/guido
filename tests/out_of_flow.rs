//! A child out of its parent's flow is laid out, placed, drawn and answers
//! the pointer as any child does, but takes no part in its parent's size
//! (#922). Where it overflows the parent it is still reachable, as GTK's
//! overlay children and Compose's are, and unlike Flutter's `Stack`.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::widgets::Rect;

mod common;
use common::Harness;

/// A press on the part of an out-of-flow panel that stands outside the stack
/// holding it reaches the panel.
#[test]
fn a_press_where_an_out_of_flow_child_overflows_reaches_it() {
    let pressed = Rc::new(Cell::new(false));
    let heard = pressed.clone();
    let holder = container()
        .layout(ZStack::new().align(Alignment::TOP_CENTER))
        .child(container().width(100.0).height(30.0))
        .child(
            container()
                .width(300.0)
                .height(200.0)
                .out_of_flow(true)
                .on_click(move || heard.set(true)),
        );
    let mut h = Harness::laid_out(
        container()
            .width(400.0)
            .height(400.0)
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(holder),
        400.0,
        400.0,
    );
    let holder = h.tree.get_children(h.root)[0];
    let Rect { x, y, width, .. } = h.tree.get_bounds(holder).expect("laid out");
    assert_eq!((x, y, width), (150.0, 185.0, 100.0), "the island's box");

    // Left of the island and below it, inside the panel only.
    let (px, py) = (60.0, 300.0);
    h.paint();
    h.click(px, py);
    assert!(
        pressed.get(),
        "the press at ({px}, {py}) did not reach the panel"
    );
}
