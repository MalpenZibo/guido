//! Test: Basic scale transforms
//!
//! Run with: cargo run --example transform_test_scale --features renderer_v2

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(140)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.12, 0.12, 0.16)),
        move || {
            container()
                .layout(
                    Flex::row()
                        .spacing(40.0)
                        .main_axis_alignment(MainAxisAlignment::Center),
                )
                .padding(20.0)
                .children([
                    // 1.0 (reference)
                    box_60(Color::rgb(0.8, 0.3, 0.3)),
                    // 0.5
                    box_60(Color::rgb(0.3, 0.8, 0.3)).scale(0.5),
                    // 0.8
                    box_60(Color::rgb(0.3, 0.3, 0.8)).scale(0.8),
                    // 1.2
                    box_60(Color::rgb(0.8, 0.8, 0.3)).scale(1.2),
                    // Non-uniform: 1.3 x 0.6
                    box_60(Color::rgb(0.8, 0.3, 0.8)).scale_xy(1.3, 0.6),
                ])
        },
    );
    app.run();
}

fn box_60(color: Color) -> Container {
    container()
        .width(60.0)
        .height(60.0)
        .background(color)
        .corner_radius(8.0)
}
