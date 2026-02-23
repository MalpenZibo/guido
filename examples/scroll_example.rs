//! Example demonstrating scrollable containers.
//!
//! This example shows:
//! - Vertical scrolling with overflow content
//! - Horizontal scrolling
//! - Custom scrollbar styling
//! - Hidden scrollbars

use guido::prelude::*;

fn main() {
    // Create a view with multiple scrollable sections
    let view = container()
        .layout(Flex::row().spacing(16.0))
        .padding(8.0)
        // Vertical scroll example
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text("Vertical Scroll").color(Color::WHITE))
                .child(
                    container()
                        .width(200.0)
                        .height(200.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(8.0)
                        .scrollable(ScrollAxis::Vertical)
                        .child(
                            container()
                                .layout(Flex::column().spacing(8.0))
                                .padding(8.0)
                                .children(
                                    (0..20)
                                        .map(|i| {
                                            container()
                                                .padding(8.0)
                                                .background(Color::rgb(0.25, 0.25, 0.35))
                                                .corner_radius(4.0)
                                                .hover_state(|s| s.lighter(0.05))
                                                .child(
                                                    text(format!("Item {}", i + 1))
                                                        .color(Color::WHITE),
                                                )
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        ),
                ),
        )
        // Horizontal scroll example
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text("Horizontal Scroll").color(Color::WHITE))
                .child(
                    container()
                        .width(200.0)
                        .height(80.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(8.0)
                        .scrollable(ScrollAxis::Horizontal)
                        .child(
                            container()
                                .layout(Flex::row().spacing(8.0))
                                .padding(8.0)
                                .children(
                                    (0..15)
                                        .map(|i| {
                                            container()
                                                .width(60.0)
                                                .height(60.0)
                                                .background(Color::rgb(
                                                    0.2 + (i as f32) * 0.03,
                                                    0.3,
                                                    0.4,
                                                ))
                                                .corner_radius(8.0)
                                                .hover_state(|s| s.lighter(0.1))
                                                .layout(
                                                    Flex::column()
                                                        .main_axis_alignment(
                                                            MainAxisAlignment::Center,
                                                        )
                                                        .cross_axis_alignment(
                                                            CrossAxisAlignment::Center,
                                                        ),
                                                )
                                                .child(
                                                    text(format!("{}", i + 1)).color(Color::WHITE),
                                                )
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        ),
                ),
        )
        // Custom styled scrollbar
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text("Custom Scrollbar").color(Color::WHITE))
                .child(
                    container()
                        .width(200.0)
                        .height(200.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(8.0)
                        .scrollable(ScrollAxis::Vertical)
                        .scrollbar(|sb| {
                            sb.width(6.0)
                                .handle_color(Color::rgb(0.4, 0.6, 0.9))
                                .handle_hover_color(Color::rgb(0.5, 0.7, 1.0))
                                .handle_pressed_color(Color::rgb(0.6, 0.8, 1.0))
                                .handle_corner_radius(3.0)
                                .track_color(Color::rgba(0.4, 0.6, 0.9, 0.1))
                        })
                        .child(
                            container()
                                .layout(Flex::column().spacing(8.0))
                                .padding(8.0)
                                .children(
                                    (0..15)
                                        .map(|i| {
                                            container()
                                                .padding(8.0)
                                                .background(Color::rgb(0.2, 0.3, 0.45))
                                                .corner_radius(4.0)
                                                .hover_state(|s| s.lighter(0.05))
                                                .child(
                                                    text(format!("Blue Item {}", i + 1))
                                                        .color(Color::WHITE),
                                                )
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        ),
                ),
        )
        // Hidden scrollbar
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text("Hidden Scrollbar").color(Color::WHITE))
                .child(
                    container()
                        .width(200.0)
                        .height(200.0)
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(8.0)
                        .scrollable(ScrollAxis::Vertical)
                        .scrollbar_visibility(ScrollbarVisibility::Hidden)
                        .child(
                            container()
                                .layout(Flex::column().spacing(8.0))
                                .padding(8.0)
                                .children(
                                    (0..15)
                                        .map(|i| {
                                            container()
                                                .padding(8.0)
                                                .background(Color::rgb(0.3, 0.25, 0.2))
                                                .corner_radius(4.0)
                                                .hover_state(|s| s.lighter(0.05))
                                                .child(
                                                    text(format!("Hidden {}", i + 1))
                                                        .color(Color::WHITE),
                                                )
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        ),
                ),
        );

    // Run the app with a larger window for the demo
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(900)
                .height(300)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || view,
        );
    });
}
