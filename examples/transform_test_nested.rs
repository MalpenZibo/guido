//! Test: Nested transforms (parent rotation affects child)
//!
//! Run with: cargo run --example transform_test_nested

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(160)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.12, 0.12, 0.16)),
            move || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(40.0)
                            .main_alignment(MainAlignment::Center),
                    )
                    .padding(20.0)
                    .children([
                        // Reference: no rotation
                        nested(0.0, Color::rgb(0.9, 0.5, 0.3)),
                        // Parent rotated 15°
                        nested(15.0, Color::rgb(0.3, 0.9, 0.5)),
                        // Parent rotated 30°
                        nested(30.0, Color::rgb(0.5, 0.3, 0.9)),
                        // Parent rotated 45°
                        nested(45.0, Color::rgb(0.9, 0.9, 0.3)),
                    ])
            },
        );
    });
}

fn nested(parent_rotation: f32, child_color: Color) -> Container {
    container()
        .width(100.0)
        .height(100.0)
        .background(Color::rgb(0.3, 0.3, 0.4))
        .corner_radius(10.0)
        .rotate(parent_rotation)
        .padding(20.0)
        .child(
            container()
                .width(60.0)
                .height(60.0)
                .background(child_color)
                .corner_radius(6.0),
        )
}
