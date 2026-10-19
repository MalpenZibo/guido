//! A key goes to the focused widget and up through its ancestors, then to the
//! containers that listen for keys — and to nothing else (#241).
//!
//! Keys used to be offered to every widget of the surface holding the
//! keyboard, and each widget asked itself whether it had the focus. These say
//! the route is narrow, and that what the walk used to give a listener — a key
//! with nothing focused, the innermost listener first, nothing through a hidden
//! container — it still gives.
//!
//! And a held key arrives as presses that say they repeat (#613): every one
//! is delivered, along either route, and a listener that wants presses alone
//! tells them apart by the flag.
//!
//! And a release goes where a press goes (#631): along the focus path, then to
//! the listeners when nothing there took it.
//!
//! And a listener says whether it took the key (#634): one answering `Ignored`
//! lets it go on along the route, and is not asked again.
//!
//! And a container can hear a key before the focused widget inside it does
//! (#848): its capture listeners run down the focus path from the outermost,
//! and one answering `Handled` keeps the key from everything after it.

#![cfg(feature = "testing")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::reactive::focus::{focused_widget, request_focus};
use guido::testing::Headless;
use guido::widget_prelude::*;

mod common;
use common::headless;

const ROWS: usize = 1000;

/// The keys each named listener heard, in the order it heard them.
type Log = Rc<RefCell<Vec<(&'static str, Key)>>>;

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
    send(app, id, at, key_down(key, false));
}

fn key_down(key: Key, repeat: bool) -> Event {
    Event::KeyDown {
        key,
        modifiers: Modifiers::default(),
        repeat,
    }
}

fn key_up(key: Key, modifiers: Modifiers) -> Event {
    Event::KeyUp { key, modifiers }
}

fn send(app: &mut Headless, id: SurfaceId, at: &mut Instant, event: Event) {
    *at += Duration::from_millis(16);
    app.event_at(id, event, *at);
    app.step_at(*at);
}

fn click(app: &mut Headless, id: SurfaceId, at: &mut Instant, x: f32, y: f32) {
    send(app, id, at, Event::mouse_down(x, y, MouseButton::Left));
    send(app, id, at, Event::mouse_up(x, y, MouseButton::Left));
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

/// A container that records the keys it is given under `name`, and takes them.
fn listener(name: &'static str, heard: Log) -> Container {
    answering(name, heard, EventResponse::Handled)
}

/// A container that records the keys it is given under `name`, and answers
/// `answer` to each.
fn answering(name: &'static str, heard: Log, answer: EventResponse) -> Container {
    container().width(fill()).on_key_down(move |key, _, _| {
        heard.borrow_mut().push((name, key));
        answer
    })
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
                    .on_key_down(|_, _, _| EventResponse::Handled),
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

/// Page Up, Page Down and the function keys are keys like Escape: a focused
/// field does not take them, and they go on up to the listener around it —
/// a list paging, an application binding F5.
#[test]
fn paging_and_function_keys_reach_the_listener_around_a_focused_field() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let around = heard.clone();
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        listener("around", around.clone())
            .height(30.0)
            .child(text_input(typed))
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    for k in [Key::PageDown, Key::PageUp, Key::F(5)] {
        key(&mut app, id, &mut at, k);
    }

    assert_eq!(
        *heard.borrow(),
        [
            ("around", Key::PageDown),
            ("around", Key::PageUp),
            ("around", Key::F(5))
        ]
    );
    assert_eq!(
        typed.get_untracked(),
        "",
        "and the field typed none of them"
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

/// A container that records the keys it is given under `name`, and lets them
/// all through.
fn passer(name: &'static str, heard: Log) -> Container {
    answering(name, heard, EventResponse::Ignored)
}

#[test]
fn a_listener_that_lets_a_key_through_passes_it_to_the_next() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (outer, inner) = (heard.clone(), heard.clone());
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            listener("outer", outer.clone())
                .height(100.0)
                .child(passer("inner", inner.clone()).height(50.0)),
        )
    });

    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(
        *heard.borrow(),
        [("inner", Key::Escape), ("outer", Key::Escape)],
        "the inner listener let the key through, so the outer one heard it too"
    );
}

#[test]
fn a_listener_on_the_focus_path_hears_a_key_it_let_through_once() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (around, beside) = (heard.clone(), heard.clone());
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            passer("around", around.clone())
                .height(100.0)
                .layout(Flex::column())
                .child(
                    container()
                        .height(30.0)
                        .width(fill())
                        .child(text_input(typed)),
                )
                .child(passer("beside", beside.clone()).height(30.0)),
        )
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(
        *heard.borrow(),
        [("around", Key::Escape), ("beside", Key::Escape)],
        "the container around the field heard the escape on the focus path; \
         the way down to the listener inside it passes it again, and must not \
         ask it twice"
    );
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

/// A widget that takes the focus when pressed and lets every key through.
struct Inert;

impl Widget for Inert {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(200.0, 20.0)
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        if let Event::MouseDown { .. } = event {
            request_focus(tree, id);
            return EventResponse::Handled;
        }
        EventResponse::Ignored
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

/// Which way a key the focused widget lets through reaches the listener.
#[derive(Clone, Copy)]
enum Route {
    /// The listener is around the focused widget, so the key reaches it on
    /// its way up the focus path.
    Focused,
    /// The listener is beside it, and hears what the focus path left.
    Listener,
}

/// Enter, held for two repeats, released, and pressed again — played into a
/// listener that counts the key-downs `count` accepts.
fn replay(route: Route, count: fn(bool) -> bool) -> u32 {
    let Some(mut app) = headless() else { return 0 };
    let heard = Rc::new(Cell::new(0));
    let log = heard.clone();
    let listener = move || {
        let log = log.clone();
        container()
            .width(fill())
            .height(30.0)
            .on_key_down(move |key, _, repeat| {
                if key == Key::Enter && count(repeat) {
                    log.set(log.get() + 1);
                }
                EventResponse::Handled
            })
    };
    let (id, mut at) = surface(&mut app, move || match route {
        Route::Focused => container().child(listener().child(Inert)),
        Route::Listener => container()
            .layout(Flex::column())
            .child(container().height(30.0).width(fill()).child(Inert))
            .child(listener()),
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    // Only the inert row takes the focus, so with it held the focused route
    // really is the focus path, and the listener route really is past it.
    assert!(
        focused_widget().is_some(),
        "the click focused the inert row"
    );
    for event in [
        key_down(Key::Enter, false),
        key_down(Key::Enter, true),
        key_down(Key::Enter, true),
        key_up(Key::Enter, Modifiers::default()),
        key_down(Key::Enter, false),
    ] {
        send(&mut app, id, &mut at, event);
    }
    heard.get()
}

#[test]
fn a_held_key_activates_once_per_press_on_the_focus_path() {
    if headless().is_none() {
        return;
    }
    assert_eq!(replay(Route::Focused, |repeat| !repeat), 2, "two presses");
    assert_eq!(
        replay(Route::Focused, |_| true),
        4,
        "two presses and two repeats, every one delivered"
    );
}

#[test]
fn a_held_key_activates_once_per_press_past_an_unhandling_focus() {
    if headless().is_none() {
        return;
    }
    assert_eq!(replay(Route::Listener, |repeat| !repeat), 2, "two presses");
    assert_eq!(
        replay(Route::Listener, |_| true),
        4,
        "two presses and two repeats, every one delivered"
    );
}

/// Moving a key somewhere else — what a container does to every event it hands
/// a child — leaves it the repeat it was.
#[test]
fn a_moved_key_down_keeps_its_repeat() {
    for repeat in [false, true] {
        for at in [Some(Point::new(1.0, 2.0)), None] {
            let moved = key_down(Key::Enter, repeat).with_coords(at);
            assert!(
                matches!(moved, Event::KeyDown { repeat: r, .. } if r == repeat),
                "{moved:?}"
            );
        }
    }
}

/// What a container heard, in the order it heard it.
#[derive(Debug, PartialEq)]
enum Heard {
    Down(&'static str, Key, bool),
    Up(&'static str, Key, Modifiers),
}

/// A container that records the presses and releases it is given under `name`.
fn both_ways(name: &'static str, heard: Rc<RefCell<Vec<Heard>>>) -> Container {
    let up = heard.clone();
    container()
        .width(fill())
        .on_key_down(move |key, _, repeat| {
            heard.borrow_mut().push(Heard::Down(name, key, repeat));
            EventResponse::Handled
        })
        .on_key_up(move |key, modifiers| {
            up.borrow_mut().push(Heard::Up(name, key, modifiers));
            EventResponse::Handled
        })
}

#[test]
fn a_release_visits_the_focus_path_after_its_press() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (around, beside) = (heard.clone(), heard.clone());
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(
                both_ways("around", around.clone())
                    .height(30.0)
                    .child(Inert),
            )
            .child(both_ways("beside", beside.clone()).height(30.0))
    });
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };

    click(&mut app, id, &mut at, 10.0, 10.0);
    for event in [
        key_down(Key::Char(' '), false),
        key_down(Key::Char(' '), true),
        key_down(Key::Char(' '), true),
        key_up(Key::Char(' '), shift),
    ] {
        send(&mut app, id, &mut at, event);
    }

    assert_eq!(
        *heard.borrow(),
        [
            Heard::Down("around", Key::Char(' '), false),
            Heard::Down("around", Key::Char(' '), true),
            Heard::Down("around", Key::Char(' '), true),
            Heard::Up("around", Key::Char(' '), shift),
        ],
        "the container around the focus hears the press, its repeats, then one \
         release with its modifiers, and takes each before the listener beside it"
    );
}

/// A container that declares only `on_key_up` listens too.
#[test]
fn a_listener_hears_a_release_with_nothing_focused() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = heard.clone();
    let (id, mut at) = surface(&mut app, move || {
        let log = log.clone();
        container().child(container().width(fill()).height(100.0).on_key_up(
            move |key, modifiers| {
                log.borrow_mut().push((key, modifiers));
                EventResponse::Handled
            },
        ))
    });

    send(&mut app, id, &mut at, key_down(Key::Escape, false));
    send(
        &mut app,
        id,
        &mut at,
        key_up(Key::Escape, Modifiers::default()),
    );

    assert_eq!(*heard.borrow(), [(Key::Escape, Modifiers::default())]);
}

/// A release is not paired with its press: the field takes the character and
/// lets its release through.
#[test]
fn a_listener_hears_the_release_of_a_key_the_field_took() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let beside = heard.clone();
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(
                container()
                    .height(30.0)
                    .width(fill())
                    .child(text_input(typed)),
            )
            .child(both_ways("beside", beside.clone()).height(30.0))
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    send(&mut app, id, &mut at, key_down(Key::Char('a'), false));
    send(
        &mut app,
        id,
        &mut at,
        key_up(Key::Char('a'), Modifiers::default()),
    );

    assert_eq!(typed.get_untracked(), "a");
    assert_eq!(
        *heard.borrow(),
        [Heard::Up("beside", Key::Char('a'), Modifiers::default())]
    );
}

/// A container that hears presses and releases before what is inside it,
/// records them under `name`, and answers `answer` to the presses and
/// releases of `takes`.
fn capturing(name: &'static str, log: Log, takes: &'static [Key]) -> Container {
    let up = log.clone();
    let answer = move |key: Key| {
        if takes.contains(&key) {
            EventResponse::Handled
        } else {
            EventResponse::Ignored
        }
    };
    container()
        .width(fill())
        .on_key_down_capture(move |key, _, _| {
            log.borrow_mut().push((name, key));
            answer(key)
        })
        .on_key_up_capture(move |key, _| {
            up.borrow_mut().push((name, key));
            answer(key)
        })
}

/// The launcher in #848: a search field keeps the focus while the arrows move
/// a selection. The container takes Left and Right before the field can move
/// its caret with them, and hears Up and Down as before, once the field has
/// let them through.
#[test]
fn a_container_takes_the_arrows_before_the_focused_field_under_it() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (captured, bubbled) = (heard.clone(), heard.clone());
    let query = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        let bubbled = bubbled.clone();
        capturing("capture", captured.clone(), &[Key::Left, Key::Right])
            .height(30.0)
            .on_key_down(move |key, _, _| {
                bubbled.borrow_mut().push(("bubble", key));
                EventResponse::Handled
            })
            .child(text_input(query).autofocus())
    });

    assert!(focused_widget().is_some(), "the field took the focus");
    for k in [
        Key::Char('a'),
        Key::Char('b'),
        Key::Left,
        Key::Left,
        Key::Right,
        Key::Char('c'),
        Key::Up,
        Key::Down,
    ] {
        key(&mut app, id, &mut at, k);
    }

    assert_eq!(
        query.get_untracked(),
        "abc",
        "the caret stayed at the end: the field never saw Left or Right"
    );
    assert_eq!(
        *heard.borrow(),
        [
            ("capture", Key::Char('a')),
            ("capture", Key::Char('b')),
            ("capture", Key::Left),
            ("capture", Key::Left),
            ("capture", Key::Right),
            ("capture", Key::Char('c')),
            ("capture", Key::Up),
            ("bubble", Key::Up),
            ("capture", Key::Down),
            ("bubble", Key::Down),
        ],
        "the capture heard every key first and let the characters through to \
         the field; Up and Down, which the field lets through, bubbled up to \
         the same container"
    );
}

/// An [`Inert`] row that records the keys it is given.
struct Recorder(Log);

impl Widget for Recorder {
    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        Inert.layout(ctx, constraints)
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        if let Event::KeyDown { key, .. } | Event::KeyUp { key, .. } = event {
            self.0.borrow_mut().push(("focused", *key));
        }
        Inert.event(tree, id, event)
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

/// Two capturing containers around a focused widget, each also listening the
/// usual way.
fn nested_captures(app: &mut Headless, heard: &Log) -> (SurfaceId, Instant) {
    let log = heard.clone();
    let (id, mut at) = surface(app, move || {
        let bubble = |name: &'static str, container: Container| {
            let (down, up) = (log.clone(), log.clone());
            container
                .on_key_down(move |key, _, _| {
                    down.borrow_mut().push((name, key));
                    EventResponse::Ignored
                })
                .on_key_up(move |key, _| {
                    up.borrow_mut().push((name, key));
                    EventResponse::Ignored
                })
        };
        // A field after them, so a Tab nobody took has somewhere to go.
        container()
            .layout(Flex::column())
            .child(bubble(
                "outer",
                capturing("outer capture", log.clone(), &[Key::Escape])
                    .height(100.0)
                    .child(bubble(
                        "inner",
                        capturing("inner capture", log.clone(), &[Key::Tab])
                            .height(50.0)
                            .child(Recorder(log.clone())),
                    )),
            ))
            .child(text_input(create_signal(String::new())))
    });
    click(app, id, &mut at, 10.0, 10.0);
    assert!(focused_widget().is_some(), "the click focused the recorder");
    (id, at)
}

#[test]
fn captures_run_outside_in_before_the_focused_widget_and_bubbling_after() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (id, mut at) = nested_captures(&mut app, &heard);

    send(&mut app, id, &mut at, key_down(Key::Enter, false));
    send(
        &mut app,
        id,
        &mut at,
        key_up(Key::Enter, Modifiers::default()),
    );

    let route = [
        ("outer capture", Key::Enter),
        ("inner capture", Key::Enter),
        ("focused", Key::Enter),
        ("inner", Key::Enter),
        ("outer", Key::Enter),
    ];
    assert_eq!(
        *heard.borrow(),
        [route, route].concat(),
        "the press went down through both captures to the focused widget and \
         bubbled back up; its release went the same way"
    );
}

#[test]
fn a_capture_that_takes_a_key_keeps_it_from_everything_after_it() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (id, mut at) = nested_captures(&mut app, &heard);
    let focused = focused_widget();

    key(&mut app, id, &mut at, Key::Escape);
    send(
        &mut app,
        id,
        &mut at,
        key_up(Key::Escape, Modifiers::default()),
    );
    key(&mut app, id, &mut at, Key::Tab);

    assert_eq!(
        *heard.borrow(),
        [
            ("outer capture", Key::Escape),
            ("outer capture", Key::Escape),
            ("outer capture", Key::Tab),
            ("inner capture", Key::Tab),
        ],
        "the outer capture took Escape, so neither the inner capture, the \
         focused widget nor a bubbling listener heard it go down or up; the \
         inner capture took Tab"
    );
    assert_eq!(
        focused_widget(),
        focused,
        "a Tab a capture took does not move the focus"
    );
}

/// With nothing focused, a key goes down to the containers listening for it,
/// and a capture listener hears it on the way, before the listeners inside it.
#[test]
fn a_capture_hears_a_key_with_nothing_focused_before_the_listeners_inside_it() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (outer, inner) = (heard.clone(), heard.clone());
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            capturing("capture", outer.clone(), &[Key::Escape])
                .height(100.0)
                .child(passer("inside", inner.clone()).height(50.0)),
        )
    });

    key(&mut app, id, &mut at, Key::Enter);
    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(focused_widget(), None);
    assert_eq!(
        *heard.borrow(),
        [
            ("capture", Key::Enter),
            ("inside", Key::Enter),
            ("capture", Key::Escape),
        ],
        "the capture heard each key before the listener inside it, and took Escape"
    );
}

/// A capture on the focus path that let a key through is not asked again when
/// the way down to a listener beside the focus passes it.
#[test]
fn a_capture_on_the_focus_path_hears_a_key_once() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (around, beside) = (heard.clone(), heard.clone());
    let typed = create_signal(String::new());
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            capturing("capture", around.clone(), &[])
                .height(100.0)
                .layout(Flex::column())
                .child(
                    container()
                        .height(30.0)
                        .width(fill())
                        .child(text_input(typed)),
                )
                .child(passer("beside", beside.clone()).height(30.0)),
        )
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    key(&mut app, id, &mut at, Key::Escape);
    send(
        &mut app,
        id,
        &mut at,
        key_up(Key::Escape, Modifiers::default()),
    );

    assert_eq!(
        *heard.borrow(),
        [
            ("capture", Key::Escape),
            ("beside", Key::Escape),
            ("capture", Key::Escape)
        ],
        "the press and its release, each heard once by the capture; the \
         listener beside the field hears presses alone"
    );
}

/// A focused container is on its own focus path, so it hears its own capture
/// first, as a capture listener on the web's target does.
#[test]
fn a_focused_container_hears_its_own_capture_before_its_own_listener() {
    let Some(mut app) = headless() else { return };
    let heard = Rc::new(RefCell::new(Vec::new()));
    let (captured, bubbled) = (heard.clone(), heard.clone());
    let (id, mut at) = surface(&mut app, move || {
        let bubbled = bubbled.clone();
        container().child(
            capturing("capture", captured.clone(), &[Key::Escape])
                .height(30.0)
                .focusable(true)
                .on_key_down(move |key, _, _| {
                    bubbled.borrow_mut().push(("own", key));
                    EventResponse::Handled
                }),
        )
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    assert!(
        focused_widget().is_some(),
        "the click focused the container"
    );
    key(&mut app, id, &mut at, Key::Enter);
    key(&mut app, id, &mut at, Key::Escape);

    assert_eq!(
        *heard.borrow(),
        [
            ("capture", Key::Enter),
            ("own", Key::Enter),
            ("capture", Key::Escape),
        ]
    );
}
