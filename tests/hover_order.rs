//! Within one pointer event, every `on_hover(false)` is heard before any
//! `on_hover(true)` (#657).
//!
//! Leaves go from the widget the pointer left outward, enters from the
//! outermost inward to the widget it reached, and a container that holds both
//! hears nothing. The order is the tree's: not the order the siblings are
//! drawn in, and not the direction the pointer moved. It is the order the DOM,
//! Chromium's boundary dispatcher, Flutter's mouse tracker, GTK and Qt all
//! give.

use std::cell::RefCell;
use std::rc::Rc;

use guido::layout::ZStack;
use guido::prelude::*;

mod common;
use common::Harness;

/// What each `on_hover` said, in the order it said it.
#[derive(Clone, Default)]
struct Heard(Rc<RefCell<Vec<(&'static str, bool)>>>);

impl Heard {
    fn hover(&self, name: &'static str) -> impl Fn(bool) + 'static {
        let heard = self.0.clone();
        move |hovered| heard.borrow_mut().push((name, hovered))
    }

    /// What was heard since the last time this was asked.
    fn take(&self) -> Vec<(&'static str, bool)> {
        self.0.take()
    }
}

/// A 40×40 square that says when it is hovered.
fn square(heard: &Heard, name: &'static str, color: Color) -> Container {
    container()
        .width(40.0)
        .height(40.0)
        .background(color)
        .on_hover(heard.hover(name))
}

#[test]
fn moving_between_siblings_leaves_one_before_entering_the_other() {
    let heard = Heard::default();
    let mut h = Harness::laid_out(
        container().layout(Flex::row()).children([
            square(&heard, "A", Color::RED),
            square(&heard, "B", Color::BLUE),
        ]),
        200.0,
        100.0,
    );

    h.send(Event::mouse_move(20.0, 20.0));
    assert_eq!(heard.take(), [("A", true)], "the setup starts over A");

    h.send(Event::mouse_move(60.0, 20.0));
    assert_eq!(heard.take(), [("A", false), ("B", true)], "A to B");

    h.send(Event::mouse_move(20.0, 20.0));
    assert_eq!(heard.take(), [("B", false), ("A", true)], "and back");
}

#[test]
fn moving_onto_a_sibling_drawn_on_top_leaves_the_one_beneath_first() {
    let heard = Heard::default();
    let mut h = Harness::laid_out(
        container()
            .layout(ZStack::new())
            .width(80.0)
            .height(40.0)
            .children([
                container()
                    .width(80.0)
                    .height(40.0)
                    .background(Color::RED)
                    .on_hover(heard.hover("card")),
                container()
                    .width(16.0)
                    .height(16.0)
                    .background(Color::BLUE)
                    .on_hover(heard.hover("button")),
            ]),
        400.0,
        200.0,
    );

    h.send(Event::mouse_move(60.0, 30.0));
    assert_eq!(
        heard.take(),
        [("card", true)],
        "the setup starts on the card"
    );

    h.send(Event::mouse_move(8.0, 8.0));
    assert_eq!(
        heard.take(),
        [("card", false), ("button", true)],
        "onto the button, which covers the card"
    );

    h.send(Event::mouse_move(60.0, 30.0));
    assert_eq!(
        heard.take(),
        [("button", false), ("card", true)],
        "and off it again"
    );
}

/// A in A1 and B in B1, both under C.
fn nested(heard: &Heard) -> Container {
    let wrap = |name: &'static str, inner: Container| {
        container()
            .padding(10.0)
            .background(Color::rgb(0.2, 0.2, 0.2))
            .on_hover(heard.hover(name))
            .child(inner)
    };
    container()
        .layout(Flex::row())
        .on_hover(heard.hover("C"))
        .children([
            wrap("A1", square(heard, "A", Color::RED)),
            wrap("B1", square(heard, "B", Color::BLUE)),
        ])
}

#[test]
fn moving_between_cousins_leaves_inward_out_then_enters_outward_in() {
    let heard = Heard::default();
    let mut h = Harness::laid_out(nested(&heard), 400.0, 100.0);

    // A1 spans 0..60, A 10..50; B1 60..120, B 70..110.
    h.send(Event::mouse_move(30.0, 30.0));
    assert_eq!(
        heard.take(),
        [("C", true), ("A1", true), ("A", true)],
        "the setup starts over A"
    );

    h.send(Event::mouse_move(90.0, 30.0));
    assert_eq!(
        heard.take(),
        [("A", false), ("A1", false), ("B1", true), ("B", true)],
        "A to B: C holds both, so it hears nothing"
    );

    h.send(Event::mouse_move(30.0, 30.0));
    assert_eq!(
        heard.take(),
        [("B", false), ("B1", false), ("A1", true), ("A", true)],
        "and back"
    );
}

#[test]
fn leaving_the_surface_leaves_the_inner_before_the_outer() {
    let heard = Heard::default();
    let mut h = Harness::laid_out(nested(&heard), 400.0, 100.0);

    h.send(Event::mouse_move(90.0, 30.0));
    heard.take();

    h.send(Event::MouseLeave);
    assert_eq!(
        heard.take(),
        [("B", false), ("B1", false), ("C", false)],
        "from the widget the pointer was on outward"
    );
}

/// A key routed through a container that was disabled under the pointer takes
/// its hover away, and its `on_hover` hears that in the same dispatch rather
/// than at the next pointer event.
#[test]
fn a_key_through_a_container_disabled_under_the_pointer_says_its_hover_went() {
    let heard = Heard::default();
    let enabled = create_signal(true);
    let mut h = Harness::laid_out(
        square(&heard, "A", Color::RED)
            .enabled(enabled)
            .on_key_down(|_, _, _| {}),
        200.0,
        100.0,
    );

    h.send(Event::mouse_move(20.0, 20.0));
    assert_eq!(heard.take(), [("A", true)], "the setup starts over A");

    enabled.set(false);
    h.send(Event::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
        repeat: false,
    });
    assert_eq!(heard.take(), [("A", false)]);
}
