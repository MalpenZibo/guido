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
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .padding(16.0)
                    .children([
                        // 1. Static rotation (45 degrees)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.3, 0.3))
                            .corners(8.0)
                            .transform(Transform::rotate_degrees(45.0))
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(text("45").font_size(12.0).color(Color::WHITE)),
                            ),
                        // 2. Click to rotate (increments by 45 degrees)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.3, 0.6, 0.8))
                            .corners(8.0)
                            .transform(move || Transform::rotate_degrees(rotation.get()))
                            .animate_transform(Transition::new(300.0, TimingFunction::EaseOut))
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(move || {
                                rotation.update(|r| *r += 45.0);
                            })
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(
                                        text("Click").font_size(10.0).color(Color::WHITE).nowrap(),
                                    ),
                            ),
                        // 3. Click to toggle scale with spring animation
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.3, 0.8, 0.4))
                            .corners(8.0)
                            .transform(move || Transform::scale(scale_factor.get()))
                            .animate_transform(Transition::spring(SpringConfig::BOUNCY))
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(move || {
                                is_scaled.update(|s| *s = !*s);
                                let target = if is_scaled.get() { 1.3 } else { 1.0 };
                                scale_factor.set(target);
                            })
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(
                                        text("Scale").font_size(10.0).color(Color::WHITE).nowrap(),
                                    ),
                            ),
                        // 4. Static scale (smaller)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.6, 0.4, 0.8))
                            .corners(8.0)
                            .transform(Transform::scale(0.7))
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(text("0.7x").font_size(12.0).color(Color::WHITE)),
                            ),
                        // 5. Combined rotation + scale
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.6, 0.2))
                            .corners(8.0)
                            .transform(Transform::rotate_degrees(30.0).then(&Transform::scale(0.8)))
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(
                                        text("Both").font_size(10.0).color(Color::WHITE).nowrap(),
                                    ),
                            ),
                        // 6. Rotation around top-left corner (transform origin)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.8, 0.5, 0.7))
                            .corners(8.0)
                            .transform(Transform::rotate_degrees(45.0))
                            .pivot(Pivot::TOP_LEFT)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(text("TL").font_size(12.0).color(Color::WHITE)),
                            ),
                        // 7. Scale from bottom-right corner (transform origin)
                        container()
                            .width(60.0)
                            .height(60.0)
                            .background(Color::rgb(0.5, 0.7, 0.8))
                            .corners(8.0)
                            .transform(Transform::scale(0.8))
                            .pivot(Pivot::BOTTOM_RIGHT)
                            .child(
                                container()
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .child(text("BR").font_size(12.0).color(Color::WHITE)),
                            ),
                        // 8. Interactive: click to cycle through origins
                        {
                            let origin_index = create_signal(0usize);
                            container()
                                .width(60.0)
                                .height(60.0)
                                .background(Color::rgb(0.7, 0.8, 0.5))
                                .corners(8.0)
                                .transform(Transform::rotate_degrees(30.0))
                                .pivot(move || match origin_index.get() % 5 {
                                    0 => Pivot::CENTER,
                                    1 => Pivot::TOP_LEFT,
                                    2 => Pivot::TOP_RIGHT,
                                    3 => Pivot::BOTTOM_LEFT,
                                    _ => Pivot::BOTTOM_RIGHT,
                                })
                                .when_hovered(|s| s.lighter(0.1))
                                .when_pressed(|s| s.ripple())
                                .on_click(move || origin_index.update(|i| *i += 1))
                                .child(
                                    container()
                                        .layout(
                                            Flex::column()
                                                .main_alignment(MainAlignment::Center)
                                                .cross_alignment(CrossAlignment::Center),
                                        )
                                        .child(
                                            text("Cycle")
                                                .font_size(10.0)
                                                .color(Color::WHITE)
                                                .nowrap(),
                                        ),
                                )
                        },
                    ])
            },
        );
    });
}
