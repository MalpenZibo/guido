//! One route per chord (#625).
//!
//! The first button pressed decides the press route, and every later press,
//! move and release follows it until the last button is up. A press or release
//! of another button while one is held is still its own event, but it goes
//! along that route and is not hit-tested again. Before this, any release
//! dropped the route and any press replaced it, so a right-click in the middle
//! of a left drag lost the drag's moves and its release.
//!
//! The sequences are the ones the issue and its discussion reported; the
//! controls are the routes that already worked and must keep working.

use std::cell::RefCell;
use std::rc::Rc;

use guido::prelude::*;
use guido::widget_prelude::*;

mod common;
use common::Harness;

/// What a widget was told, in the order it was told it.
type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn x_of(at: &Option<Point>) -> String {
    at.map_or("none".into(), |at| format!("{}", at.x))
}

/// A 100x20 leaf that writes down every pointer event it is offered.
struct Recorder(Log);

impl Widget for Recorder {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(100.0, 20.0)
    }

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, event: &Event) -> EventResponse {
        let entry = match event {
            Event::MouseDown { at, button, .. } => format!("down {button:?} {}", x_of(at)),
            Event::MouseUp { at, button } => format!("up {button:?} {}", x_of(at)),
            Event::MouseMove { at, .. } => format!("move {}", x_of(at)),
            _ => return EventResponse::Ignored,
        };
        self.0.borrow_mut().push(entry);
        EventResponse::Ignored
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

/// A 100x20 container that listens for a press, its moves and its release.
fn listening(log: &Log) -> Container {
    let (down, moved, up) = (log.clone(), log.clone(), log.clone());
    container()
        .width(100.0)
        .height(20.0)
        .on_mouse_down(move |x, _| down.borrow_mut().push(format!("down{x}")))
        .on_pointer_move(move |x, _| moved.borrow_mut().push(format!("move{x}")))
        .on_mouse_up(move |x, _| up.borrow_mut().push(format!("up{x}")))
}

/// The issue's layout: a 500px row of five 100px containers, the first of
/// which is listening.
fn row_with_a_listening_first(first: &Log) -> Harness {
    let row = container()
        .layout(Flex::row())
        .child(listening(first))
        .children((0..4).map(|_| container().width(100.0).height(20.0)));
    Harness::laid_out(row, 500.0, 20.0)
}

/// A 500px row of five recorders.
fn row_of_recorders() -> (Harness, Vec<Log>) {
    let logs: Vec<Log> = (0..5).map(|_| log()).collect();
    let row = container().layout(Flex::row()).children(
        logs.iter()
            .map(|log| Box::new(Recorder(log.clone())) as Box<dyn Widget>)
            .collect::<Vec<_>>(),
    );
    (Harness::laid_out(row, 500.0, 20.0), logs)
}

fn presses_and_releases(log: &Log) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|entry| !entry.starts_with("move"))
        .cloned()
        .collect()
}

#[test]
fn another_button_pressed_and_released_during_a_left_drag_does_not_end_it() {
    for other in [MouseButton::Right, MouseButton::Middle] {
        let first = log();
        let mut h = row_with_a_listening_first(&first);

        h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
        h.send(Event::mouse_down(10.0, 10.0, other));
        h.send(Event::mouse_move(450.0, 10.0));
        h.send(Event::mouse_up(450.0, 10.0, other));
        h.send(Event::mouse_move(460.0, 10.0));
        h.send(Event::mouse_up(460.0, 10.0, MouseButton::Left));

        assert_eq!(
            *first.borrow(),
            ["down10", "move450", "move460", "up460"],
            "a {other:?} press and release while Left was held dropped the \
             left drag's route"
        );
    }
}

#[test]
fn a_second_press_over_another_container_does_not_take_the_route() {
    let first = log();
    let other = log();
    let (clicked, down, up) = (other.clone(), other.clone(), other.clone());
    let row = container()
        .layout(Flex::row())
        .child(listening(&first))
        .children((0..3).map(|_| container().width(100.0).height(20.0)))
        .child(
            container()
                .width(100.0)
                .height(20.0)
                .on_right_click(move || clicked.borrow_mut().push("right click".into()))
                .on_mouse_down(move |x, _| down.borrow_mut().push(format!("down{x}")))
                .on_mouse_up(move |x, _| up.borrow_mut().push(format!("up{x}"))),
        );
    let mut h = Harness::laid_out(row, 500.0, 20.0);

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(450.0, 10.0));
    h.send(Event::mouse_down(450.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_move(460.0, 10.0));
    h.send(Event::mouse_up(460.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_up(460.0, 10.0, MouseButton::Right));

    assert_eq!(
        *first.borrow(),
        ["down10", "move450", "move460", "up460"],
        "the first container keeps the drag it started"
    );
    assert!(
        other.borrow().is_empty(),
        "a press inside a chord is not hit-tested, so the container under it \
         is told nothing: {:?}",
        other.borrow()
    );
}

#[test]
fn the_first_button_of_a_chord_decides_the_route_for_the_others() {
    let (mut h, logs) = row_of_recorders();

    // Right goes down on the first widget, Left on the fourth while Right is
    // still held, and Right comes up before Left.
    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_move(350.0, 10.0));
    h.send(Event::mouse_down(350.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_up(350.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_move(360.0, 10.0));
    h.send(Event::mouse_up(360.0, 10.0, MouseButton::Left));

    assert_eq!(
        *logs[0].borrow(),
        [
            "down Right 10",
            "move 350",
            "down Left 350",
            "up Right 350",
            "move 360",
            "up Left 360",
        ],
        "the widget Right went down on holds the route for the whole chord"
    );
    // The route is what the first press was offered, which is the widget under
    // it and one either side; the widgets around the Left press are not on it.
    for (index, log) in logs.iter().enumerate().skip(2) {
        let chord_events: Vec<String> = presses_and_releases(log)
            .into_iter()
            .filter(|entry| entry != "up Left 360")
            .collect();
        assert!(
            chord_events.is_empty(),
            "widget {index} sits around the Left press, which was not \
             hit-tested again, yet it was told {chord_events:?}"
        );
    }
}

#[test]
fn a_press_inside_a_chord_leaves_the_hover_record_as_it_was() {
    let (mut h, logs) = row_of_recorders();

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(450.0, 10.0));
    h.send(Event::mouse_down(450.0, 10.0, MouseButton::Right));
    logs[4].borrow_mut().clear();
    h.send(Event::mouse_move(50.0, 10.0));

    assert_eq!(
        *logs[4].borrow(),
        ["move 50"],
        "the widget the pointer was over is still owed the move that leaves \
         it, though the press in between was offered to the route alone"
    );
}

// The controls: routes that worked before and must keep working.

#[test]
fn a_left_drag_on_its_own_is_unchanged() {
    let first = log();
    let mut h = row_with_a_listening_first(&first);

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(450.0, 10.0));
    h.send(Event::mouse_move(460.0, 10.0));
    h.send(Event::mouse_up(460.0, 10.0, MouseButton::Left));

    assert_eq!(*first.borrow(), ["down10", "move450", "move460", "up460"]);
}

#[test]
fn the_last_release_drops_the_route() {
    let (mut h, logs) = row_of_recorders();

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_up(10.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_up(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(450.0, 10.0));
    logs[0].borrow_mut().clear();
    h.send(Event::mouse_move(460.0, 10.0));
    // A new press after the chord is hit-tested where it lands.
    h.send(Event::mouse_down(460.0, 10.0, MouseButton::Right));

    assert!(
        logs[0].borrow().is_empty(),
        "with every button up the route is gone, yet the first widget was told \
         {:?}",
        logs[0].borrow()
    );
    assert_eq!(
        presses_and_releases(&logs[4]),
        ["down Right 460"],
        "and the next press is hit-tested where it lands"
    );
}

#[test]
fn a_leave_during_a_chord_drops_the_route_and_the_held_buttons() {
    let (mut h, logs) = row_of_recorders();

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Right));
    h.send(Event::MouseLeave);
    // Back on the surface, a press is a new chord: hit-tested where it lands,
    // not sent down the route the leave dropped.
    h.send(Event::mouse_move(460.0, 10.0));
    logs[0].borrow_mut().clear();
    h.send(Event::mouse_down(460.0, 10.0, MouseButton::Left));
    h.send(Event::mouse_move(470.0, 10.0));

    assert!(
        logs[0].borrow().is_empty(),
        "the leave dropped the route, yet the first widget was told {:?}",
        logs[0].borrow()
    );
    assert_eq!(presses_and_releases(&logs[4]), ["down Left 460"]);
}

#[test]
fn a_custom_widget_keeps_its_right_button_route_outside_its_bounds() {
    let (mut h, logs) = row_of_recorders();

    h.send(Event::mouse_down(10.0, 10.0, MouseButton::Right));
    h.send(Event::mouse_move(450.0, 10.0));
    h.send(Event::mouse_up(450.0, 10.0, MouseButton::Right));

    assert_eq!(
        *logs[0].borrow(),
        ["down Right 10", "move 450", "up Right 450"],
        "a route a right press begins is a route like any other"
    );
}
