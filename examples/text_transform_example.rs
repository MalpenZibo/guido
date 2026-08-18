//! Example demonstrating text transform support.
//!
//! This example shows text following parent container transforms:
//! - Text with rotation
//! - Text with scale
//! - Text with translation
//! - Combined transformations (rotation + scale + translation)
//! - Custom transform origin
//! - Nested transforms with text
//! - Animated text rotation

use guido::prelude::*;
use std::f32::consts::PI;
use std::time::Duration;

#[tokio::main]
async fn main() {
    App::new().run(|app| {
        // Animated rotation angle
        let angle = create_signal(0.0_f32);

        // Animation service - updates the angle signal continuously
        let start_time = std::time::Instant::now();
        let angle_w = angle.writer();
        create_task(move |ctx| async move {
            while ctx.is_running() {
                let elapsed = start_time.elapsed().as_secs_f32();
                let new_angle = (elapsed * PI / 2.0).to_degrees() % 360.0;
                angle_w.set(new_angle);
                tokio::time::sleep(Duration::from_millis(16)).await;
            }
        });

        let view = container()
            .layout(
                Flex::column()
                    .spacing(20.0)
                    .main_alignment(MainAlignment::Center)
                    .cross_alignment(CrossAlignment::Center),
            )
            .padding(30.0)
            .children([
                // Title
                container()
                    .font_size(24.0)
                    .text_color(Color::WHITE)
                    .child(text("Text Transform Demo")),
                // Row 1: Basic transforms (rotation, scale, translation)
                container()
                    .layout(
                        Flex::row()
                            .spacing(30.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .children([
                        // Rotation
                        container()
                            .width(110.0)
                            .height(70.0)
                            .background(Color::rgba(0.3, 0.5, 0.8, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(15.0)
                            .font_size(13.0)
                            .text_color(Color::WHITE)
                            .child(text("Rotate 15°")),
                        // Scale
                        container()
                            .width(110.0)
                            .height(70.0)
                            .background(Color::rgba(0.8, 0.5, 0.3, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .scale(1.2)
                            .font_size(13.0)
                            .text_color(Color::WHITE)
                            .child(text("Scale 1.2x")),
                        // Translation
                        container()
                            .width(110.0)
                            .height(70.0)
                            .background(Color::rgba(0.5, 0.8, 0.3, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .translate(10.0, -10.0)
                            .font_size(13.0)
                            .text_color(Color::WHITE)
                            .child(text("Translate")),
                        // Rotation + Scale
                        container()
                            .width(110.0)
                            .height(70.0)
                            .background(Color::rgba(0.8, 0.3, 0.8, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(-20.0)
                            .scale(0.9)
                            .font_size(13.0)
                            .text_color(Color::WHITE)
                            .child(text("Rot + Scale")),
                    ]),
                // Row 2: Combined transforms and custom origin
                container()
                    .layout(
                        Flex::row()
                            .spacing(30.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .children([
                        // All three: rotation + scale + translation
                        container()
                            .width(130.0)
                            .height(80.0)
                            .background(Color::rgba(0.3, 0.7, 0.7, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(10.0)
                            .scale(1.1)
                            .translate(5.0, 5.0)
                            .font_size(13.0)
                            .text_color(Color::WHITE)
                            .child(text("All Combined")),
                        // Custom origin: top-left
                        container()
                            .width(130.0)
                            .height(80.0)
                            .background(Color::rgba(0.7, 0.5, 0.2, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .transform_origin(TransformOrigin::TOP_LEFT)
                            .rotate(15.0)
                            .font_size(12.0)
                            .text_color(Color::WHITE)
                            .child(text("Origin: Top-Left")),
                        // Custom origin: bottom-right
                        container()
                            .width(130.0)
                            .height(80.0)
                            .background(Color::rgba(0.2, 0.5, 0.7, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .transform_origin(TransformOrigin::BOTTOM_RIGHT)
                            .rotate(15.0)
                            .font_size(12.0)
                            .text_color(Color::WHITE)
                            .child(text("Origin: Bot-Right")),
                        // Custom origin with scale
                        container()
                            .width(130.0)
                            .height(80.0)
                            .background(Color::rgba(0.7, 0.3, 0.5, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .transform_origin(TransformOrigin::TOP_RIGHT)
                            .scale(1.15)
                            .rotate(-10.0)
                            .font_size(12.0)
                            .text_color(Color::WHITE)
                            .child(text("Origin + Scale")),
                    ]),
                // Row 3: Nested transforms
                container()
                    .layout(
                        Flex::row()
                            .spacing(30.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .children([
                        // Nested: parent rotated, child has text
                        container()
                            .width(130.0)
                            .height(90.0)
                            .background(Color::rgba(0.6, 0.3, 0.6, 0.5))
                            .corner_radius(12.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(20.0)
                            .child(
                                container()
                                    .width(90.0)
                                    .height(50.0)
                                    .background(Color::rgba(0.8, 0.6, 0.8, 0.9))
                                    .corner_radius(6.0)
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .font_size(14.0)
                                    .text_color(Color::WHITE)
                                    .child(text("Nested")),
                            ),
                        // Double nested with additional rotation
                        container()
                            .width(130.0)
                            .height(90.0)
                            .background(Color::rgba(0.3, 0.6, 0.6, 0.5))
                            .corner_radius(12.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(15.0)
                            .child(
                                container()
                                    .width(90.0)
                                    .height(50.0)
                                    .background(Color::rgba(0.5, 0.8, 0.8, 0.9))
                                    .corner_radius(6.0)
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .rotate(15.0)
                                    .font_size(13.0)
                                    .text_color(Color::rgb(0.1, 0.1, 0.1))
                                    .child(text("30° Total")),
                            ),
                        // Nested with scale + translation
                        container()
                            .width(130.0)
                            .height(90.0)
                            .background(Color::rgba(0.6, 0.6, 0.3, 0.5))
                            .corner_radius(12.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .scale(1.1)
                            .translate(5.0, 0.0)
                            .child(
                                container()
                                    .width(90.0)
                                    .height(50.0)
                                    .background(Color::rgba(0.8, 0.8, 0.5, 0.9))
                                    .corner_radius(6.0)
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .rotate(-10.0)
                                    .font_size(12.0)
                                    .text_color(Color::rgb(0.1, 0.1, 0.1))
                                    .child(text("Scale+Trans")),
                            ),
                        // Animated rotating text
                        container()
                            .width(110.0)
                            .height(70.0)
                            .background(Color::rgba(0.8, 0.3, 0.5, 0.8))
                            .corner_radius(8.0)
                            .layout(
                                Flex::column()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .rotate(move || angle.get())
                            .font_size(14.0)
                            .text_color(Color::WHITE)
                            .child(text("Spinning!")),
                    ]),
            ]);

        app.add_surface(
            SurfaceConfig::new()
                .width(900)
                .height(450)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || view,
        );
    });
}
