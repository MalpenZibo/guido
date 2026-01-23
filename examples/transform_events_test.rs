//! Test mouse events on transformed containers.
//!
//! This example tests that hover and click work correctly on:
//! - Translated containers
//! - Rotated containers
//! - Scaled containers
//! - Nested transforms (parent transformed, child clickable)
//!
//! Uses on_hover background color change to test hit testing.

use guido::prelude::*;

fn main() {
    let click_count = create_signal(0);

    let view = container()
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
        ]);

    App::new()
        .height(350)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}

fn make_box(label: &'static str, base_color: Color, click_count: Signal<i32>) -> Container {
    let is_hovered = create_signal(false);

    // Highlight color when hovered (brighter version)
    let hover_color = Color::rgb(
        (base_color.r + 0.3).min(1.0),
        (base_color.g + 0.3).min(1.0),
        (base_color.b + 0.3).min(1.0),
    );

    container()
        .width(70.0)
        .height(70.0)
        .background(move || {
            if is_hovered.get() {
                hover_color
            } else {
                base_color
            }
        })
        .corner_radius(8.0)
        .on_hover(move |hovered| is_hovered.set(hovered))
        .on_click(move || click_count.update(|c| *c += 1))
        .layout(
            Flex::column()
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .child(text(label).font_size(10.0).color(Color::WHITE).nowrap())
}
