//! A subtree collapsed to nothing answers for nothing.
//!
//! `scale(0.0)` on either axis squashes the plane onto a line, and a matrix
//! that has done that has no inverse — so the hit test, which works by undoing
//! the transform before comparing against the laid-out bounds, has nothing to
//! undo it with. It used to hand the point back unchanged, and an invisible
//! subtree answered for the whole area it covers when it is open: #227.
//!
//! Every case here is one of the five the issue says a fix has to hold at
//! once, and each is a shape one of the three rejected attempts got wrong.

use std::cell::Cell;
use std::rc::Rc;

use guido::layout::Constraints;
use guido::prelude::*;
use guido::tree::{Tree, WidgetId};
use guido::widgets::widget::EventResponse;

struct H {
    tree: Tree,
    root: WidgetId,
}

impl H {
    fn new(widget: impl Widget + 'static) -> Self {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));
        let mut h = Self { tree, root };
        h.lay_out();
        h
    }

    fn lay_out(&mut self) {
        let root = self.root;
        self.tree.with_widget_mut(root, |w, id, t| {
            w.layout(t, id, Constraints::new(0.0, 0.0, 400.0, 200.0))
        });
    }

    fn send(&mut self, event: Event) -> EventResponse {
        let root = self.root;
        guido::reactive::diagnostics::snapshot_zone(|| {
            self.tree
                .with_widget_mut(root, |w, id, t| w.event(t, id, &event))
                .expect("the root is registered")
        })
    }

    fn click(&mut self, x: f32, y: f32) {
        self.send(Event::MouseDown {
            at: Some(Point::new(x, y)),
            button: MouseButton::Left,
        });
        self.send(Event::MouseUp {
            at: Some(Point::new(x, y)),
            button: MouseButton::Left,
        });
    }
}

/// A counter a callback bumps, readable from the test.
fn counter() -> (Rc<Cell<u32>>, impl Fn() + 'static) {
    let count = Rc::new(Cell::new(0));
    let bump = counting(Rc::clone(&count));
    (count, bump)
}

/// Another callback bumping the same counter — for the handlers that take
/// arguments this file does not care about.
fn counting(count: Rc<Cell<u32>>) -> impl Fn() + 'static {
    move || count.set(count.get() + 1)
}

/// The collapsed idiom the enter transition documents: full width, no height.
const COLLAPSED: Scale = Scale::new(1.0, 0.0);

// ---------------------------------------------------------------------------
// 1. A collapsed container takes nothing, and neither does anything inside it
// ---------------------------------------------------------------------------

#[test]
fn a_collapsed_container_takes_no_click() {
    let (clicks, bump) = counter();
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .scale(COLLAPSED)
            .on_click(bump),
    );

    h.click(40.0, 20.0);
    assert_eq!(
        clicks.get(),
        0,
        "a container squashed to nothing must not answer for the area it \
         covers when it is open"
    );
}

/// The case a test with no children hides, and the one that matters: a menu
/// *is* its buttons, and every one of them used to answer while it was closed.
#[test]
fn nothing_inside_a_collapsed_container_takes_a_click() {
    let (clicks, bump) = counter();
    let mut h = H::new(
        container().width(80.0).height(40.0).scale(COLLAPSED).child(
            container()
                .width(80.0)
                .height(40.0)
                .background(Color::RED)
                .on_click(bump),
        ),
    );

    h.click(40.0, 20.0);
    assert_eq!(clicks.get(), 0, "an invisible button is not a button");
}

/// Wherever the collapse sits, everything below it goes with it — the
/// determinant of a composed transform is the product, so a collapsed
/// ancestor collapses every descendant.
#[test]
fn a_collapse_reaches_every_depth_below_it() {
    let (clicks, bump) = counter();
    let mut h = H::new(
        container().width(80.0).height(40.0).scale(COLLAPSED).child(
            container().width(80.0).height(40.0).child(
                container()
                    .width(80.0)
                    .height(40.0)
                    .background(Color::RED)
                    .on_click(bump),
            ),
        ),
    );

    h.click(40.0, 20.0);
    assert_eq!(
        clicks.get(),
        0,
        "two levels down is still below the collapse"
    );
}

/// And a sibling stacked with it gets the click the collapsed one used to
/// swallow. Children are asked in declaration order, so the collapsed one is
/// declared first: it is the one that answered before anybody else was asked.
#[test]
fn a_collapsed_container_does_not_swallow_a_siblings_click() {
    let (clicks, bump) = counter();
    let mut h = H::new(
        container()
            .layout(guido::layout::ZStack::new())
            .width(80.0)
            .height(40.0)
            .child(
                container()
                    .width(80.0)
                    .height(40.0)
                    .scale(COLLAPSED)
                    .on_click(|| panic!("the collapsed one must not answer")),
            )
            .child(
                container()
                    .width(80.0)
                    .height(40.0)
                    .background(Color::BLUE)
                    .on_click(bump),
            ),
    );

    h.click(40.0, 20.0);
    assert_eq!(
        clicks.get(),
        1,
        "the visible sibling is what was clicked, not the invisible one in \
         front of it in the dispatch order"
    );
}

#[test]
fn a_collapsed_container_takes_no_scroll() {
    let (scrolls, _) = counter();
    let bump = counting(Rc::clone(&scrolls));
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .scale(COLLAPSED)
            .on_scroll(move |_, _, _| bump()),
    );

    h.send(Event::Scroll {
        at: Some(Point::new(40.0, 20.0)),
        delta_x: 0.0,
        delta_y: 10.0,
        source: ScrollSource::Wheel,
    });
    assert_eq!(scrolls.get(), 0, "there is nothing there to scroll");
}

#[test]
fn a_collapsed_container_takes_no_hover() {
    let (hovers, _) = counter();
    let bump = counting(Rc::clone(&hovers));
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .scale(COLLAPSED)
            .on_hover(move |inside| {
                if inside {
                    bump()
                }
            }),
    );

    h.send(Event::MouseMove {
        at: Some(Point::new(40.0, 20.0)),
    });
    assert_eq!(hovers.get(), 0, "the pointer is not over anything");
}

// ---------------------------------------------------------------------------
// 2. What was in flight when it collapsed is cleared, not stranded
// ---------------------------------------------------------------------------

/// The half that refusing events strands: a button pressed when its menu
/// closes has to hear the release, or it stays pressed for good.
///
/// Asserted through `on_click`, which fires only for a release *inside* the
/// shape: the press must be given up, so the release that follows must not
/// activate anything.
#[test]
fn a_press_in_flight_is_given_up_when_the_container_collapses() {
    let open = create_signal(true);
    let (clicks, bump) = counter();
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .scale(move || if open.get() { Scale::NONE } else { COLLAPSED })
            .on_click(bump),
    );

    h.send(Event::MouseDown {
        at: Some(Point::new(40.0, 20.0)),
        button: MouseButton::Left,
    });

    open.set(false);
    h.lay_out();

    h.send(Event::MouseUp {
        at: Some(Point::new(40.0, 20.0)),
        button: MouseButton::Left,
    });
    assert_eq!(
        clicks.get(),
        0,
        "a release into nothing activates nothing — and the press has to be \
         given up rather than left standing"
    );

    // And the container is genuinely no longer pressed: reopening it and
    // releasing again must not fire the click the first release left owing.
    open.set(true);
    h.lay_out();
    h.send(Event::MouseUp {
        at: Some(Point::new(40.0, 20.0)),
        button: MouseButton::Left,
    });
    assert_eq!(clicks.get(), 0, "the press did not survive the collapse");
}

/// The same for hover, which is what a state layer paints from.
#[test]
fn a_hover_in_flight_is_cleared_when_the_container_collapses() {
    let open = create_signal(true);
    let hovers = Rc::new(Cell::new(Vec::new()));
    let seen = Rc::clone(&hovers);
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .scale(move || if open.get() { Scale::NONE } else { COLLAPSED })
            .on_hover(move |inside| {
                let mut v = seen.take();
                v.push(inside);
                seen.set(v);
            }),
    );

    h.send(Event::MouseMove {
        at: Some(Point::new(40.0, 20.0)),
    });
    assert_eq!(hovers.take(), vec![true], "hovered while it was open");

    open.set(false);
    h.lay_out();
    h.send(Event::MouseMove {
        at: Some(Point::new(40.0, 20.0)),
    });
    assert_eq!(
        hovers.take(),
        vec![false],
        "and told it is no longer hovered once there is nothing to hover"
    );
}

/// And for a widget that hit-tests on its own bounds rather than through the
/// container's `HitContext` — a text input keeps a hover flag and an I-beam
/// cursor of its own, and both have to fall when the box holding it does.
///
/// This is the half a `Some(at)` pattern silently drops: an arm that only
/// matches a positioned move is never entered by a positionless one, so the
/// state it would have cleared stays exactly as it was.
#[test]
fn a_text_input_inside_a_collapsing_container_stops_being_hovered() {
    let open = create_signal(true);
    let text = create_signal(String::from("hello"));
    let mut h = H::new(
        container()
            .width(200.0)
            .height(40.0)
            .scale(move || if open.get() { Scale::NONE } else { COLLAPSED })
            .child(text_input(text)),
    );

    h.send(Event::MouseMove {
        at: Some(Point::new(5.0, 5.0)),
    });
    assert_eq!(
        guido::reactive::cursor::get_current_cursor(),
        CursorIcon::Text,
        "the pointer is over the field while the box is open"
    );

    open.set(false);
    h.lay_out();
    h.send(Event::MouseMove {
        at: Some(Point::new(5.0, 5.0)),
    });
    assert_eq!(
        guido::reactive::cursor::get_current_cursor(),
        CursorIcon::Default,
        "and over nothing once it has collapsed"
    );
}

// ---------------------------------------------------------------------------
// 3. A key still reaches a focused descendant, which has no position
// ---------------------------------------------------------------------------

#[test]
fn a_key_still_reaches_a_descendant_of_a_collapsed_container() {
    let (keys, _) = counter();
    let bump = counting(Rc::clone(&keys));
    let mut h = H::new(
        container().width(80.0).height(40.0).scale(COLLAPSED).child(
            container()
                .width(80.0)
                .height(40.0)
                .on_key_down(move |_, _| bump()),
        ),
    );

    h.send(Event::KeyDown {
        key: Key::Enter,
        modifiers: Modifiers::default(),
    });
    assert_eq!(
        keys.get(),
        1,
        "a key has no position to be wrong about, so a collapse says nothing \
         about it"
    );
}

// ---------------------------------------------------------------------------
// 4. A selection or a drag in flight is not snapped to zero
//
// Asserted where the state lives, because nothing outside the crate can read a
// selection back: `text_input::tests::a_move_with_no_position_leaves_the_
// selection_where_it_was`. The half of that split which *is* visible from here
// is the hover above, which falls while the selection stands.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 5. Clipping behaves like the rest
// ---------------------------------------------------------------------------

/// `skip_child_dispatch` decides whether a clipping container's children are
/// asked at all, by testing the point against the bounds. With no point there
/// is nothing to fall outside, so the children are still asked — and still
/// answer no.
#[test]
fn a_collapsed_clipping_container_neither_takes_nor_strands() {
    let (clicks, bump) = counter();
    let mut h = H::new(
        container()
            .width(80.0)
            .height(40.0)
            .overflow(Overflow::Hidden)
            .scale(COLLAPSED)
            .child(
                container()
                    .width(80.0)
                    .height(40.0)
                    .background(Color::RED)
                    .on_click(bump),
            ),
    );

    h.click(40.0, 20.0);
    assert_eq!(clicks.get(), 0, "clipped or not, it is not there");
}
