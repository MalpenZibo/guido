//! Test: Basic rotation transforms
//!
//! Run with: cargo run --example transform_test_rotation

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(120)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.12, 0.12, 0.16)),
            move || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(30.0)
                            .main_axis_alignment(MainAxisAlignment::Center),
                    )
                    .padding(20.0)
                    .children([
                        // 0° (reference)
                        box_60(Color::rgb(0.8, 0.3, 0.3)),
                        // 15°
                        box_60(Color::rgb(0.3, 0.8, 0.3)).rotate(15.0),
                        // 30°
                        box_60(Color::rgb(0.3, 0.3, 0.8)).rotate(30.0),
                        // 45°
                        box_60(Color::rgb(0.8, 0.8, 0.3)).rotate(45.0),
                        // -30°
                        box_60(Color::rgb(0.8, 0.3, 0.8)).rotate(-30.0),
                    ])
            },
        );
    });
}

fn box_60(color: Color) -> Container {
    container()
        .width(60.0)
        .height(60.0)
        .background(color)
        .corner_radius(8.0)
}
