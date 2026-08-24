//! What each animatable property looks like while it moves.
//!
//! One card per property, each with room to travel: the animated element is a
//! bare bar or swatch with nothing inside it, so its size is exactly the value
//! being animated rather than whatever its text needs. Springs overshoot, and
//! every target here leaves headroom for the overshoot to be seen — a value
//! animated into a constraint it is already touching just stops.

use guido::prelude::*;

const BG: Color = Color::rgb(0.08, 0.08, 0.12);
const CARD: Color = Color::rgb(0.15, 0.15, 0.21);
const TRACK: Color = Color::rgb(0.11, 0.11, 0.16);
const INK: Color = Color::rgb(0.35, 0.62, 0.95);
const HOT: Color = Color::rgb(0.95, 0.55, 0.25);
const LABEL: Color = Color::rgb(0.90, 0.90, 0.95);
const MUTED: Color = Color::rgb(0.60, 0.62, 0.72);

/// Wide enough that a spring's overshoot lands inside the track instead of
/// against its edge.
const NARROW: f32 = 60.0;
const WIDE: f32 = 620.0;

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
                    .padding(16.0)
                    .layout(Flex::column().spacing(10.0))
                    .children([
                        text("Animated properties — click a card to drive it")
                            .color(Color::WHITE)
                            .into_any(),
                        width_card().into_any(),
                        colour_card().into_any(),
                        corner_and_size_card().into_any(),
                        border_card().into_any(),
                    ])
            },
        );
    });
}

fn card(title: &str, hint: impl Into<String>, body: Container) -> Container {
    container()
        .background(CARD)
        .corners(12.0)
        .padding(12.0)
        .layout(Flex::column().spacing(7.0))
        .when_hovered(|s| s.lighter(0.04))
        .animate_background(Transition::new(150.0, TimingFunction::EaseOut))
        .children([
            text(title.to_string()).color(LABEL).into_any(),
            body.into_any(),
            text(hint.into())
                .bold()
                .bold()
                .font_size(14.0)
                .font_size(16.0)
                .font_size(12.0)
                .color(MUTED)
                .into_any(),
        ])
}

/// A dark strip for something to travel along.
fn track(height: f32, body: Container) -> Container {
    container()
        .width(fill())
        .height(height)
        .background(TRACK)
        .corners(8.0)
        .padding(5.0)
        .child(body)
}

/// Width, on a spring. The bar carries nothing, so its width *is* the animated
/// value — with a label inside, the text would set a floor and the narrow end
/// would never be narrow.
fn width_card() -> Container {
    let wide = create_signal(false);

    card(
        "Width — spring",
        "Click. The bar carries nothing, so it really reaches 60px — and it \
         passes 620 before settling on it.",
        track(
            36.0,
            container()
                .width(move || if wide.get() { WIDE } else { NARROW })
                .animate_width(Transition::spring(SpringConfig::DEFAULT))
                .height(fill())
                .background(INK)
                .corners(6.0),
        )
        .on_click(move || wide.update(|w| *w = !*w)),
    )
}

/// The same colour change under a spring and under a curve, side by side —
/// which is the only way to see what the spring is actually doing to it.
fn colour_card() -> Container {
    let swatch = |transition: Transition| {
        container()
            .width(280.0)
            .height(48.0)
            .corners(8.0)
            .background(Color::rgb(0.22, 0.20, 0.30))
            .animate_background(transition)
            .when_hovered(|s| s.background(HOT))
    };

    card(
        "Background colour — spring against a curve",
        "Hover both. Left is a bouncy spring — it arrives past the colour and \
         comes back; right is a 220ms curve, which stops on it.",
        container().layout(Flex::row().spacing(16.0)).children([
            swatch(Transition::spring(SpringConfig::BOUNCY)),
            swatch(Transition::new(220.0, TimingFunction::EaseOut)),
        ]),
    )
}

/// Several properties moving at once, each on its own transition.
fn corner_and_size_card() -> Container {
    let open = create_signal(false);

    card(
        "Width, corner radius and colour together",
        "Click. Three properties, three transitions — the corner is on a curve \
         and the width on a spring, so they do not arrive together.",
        track(
            56.0,
            container()
                .width(move || if open.get() { 520.0 } else { 120.0 })
                .animate_width(Transition::spring(SpringConfig::SNAPPY))
                .height(fill())
                .background(move || {
                    if open.get() {
                        Color::rgb(0.25, 0.55, 0.40)
                    } else {
                        Color::rgb(0.25, 0.30, 0.45)
                    }
                })
                .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
                .corners(move || if open.get() { 30.0 } else { 6.0 })
                .animate_corners(Transition::new(250.0, TimingFunction::EaseInOut)),
        )
        .on_click(move || open.update(|o| *o = !*o)),
    )
}

/// Border width on a bouncy spring, where the overshoot is the whole effect.
fn border_card() -> Container {
    let thick = create_signal(false);

    card(
        "Border width — bouncy spring",
        "Click. 2px to 14px on a bouncy spring wobbles before it settles; \
         hovering changes the colour on a curve underneath.",
        container()
            .width(fill())
            .height(56.0)
            .background(Color::rgb(0.13, 0.13, 0.18))
            .corners(10.0)
            .border(
                move || if thick.get() { 14.0 } else { 2.0 },
                Color::rgb(0.40, 0.50, 0.70),
            )
            .animate_border_width(Transition::spring(SpringConfig::BOUNCY))
            .animate_border_color(Transition::new(300.0, TimingFunction::EaseOut))
            // A border is declared as a pair, so the hover restates the width —
            // and restates it as the same signal, or hovering would pin it and
            // the click would have nothing left to spring. Each half of a state
            // layer's border takes a signal of its own, which is what makes that
            // possible.
            .when_hovered(|s| {
                s.border(
                    move || if thick.get() { 14.0 } else { 2.0 },
                    Color::rgb(0.40, 0.80, 0.60),
                )
            })
            .on_click(move || thick.update(|t| *t = !*t)),
    )
}
