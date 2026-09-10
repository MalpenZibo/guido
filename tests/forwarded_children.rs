//! A component places the children it was handed among its own.
//!
//! `#[prop(children)]` gives a component a [`ChildrenSource`] the caller
//! filled in, and the component decides where in its tree those rows belong.
//! The card in the book's component chapter puts a title above them; #353 is
//! that the title used to be thrown away, because forwarding *assigned* the
//! source over whatever the container had already been given.

use guido::component;
use guido::prelude::*;
use guido::widgets::{ChildrenSource, IntoChildren};

mod common;
use common::Harness;

/// A container that paints a rect of its own, so `painted_rects` can see it.
fn block(width: f32) -> Container {
    container()
        .width(width)
        .height(10.0)
        .background(Color::WHITE)
}

/// What `#[prop(children)]` hands a component: a source the caller filled.
fn caller_children(widths: [f32; 2]) -> ChildrenSource {
    let mut source = ChildrenSource::default();
    widths.map(block).add_to_container(&mut source);
    source
}

/// The widths of everything the tree drew, in the order it drew them.
fn widths(h: &mut Harness) -> Vec<f32> {
    h.painted_rects().iter().map(|r| r.width).collect()
}

#[test]
fn a_container_keeps_its_own_children_when_it_forwards() {
    let mut h = Harness::laid_out(
        container()
            .layout(Flex::column())
            .child(block(11.0))
            .children(caller_children([22.0, 33.0])),
        200.0,
        200.0,
    );

    assert_eq!(
        widths(&mut h),
        vec![11.0, 22.0, 33.0],
        "the title a component puts above the children it was handed has to \
         survive the forwarding, and come first"
    );
}

/// The caller's rows can be a reactive list, and the component's own children
/// sit on both sides of it. Segments are what carries that: the forwarded
/// source arrives as its own run, and the static widgets around it keep their
/// place in the order they were written.
#[test]
fn a_forwarded_reactive_list_keeps_the_component_s_own_rows_apart() {
    let rows = create_signal(2usize);

    let mut source = ChildrenSource::default();
    (move || {
        (0..rows.get())
            .map(|i| block(20.0 + i as f32))
            .collect::<Vec<_>>()
    })
    .add_to_container(&mut source);

    let mut h = Harness::laid_out(
        container()
            .layout(Flex::column())
            .child(block(11.0))
            .children(source)
            .child(block(99.0)),
        200.0,
        200.0,
    );

    assert_eq!(
        widths(&mut h),
        vec![11.0, 20.0, 21.0, 99.0],
        "a header, the caller's two reactive rows, then a footer"
    );
}

/// Forwarded rows sit where the call sits, so a component that forwards before
/// adding anything of its own puts its row last.
#[test]
fn forwarded_children_come_before_a_row_written_after_them() {
    let mut h = Harness::laid_out(
        container()
            .layout(Flex::column())
            .children(caller_children([22.0, 33.0]))
            .child(block(11.0)),
        200.0,
        200.0,
    );

    assert_eq!(widths(&mut h), vec![22.0, 33.0, 11.0]);
}

/// The card the book teaches, through the path a caller takes: the macro fills
/// a source from the builder's own `child` calls, hands it to the body as
/// `children`, and the body decides where it goes.
#[component]
fn card(#[prop(children)] children: ()) -> impl Widget {
    container()
        .layout(Flex::column())
        .child(block(11.0))
        .children(children)
        .child(block(99.0))
}

#[test]
fn a_component_keeps_its_own_rows_around_the_ones_it_was_handed() {
    let mut h = Harness::laid_out(card().child(block(22.0)).child(block(33.0)), 200.0, 200.0);

    assert_eq!(
        widths(&mut h),
        vec![11.0, 22.0, 33.0, 99.0],
        "the component's header and footer keep their places around the \
         caller's two rows"
    );
}
