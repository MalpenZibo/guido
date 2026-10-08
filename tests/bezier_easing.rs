use guido::prelude::*;
use guido::widget_prelude::FrameInstant;
use guido::widgets::container::AnimationState;
use std::time::{Duration, Instant};

#[test]
fn a_flat_bezier_transition_does_not_jump_outside_its_stops() {
    let mut animation = AnimationState::new(
        0.0_f32,
        Transition::new(1000.0, TimingFunction::CubicBezier(0.0, 0.0, 0.0, 1.0)),
    );
    let start = Instant::now();
    animation.animate_to(100.0, FrameInstant::from(start));
    animation.advance(FrameInstant::from(start + Duration::from_millis(1)));
    assert!((animation.current() - 2.8).abs() < 1e-4);
}

#[test]
fn a_flat_bezier_keyframe_segment_does_not_jump_outside_its_stops() {
    let frames = Keyframes::new(1000.0)
        .at_with(
            0.0,
            0.0_f32,
            TimingFunction::CubicBezier(0.0, 0.0, 0.0, 1.0),
        )
        .at(1.0, 100.0);
    assert!((frames.value_at(1.0).unwrap() - 2.8).abs() < 1e-4);
}
