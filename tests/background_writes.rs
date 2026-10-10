#![cfg(feature = "testing")]
//! A background write lands in the application whose signal it names, though
//! another application in the process came and went before it was applied
//! (#642).
//!
//! The write queue and the epoch that retires stale writes are the process's,
//! not a thread's, and guido runs one application at a time per process. A test
//! binary is the one place that runs several: cargo gives each test a thread
//! and each test a `Headless`. Dropping one used to retire every write queued
//! in the process, including the writes of a `Headless` still running beside
//! it. `App::drop` still does, because a program's next application must not
//! receive the last one's writes; `Headless::drop` leaves the process alone.
//!
//! A binary of its own: the queue is drained by whichever thread steps, so a
//! test beside it that stepped at the wrong moment would apply this write on
//! its own thread.

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
