//! A click lands on a widget only where it is drawn: inside every clip above
//! it, each in its own shape and its own frame (#704, the hit-testing half of
//! #698).
//!
//! Each case pairs a point the shape cuts away with one it keeps, so a test
//! that stopped every click would not pass either.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::transform::Transform;

mod common;
use common::Harness;

/// Where every card is placed, so that what a turn or a stretch moves stays on
/// the surface.
const AT: f32 = 100.0;

fn counter() -> (Rc<Cell<u32>>, impl Fn() + 'static) {
    let count = Rc::new(Cell::new(0));
    let bump = {
        let count = Rc::clone(&count);
        move || count.set(count.get() + 1)
    };
    (count, bump)
}

/// A row taller than any card here, which takes the click it is given.
fn row(bump: impl Fn() + 'static) -> Container {
    container()
        .width(fill())
        .height(400.0)
        .background(Color::RED)
        .on_click(bump)
}

/// How many of the clicks at `points` reach the row inside `card`, placed at
/// (`AT`, `AT`), one count per point.
fn clicks_at(card: impl FnOnce(Container) -> Container, points: &[(f32, f32)]) -> Vec<u32> {
    let (count, bump) = counter();
    let mut h = Harness::laid_out(container().padding(AT).child(card(row(bump))), 500.0, 500.0);
    points
        .iter()
        .map(|&(x, y)| {
            count.set(0);
            h.click(AT + x, AT + y);
            count.get()
        })
        .collect()
}

/// Where `transform`, applied as a `w`×`h` card applies it — about its
/// default pivot — puts a point of the card's own frame.
fn placed(transform: Transform, w: f32, h: f32, (x, y): (f32, f32)) -> (f32, f32) {
    transform
        .about(Pivot::default(), Rect::new(0.0, 0.0, w, h))
        .transform_point(x, y)
}

#[test]
fn a_rounded_cards_corner_does_not_reach_the_row_inside() {
    let card = |row| {
        container()
            .width(200.0)
            .height(200.0)
            .corners(20.0)
            .overflow(Overflow::Hidden)
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .scroll(Scroll::vertical())
                    .child(row),
            )
    };
    // (2, 2) is 25.5 from the corner's centre at (20, 20), past its radius;
    // (10, 10) is 14.1 from it, inside.
    assert_eq!(clicks_at(card, &[(2.0, 2.0), (10.0, 10.0)]), [0, 1]);
}

#[test]
fn a_circle_over_a_squircle_cuts_where_their_corners_cross() {
    let card = |row| {
        container()
            .width(200.0)
            .height(200.0)
            .padding(10.0)
            .corners(100.0)
            .overflow(Overflow::Hidden)
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .corners(Corners::squircle(60.0))
                    .overflow(Overflow::Hidden)
                    .child(row),
            )
    };
    // (25, 25) is inside the squircle — 53.5 from its corner's centre at
    // (70, 70) in its own norm, radius 60 — and 106 from the circle's centre,
    // radius 100. (40, 40) is inside both.
    assert_eq!(clicks_at(card, &[(25.0, 25.0), (40.0, 40.0)]), [0, 1]);
}

#[test]
fn a_turned_clip_cuts_in_its_own_frame() {
    let card = |row| {
        container()
            .width(200.0)
            .height(100.0)
            .corners(20.0)
            .rotate(25.0)
            .scroll(Scroll::vertical())
            .child(row)
    };
    let turn = |at| placed(Transform::rotate_degrees(25.0), 200.0, 100.0, at);
    // The cut-off corner; below the card, where the row it scrolls goes on;
    // and well inside.
    assert_eq!(
        clicks_at(
            card,
            &[turn((2.0, 2.0)), turn((100.0, 130.0)), turn((10.0, 50.0))]
        ),
        [0, 0, 1]
    );
}

#[test]
fn a_stretched_clip_cuts_in_its_own_frame() {
    let card = |row| {
        container()
            .width(100.0)
            .height(100.0)
            .corners(20.0)
            .scale(Scale::new(2.0, 1.0))
            .scroll(Scroll::vertical())
            .child(row)
    };
    let stretch = |at| placed(Transform::scale_xy(2.0, 1.0), 100.0, 100.0, at);
    // The cut-off corner, and well inside.
    assert_eq!(
        clicks_at(card, &[stretch((2.0, 2.0)), stretch((10.0, 50.0))]),
        [0, 1]
    );
}
