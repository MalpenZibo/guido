//! Simple test for transform rotation with shadow

use guido::prelude::*;

fn main() {
    App::new()
        .add_surface(
            SurfaceConfig::new()
                .height(300)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .layout(
                        Flex::column()
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .padding(16.0)
                    .child(
                        container()
                            .width(200.0)
                            .height(100.0)
                            .padding(10.0)
                            .background(Color::rgb(0.8, 0.3, 0.3))
                            .corner_radius(8.0)
                            .elevation(4.0) // Add shadow to test shadow rendering with rotation
                            .rotate(45.0), // Should be clearly rotated
                    )
            },
        )
        .run();
}
