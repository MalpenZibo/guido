//! Test hit testing with rounded corners.
//!
//! This example tests that hover and click correctly respect corner radius.
//! The corners outside the rounded area should NOT trigger events.
//! Uses state layer API with ripple effects for visual feedback.

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        let click_count = create_signal(0);

        app.add_surface(
            SurfaceConfig::new()
                .height(300)
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
                    // Row with different corner radii
                    container()
                        .layout(
                            Flex::row()
                                .spacing(40.0)
                                .main_alignment(MainAlignment::Center),
                        )
                        .children([
                            // No corner radius (control)
                            make_box("r=0", 0.0, Color::rgb(0.5, 0.5, 0.5), click_count),
                            // Small corner radius
                            make_box("r=10", 10.0, Color::rgb(0.3, 0.8, 0.3), click_count),
                            // Medium corner radius
                            make_box("r=25", 25.0, Color::rgb(0.3, 0.3, 0.8), click_count),
                            // Large corner radius (half of size = circle-ish)
                            make_box("r=50", 50.0, Color::rgb(0.8, 0.3, 0.8), click_count),
                        ]),
                    // Instructions
                    container().child(
                        text("Hover over corners - should NOT highlight outside the rounded area")
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

fn make_box(
    label: &'static str,
    corner_radius: f32,
    base_color: Color,
    click_count: Signal<i32>,
) -> Container {
    container()
        .width(100.0)
        .height(100.0)
        .background(base_color)
        .corner_radius(corner_radius)
        .hover_state(|s| s.lighter(0.15))
        .pressed_state(|s| s.ripple())
        .on_click(move || click_count.update(|c| *c += 1))
        .layout(
            Flex::column()
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(text(label).font_size(14.0).color(Color::WHITE))
}
