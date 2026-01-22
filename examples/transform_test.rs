//! Simple test for transform rotation with shadow

use guido::prelude::*;

fn main() {
    // A single large box with obvious rotation and shadow
    let view = container()
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
        );

    App::new()
        .height(300)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
