//! A positioned event visits the children that can be under it (#584).
//!
//! A container used to offer every pointer event to every child, so a wheel
//! event over a list cost the whole list. It now offers the window paint
//! narrows to, plus what the pointer record still owes something: the children
//! the last event was offered to, and the ones the press landed on. These say
//! both halves hold — the window is narrow, and nothing the walk used to reach
//! for a reason is lost.

#![cfg(feature = "testing")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;
use guido::widget_prelude::*;

mod common;
use common::headless;

const ROWS: usize = 1000;
const ROW_HEIGHT: f32 = 20.0;

/// A 100x600 surface over `view`, stepped once so every row has its bounds.
fn surface(app: &mut Headless, view: impl Fn() -> Container + 'static) -> (SurfaceId, Instant) {
    let id = app.surface(
        SurfaceConfig::new()
            .width(100)
            .height(600)
            .anchor(Anchor::TOP | Anchor::LEFT),
        view,
    );
    app.configure(id, 100, 600, 1.0);
    let at = Instant::now();
    app.step_at(at);
    (id, at)
}

fn list(rows: impl Iterator<Item = Box<dyn Widget>>) -> Container {
    container()
        .scroll(Scroll::vertical())
        .child(container().layout(Flex::column()).children(rows))
}

/// A row that counts every event it is offered.
struct Counted(Rc<Cell<u32>>);

impl Widget for Counted {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(100.0, ROW_HEIGHT)
    }

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, _event: &Event) -> EventResponse {
        self.0.set(self.0.get() + 1);
        EventResponse::Ignored
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

#[test]
fn a_wheel_event_over_a_list_visits_the_rows_near_the_pointer() {
    let Some(mut app) = headless() else { return };
    let visits = Rc::new(Cell::new(0));
    let counter = visits.clone();
    let (id, mut at) = surface(&mut app, move || {
        let counter = counter.clone();
        list((0..ROWS).map(move |_| Box::new(Counted(counter.clone())) as Box<dyn Widget>))
    });

    // The pointer crosses every visible row first: what the record owes is
    // the last event's window, not every row the pointer has been over.
    for y in (0..600).step_by(ROW_HEIGHT as usize) {
        at += Duration::from_millis(16);
        app.event_at(id, Event::mouse_move(50.0, y as f32 + 10.0), at);
        app.step_at(at);
    }
    // The first wheel event is owed the last move's window; the second is the
    // steady state, which is what a wheel turning looks like.
    for _ in 0..2 {
        visits.set(0);
        at += Duration::from_millis(16);
        app.event_at(
            id,
            Event::scroll(50.0, 300.0, 0.0, 20.0, ScrollSource::Wheel),
            at,
        );
        app.step_at(at);
    }

    assert!(
        visits.get() <= 8,
        "one wheel event was offered to {} of {ROWS} rows; the rows under \
         the pointer and their neighbours are all it can land on",
        visits.get()
    );
}

/// Rows that report hover changes, presses, releases and clicks by index.
#[derive(Default)]
struct Log {
    hover: Vec<(usize, bool)>,
    moves: Vec<usize>,
    up: Vec<usize>,
    clicks: Vec<usize>,
}

fn logged_rows(log: Rc<RefCell<Log>>) -> impl Iterator<Item = Box<dyn Widget>> {
    (0..ROWS).map(move |i| {
        let (h, m, u, c) = (log.clone(), log.clone(), log.clone(), log.clone());
        Box::new(
            container()
                .height(ROW_HEIGHT)
                .width(fill())
                .on_hover(move |on| h.borrow_mut().hover.push((i, on)))
                .on_pointer_move(move |_, _| m.borrow_mut().moves.push(i))
                .on_mouse_up(move |_, _| u.borrow_mut().up.push(i))
                .on_click(move || c.borrow_mut().clicks.push(i)),
        ) as Box<dyn Widget>
    })
}

#[test]
fn a_row_the_pointer_jumps_away_from_is_no_longer_hovered() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    let (id, mut at) = surface(&mut app, move || list(logged_rows(rows.clone())));

    at += Duration::from_millis(16);
    app.event_at(id, Event::mouse_move(50.0, 50.0), at);
    app.step_at(at);
    at += Duration::from_millis(16);
    app.event_at(id, Event::mouse_move(50.0, 550.0), at);
    app.step_at(at);

    let hover = &log.borrow().hover;
    assert_eq!(
        hover.as_slice(),
        &[(2, true), (2, false), (27, true)],
        "the row left five hundred pixels behind is told, and the one arrived at"
    );
}

#[test]
fn a_press_dragged_far_away_still_gets_its_moves_and_its_release() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    let (id, mut at) = surface(&mut app, move || list(logged_rows(rows.clone())));

    for event in [
        Event::mouse_down(50.0, 50.0, MouseButton::Left),
        Event::mouse_move(50.0, 400.0),
        Event::mouse_move(50.0, 590.0),
        Event::mouse_up(50.0, 590.0, MouseButton::Left),
    ] {
        at += Duration::from_millis(16);
        app.event_at(id, event, at);
        app.step_at(at);
    }

    let log = log.borrow();
    assert_eq!(
        log.moves.iter().filter(|&&i| i == 2).count(),
        2,
        "both moves reach the pressed row, however far from it they land"
    );
    assert_eq!(log.up, vec![2], "and so does the release");
    assert!(log.clicks.is_empty(), "which was not over it: no click");
}

#[test]
fn a_row_moved_by_a_transform_is_clicked_where_it_is_drawn() {
    let Some(mut app) = headless() else { return };
    let clicked = Rc::new(Cell::new(None));
    let rows = clicked.clone();
    let (id, mut at) = surface(&mut app, move || {
        let rows = rows.clone();
        list((0..ROWS).map(move |i| {
            let clicked = rows.clone();
            let row = container()
                .height(ROW_HEIGHT)
                .width(fill())
                .on_click(move || clicked.set(Some(i)));
            // Row 0 is laid out at the top and drawn three hundred pixels
            // below, over row 15.
            let row = if i == 0 {
                row.translate((0.0, 300.0))
            } else {
                row
            };
            Box::new(row) as Box<dyn Widget>
        }))
    });

    at += Duration::from_millis(16);
    app.click(id, 50.0, 310.0);
    app.step_at(at);

    assert_eq!(
        clicked.get(),
        Some(0),
        "the click lands on what is drawn there, which is the first row"
    );
}

#[test]
fn a_row_left_for_a_header_and_back_is_no_longer_hovered() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    // A header above the list, the shape of a launcher's search field over its
    // results: a move onto the header never reaches the list, which clips.
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(container().height(50.0).width(fill()))
            .child(list(logged_rows(rows.clone())).height(550.0))
    });

    for y in [120.0, 25.0, 260.0] {
        at += Duration::from_millis(16);
        app.event_at(id, Event::mouse_move(50.0, y), at);
        app.step_at(at);
    }

    let hover = &log.borrow().hover;
    assert!(
        hover.contains(&(3, false)),
        "row 3 was hovered, the pointer went to the header and came back on \
         row 10, and row 3 still believes it is under the pointer: {hover:?}"
    );
}
