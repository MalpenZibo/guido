//! What a spring does when it is interrupted.
//!
//! Every panel is one property of the carried momentum, and every one of them
//! is something you have to *interrupt by hand* to see — which is why this is
//! an example and not a test. Each says what to do to it and what should
//! happen.

use guido::prelude::*;

const BG: Color = Color::rgb(0.08, 0.08, 0.12);
const CARD: Color = Color::rgb(0.16, 0.16, 0.22);
const INK: Color = Color::rgb(0.35, 0.62, 0.95);
const HOT: Color = Color::rgb(0.95, 0.55, 0.25);

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(900)
                .height(620)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(BG),
            move || {
                container()
                    .background(BG)
                    .padding(24.0)
                    .layout(Flex::column().spacing(16.0))
                    .children([
                        text("Spring momentum — interrupt each one by hand")
                            .color(Color::WHITE)
                            .into_any(),
                        turn_around().into_any(),
                        past_the_overshoot().into_any(),
                        own_channel().into_any(),
                        from_rest().into_any(),
                        shake().into_any(),
                    ])
            },
        );
    });
}

fn panel(title: &str, what_to_do: &str, body: Container) -> Container {
    container()
        .background(CARD)
        .corners(10.0)
        .padding(14.0)
        .layout(Flex::column().spacing(8.0))
        .children([
            text(title.to_string()).color(Color::WHITE).into_any(),
            text(what_to_do.to_string())
                .color(Color::rgb(0.62, 0.64, 0.74))
                .into_any(),
            body.into_any(),
        ])
}

/// A track with a dot that slides along it.
fn track(dot: Container) -> Container {
    container()
        .width(fill())
        .height(46.0)
        .background(Color::rgb(0.11, 0.11, 0.16))
        .corners(8.0)
        .padding(6.0)
        .child(dot)
}

fn dot(color: Color) -> Container {
    container()
        .width(34.0)
        .height(34.0)
        .background(color)
        .corners(17.0)
}

/// The headline: reversed mid-flight, the spring turns around instead of
/// stopping first.
fn turn_around() -> Container {
    let out = create_signal(false);
    panel(
        "1 — It turns around",
        "Hover the strip, then leave it while the dot is still travelling. It \
         should reverse without pausing at the point you left it.",
        track(
            dot(INK)
                .transform(move || Transform::translate(if out.get() { 760.0 } else { 0.0 }, 0.0))
                .animate_transform(Transition::spring(SpringConfig::GENTLE)),
        )
        .control()
        .on_hover(move |hovered| out.set(hovered)),
    )
}

/// The case the old code inverted: interrupt *after* the overshoot, while the
/// dot is already falling back toward its target.
fn past_the_overshoot() -> Container {
    let out = create_signal(false);
    panel(
        "2 — Interrupted past its overshoot",
        "Same, but this spring overshoots hard. Leave the strip just *after* \
         the dot has shot past the end and is settling back — it should carry \
         on the way it was going, not lurch forward first.",
        track(
            dot(HOT)
                .transform(move || Transform::translate(if out.get() { 700.0 } else { 0.0 }, 0.0))
                .animate_transform(Transition::spring(SpringConfig::BOUNCY)),
        )
        .control()
        .on_hover(move |hovered| out.set(hovered)),
    )
}

/// Momentum stays in the channel it belonged to.
fn own_channel() -> Container {
    let out = create_signal(false);
    let big = create_signal(false);
    panel(
        "3 — Momentum stays in its own channel",
        "Hover to send it sliding, then press while it is still moving. The \
         press only scales it: the slide's speed must not leak into the size \
         and make it jump.",
        track(
            dot(INK)
                .transform(move || {
                    let slide = Transform::translate(if out.get() { 700.0 } else { 0.0 }, 0.0);
                    if big.get() {
                        Transform::scale(1.6).then(&slide)
                    } else {
                        slide
                    }
                })
                .animate_transform(Transition::spring(SpringConfig::GENTLE)),
        )
        .control()
        .on_hover(move |hovered| out.set(hovered))
        .on_mouse_down(move |_, _| big.set(true))
        .on_mouse_up(move |_, _| big.set(false)),
    )
}

/// A spring that has settled is at rest, so the next leg starts from a stop.
fn from_rest() -> Container {
    let out = create_signal(false);
    panel(
        "4 — A settled spring starts from rest",
        "Hover and wait for the dot to come to a complete stop at the far end, \
         then leave. It should ease away, not start with a kick.",
        track(
            dot(INK)
                .transform(move || Transform::translate(if out.get() { 760.0 } else { 0.0 }, 0.0))
                .animate_transform(Transition::spring(SpringConfig::SNAPPY)),
        )
        .control()
        .on_hover(move |hovered| out.set(hovered)),
    )
}

/// Out and back on two different frames: the return leg overshoots on its own.
fn shake() -> Container {
    let angle = create_signal(0.0_f32);
    panel(
        "5 — Out and back is a wobble",
        "Press and release the tile. A press and its release are two frames, \
         so the return leg crosses zero and comes back — a shake nobody \
         choreographed.",
        container()
            .width(140.0)
            .height(46.0)
            .background(HOT)
            .corners(8.0)
            .padding(10.0)
            .transform(move || Transform::rotate_degrees(angle.get()))
            .animate_transform(Transition::spring(SpringConfig::BOUNCY))
            .on_mouse_down(move |_, _| angle.set(6.0))
            .on_mouse_up(move |_, _| angle.set(0.0))
            .child(
                text("press me")
                    .bold()
                    .bold()
                    .font_size(12.0)
                    .font_size(18.0)
                    .font_size(14.0)
                    .font_size(13.0)
                    .color(Color::rgb(0.1, 0.08, 0.05)),
            ),
    )
}
