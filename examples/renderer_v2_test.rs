//! Test example for the hierarchical render tree system.
//!
//! This example tests the renderer with basic shapes and transforms.
//! Run with: cargo run --example renderer_v2_test

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(140)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(10.0)
                            .main_alignment(MainAlignment::Center),
                    )
                    .padding(16.0)
                    .children([
                        // Simple colored box
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.2, 0.2))
                            .corners(8.0),
                        // Box with border
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.2, 0.8, 0.2))
                            .corners(8.0)
                            .border(2.0, Color::WHITE),
                        // Rotated box
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.2, 0.2, 0.8))
                            .corners(8.0)
                            .transform(Transform::rotate_degrees(15.0)),
                        // Scaled box
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.8, 0.2))
                            .corners(8.0)
                            .transform(Transform::scale(0.8)),
                        // Box with squircle corners
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.2, 0.8))
                            .corners(Corners::squircle(12.0)),
                        // Squircle with border
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.6, 0.3, 0.7))
                            .corners(Corners::squircle(12.0))
                            .border(2.0, Color::WHITE),
                        // Scoop corners (concave)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.9, 0.5, 0.2))
                            .corners(Corners::scoop(16.0)),
                        // Scoop with border
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.7, 0.4, 0.1))
                            .corners(Corners::scoop(16.0))
                            .border(2.0, Color::WHITE),
                        // Box with shadow (elevation)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.2, 0.8, 0.8))
                            .corners(8.0)
                            .elevation(4.0),
                        // Clickable box with ripple
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.5, 0.5, 0.5))
                            .corners(8.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(|| {
                                println!("Clicked!");
                            }),
                        // Nested containers
                        container()
                            .width(70.0)
                            .height(60.0)
                            .background(Color::rgb(0.3, 0.3, 0.4))
                            .corners(8.0)
                            .padding(8.0)
                            .child(
                                container()
                                    .background(Color::rgb(0.6, 0.4, 0.2))
                                    .corners(4.0),
                            ),
                    ])
            },
        );
    });
}
