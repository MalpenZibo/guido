//! Test scale transform

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(150)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.15, 0.15, 0.2)),
            || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(40.0)
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .padding(16.0)
                    .children([
                        // Normal box
                        container()
                            .width(60.0)
                            .height(60.0)
                            .padding(10.)
                            .background(Color::rgb(0.8, 0.3, 0.3))
                            .corner_radius(8.0),
                        // Scaled up box (should be bigger)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .padding(10.0)
                            .background(Color::rgb(0.3, 0.8, 0.3))
                            .corner_radius(8.0)
                            .scale(1.5),
                        // Scaled down box (should be smaller)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .padding(10.0)
                            .background(Color::rgb(0.3, 0.3, 0.8))
                            .corner_radius(8.0)
                            .scale(0.5),
                    ])
            },
        );
    });
}
