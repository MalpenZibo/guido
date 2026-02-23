//! Example demonstrating scrollable containers with mixed content.
//!
//! This example shows scrolling with:
//! - Text widgets
//! - Text input widgets
//! - Image widgets (raster and SVG)
//! - Interactive elements with hover states

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        // Signals for text inputs
        let name = create_signal(String::new());
        let email = create_signal(String::new());
        let bio = create_signal(String::new());

        // Helper to create a form field
        fn form_field(label: &'static str, input_signal: Signal<String>) -> Container {
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text(label).color(Color::rgb(0.7, 0.7, 0.8)).font_size(12.0))
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(6.0)
                        .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                        .child(
                            text_input(input_signal)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                                .font_size(14.0),
                        ),
                )
        }

        // Helper to create an image card
        fn image_card(path: &'static str, label: &'static str) -> Container {
            container()
                .layout(Flex::column().spacing(8.0))
                .child(
                    container()
                        .background(Color::rgb(0.2, 0.2, 0.25))
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.05))
                        .child(
                            image(path)
                                .width(120.0)
                                .height(90.0)
                                .content_fit(ContentFit::Cover),
                        ),
                )
                .child(text(label).font_size(11.0).color(Color::rgb(0.6, 0.6, 0.7)))
        }

        // Main scrollable content
        let view = container()
            .padding(16.0)
            .layout(Flex::row().spacing(16.0))
            // Left panel: Vertical scroll with form and content
            .child(
                container()
                    .layout(Flex::column().spacing(8.0))
                    .child(
                        text("User Profile")
                            .color(Color::WHITE)
                            .font_size(16.0)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        container()
                            .width(320.0)
                            .height(400.0)
                            .background(Color::rgb(0.12, 0.12, 0.18))
                            .corner_radius(12.0)
                            .scrollable(ScrollAxis::Vertical)
                            .scrollbar(|sb| {
                                sb.width(6.0)
                                    .handle_color(Color::rgb(0.4, 0.5, 0.6))
                                    .handle_hover_color(Color::rgb(0.5, 0.6, 0.7))
                                    .track_color(Color::rgba(0.3, 0.3, 0.4, 0.3))
                            })
                            .child(
                                container()
                                    .layout(Flex::column().spacing(16.0))
                                    .padding(16.0)
                                    // Profile header with image
                                    .child(
                                        container()
                                            .layout(
                                                Flex::row()
                                                    .spacing(16.0)
                                                    .cross_alignment(CrossAlignment::Center),
                                            )
                                            .child(
                                                container()
                                                    .corner_radius(40.0)
                                                    .background(Color::rgb(0.25, 0.25, 0.3))
                                                    .child(
                                                        image("examples/assets/photo.webp")
                                                            .width(80.0)
                                                            .height(80.0)
                                                            .content_fit(ContentFit::Cover),
                                                    ),
                                            )
                                            .child(
                                                container()
                                                    .layout(Flex::column().spacing(4.0))
                                                    .child(
                                                        text("Edit Profile")
                                                            .color(Color::WHITE)
                                                            .font_size(18.0),
                                                    )
                                                    .child(
                                                        text("Update your information")
                                                            .color(Color::rgb(0.5, 0.5, 0.6))
                                                            .font_size(12.0),
                                                    ),
                                            ),
                                    )
                                    // Form fields
                                    .child(form_field("Name", name))
                                    .child(form_field("Email", email))
                                    .child(form_field("Bio", bio))
                                    // Additional info section
                                    .child(
                                        container()
                                            .layout(Flex::column().spacing(8.0))
                                            .child(
                                                text("Account Settings")
                                                    .color(Color::WHITE)
                                                    .font_size(14.0),
                                            )
                                            .children(
                                                [
                                                    "Notifications",
                                                    "Privacy",
                                                    "Security",
                                                    "Appearance",
                                                    "Language",
                                                ]
                                                .into_iter()
                                                .map(|item| {
                                                    container()
                                                        .padding(12.0)
                                                        .background(Color::rgb(0.18, 0.18, 0.24))
                                                        .corner_radius(6.0)
                                                        .hover_state(|s| s.lighter(0.05))
                                                        .pressed_state(|s| s.ripple())
                                                        .child(
                                                            text(item)
                                                                .color(Color::rgb(0.8, 0.8, 0.9)),
                                                        )
                                                })
                                                .collect::<Vec<_>>(),
                                            ),
                                    )
                                    // More content to ensure scroll
                                    .child(
                                        container()
                                            .layout(Flex::column().spacing(8.0))
                                            .child(
                                                text("Recent Activity")
                                                    .color(Color::WHITE)
                                                    .font_size(14.0),
                                            )
                                            .children(
                                                (1..=8)
                                                    .map(|i| {
                                                        container()
                                                            .layout(Flex::row().spacing(8.0))
                                                            .padding(8.0)
                                                            .background(Color::rgb(0.15, 0.15, 0.2))
                                                            .corner_radius(4.0)
                                                            .child(
                                                                container()
                                                                    .width(32.0)
                                                                    .height(32.0)
                                                                    .background(Color::rgb(
                                                                        0.2 + (i as f32) * 0.02,
                                                                        0.3,
                                                                        0.4,
                                                                    ))
                                                                    .corner_radius(16.0),
                                                            )
                                                            .child(
                                                                container()
                                                                    .layout(
                                                                        Flex::column().spacing(2.0),
                                                                    )
                                                                    .child(
                                                                        text(format!(
                                                                            "Activity {}",
                                                                            i
                                                                        ))
                                                                        .color(Color::rgb(
                                                                            0.8, 0.8, 0.9,
                                                                        ))
                                                                        .font_size(13.0),
                                                                    )
                                                                    .child(
                                                                        text(format!(
                                                                            "{} hours ago",
                                                                            i * 2
                                                                        ))
                                                                        .color(Color::rgb(
                                                                            0.5, 0.5, 0.6,
                                                                        ))
                                                                        .font_size(11.0),
                                                                    ),
                                                            )
                                                    })
                                                    .collect::<Vec<_>>(),
                                            ),
                                    ),
                            ),
                    ),
            )
            // Right panel: Horizontal scroll gallery
            .child(
                container()
                    .layout(Flex::column().spacing(8.0))
                    .child(
                        text("Image Gallery")
                            .color(Color::WHITE)
                            .font_size(16.0)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        container()
                            .width(400.0)
                            .height(160.0)
                            .background(Color::rgb(0.12, 0.12, 0.18))
                            .corner_radius(12.0)
                            .scrollable(ScrollAxis::Horizontal)
                            .scrollbar(|sb| {
                                sb.width(6.0)
                                    .handle_color(Color::rgb(0.5, 0.6, 0.4))
                                    .handle_hover_color(Color::rgb(0.6, 0.7, 0.5))
                                    .track_color(Color::rgba(0.3, 0.4, 0.3, 0.3))
                            })
                            .child(
                                container()
                                    .layout(Flex::row().spacing(12.0))
                                    .padding(12.0)
                                    .child(image_card("examples/assets/photo.webp", "Photo 1"))
                                    .child(image_card("examples/assets/photo.webp", "Photo 2"))
                                    .child(image_card("examples/assets/photo.webp", "Photo 3"))
                                    .child(image_card("examples/assets/photo.webp", "Photo 4"))
                                    .child(image_card("examples/assets/photo.webp", "Photo 5"))
                                    .child(image_card("examples/assets/photo.webp", "Photo 6")),
                            ),
                    )
                    // SVG icons row
                    .child(
                        text("Icons")
                            .color(Color::WHITE)
                            .font_size(16.0)
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        container()
                            .width(400.0)
                            .height(100.0)
                            .background(Color::rgb(0.12, 0.12, 0.18))
                            .corner_radius(12.0)
                            .scrollable(ScrollAxis::Horizontal)
                            .scrollbar(|sb| {
                                sb.width(4.0)
                                    .handle_color(Color::rgb(0.6, 0.5, 0.7))
                                    .handle_hover_color(Color::rgb(0.7, 0.6, 0.8))
                            })
                            .child(
                                container()
                                    .layout(Flex::row().spacing(16.0))
                                    .padding(12.0)
                                    .children(
                                        (0..12)
                                            .map(|i| {
                                                container()
                                                    .layout(
                                                        Flex::column()
                                                            .spacing(4.0)
                                                            .cross_alignment(
                                                                CrossAlignment::Center,
                                                            ),
                                                    )
                                                    .child(
                                                        container()
                                                            .width(48.0)
                                                            .height(48.0)
                                                            .background(Color::rgb(0.2, 0.2, 0.28))
                                                            .corner_radius(8.0)
                                                            .hover_state(|s| s.lighter(0.1))
                                                            .pressed_state(|s| s.ripple())
                                                            .layout(
                                                                Flex::column()
                                                                    .main_alignment(
                                                                        MainAlignment::Center,
                                                                    )
                                                                    .cross_alignment(
                                                                        CrossAlignment::Center,
                                                                    ),
                                                            )
                                                            .child(
                                                                image("examples/assets/logo.svg")
                                                                    .width(32.0)
                                                                    .height(24.0),
                                                            ),
                                                    )
                                                    .child(
                                                        text(format!("Icon {}", i + 1))
                                                            .font_size(10.0)
                                                            .color(Color::rgb(0.5, 0.5, 0.6)),
                                                    )
                                            })
                                            .collect::<Vec<_>>(),
                                    ),
                            ),
                    )
                    // Current input values display
                    .child(
                        container()
                            .padding(12.0)
                            .background(Color::rgb(0.12, 0.12, 0.18))
                            .corner_radius(8.0)
                            .layout(Flex::column().spacing(4.0))
                            .child(
                                text("Form Values:")
                                    .color(Color::rgb(0.5, 0.5, 0.6))
                                    .font_size(11.0),
                            )
                            .child(
                                text(move || format!("Name: {}", name.get()))
                                    .color(Color::rgb(0.7, 0.7, 0.8))
                                    .font_size(12.0),
                            )
                            .child(
                                text(move || format!("Email: {}", email.get()))
                                    .color(Color::rgb(0.7, 0.7, 0.8))
                                    .font_size(12.0),
                            )
                            .child(
                                text(move || format!("Bio: {}", bio.get()))
                                    .color(Color::rgb(0.7, 0.7, 0.8))
                                    .font_size(12.0),
                            ),
                    ),
            );

        app.add_surface(
            SurfaceConfig::new()
                .width(780)
                .height(500)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("scroll-mixed-content")
                .background_color(Color::rgb(0.08, 0.08, 0.12)),
            move || view,
        );
    });
}
