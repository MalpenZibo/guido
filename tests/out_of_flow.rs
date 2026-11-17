//! A child out of its parent's flow is laid out, placed, drawn and answers
//! the pointer as any child does, but takes no part in its parent's size
//! (#922). Where it overflows the parent it is still reachable, as GTK's
//! overlay children and Compose's are, and unlike Flutter's `Stack`.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::tree::LayoutCtx;
use guido::widget_prelude::{Constraints, Layout, WidgetId};
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

/// Lays its children out one beside the other and knows nothing of out of
/// flow: what a user writes.
struct Beside;

impl Layout for Beside {
    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        children: &[WidgetId],
        constraints: Constraints,
        origin: (f32, f32),
    ) -> Size {
        let mut size = Size::zero();
        for &child in children {
            let own = ctx.layout_child(child, constraints).unwrap_or_default();
            ctx.tree()
                .set_origin(child, origin.0 + size.width, origin.1);
            size.width += own.width;
            size.height = size.height.max(own.height);
        }
        size
    }
}

/// A layout written outside the crate leaves a child out of flow out of its
/// size and its sequence without a line for it: the container lays that
/// child out and places it at the top start, the default for a lone child.
#[test]
fn a_user_written_layout_leaves_a_child_out_of_flow_alone() {
    let h = Harness::laid_out(
        container()
            .layout(Beside)
            .child(container().width(40.0).height(20.0))
            .child(container().width(300.0).height(90.0).out_of_flow(true))
            .child(container().width(40.0).height(20.0)),
        400.0,
        400.0,
    );
    let bounds = |h: &Harness, index: usize| {
        let child = h.tree.get_children(h.root)[index];
        h.tree.get_bounds(child).expect("laid out")
    };
    let root = h.tree.get_bounds(h.root).expect("laid out");
    assert_eq!((root.width, root.height), (80.0, 20.0));
    assert_eq!((bounds(&h, 2).x, bounds(&h, 2).y), (40.0, 0.0));
    let out = bounds(&h, 1);
    assert_eq!(
        (out.x, out.y, out.width, out.height),
        (0.0, 0.0, 300.0, 90.0)
    );
}
