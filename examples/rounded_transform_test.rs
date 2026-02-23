//! Test transforms on rectangular boxes.
//!
//! This verifies that transforms (translate, rotate, scale)
//! work correctly on containers with rounded corners.
//! Uses state layer API with ripple effects for visual feedback.

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        let click_count = create_signal(0);

        app.add_surface(
            SurfaceConfig::new()
                .height(280)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .layout(
                        Flex::column()
                            .spacing(30.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .padding(40.0)
                    .children([
                        // Row with transforms
                        container()
                            .layout(
                                Flex::row()
                                    .spacing(50.0)
                                    .main_alignment(MainAlignment::Center),
                            )
                            .children([
                                // No transform (control)
                                make_box("None", Color::rgb(0.5, 0.5, 0.5), click_count),
                                // Translate
                                make_box("Translate", Color::rgb(0.3, 0.8, 0.3), click_count)
                                    .translate(15.0, 10.0),
                                // Rotate
                                make_box("Rotate", Color::rgb(0.3, 0.3, 0.8), click_count)
                                    .rotate(20.0),
                                // All 3 transforms (nested)
                                container().translate(10.0, 15.0).child(
                                    container().scale(1.15).child(
                                        make_box("All 3", Color::rgb(0.8, 0.8, 0.3), click_count)
                                            .rotate(25.0),
                                    ),
                                ),
                                // Scale
                                make_box("Scale", Color::rgb(0.8, 0.3, 0.8), click_count)
                                    .scale(1.2),
                            ]),
                        // Instructions
                        container().child(
                            text("Rectangular boxes with transforms. Hover and click to test.")
                                .font_size(14.0)
                                .color(Color::rgb(0.7, 0.7, 0.7)),
                        ),
                        // Click counter display
                        container().child(
                            text(move || format!("Clicks: {}", click_count.get()))
                                .font_size(20.0)
                                .color(Color::WHITE),
                        ),
                    ])
            },
        );
    });
}

fn make_box(label: &'static str, base_color: Color, click_count: Signal<i32>) -> Container {
    container()
        .width(100.0)
        .height(100.0)
        .background(base_color)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .on_click(move || click_count.update(|c| *c += 1))
        .layout(
            Flex::column()
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(text(label).font_size(12.0).color(Color::WHITE))
}
