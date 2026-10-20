//! A component forwards a callback prop as it reads.
//!
//! Inside a `#[component]` body a callback prop is an `Option<Callback<..>>`.
//! Handing it to an inner component's builder is `.on_change(on_change)`, the
//! same spelling `Container::on_click` already takes, and a closure still goes
//! in where it always did.
#![cfg(feature = "testing")]

use std::cell::Cell;

use guido::prelude::*;
mod common;
use common::headless;

thread_local! {
    /// Whether the inner picker was built holding a callback: `None` until it
    /// is built.
    static PICKER_HELD_ONE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// The inner component: a click reports `7`.
#[component]
fn picker(#[prop(callback)] on_change: fn(u32)) -> impl Widget {
    PICKER_HELD_ONE.with(|held| held.set(Some(on_change.is_some())));
    container().width(fill()).height(fill()).on_click(move || {
        if let Some(cb) = on_change {
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
    let Some(mut app) = headless() else { return };
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
    let Some(mut app) = headless() else { return };
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
    let Some(mut app) = headless() else { return };
    let seen = create_signal(0u32);
    let surface = app.surface(bar(), move || picker().on_change(move |v| seen.set(v + 1)));
    app.configure(surface, 200, 40, 1.0);
    app.step();

    app.click(surface, 20.0, 20.0);
    app.step();

    assert_eq!(seen.get_untracked(), 8);
}
