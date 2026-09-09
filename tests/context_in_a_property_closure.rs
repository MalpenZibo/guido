//! One read, two answers — #335.
//!
//! A property closure becomes a derived signal when the builder runs and its
//! body runs later, when something reads it. What scope is current then is not
//! the same on both phases of a frame: laying out a dynamic list re-enters that
//! row's scope (`OwnedWidget::layout`), painting does not. So two properties
//! written side by side, reading the same context, could disagree — the row's
//! value in layout, nothing in paint.

use std::cell::RefCell;
use std::rc::Rc;

use guido::prelude::*;

mod common;
use common::Harness;

/// What a property closure saw, in the order the frame asked.
type Seen = Rc<RefCell<Vec<(&'static str, Option<u32>)>>>;

/// A row in a dynamic list — the library opens a scope for the factory, so the
/// declaration belongs to the row rather than to the surface around it.
///
/// Two properties on one container, reading the same declaration: `width` is
/// read when the tree is laid out, `background` when it is painted.
fn a_row_declaring_its_own_value(seen: Seen) -> Container {
    let for_layout = Rc::clone(&seen);
    let for_paint = seen;

    container().width(fill()).height(fill()).child(move || {
        // The factory runs once per build of the row, so its captures are
        // cloned rather than moved out.
        let for_layout = Rc::clone(&for_layout);
        let for_paint = Rc::clone(&for_paint);
        provide_context(42u32);

        container()
            .width(move || {
                for_layout
                    .borrow_mut()
                    .push(("layout", use_context::<u32>()));
                80.0
            })
            .height(20.0)
            .background(move || {
                for_paint.borrow_mut().push(("paint", use_context::<u32>()));
                Color::rgb(0.5, 0.5, 0.5)
            })
    })
}

#[test]
fn a_property_closure_reads_the_same_scope_on_both_phases_of_a_frame() {
    let seen: Seen = Rc::default();
    let mut surface = Harness::new(a_row_declaring_its_own_value(Rc::clone(&seen)));

    surface.lay_out(200.0, 100.0);
    surface.paint();

    let seen = seen.borrow();
    let in_layout: Vec<Option<u32>> = seen
        .iter()
        .filter(|(phase, _)| *phase == "layout")
        .map(|(_, value)| *value)
        .collect();
    let in_paint: Vec<Option<u32>> = seen
        .iter()
        .filter(|(phase, _)| *phase == "paint")
        .map(|(_, value)| *value)
        .collect();

    assert!(
        !in_layout.is_empty() && !in_paint.is_empty(),
        "both phases have to have read something for this to say anything: \
         layout {in_layout:?}, paint {in_paint:?}"
    );
    assert_eq!(
        in_layout[0], in_paint[0],
        "one read, two answers: the row's declaration is visible to a property \
         closure evaluated during layout and not to one evaluated during paint"
    );
    assert_eq!(
        in_layout[0],
        Some(42),
        "and the answer is the scope the closure was written in"
    );
}

/// The other half of the same rule: a closure written where nothing declared
/// the type reads nothing, on either phase, rather than picking up whatever the
/// scope it happens to be evaluated under has.
#[test]
fn a_property_closure_outside_the_declaration_reads_nothing_on_either_phase() {
    let seen: Seen = Rc::default();
    let for_layout = Rc::clone(&seen);
    let for_paint = Rc::clone(&seen);

    let mut surface = Harness::new(
        container()
            .width(move || {
                for_layout
                    .borrow_mut()
                    .push(("layout", use_context::<u32>()));
                80.0
            })
            .height(20.0)
            .background(move || {
                for_paint.borrow_mut().push(("paint", use_context::<u32>()));
                Color::rgb(0.5, 0.5, 0.5)
            })
            .child(|| {
                provide_context(42u32);
                container().width(10.0).height(10.0)
            }),
    );

    surface.lay_out(200.0, 100.0);
    surface.paint();

    for (phase, value) in seen.borrow().iter() {
        assert_eq!(
            *value, None,
            "a declaration inside the row is not above the closure that reads \
             here, on either phase — {phase} saw {value:?}"
        );
    }
}
