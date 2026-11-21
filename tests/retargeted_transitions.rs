//! A retarget starts the next segment from where the running one is on the
//! frame of the change, and only a delay holds a value back (#939, #940).
//!
//! Each case is a translate, read every 16 ms frame through the real loop as
//! the distance between where the widget is drawn and where it is laid out:
//! a translate moves the first and never the second.

#![cfg(feature = "testing")]

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;

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

/// The target set on each of ten frames, 3 px further each time, and where the
/// box is drawn on that frame — or `None` without an adapter.
fn chased(transition: Transition) -> Option<Vec<(f32, f32)>> {
    let mut app = Headless::new_or_skip()?;
    let offset = create_signal(0.0_f32);
    let placed = translated(&mut app, offset, transition);
    let mut at = Instant::now();
    app.step_at(at);
    Some(
        (1..=10)
            .map(|frame| {
                let target = frame as f32 * 3.0;
                offset.set(target);
                (target, moved(&mut app, &mut at, placed))
            })
            .collect(),
    )
}

/// Every frame is further along than the one before.
fn moves_on_every_frame(frames: &[(f32, f32)]) -> bool {
    frames.windows(2).all(|pair| pair[1].1 > pair[0].1)
}

/// The target changes on every frame, as one that follows something animating
/// does: with no duration, every frame shows that frame's target.
#[test]
fn a_zero_duration_transition_is_at_each_frames_target_on_that_frame() {
    let Some(frames) = chased(Transition::new(0.0, TimingFunction::Linear)) else {
        return;
    };
    assert!(
        frames.iter().all(|&(target, drawn)| drawn == target),
        "(target, drawn) on each frame: {frames:?}"
    );
}

#[test]
fn a_spring_retargeted_on_every_frame_keeps_moving() {
    let Some(frames) = chased(Transition::spring(SpringConfig::DEFAULT)) else {
        return;
    };
    assert!(moves_on_every_frame(&frames), "(target, drawn): {frames:?}");
}

#[test]
fn a_timed_transition_retargeted_on_every_frame_keeps_moving() {
    let Some(frames) = chased(Transition::new(200.0, TimingFunction::Linear)) else {
        return;
    };
    assert!(moves_on_every_frame(&frames), "(target, drawn): {frames:?}");
}

/// A delay counts from the retarget, as CSS has it: retargeted more often
/// than its delay, a value stands still.
#[test]
fn a_delay_retargeted_on_every_frame_holds_the_value() {
    let Some(frames) = chased(Transition::new(200.0, TimingFunction::Linear).delay(100.0)) else {
        return;
    };
    assert!(
        frames.iter().all(|&(_, drawn)| drawn == 0.0),
        "(target, drawn): {frames:?}"
    );
}

/// What still holds a value back is a delay, and only for as long as it says.
#[test]
fn a_delay_holds_a_zero_duration_transition_until_it_has_passed() {
    let Some(mut app) = Headless::new_or_skip() else {
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

/// A delayed transition retargeted while it moves is drawn where the catch-up
/// took it on the frame of the change, though the segment that follows is
/// still waiting out its own delay.
#[test]
fn a_retarget_inside_the_next_delay_draws_where_the_last_segment_got_to() {
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let offset = create_signal(0.0_f32);
    let placed = translated(
        &mut app,
        offset,
        Transition::new(400.0, TimingFunction::Linear).delay(50.0),
    );
    let mut at = Instant::now();
    app.step_at(at);

    offset.set(100.0);
    let before: Vec<f32> = (0..8).map(|_| moved(&mut app, &mut at, placed)).collect();
    let last = *before.last().expect("eight frames");
    assert!(last > 0.0, "moving by now: {before:?}");

    offset.set(200.0);
    let retargeted = moved(&mut app, &mut at, placed);
    assert!(
        retargeted > last,
        "a frame further along than {last}, got {retargeted}: {before:?}"
    );
}
