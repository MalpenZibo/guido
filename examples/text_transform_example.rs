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

fn main() {
    // Animated rotation angle
    let angle = create_signal(0.0_f32);

    // Animation effect - the signal is Copy so we can use it directly
    create_effect(move || {
        let current = angle.get();
        // This will be updated by the on_update callback
        let _ = current;
    });

    let view = container()
        .layout(
            Flex::column()
                .spacing(20.0)
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .padding(30.0)
        .children([
            // Title
            container().child(
                text("Text Transform Demo")
                    .font_size(24.0)
                    .color(Color::WHITE),
            ),
            // Row 1: Basic transforms (rotation, scale, translation)
            container()
                .layout(
                    Flex::row()
                        .spacing(30.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .rotate(15.0)
                        .child(text("Rotate 15°").font_size(13.0).color(Color::WHITE)),
                    // Scale
                    container()
                        .width(110.0)
                        .height(70.0)
                        .background(Color::rgba(0.8, 0.5, 0.3, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .scale(1.2)
                        .child(text("Scale 1.2x").font_size(13.0).color(Color::WHITE)),
                    // Translation
                    container()
                        .width(110.0)
                        .height(70.0)
                        .background(Color::rgba(0.5, 0.8, 0.3, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .translate(10.0, -10.0)
                        .child(text("Translate").font_size(13.0).color(Color::WHITE)),
                    // Rotation + Scale
                    container()
                        .width(110.0)
                        .height(70.0)
                        .background(Color::rgba(0.8, 0.3, 0.8, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .rotate(-20.0)
                        .scale(0.9)
                        .child(text("Rot + Scale").font_size(13.0).color(Color::WHITE)),
                ]),
            // Row 2: Combined transforms and custom origin
            container()
                .layout(
                    Flex::row()
                        .spacing(30.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .rotate(10.0)
                        .scale(1.1)
                        .translate(5.0, 5.0)
                        .child(text("All Combined").font_size(13.0).color(Color::WHITE)),
                    // Custom origin: top-left
                    container()
                        .width(130.0)
                        .height(80.0)
                        .background(Color::rgba(0.7, 0.5, 0.2, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .transform_origin(TransformOrigin::TOP_LEFT)
                        .rotate(15.0)
                        .child(text("Origin: Top-Left").font_size(12.0).color(Color::WHITE)),
                    // Custom origin: bottom-right
                    container()
                        .width(130.0)
                        .height(80.0)
                        .background(Color::rgba(0.2, 0.5, 0.7, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .transform_origin(TransformOrigin::BOTTOM_RIGHT)
                        .rotate(15.0)
                        .child(text("Origin: Bot-Right").font_size(12.0).color(Color::WHITE)),
                    // Custom origin with scale
                    container()
                        .width(130.0)
                        .height(80.0)
                        .background(Color::rgba(0.7, 0.3, 0.5, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .transform_origin(TransformOrigin::TOP_RIGHT)
                        .scale(1.15)
                        .rotate(-10.0)
                        .child(text("Origin + Scale").font_size(12.0).color(Color::WHITE)),
                ]),
            // Row 3: Nested transforms
            container()
                .layout(
                    Flex::row()
                        .spacing(30.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .child(text("Nested").font_size(14.0).color(Color::WHITE)),
                        ),
                    // Double nested with additional rotation
                    container()
                        .width(130.0)
                        .height(90.0)
                        .background(Color::rgba(0.3, 0.6, 0.6, 0.5))
                        .corner_radius(12.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .rotate(15.0)
                                .child(text("30° Total").font_size(13.0).color(Color::rgb(0.1, 0.1, 0.1))),
                        ),
                    // Nested with scale + translation
                    container()
                        .width(130.0)
                        .height(90.0)
                        .background(Color::rgba(0.6, 0.6, 0.3, 0.5))
                        .corner_radius(12.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
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
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .rotate(-10.0)
                                .child(text("Scale+Trans").font_size(12.0).color(Color::rgb(0.1, 0.1, 0.1))),
                        ),
                    // Animated rotating text
                    container()
                        .width(110.0)
                        .height(70.0)
                        .background(Color::rgba(0.8, 0.3, 0.5, 0.8))
                        .corner_radius(8.0)
                        .layout(
                            Flex::column()
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .rotate(move || angle.get())
                        .child(text("Spinning!").font_size(14.0).color(Color::WHITE)),
                ]),
        ]);

    // Track time for animation
    let start_time = std::time::Instant::now();

    App::new()
        .width(900)
        .height(450)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .on_update(move || {
            // Update animation angle (full rotation every 4 seconds)
            let elapsed = start_time.elapsed().as_secs_f32();
            let new_angle = (elapsed * PI / 2.0).to_degrees() % 360.0;
            angle.set(new_angle);
        })
        .run(view);
}
