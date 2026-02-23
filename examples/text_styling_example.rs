//! Text Styling Example
//!
//! Demonstrates text styling with font family and font weight options.
//! Run with: cargo run --example text_styling_example

use guido::prelude::*;

fn main() {
    // Set an app-level default font (widgets will use this unless overridden)
    // Note: The actual font availability depends on the system
    App::new()
        .default_font_family(FontFamily::SansSerif)
        .run(|app| {
            app.add_surface(
                SurfaceConfig::new()
                    .width(600)
                    .height(600)
                    .anchor(Anchor::TOP | Anchor::LEFT)
                    .background_color(Color::rgb(0.08, 0.08, 0.12)),
                || {
                    container()
                        .background(Color::rgb(0.08, 0.08, 0.12))
                        .padding(24.0)
                        .layout(Flex::column().spacing(20.0))
                        .child(
                            // Title
                            text("Text Styling Demo")
                                .font_size(28.0)
                                .bold()
                                .color(Color::rgb(0.9, 0.9, 0.95)),
                        )
                        .child(create_font_family_section())
                        .child(create_font_weight_section())
                        .child(create_combined_section())
                        .child(create_text_input_section())
                },
            );
        });
}

/// Section demonstrating different font families
fn create_font_family_section() -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            text("Font Families:")
                .font_size(16.0)
                .bold()
                .color(Color::rgb(0.7, 0.7, 0.8)),
        )
        .child(
            container()
                .padding(12.0)
                .background(Color::rgb(0.12, 0.12, 0.18))
                .corner_radius(8.0)
                .layout(Flex::column().spacing(8.0))
                .child(
                    text("Sans-Serif (default)")
                        .font_family(FontFamily::SansSerif)
                        .color(Color::WHITE),
                )
                .child(
                    text("Serif font family")
                        .font_family(FontFamily::Serif)
                        .color(Color::WHITE),
                )
                .child(
                    text("Monospace font family")
                        .font_family(FontFamily::Monospace)
                        .color(Color::WHITE),
                )
                .child(
                    text("Using .mono() shorthand")
                        .mono()
                        .color(Color::rgb(0.6, 0.9, 0.6)),
                ),
        )
}

/// Section demonstrating different font weights
fn create_font_weight_section() -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            text("Font Weights:")
                .font_size(16.0)
                .bold()
                .color(Color::rgb(0.7, 0.7, 0.8)),
        )
        .child(
            container()
                .padding(12.0)
                .background(Color::rgb(0.12, 0.12, 0.18))
                .corner_radius(8.0)
                .layout(Flex::column().spacing(8.0))
                .child(
                    text("Thin (100)")
                        .font_weight(FontWeight::THIN)
                        .color(Color::WHITE),
                )
                .child(
                    text("Light (300)")
                        .font_weight(FontWeight::LIGHT)
                        .color(Color::WHITE),
                )
                .child(
                    text("Normal (400)")
                        .font_weight(FontWeight::NORMAL)
                        .color(Color::WHITE),
                )
                .child(
                    text("Medium (500)")
                        .font_weight(FontWeight::MEDIUM)
                        .color(Color::WHITE),
                )
                .child(
                    text("Semi-Bold (600)")
                        .font_weight(FontWeight::SEMI_BOLD)
                        .color(Color::WHITE),
                )
                .child(
                    text("Bold (700)")
                        .font_weight(FontWeight::BOLD)
                        .color(Color::WHITE),
                )
                .child(
                    text("Using .bold() shorthand")
                        .bold()
                        .color(Color::rgb(0.9, 0.7, 0.4)),
                ),
        )
}

/// Section demonstrating combined font family and weight
fn create_combined_section() -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            text("Combined Styling:")
                .font_size(16.0)
                .bold()
                .color(Color::rgb(0.7, 0.7, 0.8)),
        )
        .child(
            container()
                .padding(12.0)
                .background(Color::rgb(0.12, 0.12, 0.18))
                .corner_radius(8.0)
                .layout(Flex::column().spacing(8.0))
                .child(
                    text("Bold Monospace")
                        .mono()
                        .bold()
                        .color(Color::rgb(0.4, 0.8, 1.0)),
                )
                .child(
                    text("Light Serif")
                        .font_family(FontFamily::Serif)
                        .font_weight(FontWeight::LIGHT)
                        .color(Color::rgb(0.9, 0.8, 0.7)),
                )
                .child(
                    text("Bold Serif")
                        .font_family(FontFamily::Serif)
                        .bold()
                        .color(Color::rgb(1.0, 0.9, 0.8)),
                ),
        )
}

/// Section demonstrating text input with styling
fn create_text_input_section() -> Container {
    let input_value = create_signal("Type here...".to_string());

    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            text("Styled Text Input:")
                .font_size(16.0)
                .bold()
                .color(Color::rgb(0.7, 0.7, 0.8)),
        )
        .child(
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    text_input(input_value)
                        .mono()
                        .font_size(16.0)
                        .text_color(Color::rgb(0.4, 1.0, 0.6)),
                ),
        )
}
