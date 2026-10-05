//! A key goes to the focused widget and up through its ancestors, then to the
//! containers that listen for keys — and to nothing else (#241).
//!
//! Keys used to be offered to every widget of the surface holding the
//! keyboard, and each widget asked itself whether it had the focus. These say
//! the route is narrow, and that what the walk used to give a listener — a key
//! with nothing focused, the innermost listener first, nothing through a hidden
//! container — it still gives.

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

fn surface(app: &mut Headless, view: impl Fn() -> Container + 'static) -> (SurfaceId, Instant) {
    let id = app.surface(
        SurfaceConfig::new()
            .width(200)
            .height(600)
            .anchor(Anchor::TOP | Anchor::LEFT),
        view,
    );
    app.configure(id, 200, 600, 1.0);
    let at = Instant::now();
    app.step_at(at);
    (id, at)
}

fn key(app: &mut Headless, id: SurfaceId, at: &mut Instant, key: Key) {
    *at += Duration::from_millis(16);
    app.event_at(
        id,
        Event::KeyDown {
            key,
            modifiers: Modifiers::default(),
            repeat: false,
        },
        *at,
    );
    app.step_at(*at);
}

fn click(app: &mut Headless, id: SurfaceId, at: &mut Instant, x: f32, y: f32) {
    for event in [
        Event::mouse_down(x, y, MouseButton::Left),
        Event::mouse_up(x, y, MouseButton::Left),
    ] {
        *at += Duration::from_millis(16);
        app.event_at(id, event, *at);
        app.step_at(*at);
    }
}

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

/// A container that records the keys it is given under `name`.
fn listener(name: &'static str, heard: Rc<RefCell<Vec<(&'static str, Key)>>>) -> Container {
    container()
        .width(fill())
        .on_key_down(move |key, _| heard.borrow_mut().push((name, key)))
}

#[test]
fn a_key_visits_the_focus_path_and_nothing_else() {
    let Some(mut app) = headless() else { return };
    let visits = Rc::new(Cell::new(0));
    let counter = visits.clone();
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        let counter = counter.clone();
        container()
            .layout(Flex::column())
            .child(
                container()
                    .height(30.0)
                    .width(fill())
                    .child(text_input(typed)),
            )
            // A listener too: the escape the field lets through is offered to
            // it, down its own way, and that way does not pass the list.
            .child(
                container()
                    .height(10.0)
                    .width(fill())
                    .on_key_down(|_, _| {}),
            )
            .child(container().scroll(Scroll::vertical()).height(560.0).child(
                container().layout(Flex::column()).children(
                    (0..ROWS).map(move |_| Box::new(Counted(counter.clone())) as Box<dyn Widget>),
                ),
            ))
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    visits.set(0);
    // An escape the field does not take, and nothing else listens for: it goes
    // up the focus path and stops there.
    key(&mut app, id, &mut at, Key::Escape);
    key(&mut app, id, &mut at, Key::Char('a'));

    assert_eq!(typed.get_untracked(), "a", "the field has the focus");
    assert_eq!(
        visits.get(),
        0,
        "keys for the field were offered to {} rows of a list beside it",
        visits.get()
    );
}

#[test]
fn a_listener_hears_a_key_with_nothing_focused() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let (id, mut at) = surface(&mut app, move || {
        container().child(container().child(listener("menu", log.clone()).height(100.0)))
    });

    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(*heard.borrow(), [("menu", Key::Escape)]);
}

#[test]
fn a_focused_field_keeps_the_keys_it_takes_and_passes_the_rest_on() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (around, beside) = (heard.clone(), heard.clone());
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            // A listener around the field, and one beside it.
            .child(
                listener("around", around.clone())
                    .height(30.0)
                    .child(text_input(typed)),
            )
            .child(listener("beside", beside.clone()).height(30.0))
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    key(&mut app, id, &mut at, Key::Char('a'));
    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(typed.get_untracked(), "a");
    assert_eq!(
        *heard.borrow(),
        [("around", Key::Escape)],
        "the field took the character; the escape it does not take went up \
         to the listener around it, which took it before the one beside"
    );
}

#[test]
fn the_innermost_listener_hears_a_key_first() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (outer, inner) = (heard.clone(), heard.clone());
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            listener("outer", outer.clone())
                .height(100.0)
                .child(listener("inner", inner.clone()).height(50.0)),
        )
    });

    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(*heard.borrow(), [("inner", Key::Escape)]);
}

#[test]
fn a_listener_inside_a_hidden_container_hears_nothing() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            container()
                .visible(false)
                .child(listener("hidden", log.clone()).height(50.0)),
        )
    });

    key(&mut app, id, &mut at, Key::Escape);

    assert!(heard.borrow().is_empty(), "heard {:?}", heard.borrow());
}

#[test]
fn a_listener_before_the_focused_field_no_longer_takes_its_typing() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let before = heard.clone();
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            // Earlier in the tree than the field and not around it: the walk
            // over every widget offered it each key first, and it took them all.
            .child(listener("before", before.clone()).height(30.0))
            .child(
                container()
                    .height(30.0)
                    .width(fill())
                    .child(text_input(typed)),
            )
    });

    click(&mut app, id, &mut at, 10.0, 40.0);
    key(&mut app, id, &mut at, Key::Char('a'));
    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(
        typed.get_untracked(),
        "a",
        "the focused field hears its keys first"
    );
    assert_eq!(*heard.borrow(), [("before", Key::Escape)]);
}
