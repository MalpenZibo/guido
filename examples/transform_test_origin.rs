//! Test: Transform origins for rotation
//!
//! Run with: cargo run --example transform_test_origin

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
                            .spacing(50.0)
                            .main_alignment(MainAlignment::Center),
                    )
                    .padding(30.0)
                    .children([
                        // CENTER (default)
                        box_with_origin(Color::rgb(0.8, 0.3, 0.3), Pivot::CENTER),
                        // TOP_LEFT
                        box_with_origin(Color::rgb(0.3, 0.8, 0.3), Pivot::TOP_LEFT),
                        // TOP_RIGHT
                        box_with_origin(Color::rgb(0.3, 0.3, 0.8), Pivot::TOP_RIGHT),
                        // BOTTOM_LEFT
                        box_with_origin(Color::rgb(0.8, 0.8, 0.3), Pivot::BOTTOM_LEFT),
                        // BOTTOM_RIGHT
                        box_with_origin(Color::rgb(0.8, 0.3, 0.8), Pivot::BOTTOM_RIGHT),
                    ])
            },
        );
    });
}

fn box_with_origin(color: Color, origin: Pivot) -> Container {
    container()
        .width(60.0)
        .height(60.0)
        .background(color)
        .corners(8.0)
        .rotate(30.0)
        .pivot(origin)
}
