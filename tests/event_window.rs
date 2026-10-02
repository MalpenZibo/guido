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
    down: Vec<usize>,
    moves: Vec<usize>,
    up: Vec<usize>,
    clicks: Vec<usize>,
}

fn logged_rows(log: Rc<RefCell<Log>>, count: usize) -> impl Iterator<Item = Box<dyn Widget>> {
    (0..count).map(move |i| {
        let (h, d, m, u, c) = (
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
            log.clone(),
        );
        Box::new(
            container()
                .height(ROW_HEIGHT)
                .width(fill())
                .on_hover(move |on| h.borrow_mut().hover.push((i, on)))
                .on_mouse_down(move |_, _| d.borrow_mut().down.push(i))
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
    let (id, mut at) = surface(&mut app, move || list(logged_rows(rows.clone(), ROWS)));

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
    let (id, mut at) = surface(&mut app, move || list(logged_rows(rows.clone(), ROWS)));

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
            .child(list(logged_rows(rows.clone(), ROWS)).height(550.0))
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

/// Sends `events` one per frame, each sixteen milliseconds after the last.
fn play(
    app: &mut Headless,
    id: SurfaceId,
    at: &mut Instant,
    events: impl IntoIterator<Item = Event>,
) {
    for event in events {
        *at += Duration::from_millis(16);
        app.event_at(id, event, *at);
        app.step_at(*at);
    }
}

#[test]
fn a_release_and_a_leave_leave_nothing_owed() {
    let Some(mut app) = headless() else { return };
    let visits = Rc::new(Cell::new(0));
    let counter = visits.clone();
    let (id, mut at) = surface(&mut app, move || {
        let counter = counter.clone();
        list((0..ROWS).map(move |_| Box::new(Counted(counter.clone())) as Box<dyn Widget>))
    });
    // The window over a point inside row 15 is rows 14 to 16: the row, and one
    // either side for the boundary.
    let window_over_row_15 = 3;

    // A press and its release at the top: once released, the press is owed
    // nothing.
    play(
        &mut app,
        id,
        &mut at,
        [
            Event::mouse_down(50.0, 50.0, MouseButton::Left),
            Event::mouse_up(50.0, 50.0, MouseButton::Left),
            Event::mouse_move(50.0, 310.0),
        ],
    );
    visits.set(0);
    #[cfg(feature = "render-stats")]
    guido::render_stats::reset_stats();
    play(&mut app, id, &mut at, [Event::mouse_move(50.0, 310.0)]);
    assert_eq!(
        visits.get(),
        window_over_row_15,
        "a move over row 15 visits its window, and nothing the released press held"
    );
    #[cfg(feature = "render-stats")]
    {
        // The scroller offers its one child, the list, and the list its rows.
        let stats = guido::render_stats::get_stats();
        assert_eq!(stats.event_window_children_total, 1 + ROWS as u64);
        assert_eq!(
            stats.event_window_children_offered,
            1 + window_over_row_15 as u64
        );
    }

    // The pointer leaves the surface and comes back somewhere else: what it
    // was over before is owed nothing either.
    play(&mut app, id, &mut at, [Event::MouseLeave]);
    visits.set(0);
    play(&mut app, id, &mut at, [Event::mouse_move(50.0, 510.0)]);
    assert_eq!(
        visits.get(),
        window_over_row_15,
        "the first move after a leave visits its own window only"
    );
}

/// A row that takes every move it is offered, and says which it was.
struct Greedy(usize, Rc<RefCell<Vec<usize>>>);

impl Widget for Greedy {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(100.0, ROW_HEIGHT)
    }

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, event: &Event) -> EventResponse {
        if matches!(event, Event::MouseMove { .. }) {
            self.1.borrow_mut().push(self.0);
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

#[test]
fn the_children_owed_an_event_are_offered_it_in_the_order_they_stand() {
    let Some(mut app) = headless() else { return };
    let took = Rc::new(RefCell::new(Vec::new()));
    let rows = took.clone();
    let (id, mut at) = surface(&mut app, move || {
        let rows = rows.clone();
        list((0..ROWS).map(move |i| Box::new(Greedy(i, rows.clone())) as Box<dyn Widget>))
    });

    // The first move's window over row 25 starts at row 24, which takes it.
    // The second lands on row 2, whose window starts at row 1 — and row 24 is
    // owed it, but stands below row 1, so row 1 takes it first, as the walk
    // over every child would have had it.
    play(
        &mut app,
        id,
        &mut at,
        [
            Event::mouse_move(50.0, 510.0),
            Event::mouse_move(50.0, 50.0),
        ],
    );
    assert_eq!(*took.borrow(), [24, 1]);
}

#[test]
fn a_press_held_while_the_list_changes_still_gets_its_release() {
    let Some(mut app) = headless() else { return };
    let keys = Rc::new(Cell::new(None));
    let released = Rc::new(RefCell::new(Vec::new()));
    let (data, ups) = (keys.clone(), released.clone());
    let (id, mut at) = surface(&mut app, move || {
        let shown = create_signal((0..ROWS as u32).collect::<Vec<_>>());
        data.set(Some(shown));
        let ups = ups.clone();
        container()
            .scroll(Scroll::vertical())
            .child(container().layout(Flex::column()).children(keyed(
                move || shown.get(),
                |k| *k,
                move |k| {
                    let ups = ups.clone();
                    container()
                        .height(ROW_HEIGHT)
                        .width(fill())
                        .on_mouse_up(move |_, _| ups.borrow_mut().push(k))
                },
            )))
    });
    let keys = keys.get().expect("the view ran");

    // Row 2 is pressed. Ten rows arrive above it before the release, so it is
    // no longer where the press found it.
    play(
        &mut app,
        id,
        &mut at,
        [Event::mouse_down(50.0, 50.0, MouseButton::Left)],
    );
    keys.update(|k| k.splice(0..0, 10_000..10_010).for_each(drop));
    at += Duration::from_millis(16);
    app.step_at(at);
    play(
        &mut app,
        id,
        &mut at,
        [Event::mouse_up(50.0, 590.0, MouseButton::Left)],
    );

    assert_eq!(
        *released.borrow(),
        [2],
        "the release reaches the row the press landed on"
    );
}

/// `rows` in a list under a 50-pixel header, the shape of a launcher's search
/// field over its results: the list clips, so a point on the header is outside
/// it.
fn under_a_header(rows: impl Iterator<Item = Box<dyn Widget>>) -> Container {
    container()
        .layout(Flex::column())
        .child(container().height(50.0).width(fill()))
        .child(list(rows).height(550.0))
}

#[test]
fn a_row_pressed_in_a_list_and_released_outside_it_gets_its_release() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    let (id, mut at) = surface(&mut app, move || {
        under_a_header(logged_rows(rows.clone(), ROWS))
    });

    play(
        &mut app,
        id,
        &mut at,
        [
            Event::mouse_down(50.0, 120.0, MouseButton::Left),
            Event::mouse_move(50.0, 25.0),
            Event::mouse_up(50.0, 25.0, MouseButton::Left),
        ],
    );

    let log = log.borrow();
    assert_eq!(
        log.moves,
        vec![3],
        "the drag reaches the pressed row over the header"
    );
    assert_eq!(log.up, vec![3], "and so does the release");
    assert!(log.clicks.is_empty());
}

#[test]
fn a_row_hovered_in_a_list_is_no_longer_hovered_once_the_pointer_leaves_the_list() {
    for count in [5, ROWS] {
        let Some(mut app) = headless() else { return };
        let log = Rc::new(RefCell::new(Log::default()));
        let rows = log.clone();
        let (id, mut at) = surface(&mut app, move || {
            under_a_header(logged_rows(rows.clone(), count))
        });

        play(
            &mut app,
            id,
            &mut at,
            [
                Event::mouse_move(50.0, 120.0),
                Event::mouse_move(50.0, 25.0),
            ],
        );

        assert_eq!(
            log.borrow().hover,
            [(3, true), (3, false)],
            "a list of {count}: the pointer left row 3 for the header"
        );
    }
}

#[test]
fn a_leave_visits_what_the_pointer_was_over_and_nothing_else() {
    let Some(mut app) = headless() else { return };
    let visits = Rc::new(Cell::new(0));
    let counter = visits.clone();
    let (id, mut at) = surface(&mut app, move || {
        let counter = counter.clone();
        list((0..ROWS).map(move |_| Box::new(Counted(counter.clone())) as Box<dyn Widget>))
    });

    play(&mut app, id, &mut at, [Event::mouse_move(50.0, 310.0)]);
    visits.set(0);
    play(&mut app, id, &mut at, [Event::MouseLeave]);

    assert_eq!(
        visits.get(),
        3,
        "the pointer was over row 15's window when it left the surface"
    );
}

/// The list under the header, scrolled ten pixels: row 0 now stands half under
/// the header, from 40 to 60, and the list clips the half above 50 away.
///
/// `direct` puts the rows straight into the scroller rather than into a column
/// inside it: the scroller's own children are then the ones the clip hides.
fn row_0_half_under_the_header(
    log: Rc<RefCell<Log>>,
    direct: bool,
) -> (Headless, SurfaceId, Instant) {
    let mut app = headless().expect("checked by the caller");
    let rows = log.clone();
    let (id, mut at) = surface(&mut app, move || {
        if direct {
            container()
                .layout(Flex::column())
                .child(container().height(50.0).width(fill()))
                .child(
                    container()
                        .scroll(Scroll::vertical())
                        .layout(Flex::column())
                        .height(550.0)
                        .children(logged_rows(rows.clone(), ROWS)),
                )
        } else {
            under_a_header(logged_rows(rows.clone(), ROWS))
        }
    });
    play(
        &mut app,
        id,
        &mut at,
        [Event::scroll(50.0, 300.0, 0.0, 10.0, ScrollSource::Wheel)],
    );
    (app, id, at)
}

#[test]
fn a_row_half_under_the_clip_loses_its_hover_to_the_header_over_it() {
    if headless().is_none() {
        return;
    }
    for direct in [false, true] {
        let log = Rc::new(RefCell::new(Log::default()));
        let (mut app, id, mut at) = row_0_half_under_the_header(log.clone(), direct);

        // Onto the half of row 0 that shows, then onto the header over the
        // half that does not.
        play(
            &mut app,
            id,
            &mut at,
            [Event::mouse_move(50.0, 55.0), Event::mouse_move(50.0, 45.0)],
        );

        assert_eq!(
            log.borrow().hover,
            [(0, true), (0, false)],
            "rows held directly: {direct}. The header is what is under the \
             pointer, not the part of row 0 it hides"
        );
    }
}

#[test]
fn a_press_on_the_header_does_not_reach_the_row_it_hides() {
    if headless().is_none() {
        return;
    }
    for direct in [false, true] {
        let log = Rc::new(RefCell::new(Log::default()));
        let (mut app, id, mut at) = row_0_half_under_the_header(log.clone(), direct);

        play(
            &mut app,
            id,
            &mut at,
            [
                // A tap on the half of row 0 that shows, which leaves it owed
                // the next event…
                Event::mouse_down(50.0, 55.0, MouseButton::Left),
                Event::mouse_up(50.0, 55.0, MouseButton::Left),
                // …and a press on the header over the half that does not.
                Event::mouse_down(50.0, 45.0, MouseButton::Left),
                Event::mouse_up(50.0, 45.0, MouseButton::Left),
            ],
        );

        let log = log.borrow();
        assert_eq!(
            log.down,
            [0],
            "rows held directly: {direct}. Pressed through the clip"
        );
        assert_eq!(
            log.clicks,
            [0],
            "rows held directly: {direct}. Clicked through it"
        );
    }
}

#[test]
fn a_leave_clears_the_hover_after_another_surface_had_the_pointer() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    let (list_surface, mut at) = surface(&mut app, move || list(logged_rows(rows.clone(), ROWS)));
    let (other, _) = surface(&mut app, container);

    // Row 2 hovered on the first surface; the pointer is then reported on the
    // second before the first hears it left — two bars, or a bar and its popup.
    play(
        &mut app,
        list_surface,
        &mut at,
        [Event::mouse_move(50.0, 50.0)],
    );
    play(&mut app, other, &mut at, [Event::mouse_move(10.0, 10.0)]);
    play(&mut app, list_surface, &mut at, [Event::MouseLeave]);

    assert_eq!(
        log.borrow().hover,
        [(2, true), (2, false)],
        "the leave reached the row the other surface's event had no business with"
    );
}

#[test]
fn a_list_below_a_list_is_not_withheld_by_the_one_above() {
    let Some(mut app) = headless() else { return };
    let log = Rc::new(RefCell::new(Log::default()));
    let rows = log.clone();
    // Two lists stacked: a point on the lower one is outside the upper one's
    // clip, which withholds it from what it holds — and from nothing else.
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(list(std::iter::empty()).height(300.0))
            .child(list(logged_rows(rows.clone(), ROWS)).height(300.0))
    });

    play(&mut app, id, &mut at, [Event::mouse_move(50.0, 350.0)]);

    assert_eq!(
        log.borrow().hover,
        [(2, true)],
        "the lower list's row 2 is under the pointer"
    );
}
