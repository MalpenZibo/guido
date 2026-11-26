//! A component forwards a callback prop as it reads.
//!
//! Inside a `#[component]` body a callback prop is a
//! `Signal<Option<Callback<..>>>`. Handing it to an inner component's builder
//! is `.on_change(on_change)`, the same spelling `Container::on_click` already
//! takes, and a closure still goes in where it always did. A caller whose
//! callback comes and goes with state gives a closure returning an
//! `Option<Callback>`, and the body follows it.
#![cfg(feature = "testing")]

use std::cell::Cell;

use guido::prelude::*;
use guido::testing::Headless;
use guido::widgets::{Event, MouseButton};

thread_local! {
    /// Whether the inner picker was built holding a callback: `None` until it
    /// is built.
    static PICKER_HELD_ONE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The inner component: a click reports `7`.
#[component]
fn picker(#[prop(callback)] on_change: fn(u32)) -> impl Widget {
    PICKER_HELD_ONE.with(|held| held.set(Some(on_change.with_untracked(Option::is_some))));
    container().width(fill()).height(fill()).on_click(move || {
        if let Some(cb) = on_change.get_untracked() {
            cb.run(7);
        }
    })
}

/// The outer component hands its own prop straight to the inner one.
#[component]
fn panel(#[prop(callback)] on_change: fn(u32)) -> impl Widget {
    picker().on_change(on_change)
}

fn bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(40)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
}

#[test]
fn a_click_in_the_inner_component_reaches_the_outer_callers_closure() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let seen = create_signal(0u32);
    let surface = app.surface(bar(), move || panel().on_change(move |v: u32| seen.set(v)));
    app.configure(surface, 200, 40, 1.0);
    app.step();

    app.click(surface, 20.0, 20.0);
    app.step();

    assert_eq!(
        seen.get_untracked(),
        7,
        "the outer prop, forwarded as it is, is the closure the click runs"
    );
}

#[test]
fn forwarding_none_leaves_the_inner_prop_unset() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    PICKER_HELD_ONE.with(|held| held.set(None));
    let surface = app.surface(bar(), panel);
    app.configure(surface, 200, 40, 1.0);
    app.step();

    assert_eq!(
        PICKER_HELD_ONE.with(Cell::get),
        Some(false),
        "an outer prop nobody set reaches the inner component as no callback"
    );
}

/// A closure goes in as it did, and one that leaves its parameter's type to
/// the prop still compiles. (A body that calls a method on that parameter
/// before anything else pins its type is the exception: the prop's type
/// reaches the closure through a trait, so it has to say `|v: u32|`.)
#[test]
fn a_closure_still_goes_in_as_it_did() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let seen = create_signal(0u32);
    let surface = app.surface(bar(), move || picker().on_change(move |v| seen.set(v + 1)));
    app.configure(surface, 200, 40, 1.0);
    app.step();

    app.click(surface, 20.0, 20.0);
    app.step();

    assert_eq!(seen.get_untracked(), 8);
}

/// A row that opens more when its chevron is pressed, and only while it is
/// given somewhere to go: the chevron forwards the prop to its own
/// `on_click`, and paints red while the prop holds a callback.
#[component]
fn slider_row(#[prop(callback)] on_more: ()) -> impl Widget {
    container()
        .width(fill())
        .height(fill())
        .background(move || {
            if on_more.with(Option::is_some) {
                Color::rgb(1.0, 0.0, 0.0)
            } else {
                Color::rgb(0.0, 0.0, 1.0)
            }
        })
        .on_click(on_more)
}

/// A component that hands its prop, a signal inside the body, straight to
/// another component's setter.
#[component]
fn forwarding_row(#[prop(callback)] on_more: ()) -> impl Widget {
    slider_row().on_more(on_more)
}

/// Every form a callback prop's setter takes. A handler closure, a
/// `Callback` and an `Option` of one are fixed when the component is built;
/// a closure giving an `Option<Callback>`, and the signal a body forwards,
/// follow what they read.
#[test]
fn a_callback_prop_takes_a_handler_or_anything_that_gives_one() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let open = Callback::new(|| {});
    let several = create_signal(true);
    let surface = app.surface(bar(), move || {
        container()
            .layout(Flex::row())
            .child(slider_row().on_more(|| {}))
            .child(slider_row().on_more(open))
            .child(slider_row().on_more(Some(open)))
            .child(slider_row().on_more(None))
            .child(slider_row().on_more(move || several.get().then_some(open)))
            .child(forwarding_row().on_more(move || several.get().then_some(open)))
            .child(picker().on_change(|v| assert_eq!(v, 7)))
    });
    app.configure(surface, 200, 40, 1.0);
    app.step();
}

/// The caller's signal decides, press by press, whether the chevron takes the
/// press or lets it through to the row it sits in.
#[test]
fn a_press_follows_the_callback_the_signal_holds_now() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let several = create_signal(false);
    let opened = create_signal(0u32);
    let row_pressed = create_signal(0u32);
    let open = Callback::new(move || opened.update(|n| *n += 1));
    let surface = app.surface(bar(), move || {
        container()
            .width(fill())
            .height(fill())
            .on_click(move || row_pressed.update(|n| *n += 1))
            .child(
                container()
                    .width(40.0)
                    .height(40.0)
                    .child(slider_row().on_more(move || several.get().then_some(open))),
            )
    });
    app.configure(surface, 200, 40, 1.0);
    app.step();

    app.click(surface, 20.0, 20.0);
    app.step();
    assert_eq!(
        (opened.get_untracked(), row_pressed.get_untracked()),
        (0, 1),
        "with no callback the chevron is not a target, and the row takes the press"
    );

    several.set(true);
    app.step();
    app.click(surface, 20.0, 20.0);
    app.step();
    assert_eq!(
        (opened.get_untracked(), row_pressed.get_untracked()),
        (1, 1),
        "once the signal holds one, the chevron's callback takes the press"
    );

    several.set(false);
    app.step();
    app.click(surface, 20.0, 20.0);
    app.step();
    assert_eq!(
        (opened.get_untracked(), row_pressed.get_untracked()),
        (1, 2),
        "and when it goes again, the press is the row's again"
    );
}

/// A body that reads whether the prop is set repaints when the caller's
/// signal changes, without the component being built again.
#[test]
fn a_body_reading_the_prop_follows_the_callers_signal() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let several = create_signal(false);
    let open = Callback::new(|| {});
    let surface = app.surface(bar(), move || {
        slider_row().on_more(move || several.get().then_some(open))
    });
    app.configure(surface, 200, 40, 1.0);
    app.step();
    let [r, _, b, _] = app.read_pixel(surface, 100, 20);
    assert!(b > 200 && r < 50, "no callback yet: blue, got {r}/{b}");

    several.set(true);
    app.step();
    let [r, _, b, _] = app.read_pixel(surface, 100, 20);
    assert!(r > 200 && b < 50, "a callback now: red, got {r}/{b}");
}

/// While the signal holds nothing, a container that paints nothing and has no
/// other handler is not there to be hit at all: a press goes on to the layer
/// beneath it, which is not one of its ancestors and would never hear a press
/// it let bubble.
#[test]
fn an_unset_callback_leaves_the_press_to_the_layer_beneath() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let several = create_signal(false);
    let opened = create_signal(0u32);
    let beneath = create_signal(0u32);
    let open = Callback::new(move || opened.update(|n| *n += 1));
    let surface = app.surface(bar(), move || {
        container()
            .width(fill())
            .height(fill())
            .layout(ZStack::new())
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .on_click(move || beneath.update(|n| *n += 1)),
            )
            .child(
                container()
                    .width(fill())
                    .height(fill())
                    .on_click(move || several.get().then_some(open)),
            )
    });
    app.configure(surface, 200, 40, 1.0);
    app.step();

    app.click(surface, 20.0, 20.0);
    app.step();
    assert_eq!(
        (opened.get_untracked(), beneath.get_untracked()),
        (0, 1),
        "nothing to open: the layer beneath takes the press"
    );

    several.set(true);
    app.step();
    app.click(surface, 20.0, 20.0);
    app.step();
    assert_eq!(
        (opened.get_untracked(), beneath.get_untracked()),
        (1, 1),
        "something to open: the layer on top takes it"
    );
}

/// The right button follows the signal as the left one does: with nothing to
/// run the press is not the container's, and the row around it hears it.
#[test]
fn a_right_press_follows_the_callback_the_signal_holds_now() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let several = create_signal(false);
    let opened = create_signal(0u32);
    let row_pressed = create_signal(0u32);
    let open = Callback::new(move || opened.update(|n| *n += 1));
    let surface = app.surface(bar(), move || {
        container()
            .width(fill())
            .height(fill())
            .on_right_click(move || row_pressed.update(|n| *n += 1))
            .child(
                container()
                    .width(40.0)
                    .height(40.0)
                    .on_right_click(move || several.get().then_some(open)),
            )
    });
    app.configure(surface, 200, 40, 1.0);
    app.step();
    let right_press = |app: &mut Headless| {
        let now = std::time::Instant::now();
        app.event_at(
            surface,
            Event::mouse_down(20.0, 20.0, MouseButton::Right),
            now,
        );
        app.event_at(
            surface,
            Event::mouse_up(20.0, 20.0, MouseButton::Right),
            now,
        );
        app.step();
    };

    right_press(&mut app);
    assert_eq!(
        (opened.get_untracked(), row_pressed.get_untracked()),
        (0, 1),
        "nothing to run: the row takes the right press"
    );

    several.set(true);
    app.step();
    right_press(&mut app);
    assert_eq!(
        (opened.get_untracked(), row_pressed.get_untracked()),
        (1, 1),
        "a callback now: the inner container takes it"
    );
}
