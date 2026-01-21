//! Example demonstrating superellipse corner curvature variations.
//!
//! This example showcases different corner styles using CSS K-values:
//! - Squircle (K=2, n=4): iOS-style smooth corners
//! - Circle (K=1, n=2): Standard circular corners (default)
//! - Bevel (K=0, n=1): Diagonal cut corners
//! - Scoop (K=-1, n=0.5): Concave/scooped inward corners

use guido::prelude::*;

fn main() {
    let hover_color = create_signal(Color::rgb(0.2, 0.2, 0.3));
    let hover_for_callback2 = hover_color.clone();
    let hover_for_callback = hover_color.clone();

    let view = container()
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
                        .child(text("Squircle\n(K=2)").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.2, 0.3, 0.4))
                        .corner_radius(12.0)
                        // Default circular K=1 → n=2
                        .child(text("Circle\n(K=1)").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.4, 0.3, 0.2))
                        .corner_radius(12.0)
                        .bevel() // K=0 → n=1
                        .child(text("Bevel\n(K=0)").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.2, 0.4, 0.3))
                        .corner_radius(12.0)
                        .scoop() // K=-1 → n=0.5
                        .ripple()
                        .on_hover(move |hovered| {
                            if hovered {
                                hover_for_callback2.set(Color::rgb(0.3, 0.3, 0.4));
                            } else {
                                hover_for_callback2.set(Color::rgb(0.2, 0.2, 0.3));
                            }
                        })
                        .child(text("Scoop\n(K=-1)").color(Color::WHITE))
                )
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
                        .child(text("Squircle\nBorder").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(12.0)
                        .border(2.0, Color::rgb(0.3, 0.5, 0.7))
                        .child(text("Circle\nBorder").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(12.0)
                        .border(2.0, Color::rgb(0.7, 0.5, 0.3))
                        .bevel()
                        .child(text("Bevel\nBorder").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(12.0)
                        .border(2.0, Color::rgb(0.3, 0.7, 0.5))
                        .scoop()
                        .ripple()
                        .on_hover(move |hovered| {
                            if hovered {
                                hover_for_callback.set(Color::rgb(0.3, 0.3, 0.4));
                            } else {
                                hover_for_callback.set(Color::rgb(0.2, 0.2, 0.3));
                            }
                        })
                        .child(text("Scoop\nBorder").color(Color::WHITE))
                )
        )
        .child(
            // Row 3: With gradients
            container()
                .layout(Flex::row().spacing(8.0))
                .child(
                    container()
                        .padding(12.0)
                        .gradient_horizontal(Color::rgb(0.4, 0.2, 0.5), Color::rgb(0.2, 0.4, 0.6))
                        .corner_radius(12.0)
                        .squircle()
                        .child(text("Squircle\nGradient").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .gradient_horizontal(Color::rgb(0.2, 0.4, 0.5), Color::rgb(0.4, 0.2, 0.6))
                        .corner_radius(12.0)
                        .child(text("Circle\nGradient").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .gradient_horizontal(Color::rgb(0.5, 0.4, 0.2), Color::rgb(0.6, 0.2, 0.4))
                        .corner_radius(12.0)
                        .bevel()
                        .child(text("Bevel\nGradient").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .gradient_horizontal(Color::rgb(0.2, 0.5, 0.4), Color::rgb(0.4, 0.6, 0.2))
                        .corner_radius(12.0)
                        .scoop()
                        .child(text("Scoop\nGradient").color(Color::WHITE))
                )
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
                        .child(text("K=0.5").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.3, 0.4, 0.3))
                        .corner_radius(12.0)
                        .corner_curvature(1.5) // K=1.5 → n=2.83
                        .child(text("K=1.5").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.4, 0.3, 0.3))
                        .corner_radius(12.0)
                        .corner_curvature(2.5) // K=2.5 → n=5.66
                        .child(text("K=2.5").color(Color::WHITE))
                )
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.35, 0.3, 0.4))
                        .corner_radius(12.0)
                        .corner_curvature(-0.5) // K=-0.5 → n=0.707
                        .child(text("K=-0.5").color(Color::WHITE))
                )
        );

    // Run the app
    App::new()
        .width(500)
        .height(250)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
