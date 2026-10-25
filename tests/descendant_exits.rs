//! A removed row asks the widgets built inside it for their exits too, as far
//! as the next dynamic boundary, and is disposed when the last of them has
//! settled (#891).
//!
//! Each case is a keyed row in a `Flex` row, read every 16 ms frame through the
//! real loop: the list is as wide as the rows it still holds, so its width
//! says when a row is disposed, and the child's own width says whether its
//! exit is playing.

#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;

mod common;

const FRAME: Duration = Duration::from_millis(16);

fn linear(ms: f32) -> Transition {
    Transition::new(ms, TimingFunction::Linear)
}

fn bar() -> SurfaceConfig {
    SurfaceConfig::new()
        .height(100)
        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
}

/// A child 300 wide that leaves by narrowing to nothing over 100 ms.
fn narrowing(placed: WidgetRef) -> Container {
    container()
        .height(20.0)
        .width(
            Length::exact(300.0)
                .transition(linear(100.0))
                .exiting_to(Length::exact(0.0)),
        )
        .widget_ref(placed)
}

struct Run {
    app: guido::testing::Headless,
    at: Instant,
    list: WidgetRef,
    child: WidgetRef,
}

impl Run {
    /// One row while `shown`, built by `row` around a child that `child`
    /// points at.
    fn new(shown: Signal<bool>, row: fn(WidgetRef) -> Container) -> Option<Self> {
        let mut app = common::headless()?;
        let list = create_widget_ref();
        let child = create_widget_ref();
        let surface = app.surface(bar(), move || {
            container().child(
                container()
                    .layout(Flex::row())
                    .children(keyed(
                        move || shown.get().then_some(()).into_iter().collect::<Vec<_>>(),
                        |_| (),
                        move |_| row(child),
                    ))
                    .widget_ref(list),
            )
        });
        app.configure(surface, 800, 100, 1.0);
        let at = Instant::now();
        app.step_at(at);
        let mut run = Self {
            app,
            at,
            list,
            child,
        };
        run.frames(4);
        Some(run)
    }

    /// The list's width and the child's after each of `n` frames.
    fn frames(&mut self, n: usize) -> Vec<(f32, f32)> {
        (0..n)
            .map(|_| {
                self.at += FRAME;
                self.app.step_at(self.at);
                (
                    self.list.layout_rect().get_untracked().width,
                    self.child.layout_rect().get_untracked().width,
                )
            })
            .collect()
    }
}

/// A child built inside the row plays its exit when the row is removed, though
/// the row declares none, and the row stays until it has settled.
#[test]
fn a_child_built_in_a_removed_row_plays_its_exit() {
    let shown = create_signal(true);
    let Some(mut run) = Run::new(shown.into(), |child| container().child(narrowing(child))) else {
        return;
    };

    shown.set(false);
    let leaving = run.frames(10);
    assert!(
        leaving[2].1 < 300.0 && leaving[2].1 > 0.0,
        "the child narrows rather than going at once: {leaving:?}"
    );
    assert!(
        leaving[2].0 > 0.0,
        "and the row stays in the list while it does: {leaving:?}"
    );
    assert_eq!(
        leaving.last().map(|frame| frame.0),
        Some(0.0),
        "then the row is disposed: {leaving:?}"
    );
}

/// A child behind a dynamic boundary of its own is that boundary's to remove:
/// the row going does not play its exit, and nothing waits for it.
#[test]
fn a_child_behind_its_own_boundary_does_not_play_its_exit() {
    let shown = create_signal(true);
    let Some(mut run) = Run::new(shown.into(), |child| {
        container().child(move || Some(narrowing(child)))
    }) else {
        return;
    };

    shown.set(false);
    let leaving = run.frames(2);
    assert_eq!(
        leaving[0].0, 0.0,
        "the row has nothing it asked to wait for, and goes at once: {leaving:?}"
    );
}

/// A key that comes back mid-exit reclaims its row, and every exit that row
/// asked for goes back from where it got to.
#[test]
fn a_reclaimed_row_brings_its_children_back() {
    let shown = create_signal(true);
    let Some(mut run) = Run::new(shown.into(), |child| container().child(narrowing(child))) else {
        return;
    };

    shown.set(false);
    let leaving = run.frames(3);
    assert!(leaving[2].1 < 300.0, "the exit is under way: {leaving:?}");

    shown.set(true);
    let back = run.frames(10);
    assert!(
        back[0].1 < 300.0 && back[1].1 > back[0].1,
        "it turns back from where it got to rather than jumping home: {back:?}"
    );
    assert_eq!(
        back.last().map(|frame| frame.1),
        Some(300.0),
        "and arrives home: {back:?}"
    );
}
