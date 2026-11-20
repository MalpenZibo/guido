#![cfg(feature = "testing")]
//! A background write lands in the application whose signal it names, and in
//! no other.
//!
//! A test binary is the one place that runs several applications at once:
//! cargo gives each test a thread and each test a `Headless`. Their signal ids
//! are slots in their own threads' storage, so two of them hand out the same
//! ones, and a write has to know whose it is (#642, #943).

use guido::prelude::*;
use guido::testing::Headless;

mod common;
use common::headless;

#[test]
fn a_background_write_lands_though_another_application_was_dropped() {
    let Some(mut app) = headless() else { return };
    let value = create_signal(0u32);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        container,
    );
    app.configure(surface, 100, 20, 1.0);
    app.step();

    // Queued from a thread of its own, as a service or a decoder queues it.
    let writer = value.writer();
    std::thread::spawn(move || writer.set(7))
        .join()
        .expect("the writer thread finished");

    // Another application, on another thread, comes and goes before this one
    // applies the write.
    std::thread::spawn(|| {
        let other: Option<Headless> = headless();
        assert!(other.is_some(), "the second application started");
    })
    .join()
    .expect("the second application came and went");

    app.step();
    assert_eq!(
        value.get_untracked(),
        7,
        "the write was retired by an application it had nothing to do with"
    );
}

/// An application, and a signal holding `value` made the same way in each, so
/// two of them on two fresh threads hand out the same signal id.
fn app_holding<T: Clone + PartialEq + Send + 'static>(value: T) -> Option<(Headless, RwSignal<T>)> {
    let mut app = headless()?;
    let signal = create_signal(value);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(20)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        container,
    );
    app.configure(surface, 100, 20, 1.0);
    app.step();
    Some((app, signal))
}

/// A background write reaches the application whose signal it names, never
/// one beside it whose signal has the same id — which it used to land on, and
/// panic on when the two signals held different types (#943). Both ways: a
/// write queued from a thread of its own, which the other application used to
/// apply when it stepped, and a write made on the other application's thread,
/// which used to go straight into its signal.
#[test]
fn a_background_write_never_lands_in_another_applications_signal() {
    use std::sync::mpsc::channel;

    let Some((mut app, fonts)) = app_holding(vec![String::from("Sans")]) else {
        return;
    };

    let writer = fonts.writer();
    let (ready_tx, ready_rx) = channel();
    let (step_tx, step_rx) = channel::<()>();
    let other = std::thread::spawn(move || {
        let (mut other, count) = app_holding(0u32).expect("the first one had an adapter");
        ready_tx.send(()).expect("the test is waiting");
        step_rx.recv().expect("the test says when");
        writer.set(vec![String::from("Mono")]);
        other.step();
        count.get_untracked()
    });
    ready_rx.recv().expect("the other application started");

    std::thread::spawn(move || writer.set(vec![String::from("Serif")]))
        .join()
        .expect("the writer thread finished");

    // The other application steps first, which is when it used to take the
    // write for its own.
    step_tx.send(()).expect("the other application is waiting");
    let count = other
        .join()
        .expect("the other application stepped without a write it does not own");
    assert_eq!(count, 0, "the other application's signal is its own");

    app.step();
    assert_eq!(
        fonts.get_untracked(),
        [String::from("Mono")],
        "both writes reached the application they name, in the order they were made"
    );
}
