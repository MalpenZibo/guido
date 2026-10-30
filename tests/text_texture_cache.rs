#![cfg(feature = "testing")]
//! A text drawn as a quad gives its texture back once nothing draws it (#913).

use std::time::{Duration, Instant};

use guido::prelude::*;

mod common;

/// A panel that opens and closes over and over, its rounded corner passing
/// over its label, at the uneven pace a compositor's frames arrive at, holds
/// no more textures after thirty times than after three.
///
/// While the corner crosses it the label is drawn as a quad, rasterized into
/// a texture the size of its box, and its box changes height with the panel —
/// so nearly every frame asks for a size no frame asked for before. A steady
/// pace asks for the same sizes every time, which is why the steps here are
/// uneven: what the cache kept then was every size it had ever been asked for.
#[test]
fn an_animation_repeated_holds_no_more_text_textures_than_once() {
    let Some(mut app) = common::headless() else {
        return;
    };
    let open = create_signal(false);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(200)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
        move || {
            container().width(fill()).child(
                container()
                    .width(fill())
                    .height(
                        (move || if open.get() { 150.0 } else { 0.0 })
                            .transition(Transition::new(200.0, TimingFunction::EaseOut)),
                    )
                    .overflow(Overflow::Hidden)
                    .background(Color::rgb(0.18, 0.18, 0.26))
                    .corners(12.0)
                    .child(container().padding(16.0).child(text("A panel"))),
            )
        },
    );
    app.configure(surface, 400, 200, 2.0);

    // A fixed sequence, so a failure repeats.
    let mut seed: u64 = 42;
    let mut at = Instant::now();
    let mut cycle = |app: &mut guido::testing::Headless| {
        for opened in [true, false] {
            open.set(opened);
            for _ in 0..40 {
                app.step_at(at);
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                at += Duration::from_micros(7000 + (seed >> 33) % 2500);
            }
        }
    };

    for _ in 0..3 {
        cycle(&mut app);
    }
    let after_three = app.text_textures();
    for _ in 3..30 {
        cycle(&mut app);
    }

    assert!(
        app.text_textures() <= after_three,
        "{} text textures after thirty cycles, {after_three} after three",
        app.text_textures()
    );
}
