#![cfg(feature = "testing")]
//! A `GlobalSignal` declared outside the crate, as a library built on guido
//! declares one: state that belongs to the application and not to whichever
//! surface happened to read it first.

use std::cell::Cell;
use std::rc::Rc;

use guido::prelude::*;
use guido::reactive::GlobalSignal;
use guido::testing::Headless;

static WALLPAPER_COLORS: GlobalSignal<u32> = GlobalSignal::new(|| 0);

fn bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(24)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
}

/// The first surface to read the global creates it, inside that surface's
/// scope. Closing the surface disposes the scope; the signal is the root's,
/// so the surface opened after it reads the value the first one wrote.
#[test]
fn a_global_outlives_the_surface_that_read_it_first() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let first = app.surface(bar(), || {
        WALLPAPER_COLORS.get().set(7);
        container()
    });

    let seen = Rc::new(Cell::new(None));
    let seen_by_second = seen.clone();
    surface_handle(first).close();
    spawn_surface(bar(), move || {
        seen_by_second.set(Some(WALLPAPER_COLORS.get().get_untracked()));
        container()
    });
    app.step();

    assert_eq!(app.surfaces_destroyed(), [first]);
    assert_eq!(
        seen.get(),
        Some(7),
        "the second surface reads the signal the first one wrote"
    );
}
