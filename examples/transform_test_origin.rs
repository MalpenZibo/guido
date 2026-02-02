//! Test: Transform origins for rotation
//!
//! Run with: cargo run --example transform_test_origin

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(160)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.12, 0.12, 0.16)),
        move |_| {
            container()
                .layout(
                    Flex::row()
                        .spacing(50.0)
                        .main_axis_alignment(MainAxisAlignment::Center),
                )
                .padding(30.0)
                .children([
                    // CENTER (default)
                    box_with_origin(Color::rgb(0.8, 0.3, 0.3), TransformOrigin::CENTER),
                    // TOP_LEFT
                    box_with_origin(Color::rgb(0.3, 0.8, 0.3), TransformOrigin::TOP_LEFT),
                    // TOP_RIGHT
                    box_with_origin(Color::rgb(0.3, 0.3, 0.8), TransformOrigin::TOP_RIGHT),
                    // BOTTOM_LEFT
                    box_with_origin(Color::rgb(0.8, 0.8, 0.3), TransformOrigin::BOTTOM_LEFT),
                    // BOTTOM_RIGHT
                    box_with_origin(Color::rgb(0.8, 0.3, 0.8), TransformOrigin::BOTTOM_RIGHT),
                ])
        },
    );
    app.run();
}

fn box_with_origin(color: Color, origin: TransformOrigin) -> Container {
    container()
        .width(60.0)
        .height(60.0)
        .background(color)
        .corner_radius(8.0)
        .rotate(30.0)
        .transform_origin(origin)
}
