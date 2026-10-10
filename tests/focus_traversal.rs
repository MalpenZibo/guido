//! Tab moves the keyboard focus to the next widget that takes it, and
//! Shift+Tab to the one before (#634).
//!
//! Traversal is what a Tab does by default: it runs after the focus path and
//! the listeners, for a key-down nobody answered `Handled`. The stops are the
//! text inputs and the containers that declare `focusable(true)`, in reading
//! order — by the vertical centre of where each was laid out, then the
//! horizontal one — among those on the surface holding the keyboard, and the
//! ends wrap.

#![cfg(feature = "testing")]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::reactive::focus::{clear_focus, focus_path, focused_widget, request_focus};
use guido::testing::Headless;
use guido::widget_prelude::*;

mod common;
use common::headless;

fn surface(app: &mut Headless, view: impl Fn() -> Container + 'static) -> (SurfaceId, Instant) {
    let id = app.surface(
        SurfaceConfig::new()
            .width(200)
            .height(300)
            .anchor(Anchor::TOP | Anchor::LEFT),
        view,
    );
    app.configure(id, 200, 300, 1.0);
    let at = Instant::now();
    app.step_at(at);
    (id, at)
}

fn send(app: &mut Headless, id: SurfaceId, at: &mut Instant, event: Event) {
    *at += Duration::from_millis(16);
    app.event_at(id, event, *at);
    app.step_at(*at);
}

fn tab(app: &mut Headless, id: SurfaceId, at: &mut Instant) {
    press_tab(app, id, at, false);
}

fn shift_tab(app: &mut Headless, id: SurfaceId, at: &mut Instant) {
    press_tab(app, id, at, true);
}

fn press_tab(app: &mut Headless, id: SurfaceId, at: &mut Instant, shift: bool) {
    let modifiers = Modifiers {
        shift,
        ..Modifiers::default()
    };
    send(
        app,
        id,
        at,
        Event::KeyDown {
            key: Key::Tab,
            modifiers,
            repeat: false,
        },
    );
    send(
        app,
        id,
        at,
        Event::KeyUp {
            key: Key::Tab,
            modifiers,
        },
    );
}

fn click(app: &mut Headless, id: SurfaceId, at: &mut Instant, x: f32, y: f32) {
    send(app, id, at, Event::mouse_down(x, y, MouseButton::Left));
    send(app, id, at, Event::mouse_up(x, y, MouseButton::Left));
}

/// A text input 30 pixels tall across the width, that `r` names.
fn field(r: WidgetRef) -> Container {
    container()
        .height(30.0)
        .width(fill())
        .child(text_input(create_signal(String::new())).widget_ref(r))
}

/// Which of `refs` holds the focus, by its index.
fn focused(refs: &[WidgetRef]) -> Option<usize> {
    let widget = focused_widget()?;
    refs.iter().position(|r| r.widget() == Some(widget))
}

fn refs<const N: usize>() -> [WidgetRef; N] {
    std::array::from_fn(|_| create_widget_ref())
}

/// Three fields in a column, 30 pixels apart, the first at the top.
fn three_fields(app: &mut Headless) -> ([WidgetRef; 3], SurfaceId, Instant) {
    let fields = refs::<3>();
    let (id, at) = surface(app, move || {
        container()
            .layout(Flex::column())
            .children(fields.map(field))
    });
    (fields, id, at)
}

#[test]
fn tab_and_shift_tab_walk_the_fields_in_order() {
    let Some(mut app) = headless() else { return };
    let (fields, id, mut at) = three_fields(&mut app);

    click(&mut app, id, &mut at, 10.0, 10.0);
    assert_eq!(focused(&fields), Some(0), "the click focused the first");

    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(1));
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(2));
    shift_tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(1));
}

#[test]
fn the_ends_wrap() {
    let Some(mut app) = headless() else { return };
    let (fields, id, mut at) = three_fields(&mut app);

    click(&mut app, id, &mut at, 10.0, 70.0);
    assert_eq!(focused(&fields), Some(2));
    tab(&mut app, id, &mut at);
    assert_eq!(
        focused(&fields),
        Some(0),
        "Tab at the last goes to the first"
    );
    shift_tab(&mut app, id, &mut at);
    assert_eq!(
        focused(&fields),
        Some(2),
        "Shift+Tab at the first goes to the last"
    );
}

#[test]
fn with_nothing_focused_tab_takes_the_first_and_shift_tab_the_last() {
    let Some(mut app) = headless() else { return };
    let (fields, id, mut at) = three_fields(&mut app);

    assert_eq!(focused_widget(), None);
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(0));

    clear_focus();
    shift_tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(2));
}

/// Tab and Shift+Tab move the focus; Tab with Ctrl, Alt or the logo key held
/// is a shortcut of somebody else's, as Flutter's `SingleActivator` reads it.
#[test]
fn tab_with_another_modifier_does_not_move_the_focus() {
    let Some(mut app) = headless() else { return };
    let (fields, id, mut at) = three_fields(&mut app);

    click(&mut app, id, &mut at, 10.0, 10.0);
    for modifiers in [
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
        Modifiers {
            alt: true,
            ..Modifiers::default()
        },
        Modifiers {
            logo: true,
            ..Modifiers::default()
        },
    ] {
        send(
            &mut app,
            id,
            &mut at,
            Event::KeyDown {
                key: Key::Tab,
                modifiers,
                repeat: false,
            },
        );
        assert_eq!(focused(&fields), Some(0), "{modifiers:?}");
    }
}

#[test]
fn tab_says_the_focus_came_from_the_keyboard() {
    let Some(mut app) = headless() else { return };
    let (fields, id, mut at) = three_fields(&mut app);

    click(&mut app, id, &mut at, 10.0, 10.0);
    assert!(!focus_path().by_keyboard(), "a click is not the keyboard");
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(1));
    assert!(focus_path().by_keyboard(), "Tab is");
    click(&mut app, id, &mut at, 10.0, 40.0);
    assert_eq!(focused(&fields), Some(1));
    assert!(
        !focus_path().by_keyboard(),
        "and a click on the field Tab focused takes it back from the keyboard"
    );
    click(&mut app, id, &mut at, 10.0, 70.0);
    assert!(!focus_path().by_keyboard(), "and a click after it is not");
}

#[test]
fn hidden_and_disabled_fields_are_passed_over() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<4>();
    let shown = create_signal(true);
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(field(fields[0]))
            // Hidden after it was laid out, so it still has the bounds it had
            // when it showed: what passes it over is that it is hidden.
            .child(container().visible(shown).child(field(fields[1])))
            .child(container().enabled(false).child(field(fields[2])))
            .child(field(fields[3]))
    });
    shown.set(false);
    at += Duration::from_millis(16);
    app.step_at(at);

    click(&mut app, id, &mut at, 10.0, 10.0);
    assert_eq!(focused(&fields), Some(0));
    tab(&mut app, id, &mut at);
    assert_eq!(
        focused(&fields),
        Some(3),
        "the hidden field and the disabled one were passed over"
    );
    shift_tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(0));
}

#[test]
fn a_field_playing_its_exit_is_passed_over() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<3>();
    let shown = create_signal(true);
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(field(fields[0]))
            .children(move || {
                shown.get().then(|| {
                    field(fields[1]).translate(
                        Translate::NONE
                            .transition(Transition::new(200.0, TimingFunction::Linear))
                            .exiting_to(Translate::new(-100.0, 0.0)),
                    )
                })
            })
            .child(field(fields[2]))
    });
    shown.set(false);
    at += Duration::from_millis(16);
    app.step_at(at);
    assert!(fields[1].widget().is_some(), "still in the tree, leaving");

    click(&mut app, id, &mut at, 10.0, 10.0);
    tab(&mut app, id, &mut at);
    assert_eq!(
        focused(&fields),
        Some(2),
        "the leaving field was passed over"
    );
}

#[test]
fn reading_order_is_where_the_fields_are_not_where_they_were_declared() {
    let Some(mut app) = headless() else { return };
    let [left, right] = refs::<2>();
    let fields = [left, right];
    let (id, mut at) = surface(&mut app, move || {
        let slot = |r| container().width(80.0).child(field(r));
        container()
            .layout(ZStack::new())
            // The right one first, pushed across by a spacer.
            .child(
                container()
                    .layout(Flex::row())
                    .child(container().width(100.0))
                    .child(slot(right)),
            )
            .child(container().layout(Flex::row()).child(slot(left)))
    });

    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(0), "the left one first");
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(1), "then the right one");
}

/// Which of two focusable boxes Tab takes first and second, each box given as
/// `(x, y, width, height)` on a stack.
fn first_two(boxes: [(f32, f32, f32, f32); 2]) -> Option<[Option<usize>; 2]> {
    let mut app = headless()?;
    let stops = refs::<2>();
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(ZStack::new())
            .children(stops.into_iter().zip(boxes).map(|(r, (x, y, w, h))| {
                container()
                    .padding([y, 0.0, 0.0, x])
                    .child(container().width(w).height(h).focusable(true).widget_ref(r))
            }))
    });
    let mut order = [None; 2];
    for slot in &mut order {
        tab(&mut app, id, &mut at);
        *slot = focused(&stops);
    }
    Some(order)
}

/// Reading order is by the centre of each box, not its top or left edge, and
/// not any other mix of edge and size: one box starting higher but reaching
/// lower comes after one wholly inside its span, and a tall box high up comes
/// before a short one below its middle.
#[test]
fn reading_order_goes_by_the_centre_of_each_box() {
    let cases = [
        // Starts lower, centred higher (50 against 60).
        (
            [(0.0, 40.0, 20.0, 20.0), (40.0, 0.0, 20.0, 120.0)],
            "vertical, centre over top",
        ),
        // Centred at 30 against 45, though twice its height reaches further.
        (
            [(0.0, 0.0, 20.0, 60.0), (40.0, 40.0, 20.0, 10.0)],
            "vertical, centre over size",
        ),
        // The same two along a row, every centre at the same height.
        (
            [(40.0, 0.0, 20.0, 20.0), (0.0, 0.0, 120.0, 20.0)],
            "horizontal, centre over left",
        ),
        (
            [(0.0, 0.0, 60.0, 20.0), (40.0, 0.0, 10.0, 20.0)],
            "horizontal, centre over size",
        ),
    ];
    for (boxes, case) in cases {
        let Some(order) = first_two(boxes) else {
            return;
        };
        assert_eq!(order, [Some(0), Some(1)], "{case}");
    }
}

/// Where Tab goes from nothing focused, `presses` times over, by index into
/// `refs`.
fn tab_round(
    app: &mut Headless,
    id: SurfaceId,
    at: &mut Instant,
    refs: &[WidgetRef],
    presses: usize,
) -> Vec<Option<usize>> {
    clear_focus();
    (0..presses)
        .map(|_| {
            tab(app, id, at);
            focused(refs)
        })
        .collect()
}

/// A slot that holds a field while `shown` says so: the field leaves the tree
/// and joins it again as `shown` changes, and the slot keeps its place.
fn slot(r: WidgetRef, shown: RwSignal<bool>) -> Container {
    container().children(move || shown.get().then(|| field(r)))
}

#[test]
fn many_fields_leaving_and_joining_keep_reading_order() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<40>();
    let shown: [RwSignal<bool>; 40] = std::array::from_fn(|_| create_signal(true));
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .children((0..40).map(|i| slot(fields[i], shown[i])))
    });

    let every = |indices: &mut dyn Iterator<Item = usize>| -> Vec<Option<usize>> {
        let mut round: Vec<_> = indices.map(Some).collect();
        round.push(round[0]);
        round
    };
    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 41),
        every(&mut (0..40)),
        "top to bottom, and round"
    );

    for i in (0..40).filter(|i| i % 3 == 1) {
        shown[i].set(false);
    }
    at += Duration::from_millis(16);
    app.step_at(at);
    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 28),
        every(&mut (0..40).filter(|i| i % 3 != 1)),
        "the ones that left are not visited, the rest keep their order"
    );

    for i in (0..40).filter(|i| i % 6 == 1) {
        shown[i].set(true);
    }
    at += Duration::from_millis(16);
    app.step_at(at);
    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 35),
        every(&mut (0..40).filter(|i| i % 6 != 4)),
        "the ones that came back are visited where they stand, not last"
    );
}

#[test]
fn fields_in_one_place_go_in_the_order_they_joined() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<6>();
    let shown: [RwSignal<bool>; 6] = std::array::from_fn(|_| create_signal(true));
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(ZStack::new())
            .children((0..6).map(|i| slot(fields[i], shown[i])))
    });
    let round = |order: &[usize]| -> Vec<Option<usize>> {
        order.iter().chain(&order[..1]).map(|&i| Some(i)).collect()
    };

    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 7),
        round(&[0, 1, 2, 3, 4, 5]),
        "one on top of another, in the order they were declared"
    );

    shown[1].set(false);
    shown[3].set(false);
    at += Duration::from_millis(16);
    app.step_at(at);
    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 5),
        round(&[0, 2, 4, 5])
    );

    shown[1].set(true);
    at += Duration::from_millis(16);
    app.step_at(at);
    assert_eq!(
        tab_round(&mut app, id, &mut at, &fields, 6),
        round(&[0, 2, 4, 5, 1]),
        "the one that came back joined last, so it goes last of the tie"
    );
}

/// A widget that takes the focus when pressed, and takes Tab for itself — an
/// editor inserting a tab character.
struct TakesTab;

impl Widget for TakesTab {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(200.0, 30.0)
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        match event {
            Event::MouseDown { .. } => {
                request_focus(tree, id);
                EventResponse::Handled
            }
            Event::KeyDown { key: Key::Tab, .. } => EventResponse::Handled,
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, _ctx: &mut PaintContext) {}
}

#[test]
fn a_focused_widget_that_takes_tab_keeps_the_focus() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<2>();
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(field(fields[0]))
            .child(TakesTab)
            .child(field(fields[1]))
    });

    click(&mut app, id, &mut at, 10.0, 40.0);
    let editor = focused_widget();
    assert!(editor.is_some() && focused(&fields).is_none());
    tab(&mut app, id, &mut at);
    assert_eq!(focused_widget(), editor, "the editor took the Tab");
}

/// Three fields, the middle one inside a container listening for keys that
/// answers `answer` to every one.
fn around_the_middle(app: &mut Headless, answer: EventResponse) -> Option<usize> {
    let fields = refs::<3>();
    let (id, mut at) = surface(app, move || {
        container()
            .layout(Flex::column())
            .child(field(fields[0]))
            .child(field(fields[1]).on_key_down(move |_, _, _| answer))
            .child(field(fields[2]))
    });
    click(app, id, &mut at, 10.0, 40.0);
    assert_eq!(focused(&fields), Some(1));
    tab(app, id, &mut at);
    focused(&fields)
}

#[test]
fn a_listener_around_the_focus_that_lets_tab_through_lets_it_move() {
    let Some(mut app) = headless() else { return };
    assert_eq!(around_the_middle(&mut app, EventResponse::Ignored), Some(2));
}

#[test]
fn a_listener_around_the_focus_that_takes_tab_keeps_it() {
    let Some(mut app) = headless() else { return };
    assert_eq!(around_the_middle(&mut app, EventResponse::Handled), Some(1));
}

#[test]
fn a_popup_keeps_tab_among_its_own_fields() {
    let Some(mut app) = headless() else { return };
    let outside = refs::<2>();
    let inside = refs::<2>();
    let (parent, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .children(outside.map(field))
    });
    let popup = spawn_popup(parent, PopupConfig::new(200).height(60), move || {
        container()
            .layout(Flex::column())
            .children(inside.map(field))
    });
    at += Duration::from_millis(16);
    app.step_at(at);

    click(&mut app, popup.id(), &mut at, 10.0, 10.0);
    assert_eq!(focused(&inside), Some(0));
    tab(&mut app, popup.id(), &mut at);
    assert_eq!(focused(&inside), Some(1));
    tab(&mut app, popup.id(), &mut at);
    assert_eq!(
        focused(&inside),
        Some(0),
        "the popup's last wraps to its own first, not to the bar beneath it"
    );
}

/// Two fields with a container between them that declares `focusable(true)` —
/// inside a row that counts its clicks, when `in_a_row`.
fn a_focusable_between(
    app: &mut Headless,
    in_a_row: bool,
) -> ([WidgetRef; 3], Rc<Cell<u32>>, SurfaceId, Instant) {
    let stops = refs::<3>();
    let clicks = Rc::new(Cell::new(0));
    let counted = clicks.clone();
    let (id, at) = surface(app, move || {
        let card = container()
            .height(30.0)
            .width(fill())
            .focusable(true)
            .widget_ref(stops[1])
            .child(text("a card"));
        let counted = counted.clone();
        let middle = if in_a_row {
            container()
                .width(fill())
                .on_click(move || counted.set(counted.get() + 1))
                .child(card)
        } else {
            card
        };
        container()
            .layout(Flex::column())
            .child(field(stops[0]))
            .child(middle)
            .child(field(stops[2]))
    });
    (stops, clicks, id, at)
}

#[test]
fn a_focusable_container_is_a_stop() {
    let Some(mut app) = headless() else { return };
    let (stops, _, id, mut at) = a_focusable_between(&mut app, false);

    click(&mut app, id, &mut at, 10.0, 10.0);
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&stops), Some(1), "the container between them");
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&stops), Some(2));
    shift_tab(&mut app, id, &mut at);
    assert_eq!(focused(&stops), Some(1));
}

#[test]
fn a_press_inside_a_focusable_container_focuses_it() {
    let Some(mut app) = headless() else { return };
    for in_a_row in [false, true] {
        let (stops, clicks, id, mut at) = a_focusable_between(&mut app, in_a_row);

        click(&mut app, id, &mut at, 10.0, 10.0);
        assert_eq!(focused(&stops), Some(0));
        click(&mut app, id, &mut at, 150.0, 45.0);
        assert_eq!(
            focused(&stops),
            Some(1),
            "the press focused the container (in a row: {in_a_row})"
        );
        assert!(!focus_path().by_keyboard());
        assert_eq!(
            clicks.get(),
            u32::from(in_a_row),
            "and did not take the click from a row around it"
        );
        surface_handle(id).close();
    }
}

/// Nested focusable containers: a press goes to the innermost one it landed
/// in, as a click focuses the innermost focusable element on the web.
#[test]
fn a_press_focuses_the_innermost_focusable_container() {
    let Some(mut app) = headless() else { return };
    let [outer, inner] = refs::<2>();
    let stops = [outer, inner];
    let (id, mut at) = surface(&mut app, move || {
        container().child(
            container()
                .height(100.0)
                .width(fill())
                .focusable(true)
                .widget_ref(outer)
                .child(
                    container()
                        .height(30.0)
                        .width(fill())
                        .focusable(true)
                        .widget_ref(inner),
                ),
        )
    });

    click(&mut app, id, &mut at, 10.0, 10.0);
    assert_eq!(focused(&stops), Some(1), "the inner one");
    click(&mut app, id, &mut at, 10.0, 60.0);
    assert_eq!(focused(&stops), Some(0), "and below it, the outer one");
}

/// A widget that takes the focus when pressed and is not a Tab stop, as a
/// widget focused by a ref is not.
///
/// Pressed rather than focused through a `WidgetRef`: a ref's request is
/// applied by the frame the wake flag asks for, and that flag belongs to the
/// process, so another test's `Headless` stepping on another thread can take
/// it first and leave this one's request parked.
struct FocusedByPress;

impl Widget for FocusedByPress {
    fn layout(&mut self, _ctx: &mut LayoutCtx, _constraints: Constraints) -> Size {
        Size::new(200.0, 30.0)
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

/// A widget focused without being a stop: Tab goes on from where it stands,
/// to the next stop after it, and Shift+Tab to the one before.
#[test]
fn from_a_widget_that_is_not_a_stop_tab_goes_on_from_where_it_stands() {
    let Some(mut app) = headless() else { return };
    let fields = refs::<2>();
    let (id, mut at) = surface(&mut app, move || {
        container()
            .layout(Flex::column())
            .child(field(fields[0]))
            .child(FocusedByPress)
            .child(field(fields[1]))
    });

    click(&mut app, id, &mut at, 10.0, 40.0);
    let middle = focused_widget();
    assert!(
        middle.is_some() && focused(&fields).is_none(),
        "the press focused the widget between the fields"
    );
    tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(1), "Tab: the stop after it");

    click(&mut app, id, &mut at, 10.0, 40.0);
    assert_eq!(focused_widget(), middle);
    shift_tab(&mut app, id, &mut at);
    assert_eq!(focused(&fields), Some(0), "Shift+Tab: the stop before it");
}
