//! Test mouse events on transformed containers.
//!
//! This example tests that hover and click work correctly on:
//! - Translated containers
//! - Rotated containers
//! - Scaled containers
//! - Nested transforms (parent transformed, child clickable)
//!
//! Uses state layer API with ripple effects to test hit testing.

use guido::prelude::*;

fn main() {
    let click_count = create_signal(0);

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(350)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .layout(
                    Flex::column()
                        .spacing(20.0)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .padding(20.0)
                .children([
                    // Row 1: Basic transforms
                    container()
                        .layout(
                            Flex::row()
                                .spacing(30.0)
                                .main_axis_alignment(MainAxisAlignment::Center),
                        )
                        .children([
                            // No transform (control)
                            make_box("None", Color::rgb(0.5, 0.5, 0.5), click_count),
                            // Translation
                            make_box("Translate", Color::rgb(0.3, 0.8, 0.3), click_count)
                                .translate(20.0, 10.0),
                            // Rotation
                            make_box("Rotate", Color::rgb(0.3, 0.3, 0.8), click_count).rotate(15.0),
                            // Scale up
                            make_box("Scale+", Color::rgb(0.8, 0.3, 0.8), click_count).scale(1.2),
                            // Scale down
                            make_box("Scale-", Color::rgb(0.8, 0.6, 0.3), click_count).scale(0.7),
                        ]),
                    // Row 2: Nested transforms
                    container()
                        .layout(
                            Flex::row()
                                .spacing(50.0)
                                .main_axis_alignment(MainAxisAlignment::Center),
                        )
                        .children([
                            // Nested: parent rotated, child clickable
                            container()
                                .width(100.0)
                                .height(100.0)
                                .background(Color::rgba(0.8, 0.3, 0.3, 0.3))
                                .corner_radius(8.0)
                                .rotate(20.0)
                                .layout(
                                    Flex::column()
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .child(make_box("Nested", Color::rgb(0.8, 0.8, 0.3), click_count)),
                            // Nested: parent scaled, child rotated and clickable
                            container()
                                .width(100.0)
                                .height(100.0)
                                .background(Color::rgba(0.3, 0.8, 0.3, 0.3))
                                .corner_radius(8.0)
                                .scale(1.3)
                                .layout(
                                    Flex::column()
                                        .main_axis_alignment(MainAxisAlignment::Center)
                                        .cross_axis_alignment(CrossAxisAlignment::Center),
                                )
                                .child(
                                    make_box("Nest+Rot", Color::rgb(0.3, 0.8, 0.8), click_count)
                                        .rotate(30.0),
                                ),
                        ]),
                    // Click counter display
                    container().child(
                        text(move || format!("Clicks: {}", click_count.get()))
                            .font_size(20.0)
                            .color(Color::WHITE),
                    ),
                ])
        },
    );
    app.run();
}

fn make_box(label: &'static str, base_color: Color, click_count: Signal<i32>) -> Container {
    container()
        .width(70.0)
        .height(70.0)
        .background(base_color)
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.15))
        .pressed_state(|s| s.ripple())
        .on_click(move || click_count.update(|c| *c += 1))
        .layout(
            Flex::column()
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .child(text(label).font_size(10.0).color(Color::WHITE).nowrap())
}
