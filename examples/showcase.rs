//! Example demonstrating superellipse corner curvature variations.
//!
//! This example showcases different corner styles using CSS K-values:
//! - Squircle (K=2, n=4): iOS-style smooth corners
//! - Circle (K=1, n=2): Standard circular corners (default)
//! - Bevel (K=0, n=1): Diagonal cut corners
//! - Scoop (K=-1, n=0.5): Concave/scooped inward corners

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(500)
                .height(250)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .layout(Flex::column().spacing(12.0))
                    .child(
                        // Row 1: Different curvature values with solid colors
                        container()
                            .layout(Flex::row().spacing(8.0))
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.3, 0.2, 0.4))
                                    .corner_radius(12.0)
                                    .squircle() // K=2 → n=4
                                    .text_color(Color::WHITE)
                                    .child(text("Squircle\n(K=2)")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.2, 0.3, 0.4))
                                    .corner_radius(12.0)
                                    // Default circular K=1 → n=2
                                    .text_color(Color::WHITE)
                                    .child(text("Circle\n(K=1)")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.4, 0.3, 0.2))
                                    .corner_radius(12.0)
                                    .bevel() // K=0 → n=1
                                    .text_color(Color::WHITE)
                                    .child(text("Bevel\n(K=0)")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.2, 0.4, 0.3))
                                    .corner_radius(12.0)
                                    .scoop() // K=-1 → n=0.5
                                    .hover_state(|s| s.lighter(0.1))
                                    .pressed_state(|s| s.ripple())
                                    .text_color(Color::WHITE)
                                    .child(text("Scoop\n(K=-1)")),
                            ),
                    )
                    .child(
                        // Row 2: With borders
                        container()
                            .layout(Flex::row().spacing(8.0))
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.15, 0.15, 0.2))
                                    .corner_radius(12.0)
                                    .border(2.0, Color::rgb(0.5, 0.3, 0.7))
                                    .squircle()
                                    .text_color(Color::WHITE)
                                    .child(text("Squircle\nBorder")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.15, 0.15, 0.2))
                                    .corner_radius(12.0)
                                    .border(2.0, Color::rgb(0.3, 0.5, 0.7))
                                    .text_color(Color::WHITE)
                                    .child(text("Circle\nBorder")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.15, 0.15, 0.2))
                                    .corner_radius(12.0)
                                    .border(2.0, Color::rgb(0.7, 0.5, 0.3))
                                    .bevel()
                                    .text_color(Color::WHITE)
                                    .child(text("Bevel\nBorder")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.15, 0.15, 0.2))
                                    .corner_radius(12.0)
                                    .border(2.0, Color::rgb(0.3, 0.7, 0.5))
                                    .scoop()
                                    .hover_state(|s| s.lighter(0.1))
                                    .pressed_state(|s| s.ripple())
                                    .text_color(Color::WHITE)
                                    .child(text("Scoop\nBorder")),
                            ),
                    )
                    .child(
                        // Row 3: With gradients
                        container()
                            .layout(Flex::row().spacing(8.0))
                            .child(
                                container()
                                    .padding(12.0)
                                    .gradient_horizontal(
                                        Color::rgb(0.4, 0.2, 0.5),
                                        Color::rgb(0.2, 0.4, 0.6),
                                    )
                                    .corner_radius(12.0)
                                    .squircle()
                                    .text_color(Color::WHITE)
                                    .child(text("Squircle\nGradient")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .gradient_horizontal(
                                        Color::rgb(0.2, 0.4, 0.5),
                                        Color::rgb(0.4, 0.2, 0.6),
                                    )
                                    .corner_radius(12.0)
                                    .text_color(Color::WHITE)
                                    .child(text("Circle\nGradient")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .gradient_horizontal(
                                        Color::rgb(0.5, 0.4, 0.2),
                                        Color::rgb(0.6, 0.2, 0.4),
                                    )
                                    .corner_radius(12.0)
                                    .bevel()
                                    .text_color(Color::WHITE)
                                    .child(text("Bevel\nGradient")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .gradient_horizontal(
                                        Color::rgb(0.2, 0.5, 0.4),
                                        Color::rgb(0.4, 0.6, 0.2),
                                    )
                                    .corner_radius(12.0)
                                    .scoop()
                                    .text_color(Color::WHITE)
                                    .child(text("Scoop\nGradient")),
                            ),
                    )
                    .child(
                        // Row 4: Custom curvature values
                        container()
                            .layout(Flex::row().spacing(8.0))
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.3, 0.3, 0.4))
                                    .corner_radius(12.0)
                                    .corner_curvature(0.5) // K=0.5 → n=1.41
                                    .text_color(Color::WHITE)
                                    .child(text("K=0.5")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.3, 0.4, 0.3))
                                    .corner_radius(12.0)
                                    .corner_curvature(1.5) // K=1.5 → n=2.83
                                    .text_color(Color::WHITE)
                                    .child(text("K=1.5")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.4, 0.3, 0.3))
                                    .corner_radius(12.0)
                                    .corner_curvature(2.5) // K=2.5 → n=5.66
                                    .text_color(Color::WHITE)
                                    .child(text("K=2.5")),
                            )
                            .child(
                                container()
                                    .padding(12.0)
                                    .background(Color::rgb(0.35, 0.3, 0.4))
                                    .corner_radius(12.0)
                                    .corner_curvature(-0.5) // K=-0.5 → n=0.707
                                    .text_color(Color::WHITE)
                                    .child(text("K=-0.5")),
                            ),
                    )
            },
        );
    });
}
