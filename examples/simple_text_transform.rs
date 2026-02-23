//! Simple example to test text transform rendering.

use guido::prelude::*;

fn main() {
    let view = container()
        .layout(
            Flex::row()
                .spacing(50.0)
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .padding(50.0)
        .children([
            // No transform - should use glyphon directly
            container()
                .width(120.0)
                .height(60.0)
                .background(Color::rgba(0.3, 0.6, 0.3, 0.8))
                .corner_radius(8.0)
                .layout(
                    Flex::column()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .child(text("No Transform").font_size(14.0).color(Color::WHITE)),
            // With rotation - should use texture
            container()
                .width(120.0)
                .height(60.0)
                .background(Color::rgba(0.3, 0.3, 0.8, 0.8))
                .corner_radius(8.0)
                .layout(
                    Flex::column()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .rotate(15.0)
                .child(text("Rotated 15").font_size(14.0).color(Color::WHITE)),
            // With scale - should use texture
            container()
                .width(120.0)
                .height(60.0)
                .background(Color::rgba(0.8, 0.5, 0.3, 0.8))
                .corner_radius(8.0)
                .layout(
                    Flex::column()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .scale(1.2)
                .child(text("Scale 1.2").font_size(14.0).color(Color::WHITE)),
        ]);

    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(600)
                .height(200)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.15, 0.15, 0.2)),
            move || view,
        );
    });
}
