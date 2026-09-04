//! Every degree of freedom a `Shadow` has, and where a ladder of them lives.
//!
//! guido ships no elevation level and no table behind one. A shadow is four
//! numbers — offset, blur, spread and colour — and a design system's steps are
//! `Shadow` constants the application writes down, the way it writes down its
//! colours. Row 1 is one such ladder; the rows below it are what a single
//! number could never say.

use guido::prelude::*;

/// A ladder, defined where a ladder belongs: in the application.
///
/// `Shadow::new` is `const`, so these cost nothing and can be shared from a
/// theme module.
mod elevation {
    use guido::prelude::{Color, Shadow};

    const fn step(offset_y: f32, blur: f32, alpha: f32) -> Shadow {
        Shadow::new(
            (0.0, offset_y),
            blur,
            0.0,
            Color::rgba(0.0, 0.0, 0.0, alpha),
        )
    }

    pub const FLAT: Shadow = Shadow::none();
    pub const LOW: Shadow = step(1.0, 3.0, 0.12);
    pub const RAISED: Shadow = step(3.0, 6.0, 0.19);
    pub const HIGH: Shadow = step(6.0, 10.0, 0.22);
}

fn card<M>(label: &str, shadow: impl IntoAnimated<Shadow, M>) -> Container {
    container()
        .padding(28.0)
        .background(Color::WHITE)
        .corners(8.0)
        .shadow(shadow)
        .child(text(label.to_string()).color(Color::rgb(0.2, 0.2, 0.2)))
}

fn row() -> Container {
    container().layout(Flex::row().spacing(48.0))
}

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(1200)
                .height(700)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.85, 0.85, 0.9)),
            || {
                container()
                    .padding(48.0)
                    .layout(Flex::column().spacing(48.0))
                    // A ladder the application owns.
                    .child(
                        row()
                            .child(card("Flat", elevation::FLAT))
                            .child(card("Low", elevation::LOW))
                            .child(card("Raised", elevation::RAISED))
                            .child(card("High", elevation::HIGH)),
                    )
                    // The three the old elevation level could not reach: it
                    // always wrote `(0.0, offset_y)`, a spread of `0.0`, and
                    // black at the alpha its table chose.
                    .child(
                        row()
                            .child(card(
                                "Sideways",
                                Shadow::new(
                                    (18.0, 0.0),
                                    10.0,
                                    0.0,
                                    Color::rgba(0.0, 0.0, 0.0, 0.35),
                                ),
                            ))
                            .child(card(
                                "Coloured",
                                Shadow::new(
                                    (0.0, 8.0),
                                    14.0,
                                    0.0,
                                    Color::rgba(0.85, 0.1, 0.2, 0.55),
                                ),
                            ))
                            .child(card(
                                "Spread, no offset",
                                Shadow::new(
                                    (0.0, 0.0),
                                    8.0,
                                    10.0,
                                    Color::rgba(0.1, 0.2, 0.8, 0.45),
                                ),
                            )),
                    )
                    // The motion rides with the declaration, so a hover lifts
                    // the card rather than snapping it.
                    .child(
                        row()
                            .child(
                                card("Hover me", elevation::LOW.transition(160.0))
                                    .when_hovered(|s| s.shadow(elevation::HIGH)),
                            )
                            .child(
                                card(
                                    "Hover me too",
                                    elevation::FLAT.transition(SpringConfig::BOUNCY),
                                )
                                .when_hovered(|s| {
                                    s.shadow(Shadow::new(
                                        (0.0, 10.0),
                                        20.0,
                                        2.0,
                                        Color::rgba(0.1, 0.1, 0.4, 0.4),
                                    ))
                                }),
                            ),
                    )
            },
        );
    });
}
