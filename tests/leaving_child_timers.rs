#![cfg(feature = "testing")]
//! A leaving child's timers are paused with its effects (#845).
//!
//! A child that plays an exit is detached when it is removed and disposed when
//! the exit settles, and in between it is inert: its effects are paused, and so
//! are the `set_interval` and `set_timeout` timers its scope owns. None fires
//! between its removal and its disposal. A keyed row whose key comes back
//! before then has them resume with what was left of their period, not with a
//! catch-up fire for the time it was paused.
//!
//! The loop is the real one, stepped every 10 ms on the pass's clock, so a
//! timer's deadlines are the moments named here.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;

mod common;
use common::headless;

const MS: Duration = Duration::from_millis(1);

/// A box that slides out over 200 ms when it is taken away.
fn leaving_box() -> Container {
    container().width(20.0).height(10.0).translate(
        Translate::NONE
            .transition(Transition::new(200.0, TimingFunction::Linear))
            .exiting_to(Translate::new(-100.0, 0.0)),
    )
}

/// A box that counts into `ticks[at]` every 50 ms while it lives.
fn ticking_box(ticks: &Rc<[Cell<u32>; 2]>, at: usize) -> Container {
    let ticks = ticks.clone();
    set_interval(50 * MS, move || ticks[at].set(ticks[at].get() + 1));
    leaving_box()
}

/// A surface whose root `build` makes.
fn surface(app: &mut Headless, build: impl FnOnce() -> Container) {
    let surface = app.surface(
        SurfaceConfig::new()
            .height(10)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        build,
    );
    app.configure(surface, 400, 10, 1.0);
}

/// Steps every 10 ms from `from` to `to`, both included.
fn step_through(app: &mut Headless, t0: Instant, from: u32, to: u32) {
    for ms in (from..=to).step_by(10) {
        app.step_at(t0 + ms * MS);
    }
}

/// The issue's scene: a dynamic child that starts an interval each time it is
/// built, rebuilt at 160 ms. The old one ticked three times before it was
/// removed, and not once more through its exit or after it.
#[test]
fn a_leaving_childs_interval_does_not_fire_through_its_exit() {
    let Some(mut app) = headless() else { return };
    let step = create_signal(0usize);
    let ticks = Rc::new([Cell::new(0), Cell::new(0)]);

    let counters = ticks.clone();
    surface(&mut app, move || {
        let counters = counters.clone();
        container().child(move || ticking_box(&counters, step.get()))
    });

    let t0 = Instant::now();
    step_through(&mut app, t0, 0, 150);
    assert_eq!(
        ticks[0].get(),
        3,
        "three ticks by 150 ms, before its removal"
    );

    step.set(1);
    app.step_at(t0 + 160 * MS);
    let at_removal = ticks[0].get();
    assert_eq!(at_removal, 3);

    step_through(&mut app, t0, 170, 340);
    assert_eq!(
        ticks[0].get(),
        at_removal,
        "row 0 is leaving: its interval does not fire through its exit"
    );

    step_through(&mut app, t0, 350, 600);
    assert_eq!(
        ticks[0].get(),
        at_removal,
        "and nothing fires once the exit has settled"
    );
    assert!(ticks[1].get() > 0, "the child that replaced it ticks");
}

/// A keyed row taken away at 160 ms and asked for again at 220 ms, mid-exit.
/// Its interval last fired at 150 and was paused with 40 ms of its period
/// left; resumed at 220, it fires at 260 and then every 50 ms — not at once
/// for the beats it missed, and not a whole period after the resume.
#[test]
fn a_keyed_row_that_comes_back_resumes_its_timer_with_the_rest_of_its_period() {
    let Some(mut app) = headless() else { return };
    let rows = create_signal(vec![0usize]);
    let ticks = Rc::new([Cell::new(0), Cell::new(0)]);

    let counters = ticks.clone();
    surface(&mut app, move || {
        let counters = counters.clone();
        container().children(keyed(
            move || rows.get(),
            |&row| row,
            move |row| ticking_box(&counters, row),
        ))
    });

    let t0 = Instant::now();
    step_through(&mut app, t0, 0, 150);
    assert_eq!(ticks[0].get(), 3, "three ticks by 150 ms");

    rows.set(vec![]);
    step_through(&mut app, t0, 160, 210);
    assert_eq!(ticks[0].get(), 3, "paused while it leaves");

    rows.set(vec![0]);
    step_through(&mut app, t0, 220, 250);
    assert_eq!(
        ticks[0].get(),
        3,
        "no catch-up fire on the resume: 40 ms of its period were left"
    );

    app.step_at(t0 + 260 * MS);
    assert_eq!(
        ticks[0].get(),
        4,
        "the rest of its period, counted from 220"
    );

    step_through(&mut app, t0, 270, 300);
    assert_eq!(ticks[0].get(), 4);
    app.step_at(t0 + 310 * MS);
    assert_eq!(ticks[0].get(), 5, "and its own beat from there");
}
