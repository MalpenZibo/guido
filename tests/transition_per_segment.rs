//! A transition is read when a segment starts, so the application says which
//! one plays instead of guido guessing it from the values (#889).
//!
//! Each case is a width, laid out by the real loop and read every 16 ms frame:
//! a width is a layout answer, so `layout_rect` sees exactly what the
//! transition did. The opening transition is ten times slower than the closing
//! one, so which of the two played is never a close call.

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

/// The width `placed` has after each of `frames` frames, the first one being
/// the frame that sees whatever was just set.
fn widths(
    app: &mut guido::testing::Headless,
    at: &mut Instant,
    placed: WidgetRef,
    frames: usize,
) -> Vec<f32> {
    (0..frames)
        .map(|_| {
            *at += FRAME;
            app.step_at(*at);
            placed.layout_rect().get_untracked().width
        })
        .collect()
}

/// A closure over the application's state picks the transition each time the
/// width changes: it opens slowly and closes fast, as the state says.
#[test]
fn a_transition_read_from_state_picks_the_close_when_it_closes() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let open = create_signal(false);
    let placed = create_widget_ref();
    let surface = app.surface(bar(), move || {
        container().child(
            container()
                .height(20.0)
                .width(
                    (move || Length::exact(if open.get() { 300.0 } else { 20.0 })).transition(
                        move || {
                            if open.get() {
                                linear(1000.0)
                            } else {
                                linear(100.0)
                            }
                        },
                    ),
                )
                .widget_ref(placed),
        )
    });
    app.configure(surface, 800, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);

    open.set(true);
    let opening = widths(&mut app, &mut at, placed, 8);
    assert!(
        opening.iter().all(|&w| w < 300.0 * 0.2),
        "opening takes the slow transition: {opening:?}"
    );

    widths(&mut app, &mut at, placed, 70);
    open.set(false);
    let closing = widths(&mut app, &mut at, placed, 8);
    assert_eq!(
        closing.last(),
        Some(&20.0),
        "closing takes the fast one, and is done in 100 ms: {closing:?}"
    );
}

/// An exit reads the transition at removal, when the state that removed the
/// widget already says it is closing.
#[test]
fn an_exit_reads_the_transition_when_the_widget_is_removed() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let open = create_signal(true);
    let placed = create_widget_ref();
    let surface = app.surface(bar(), move || {
        container().children(keyed(
            move || open.get().then_some(()).into_iter().collect::<Vec<_>>(),
            |_| (),
            move |_| {
                container()
                    .height(20.0)
                    .width(
                        Length::exact(300.0)
                            .transition(move || {
                                if open.get() {
                                    linear(1000.0)
                                } else {
                                    linear(100.0)
                                }
                            })
                            .exiting_to(Length::exact(0.0)),
                    )
                    .widget_ref(placed)
            },
        ))
    });
    app.configure(surface, 800, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    widths(&mut app, &mut at, placed, 4);

    open.set(false);
    let leaving = widths(&mut app, &mut at, placed, 4);
    assert!(
        leaving[3] < 300.0 * 0.6,
        "48 ms into a 100 ms exit it is past halfway, where the 1000 ms \
         opening transition would still be within a twentieth: {leaving:?}"
    );
}

/// A transition chosen from the segment itself, for a size whose direction no
/// state knows: a box that follows its content grows late and shrinks at
/// once, and an interrupted grow is measured from where the width is.
#[test]
fn a_transition_chosen_by_the_segment_grows_late_and_shrinks_at_once() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let content = create_signal(100.0_f32);
    let placed = create_widget_ref();
    let surface = app.surface(bar(), move || {
        container().child(
            container()
                .height(20.0)
                .width(at_least(0.0).transition(|s: Segment<f32>| {
                    if s.to > s.from {
                        linear(100.0).delay(200.0)
                    } else {
                        linear(100.0)
                    }
                }))
                .child(container().height(20.0).width(move || content.get()))
                .widget_ref(placed),
        )
    });
    app.configure(surface, 800, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    widths(&mut app, &mut at, placed, 4);

    content.set(300.0);
    let growing = widths(&mut app, &mut at, placed, 8);
    assert!(
        growing.iter().all(|&w| w == 100.0),
        "a grow waits out its delay: {growing:?}"
    );

    content.set(50.0);
    let shrinking = widths(&mut app, &mut at, placed, 8);
    assert_eq!(
        shrinking.last(),
        Some(&50.0),
        "a shrink interrupting it starts at once from where the width is, \
         and lands in 100 ms: {shrinking:?}"
    );
}

/// A property that only enters and leaves needs no state to choose its
/// transitions: the segment says which it is. Here the enter shrinks and the
/// exit grows, so a choice read off the direction would pick the other one
/// both times.
#[test]
fn a_segment_says_whether_it_enters_or_leaves() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let shown = create_signal(true);
    let placed = create_widget_ref();
    let surface = app.surface(bar(), move || {
        container().children(keyed(
            move || shown.get().then_some(()).into_iter().collect::<Vec<_>>(),
            |_| (),
            move |_| {
                container()
                    .height(20.0)
                    .width(
                        Length::exact(300.0)
                            .transition(|s: Segment<f32>| {
                                if s.is_enter() {
                                    linear(1000.0)
                                } else if s.is_exit() {
                                    linear(100.0)
                                } else {
                                    // a retarget, which this row never makes: a
                                    // kind mistaken for one shows as a jump
                                    linear(0.0)
                                }
                            })
                            .entering_from(Length::exact(600.0))
                            .exiting_to(Length::exact(600.0)),
                    )
                    .widget_ref(placed)
            },
        ))
    });
    app.configure(surface, 800, 100, 1.0);
    let mut at = Instant::now();
    app.step_at(at);
    let entering = widths(&mut app, &mut at, placed, 4);
    assert!(
        entering.iter().all(|&w| w > 540.0),
        "the enter takes the slow one, shrinking though it is: {entering:?}"
    );

    widths(&mut app, &mut at, placed, 70);
    shown.set(false);
    let leaving = widths(&mut app, &mut at, placed, 4);
    assert!(
        leaving[3] > 420.0,
        "the exit takes the fast one, growing though it is: 48 ms into 100 it \
         is past 444, where the slow one would be short of 315: {leaving:?}"
    );
}
