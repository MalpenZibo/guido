//! Example demonstrating the transform system with rotation, scale, and animation.
//!
//! This example shows:
//! - Static transforms (rotate, scale)
//! - Reactive transforms that change based on signals
//! - Animated transforms with spring physics
//! - Nested transforms (parent-child composition)
//! - Custom transform origins (pivot points for rotation/scale)

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        // Signals for interactive transforms
        let rotation = create_signal(0.0f32);
        let scale_factor = create_signal(1.0f32);
        let is_scaled = create_signal(false);

        // Run the app with taller height to see transforms
        app.add_surface(
            SurfaceConfig::new()
                .height(120)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(20.0)
                            .main_axis_alignment(MainAxisAlignment::Center)
                            .cross_axis_alignment(CrossAxisAlignment::Center),
                    )
                    .padding(16.0)
                    .children([
                        // 1. Static rotation (45 degrees)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.3, 0.3))
                            .corner_radius(8.0)
                            .rotate(45.0)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(text("45").color(Color::WHITE).font_size(12.0)),
                            ),
                        // 2. Click to rotate (increments by 45 degrees)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.3, 0.6, 0.8))
                            .corner_radius(8.0)
                            .rotate(rotation)
                            .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
                            .on_click(move || {
                                rotation.update(|r| *r += 45.0);
                            })
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(
                                        text("Click").color(Color::WHITE).font_size(10.0).nowrap(),
                                    ),
                            ),
                        // 3. Click to toggle scale with spring animation
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.3, 0.8, 0.4))
                            .corner_radius(8.0)
                            .scale(scale_factor)
                            .animate_transform(Transition::spring(SpringConfig::BOUNCY))
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
                            .on_click(move || {
                                is_scaled.update(|s| *s = !*s);
                                let target = if is_scaled.get() { 1.3 } else { 1.0 };
                                scale_factor.set(target);
                            })
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(
                                        text("Scale").color(Color::WHITE).font_size(10.0).nowrap(),
                                    ),
                            ),
                        // 4. Static scale (smaller)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.6, 0.4, 0.8))
                            .corner_radius(8.0)
                            .scale(0.7)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(text("0.7x").color(Color::WHITE).font_size(12.0)),
                            ),
                        // 5. Combined rotation + scale
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.6, 0.2))
                            .corner_radius(8.0)
                            .transform(Transform::rotate_degrees(30.0).then(&Transform::scale(0.8)))
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(
                                        text("Both").color(Color::WHITE).font_size(10.0).nowrap(),
                                    ),
                            ),
                        // 6. Rotation around top-left corner (transform origin)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.5, 0.7))
                            .corner_radius(8.0)
                            .rotate(45.0)
                            .transform_origin(TransformOrigin::TOP_LEFT)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(text("TL").color(Color::WHITE).font_size(12.0)),
                            ),
                        // 7. Scale from bottom-right corner (transform origin)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.5, 0.7, 0.8))
                            .corner_radius(8.0)
                            .scale(0.8)
                            .transform_origin(TransformOrigin::BOTTOM_RIGHT)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(text("BR").color(Color::WHITE).font_size(12.0)),
                            ),
                        // 8. Interactive: click to cycle through origins
                        {
                            let origin_index = create_signal(0usize);
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.7, 0.8, 0.5))
                                .corner_radius(8.0)
                                .rotate(30.0)
                                .transform_origin(move || match origin_index.get() % 5 {
                                    0 => TransformOrigin::CENTER,
                                    1 => TransformOrigin::TOP_LEFT,
                                    2 => TransformOrigin::TOP_RIGHT,
                                    3 => TransformOrigin::BOTTOM_LEFT,
                                    _ => TransformOrigin::BOTTOM_RIGHT,
                                })
                                .hover_state(|s| s.lighter(0.1))
                                .pressed_state(|s| s.ripple())
                                .on_click(move || origin_index.update(|i| *i += 1))
                                .child(
                                    container()
                                        .layout(
                                            Flex::column()
                                                .main_axis_alignment(MainAxisAlignment::Center)
                                                .cross_axis_alignment(CrossAxisAlignment::Center),
                                        )
                                        .child(
                                            text("Cycle")
                                                .color(Color::WHITE)
                                                .font_size(10.0)
                                                .nowrap(),
                                        ),
                                )
                        },
                    ])
            },
        );
    });
}
