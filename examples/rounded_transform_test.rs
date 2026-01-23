//! Test hit testing with rounded corners AND transforms combined.
//!
//! This verifies that the rounded corner hitbox works correctly
//! when the container is also translated, rotated, or scaled.

use guido::prelude::*;

fn main() {
    let click_count = create_signal(0);

    let view = container()
        .layout(
            Flex::column()
                .spacing(30.0)
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .padding(40.0)
        .children([
            // Row with rounded corners + transforms
            container()
                .layout(
                    Flex::row()
                        .spacing(50.0)
                        .main_axis_alignment(MainAxisAlignment::Center),
                )
                .children([
                    // Rounded + no transform (control)
                    make_box("None", Color::rgb(0.5, 0.5, 0.5), click_count),
                    // Rounded + translate
                    make_box("Translate", Color::rgb(0.3, 0.8, 0.3), click_count)
                        .translate(15.0, 10.0),
                    // Rounded + rotate
                    make_box("Rotate", Color::rgb(0.3, 0.3, 0.8), click_count).rotate(20.0),
                    // Rounded + rotate + translate + scale (nested to combine transforms)
                    container()
                        .translate(10.0, 15.0)
                        .child(container().scale(1.15).child(
                            make_box("All 3", Color::rgb(0.8, 0.8, 0.3), click_count).rotate(25.0),
                        )),
                    // Rounded + scale
                    make_box("Scale", Color::rgb(0.8, 0.3, 0.8), click_count).scale(1.2),
                ]),
            // Instructions
            container().child(
                text("All boxes have r=30 corners. Hover corners to test hitbox.")
                    .font_size(14.0)
                    .color(Color::rgb(0.7, 0.7, 0.7)),
            ),
            // Click counter display
            container().child(
                text(move || format!("Clicks: {}", click_count.get()))
                    .font_size(20.0)
                    .color(Color::WHITE),
            ),
        ]);

    App::new()
        .height(280)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}

fn make_box(label: &'static str, base_color: Color, click_count: Signal<i32>) -> Container {
    let is_hovered = create_signal(false);

    let hover_color = Color::rgb(
        (base_color.r + 0.3).min(1.0),
        (base_color.g + 0.3).min(1.0),
        (base_color.b + 0.3).min(1.0),
    );

    container()
        .width(100.0)
        .height(100.0)
        .background(move || {
            if is_hovered.get() {
                hover_color
            } else {
                base_color
            }
        })
        .corner_radius(30.0) // Significant rounding to make it obvious
        .on_hover(move |hovered| is_hovered.set(hovered))
        .on_click(move || click_count.update(|c| *c += 1))
        .layout(
            Flex::column()
                .main_axis_alignment(MainAxisAlignment::Center)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .child(text(label).font_size(12.0).color(Color::WHITE))
}
