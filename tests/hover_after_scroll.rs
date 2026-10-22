//! A scroll that moves content under a still pointer moves the hover with it
//! (#842).
//!
//! Hover is worked out on pointer events, and a wheel moves what is under the
//! pointer without one. The row that scrolled away has to hear that it lost
//! the hover, and the row now under the pointer that it has it, without
//! waiting for the pointer to move.

use std::cell::RefCell;
use std::rc::Rc;

use guido::layout::Flex;
use guido::prelude::*;

mod common;
use common::Harness;

/// What each row's `on_hover` said, in the order it said it.
type Heard = Rc<RefCell<Vec<(usize, bool)>>>;

/// A 100×100 scroller over a column of four 100×50 rows, each saying when it
/// is hovered, with the pointer resting on row 0.
fn list() -> (Harness, Heard) {
    let heard: Heard = Rc::default();
    let rows = (0..4).map(|n| {
        let heard = heard.clone();
        container()
            .width(100.0)
            .height(50.0)
            .on_hover(move |hovered| heard.borrow_mut().push((n, hovered)))
            .into_any()
    });
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical())
        .child(container().layout(Flex::column()).children(rows));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 25.0));
    assert_eq!(heard.take(), [(0, true)], "the pointer rests on row 0");
    (h, heard)
}

fn content_at(h: &mut Harness) -> f32 {
    h.paint().children[0].local_transform.ty()
}

/// One wheel step of 50 under the still pointer moves the hover from row 0,
/// scrolled away, to row 1, now under it.
#[test]
fn a_wheel_step_moves_the_hover_to_the_row_under_the_pointer() {
    let (mut h, heard) = list();
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    assert_eq!(content_at(&mut h), -50.0, "the step moved the content");
    assert_eq!(heard.take(), [(0, false), (1, true)]);
}

/// A step that moves nothing, against the end of the list, changes nothing
/// under the pointer and says nothing.
#[test]
fn a_wheel_step_against_the_end_leaves_the_hover() {
    let (mut h, heard) = list();
    h.send(Event::scroll(50.0, 25.0, 0.0, -50.0, ScrollSource::Wheel));
    assert_eq!(content_at(&mut h), 0.0, "the list was already at its top");
    assert!(heard.take().is_empty(), "nothing moved, so nothing changed");
}

/// The cursor goes the same way: a row that shows the hand scrolls away, the
/// row now under the pointer shows none, and the cursor said for the point is
/// none, not the hand the scrolled-away row asked for.
#[test]
fn a_wheel_step_moves_the_cursor_to_what_is_under_the_pointer() {
    let rows = (0..4).map(|n| {
        let row = container().width(100.0).height(50.0);
        if n == 0 {
            row.cursor(CursorIcon::Pointer).into_any()
        } else {
            row.into_any()
        }
    });
    let view = container()
        .width(100.0)
        .height(100.0)
        .scroll(Scroll::vertical())
        .child(container().layout(Flex::column()).children(rows));
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    // Declared going in and read coming out, as the loop does.
    let cursor_after = |h: &mut Harness, event: Event| {
        h.tree.set_event_instant(Some(std::time::Instant::now()));
        h.send(event);
        let cursor = h.tree.cursor_under_the_point().get_untracked();
        h.tree.set_event_instant(None);
        cursor
    };
    assert_eq!(
        cursor_after(&mut h, Event::mouse_move(50.0, 25.0)),
        Some(CursorIcon::Pointer),
        "the pointer rests on the row that shows the hand"
    );
    assert_eq!(
        cursor_after(
            &mut h,
            Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel)
        ),
        None,
        "the row under the pointer after the step shows no cursor of its own"
    );
}

/// A scroll a callback takes, moving no content, moves nothing under the
/// pointer: it is not followed by a move, and what hears the pointer's moves
/// hears none.
#[test]
fn a_scroll_that_moves_no_content_moves_nothing_under_the_pointer() {
    let moves = Rc::new(RefCell::new(0u32));
    let counted = moves.clone();
    // Inside a box, so that the pointer record keeps where the pointer is.
    let view = container().child(
        container()
            .width(100.0)
            .height(100.0)
            .on_scroll(|_, _, _| {})
            .on_pointer_move(move |_, _| *counted.borrow_mut() += 1),
    );
    let mut h = Harness::laid_out(view, 100.0, 100.0);
    h.send(Event::mouse_move(50.0, 25.0));
    let before = *moves.borrow();
    assert!(before > 0, "the pointer's move was heard");
    h.send(Event::scroll(50.0, 25.0, 0.0, 50.0, ScrollSource::Wheel));
    assert_eq!(
        *moves.borrow(),
        before,
        "a scroll that moved nothing is no move"
    );
}
