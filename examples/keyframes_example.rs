//! A sequence played on a trigger, rather than a value animated towards.
//!
//! Run with: cargo run --example keyframes_example
//!
//! Click either card. The left one shakes its head — the motion a lock screen
//! wants after a wrong password — and the right one takes the same trigger and
//! nods, to show that what a timeline does is entirely in its stops.
//!
//! Both cards also declare a hover scale. That is the point of the rule that a
//! playing timeline *replaces* the declared value: the hover is still there, it
//! just does not argue with the sequence while it runs, and it has the property
//! back the moment the sequence ends. Note that the shake is a timeline on
//! `rotate` and the nod one on `translate` — a sequence belongs to the one
//! component it moves, and the hover's `scale` is untouched by either.

use guido::prelude::*;

/// Left, decaying: out, back past, out again, still.
fn shake() -> Keyframes<f32> {
    Keyframes::new(320.0)
        .at(0.0, 0.0)
        .at(0.15, 2.0)
        .at(0.40, -1.6)
        .at(0.65, 0.9)
        .at(0.85, -0.4)
        .at(1.0, 0.0)
}

/// Right, one dip and a rebound, eased so the fall is quicker than the rise.
fn nod() -> Keyframes<Translate> {
    Keyframes::new(360.0)
        .at_with(0.0, Translate::NONE, TimingFunction::EaseIn)
        .at_with(0.35, Translate::new(0.0, 14.0), TimingFunction::EaseOut)
        .at(0.7, Translate::new(0.0, -4.0))
        .at(1.0, Translate::NONE)
}

/// The card itself: the timeline is added by the caller, because a shake and a
/// nod move different components and so are different types of sequence.
fn card(label: &'static str, plays: RwSignal<u32>) -> Container {
    container()
        .width(220.0)
        .height(120.0)
        .corners(12.0)
        .background(Color::rgb(0.18, 0.18, 0.24))
        .border(1.0, Color::rgb(0.32, 0.32, 0.4))
        .layout(
            Flex::column()
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        // Declared, animated, and quietly stood aside while a sequence runs.
        .when_hovered(|s| s.scale(1.03))
        .animate_scale(Transition::spring(SpringConfig::SNAPPY))
        .on_click(move || plays.update(|p| *p += 1))
        .child(text(label).color(Color::WHITE).font_size(16.0))
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let plays = create_signal(0u32);

        let view = container()
            .padding(24.0)
            .layout(Flex::row().spacing(20.0))
            .child(card("shake", plays).keyframes_rotate(shake(), plays))
            .child(card("nod", plays).keyframes_translate(nod(), plays));

        app.add_surface(
            SurfaceConfig::new()
                .width(540)
                .height(190)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("keyframes-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
