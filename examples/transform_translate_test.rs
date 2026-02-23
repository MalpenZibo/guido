//! Test translation transform

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(200)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(20.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .padding(16.0)
                    .children([
                        // No transform
                        container()
                            .width(80.0)
                            .height(80.0)
                            .padding(10.0)
                            .background(Color::rgb(0.8, 0.3, 0.3))
                            .corner_radius(8.0),
                        // With translation - should move right and down
                        container()
                            .width(80.0)
                            .height(80.0)
                            .padding(10.0)
                            .background(Color::rgb(0.3, 0.8, 0.3))
                            .corner_radius(8.0)
                            .translate(100.0, 10.0), // Move 10px right, 10px down
                    ])
            },
        );
    });
}
