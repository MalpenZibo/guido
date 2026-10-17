//! A `ZStack` places each child by an alignment: the stack's own, or the one
//! the child declares for itself (#781).
//!
//! Every case is a 100×60 stack, so the free space on each axis is the
//! stack's size less the child's, and an alignment takes a share of it: none
//! at the start, half at the centre, all of it at the end. A child larger than
//! the stack leaves negative free space, and the share of that is an overhang.

use guido::prelude::*;
use guido::widget_prelude::*;

mod common;
use common::Harness;

fn stack(layout: ZStack, child: impl Widget + 'static) -> Container {
    container()
        .width(100.0)
        .height(60.0)
        .layout(layout)
        .child(child)
}

/// Where a 40×20 child of a stack aligned by `alignment` is placed.
fn placed(alignment: Alignment) -> (f32, f32) {
    child_at(stack(
        ZStack::new().align(alignment),
        container().width(40.0).height(20.0),
    ))
}

fn child_at(widget: Container) -> (f32, f32) {
    let h = Harness::laid_out(widget, 400.0, 400.0);
    let child = h.tree.get_children(h.root)[0];
    let Rect { x, y, .. } = h.tree.get_bounds(child).expect("laid out");
    (x, y)
}

/// A leaf that is the size it says whatever it is offered. A container's
/// exact length is cut to what its parent offers, so nothing built from one
/// is ever wider than the stack — but a widget's layout can answer anything.
struct Fixed(Size);

impl Widget for Fixed {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        self.0
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

/// Flutter's `Stack(alignment:)` and SwiftUI's `ZStack(alignment:)` do the
/// same: the overflow is shared as the free space would have been.
#[test]
fn a_child_wider_than_the_stack_overhangs_both_sides_equally() {
    // At the end, all of it past the start.
    for (alignment, at) in [
        (Alignment::CENTER, (-20.0, 20.0)),
        (Alignment::CENTER_END, (-40.0, 20.0)),
    ] {
        assert_eq!(
            child_at(stack(
                ZStack::new().align(alignment),
                Fixed(Size::new(140.0, 20.0)),
            )),
            at,
            "{alignment:?}"
        );
    }
}

/// What every existing stack was written against.
#[test]
fn a_stack_that_declares_nothing_aligns_to_the_top_start() {
    assert_eq!(
        child_at(stack(ZStack::new(), container().width(40.0).height(20.0))),
        (0.0, 0.0)
    );
}

#[test]
fn a_child_that_declares_its_own_alignment_overrides_the_stacks() {
    assert_eq!(
        child_at(stack(
            ZStack::new().align(Alignment::CENTER),
            container()
                .width(40.0)
                .height(20.0)
                .align_self(Alignment::BOTTOM_END),
        )),
        (60.0, 40.0)
    );
}

/// Every one of the nine, so a constant pointing at the wrong corner is seen
/// — the issue's own two among them: `CENTER` at (30, 20), `TOP_END` at
/// (60, 0).
#[test]
fn each_constant_names_its_place() {
    let cases = [
        (Alignment::TOP_START, (0.0, 0.0)),
        (Alignment::TOP_CENTER, (30.0, 0.0)),
        (Alignment::TOP_END, (60.0, 0.0)),
        (Alignment::CENTER_START, (0.0, 20.0)),
        (Alignment::CENTER, (30.0, 20.0)),
        (Alignment::CENTER_END, (60.0, 20.0)),
        (Alignment::BOTTOM_START, (0.0, 40.0)),
        (Alignment::BOTTOM_CENTER, (30.0, 40.0)),
        (Alignment::BOTTOM_END, (60.0, 40.0)),
    ];
    for (alignment, at) in cases {
        assert_eq!(placed(alignment), at, "{alignment:?}");
    }
}

/// A follower is laid out at the stack's size on the axes it fills, so there
/// is nothing to share there and it stays on the stack: aligning the stack
/// moves the leader, never the background that follows it.
#[test]
fn a_follower_keeps_the_stacks_size_and_place() {
    let h = Harness::laid_out(
        container()
            .layout(ZStack::new().align(Alignment::CENTER))
            .child(container().width(fill()).height(fill()))
            .child(container().width(80.0).height(20.0))
            .child(container().width(40.0).height(10.0)),
        400.0,
        400.0,
    );
    let bounds = |i: usize| {
        h.tree
            .get_bounds(h.tree.get_children(h.root)[i])
            .expect("laid out")
    };
    assert_eq!(bounds(0), Rect::new(0.0, 0.0, 80.0, 20.0));
    assert_eq!(bounds(1), Rect::new(0.0, 0.0, 80.0, 20.0));
    assert_eq!(bounds(2), Rect::new(20.0, 5.0, 40.0, 10.0));
}
