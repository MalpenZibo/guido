#![cfg(feature = "testing")]
//! A callback can wait on the UI thread and read a signal when it runs (#561).
//!
//! `set_timeout` and `set_interval` run a closure on the loop's own pass, at a
//! deadline on the pass's clock, so `Headless::step_at` is what moves time and
//! nothing here sleeps. Each test names the moments it steps to, and a timer
//! is armed by the first pass that sees it: its deadline is that pass's moment
//! plus its delay.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::reactive::dispose_owner;
use guido::reactive::owner::with_owner;
use guido::testing::Headless;

mod common;
use common::headless;

const MS: Duration = Duration::from_millis(1);

/// A surface whose whole area is `colour`, so a frame says what it was.
fn painted(app: &mut Headless, colour: RwSignal<Color>) -> SurfaceId {
    let surface = app.surface(
        SurfaceConfig::new()
            .height(10)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || container().width(fill()).height(fill()).background(colour),
    );
    app.configure(surface, 10, 10, 1.0);
    surface
}

/// The timeout runs when its deadline is reached and not a moment before, reads
/// a signal — which a `create_task` future cannot — and what it writes is in
/// the frame of the pass that ran it.
#[test]
fn a_timeout_reads_a_signal_and_writes_another_when_due() {
    let Some(mut app) = headless() else { return };
    let resting = create_signal(false);
    let colour = create_signal(Color::BLUE);
    let surface = painted(&mut app, colour);

    create_effect(move || {
        if !resting.get() {
            set_timeout(350 * MS, move || {
                if !resting.get_untracked() {
                    colour.set(Color::RED);
                }
            });
        }
    });

    let t0 = Instant::now();
    app.step_at(t0);
    app.step_at(t0 + 349 * MS);
    assert_eq!(
        colour.get_untracked(),
        Color::BLUE,
        "not before its deadline"
    );

    app.step_at(t0 + 350 * MS);
    assert_eq!(colour.get_untracked(), Color::RED, "run at its deadline");
    assert_eq!(
        app.read_pixel(surface, 5, 5),
        [255, 0, 0, 255],
        "and its write is in the frame that pass drew"
    );
}

/// An effect that schedules a timeout on every run and cancels the one before
/// it fires only the last: a debounce, with no generation counter. The cancel
/// is written out — an effect re-runs under the scope it was created in, so its
/// last run's timer is still that scope's.
#[test]
fn a_timeout_scheduled_again_by_its_effect_replaces_the_last() {
    let Some(mut app) = headless() else { return };
    let query = create_signal(String::from("gu"));
    let searched = Rc::new(RefCell::new(Vec::new()));

    let log = searched.clone();
    let last = Cell::new(None::<TimerHandle>);
    create_effect(move || {
        let q = query.get();
        let log = log.clone();
        let timer = set_timeout(300 * MS, move || log.borrow_mut().push(q));
        if let Some(previous) = last.replace(Some(timer)) {
            previous.cancel();
        }
    });

    let t0 = Instant::now();
    app.step_at(t0);
    query.set(String::from("guido"));
    app.step_at(t0 + 100 * MS);
    app.step_at(t0 + 399 * MS);
    assert!(
        searched.borrow().is_empty(),
        "nothing yet: the second was armed at +100"
    );

    app.step_at(t0 + 450 * MS);
    assert_eq!(*searched.borrow(), ["guido"], "once, with the second value");
}

/// Disposing the scope a timeout was scheduled in cancels it.
#[test]
fn a_timeout_goes_with_its_scope() {
    let Some(mut app) = headless() else { return };
    let ran = Rc::new(Cell::new(false));

    let flag = ran.clone();
    let ((), scope) = with_owner(|| {
        set_timeout(100 * MS, move || flag.set(true));
    });

    let t0 = Instant::now();
    app.step_at(t0);
    dispose_owner(scope);
    app.step_at(t0 + 50 * MS);
    app.step_at(t0 + 200 * MS);
    assert!(!ran.get(), "a timeout outlived the scope that scheduled it");
}

/// `cancel` stops a timeout before it runs, and is harmless after.
#[test]
fn a_cancelled_timeout_never_runs() {
    let Some(mut app) = headless() else { return };
    let runs = Rc::new(Cell::new(0));

    let count = runs.clone();
    let timer = set_timeout(100 * MS, move || count.set(count.get() + 1));
    assert!(timer.is_pending());

    let t0 = Instant::now();
    app.step_at(t0);
    timer.cancel();
    assert!(!timer.is_pending(), "cancelled");
    app.step_at(t0 + 200 * MS);
    assert_eq!(runs.get(), 0, "a cancelled timeout ran");

    let count = runs.clone();
    let ran_already = set_timeout(10 * MS, move || count.set(count.get() + 1));
    app.step_at(t0 + 300 * MS);
    app.step_at(t0 + 400 * MS);
    assert_eq!(runs.get(), 1);
    assert!(
        !ran_already.is_pending(),
        "a timeout that ran is no longer pending"
    );
    ran_already.cancel();
}

/// An interval runs once per period on the period's own beat, once — not a
/// burst — for a pass that arrives several periods late, and never again after
/// `cancel`.
#[test]
fn an_interval_runs_once_per_period_until_cancelled() {
    let Some(mut app) = headless() else { return };
    let ticks = Rc::new(Cell::new(0));

    let count = ticks.clone();
    let timer = set_interval(Duration::from_secs(1), move || count.set(count.get() + 1));

    let t0 = Instant::now();
    app.step_at(t0);
    for (second, expected) in [(1, 1), (2, 2), (3, 3)] {
        app.step_at(t0 + Duration::from_secs(second));
        assert_eq!(ticks.get(), expected, "at +{second} s");
    }

    app.step_at(t0 + Duration::from_millis(5500));
    assert_eq!(
        ticks.get(),
        4,
        "a late pass runs it once, not once per missed period"
    );
    app.step_at(t0 + Duration::from_millis(5900));
    assert_eq!(ticks.get(), 4, "and its beat stays on whole seconds");
    app.step_at(t0 + Duration::from_secs(6));
    assert_eq!(ticks.get(), 5);

    timer.cancel();
    app.step_at(t0 + Duration::from_secs(9));
    assert_eq!(ticks.get(), 5, "an interval ran after it was cancelled");
}

/// Timers due in the same pass run earliest first, and those due at the same
/// moment in the order they were created.
#[test]
fn timers_due_in_one_pass_run_in_deadline_order() {
    let Some(mut app) = headless() else { return };
    let order = Rc::new(RefCell::new(Vec::new()));

    for (name, delay) in [("first at 100", 100), ("second at 100", 100), ("at 50", 50)] {
        let log = order.clone();
        set_timeout(delay * MS, move || log.borrow_mut().push(name));
    }

    let t0 = Instant::now();
    app.step_at(t0);
    app.step_at(t0 + 200 * MS);
    assert_eq!(*order.borrow(), ["at 50", "first at 100", "second at 100"]);
}
