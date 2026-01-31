//! Test example for the hierarchical render tree system.
//!
//! This example tests the renderer with basic shapes and transforms.
//! Run with: cargo run --example renderer_v2_test

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(140)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .layout(
                    Flex::row()
                        .spacing(10.0)
                        .main_axis_alignment(MainAxisAlignment::Center),
                )
                .padding(16.0)
                .children([
                    // Simple colored box
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.8, 0.2, 0.2))
                        .corner_radius(8.0),
                    // Box with border
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.2, 0.8, 0.2))
                        .corner_radius(8.0)
                        .border(2.0, Color::WHITE),
                    // Rotated box
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.2, 0.2, 0.8))
                        .corner_radius(8.0)
                        .rotate(15.0),
                    // Scaled box
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.8, 0.8, 0.2))
                        .corner_radius(8.0)
                        .scale(0.8),
                    // Box with squircle corners
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.8, 0.2, 0.8))
                        .corner_radius(12.0)
                        .squircle(),
                    // Squircle with border
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.6, 0.3, 0.7))
                        .corner_radius(12.0)
                        .squircle()
                        .border(2.0, Color::WHITE),
                    // Scoop corners (concave)
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.9, 0.5, 0.2))
                        .corner_radius(16.0)
                        .scoop(),
                    // Scoop with border
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.7, 0.4, 0.1))
                        .corner_radius(16.0)
                        .scoop()
                        .border(2.0, Color::WHITE),
                    // Box with shadow (elevation)
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.2, 0.8, 0.8))
                        .corner_radius(8.0)
                        .elevation(4.0),
                    // Clickable box with ripple
                    container()
                        .width(60.0)
                        .height(60.0)
                        .background(Color::rgb(0.5, 0.5, 0.5))
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(|| {
                            println!("Clicked!");
                        }),
                    // Nested containers
                    container()
                        .width(70.0)
                        .height(60.0)
                        .background(Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(8.0)
                        .padding(8.0)
                        .child(
                            container()
                                .background(Color::rgb(0.6, 0.4, 0.2))
                                .corner_radius(4.0),
                        ),
                ])
        },
    );

    app.run();
}
