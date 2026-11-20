//! A transition with no duration lands on the frame that set its target, and a
//! delay is the only thing that holds a value back (#940).
//!
//! Each case is a translate, read every 16 ms frame through the real loop as
//! the distance between where the widget is drawn and where it is laid out:
//! a translate moves the first and never the second.

#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;

mod common;

const FRAME: Duration = Duration::from_millis(16);

/// A surface holding one box whose translate follows `offset` through
/// `transition`, and the box.
fn translated(
    app: &mut guido::testing::Headless,
    offset: RwSignal<f32>,
    transition: Transition,
) -> WidgetRef {
    let placed = create_widget_ref();
    let surface = app.surface(
        SurfaceConfig::new()
            .height(100)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().child(
                container()
                    .width(20.0)
                    .height(20.0)
                    .translate((move || Translate::new(0.0, offset.get())).transition(transition))
                    .widget_ref(placed),
            )
        },
    );
    app.configure(surface, 800, 100, 1.0);
    placed
}

/// How far `placed` is drawn from where it is laid out, after one more frame.
fn moved(app: &mut guido::testing::Headless, at: &mut Instant, placed: WidgetRef) -> f32 {
    *at += FRAME;
    app.step_at(*at);
    placed.rect().get_untracked().y - placed.layout_rect().get_untracked().y
}

/// The target changes on every frame, as one that follows something animating
/// does: with no duration, every frame shows that frame's target, where
/// before it showed none of them until the target stopped changing.
#[test]
fn a_zero_duration_transition_is_at_each_frames_target_on_that_frame() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let offset = create_signal(0.0_f32);
    let placed = translated(
        &mut app,
        offset,
        Transition::new(0.0, TimingFunction::Linear),
    );
    let mut at = Instant::now();
    app.step_at(at);

    let frames: Vec<(f32, f32)> = (1..=10)
        .map(|frame| {
            let target = frame as f32 * 3.0;
            offset.set(target);
            (target, moved(&mut app, &mut at, placed))
        })
        .collect();
    assert!(
        frames.iter().all(|&(target, drawn)| drawn == target),
        "(target, drawn) on each frame: {frames:?}"
    );
}

/// What still holds a value back is a delay, and only for as long as it says.
#[test]
fn a_delay_holds_a_zero_duration_transition_until_it_has_passed() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let offset = create_signal(0.0_f32);
    let placed = translated(
        &mut app,
        offset,
        Transition::new(0.0, TimingFunction::Linear).delay(100.0),
    );
    let mut at = Instant::now();
    app.step_at(at);

    offset.set(10.0);
    // The first of these frames starts the segment, so they are 0, 16, …,
    // 112 ms into it: the seventh is the last inside the delay.
    let drawn: Vec<f32> = (0..8).map(|_| moved(&mut app, &mut at, placed)).collect();
    assert_eq!(
        drawn,
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0],
        "held for 100 ms, then there"
    );
}
