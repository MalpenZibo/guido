//! No event visits a widget outside its route (#586).
//!
//! Every event the loop dispatches has a target worked out centrally — the
//! widgets under the pointer and those the pointer record owes one, or the
//! focus path and the key listeners — and nothing is offered to every widget.
//! This is the one test that sends each kind of event over a long list and
//! counts who it reached: a kind that slipped back into a walk over every
//! widget would reach the whole list.

#![cfg(feature = "testing")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::widget_prelude::*;

mod common;
use common::headless;

const ROWS: usize = 1000;

/// A row that counts every event it is offered.
struct Counted(Rc<Cell<u32>>);

impl Widget for Counted {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(200.0, 20.0)
    }

    fn event(&mut self, _tree: &mut Tree, _id: WidgetId, _event: &Event) -> EventResponse {
        self.0.set(self.0.get() + 1);
        EventResponse::Ignored
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

#[test]
fn no_event_visits_a_row_outside_its_route() {
    let Some(mut app) = headless() else { return };
    let visits = Rc::new(Cell::new(0));
    let counter = visits.clone();
    let typed = create_signal(String::new());
    let id =
        app.surface(
            SurfaceConfig::new()
                .width(200)
                .height(600)
                .anchor(Anchor::TOP | Anchor::LEFT),
            move || {
                let counter = counter.clone();
                container()
                    .layout(Flex::column())
                    // A field to hold the focus, and a container listening for
                    // the keys it lets through.
                    .child(
                        container()
                            .height(30.0)
                            .width(fill())
                            .on_key_down(|_, _| {})
                            .child(text_input(typed)),
                    )
                    .child(container().scroll(Scroll::vertical()).height(570.0).child(
                        container().layout(Flex::column()).children(
                            (0..ROWS).map(move |_| {
                                Box::new(Counted(counter.clone())) as Box<dyn Widget>
                            }),
                        ),
                    ))
            },
        );
    app.configure(id, 200, 600, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    let mut send = |event: Event| {
        at += Duration::from_millis(16);
        app.event_at(id, event, at);
        app.step_at(at);
    };

    // The field takes the focus, and the pointer comes to rest over the list.
    send(Event::mouse_down(10.0, 10.0, MouseButton::Left));
    send(Event::mouse_up(10.0, 10.0, MouseButton::Left));
    send(Event::mouse_move(100.0, 300.0));

    let key = |key| Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
    };
    // The keys first, while the field has the focus: a press on the list
    // below is a press on nothing that takes the focus, and so does the
    // keyboard leaving.
    let every_kind = [
        ("a move", Event::mouse_move(100.0, 310.0)),
        ("a key the field takes", key(Key::Char('a'))),
        ("a key it lets through", key(Key::Escape)),
        (
            "a key's release",
            Event::KeyUp {
                key: Key::Escape,
                modifiers: Modifiers::default(),
            },
        ),
        ("the keyboard leaving", Event::FocusOut),
        ("the keyboard coming back", Event::FocusIn),
        (
            "a press",
            Event::mouse_down(100.0, 310.0, MouseButton::Left),
        ),
        ("a drag", Event::mouse_move(100.0, 500.0)),
        (
            "a release",
            Event::mouse_up(100.0, 500.0, MouseButton::Left),
        ),
        (
            "a wheel",
            Event::scroll(100.0, 500.0, 0.0, 20.0, ScrollSource::Wheel),
        ),
        ("a scroll's end", Event::scroll_end(100.0, 500.0)),
        ("a leave", Event::MouseLeave),
        ("an enter", Event::mouse_enter(100.0, 200.0)),
    ];

    for (index, (what, event)) in every_kind.into_iter().enumerate() {
        visits.set(0);
        send(event);
        if index == 0 {
            // The first is a move over the list: it reaches the rows under it,
            // so the counting below is counting something.
            assert!(visits.get() > 0, "a move over the list reached no row");
        }
        // A window is the row under the point and one either side; what the
        // record owes is the window before it. Six is both, with nothing else.
        assert!(
            visits.get() <= 6,
            "{what} was offered to {} of {ROWS} rows",
            visits.get()
        );
    }
    assert_eq!(
        typed.get_untracked(),
        "a",
        "the field had the focus throughout"
    );
}
