//! Example demonstrating elevation with shadow casting.
//!
//! This example showcases different elevation levels that cast shadows:
//! - Level 0: No shadow
//! - Level 1-5: Progressively larger and softer shadows
//!
//! Higher elevation values create the illusion of depth by casting
//! shadows below the container.

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(600)
            .height(280)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.85, 0.85, 0.9)),
        || {
            container()
                .layout(Flex::column().spacing(16.0))
                .child(
                    // Row 1: Basic elevation levels
                    container()
                        .layout(Flex::row().spacing(16.0))
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(0.0)
                                .child(
                                    text("Level 0\n(No shadow)").color(Color::rgb(0.2, 0.2, 0.2)),
                                ),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(1.0)
                                .child(text("Level 1").color(Color::rgb(0.2, 0.2, 0.2))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(2.0)
                                .child(text("Level 2").color(Color::rgb(0.2, 0.2, 0.2))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(3.0)
                                .child(text("Level 3").color(Color::rgb(0.2, 0.2, 0.2))),
                        ),
                )
                .child(
                    // Row 2: Higher elevation levels
                    container()
                        .layout(Flex::row().spacing(16.0))
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(4.0)
                                .child(text("Level 4").color(Color::rgb(0.2, 0.2, 0.2))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(5.0)
                                .child(text("Level 5").color(Color::rgb(0.2, 0.2, 0.2))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(7.0)
                                .child(text("Level 7").color(Color::rgb(0.2, 0.2, 0.2))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::WHITE)
                                .corner_radius(8.0)
                                .elevation(10.0)
                                .child(text("Level 10").color(Color::rgb(0.2, 0.2, 0.2))),
                        ),
                )
                .child(
                    // Row 3: Colored cards with elevation
                    container()
                        .layout(Flex::row().spacing(16.0))
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.9, 0.7, 0.7))
                                .corner_radius(12.0)
                                .elevation(2.0)
                                .child(text("Card 1").color(Color::rgb(0.3, 0.1, 0.1))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.7, 0.9, 0.7))
                                .corner_radius(12.0)
                                .elevation(3.0)
                                .child(text("Card 2").color(Color::rgb(0.1, 0.3, 0.1))),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.7, 0.7, 0.9))
                                .corner_radius(12.0)
                                .elevation(4.0)
                                .child(text("Card 3").color(Color::rgb(0.1, 0.1, 0.3))),
                        ),
                )
                .child(
                    // Row 4: Squircle with elevation
                    container()
                        .layout(Flex::row().spacing(16.0))
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.95, 0.95, 0.95))
                                .corner_radius(16.0)
                                .squircle()
                                .elevation(2.0)
                                .child(
                                    text("Squircle\nElevation 2").color(Color::rgb(0.2, 0.2, 0.2)),
                                ),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.95, 0.95, 0.95))
                                .corner_radius(16.0)
                                .squircle()
                                .elevation(4.0)
                                .child(
                                    text("Squircle\nElevation 4").color(Color::rgb(0.2, 0.2, 0.2)),
                                ),
                        )
                        .child(
                            container()
                                .padding(16.0)
                                .background(Color::rgb(0.95, 0.95, 0.95))
                                .corner_radius(16.0)
                                .squircle()
                                .elevation(6.0)
                                .child(
                                    text("Squircle\nElevation 6").color(Color::rgb(0.2, 0.2, 0.2)),
                                ),
                        ),
                )
        },
    );
    app.run();
}
