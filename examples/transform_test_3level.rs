//! Test: 3-level nested transforms
//!
//! Run with: cargo run --example transform_test_3level --features renderer_v2

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(180)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.12, 0.12, 0.16)),
        move || {
            container()
                .layout(Flex::row().spacing(60.0).main_axis_alignment(MainAxisAlignment::Center))
                .padding(30.0)
                .children([
                    // Reference: no rotation at any level
                    three_level(0.0, 0.0, 0.0),
                    // Only grandparent rotates 20°
                    three_level(20.0, 0.0, 0.0),
                    // Each level rotates 10° (total 30°)
                    three_level(10.0, 10.0, 10.0),
                    // Grandparent 30°, others 0°
                    three_level(30.0, 0.0, 0.0),
                ])
        },
    );
    app.run();
}

fn three_level(gp_rot: f32, p_rot: f32, c_rot: f32) -> Container {
    container()
        .width(120.0)
        .height(120.0)
        .background(Color::rgb(0.25, 0.25, 0.35))
        .corner_radius(12.0)
        .rotate(gp_rot)
        .padding(15.0)
        .child(
            container()
                .width(90.0)
                .height(90.0)
                .background(Color::rgb(0.45, 0.45, 0.55))
                .corner_radius(10.0)
                .rotate(p_rot)
                .padding(12.0)
                .child(
                    container()
                        .width(50.0)
                        .height(50.0)
                        .background(Color::rgb(0.9, 0.6, 0.3))
                        .corner_radius(6.0)
                        .rotate(c_rot),
                ),
        )
}
