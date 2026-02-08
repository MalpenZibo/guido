//! Test example for transform_origin with shapes only (no text).
//!
//! This helps isolate transform_origin behavior from text rendering issues.

use guido::prelude::*;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let angle = create_signal(0.0_f32);

    // Animation service - updates the angle signal continuously
    let start_time = std::time::Instant::now();
    let angle_w = angle.writer();
    let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
        while ctx.is_running() {
            let elapsed = start_time.elapsed().as_secs_f32();
            let new_angle = (elapsed * 45.0) % 360.0; // 45 degrees per second
            angle_w.set(new_angle);
            tokio::time::sleep(Duration::from_millis(16)).await;
        }
    });

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(900)
            .height(550)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .layout(
                    Flex::column()
                        .spacing(40.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .padding(40.0)
                .children([
                    // Row 1: Different origins with same rotation
                    container()
                        .layout(
                            Flex::row()
                                .spacing(80.0)
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .children([
                            // Default origin (center)
                            container()
                                .width(100.0)
                                .height(80.0)
                                .background(Color::rgba(0.3, 0.5, 0.8, 0.8))
                                .corner_radius(8.0)
                                .rotate(20.0),
                            // Top-left origin
                            container()
                                .width(100.0)
                                .height(80.0)
                                .background(Color::rgba(0.8, 0.5, 0.3, 0.8))
                                .corner_radius(8.0)
                                .transform_origin(TransformOrigin::TOP_LEFT)
                                .rotate(20.0),
                            // Top-right origin
                            container()
                                .width(100.0)
                                .height(80.0)
                                .background(Color::rgba(0.5, 0.8, 0.3, 0.8))
                                .corner_radius(8.0)
                                .transform_origin(TransformOrigin::TOP_RIGHT)
                                .rotate(20.0),
                            // Bottom-left origin
                            container()
                                .width(100.0)
                                .height(80.0)
                                .background(Color::rgba(0.8, 0.3, 0.8, 0.8))
                                .corner_radius(8.0)
                                .transform_origin(TransformOrigin::BOTTOM_LEFT)
                                .rotate(20.0),
                            // Bottom-right origin
                            container()
                                .width(100.0)
                                .height(80.0)
                                .background(Color::rgba(0.3, 0.8, 0.8, 0.8))
                                .corner_radius(8.0)
                                .transform_origin(TransformOrigin::BOTTOM_RIGHT)
                                .rotate(20.0),
                        ]),
                    // Row 2: Scale with different origins
                    container()
                        .layout(
                            Flex::row()
                                .spacing(80.0)
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .children([
                            // Default origin (center) - scale
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.6, 0.3, 0.3, 0.8))
                                .corner_radius(6.0)
                                .scale(1.3),
                            // Top-left origin - scale
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.3, 0.6, 0.3, 0.8))
                                .corner_radius(6.0)
                                .transform_origin(TransformOrigin::TOP_LEFT)
                                .scale(1.3),
                            // Bottom-right origin - scale
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.3, 0.3, 0.6, 0.8))
                                .corner_radius(6.0)
                                .transform_origin(TransformOrigin::BOTTOM_RIGHT)
                                .scale(1.3),
                        ]),
                    // Row 3: Animated rotation with different origins
                    container()
                        .layout(
                            Flex::row()
                                .spacing(80.0)
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .children([
                            // Center origin - animated
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.7, 0.5, 0.2, 0.8))
                                .corner_radius(6.0)
                                .rotate(move || angle.get()),
                            // Top-left origin - animated
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.2, 0.5, 0.7, 0.8))
                                .corner_radius(6.0)
                                .transform_origin(TransformOrigin::TOP_LEFT)
                                .rotate(move || angle.get()),
                            // Custom origin (25%, 75%) - animated
                            container()
                                .width(80.0)
                                .height(60.0)
                                .background(Color::rgba(0.5, 0.7, 0.2, 0.8))
                                .corner_radius(6.0)
                                .transform_origin(TransformOrigin::percent(25.0, 75.0))
                                .rotate(move || angle.get()),
                        ]),
                    // Row 4: Nested containers with different origins
                    container()
                        .layout(
                            Flex::row()
                                .spacing(80.0)
                                .main_axis_alignment(MainAxisAlignment::Center)
                                .cross_axis_alignment(CrossAxisAlignment::Center),
                        )
                        .children([
                            // Parent rotated at center, child inside
                            container()
                                .width(120.0)
                                .height(100.0)
                                .background(Color::rgba(0.4, 0.4, 0.6, 0.5))
                                .corner_radius(10.0)
                                .layout(
                                    Flex::column()
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .rotate(15.0)
                                .child(
                                    container()
                                        .width(60.0)
                                        .height(40.0)
                                        .background(Color::rgba(0.6, 0.6, 0.8, 0.9))
                                        .corner_radius(4.0),
                                ),
                            // Parent rotated at top-left, child inside
                            container()
                                .width(120.0)
                                .height(100.0)
                                .background(Color::rgba(0.6, 0.4, 0.4, 0.5))
                                .corner_radius(10.0)
                                .layout(
                                    Flex::column()
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .transform_origin(TransformOrigin::TOP_LEFT)
                                .rotate(15.0)
                                .child(
                                    container()
                                        .width(60.0)
                                        .height(40.0)
                                        .background(Color::rgba(0.8, 0.6, 0.6, 0.9))
                                        .corner_radius(4.0),
                                ),
                        ]),
                ])
        },
    );
    app.run();
}
