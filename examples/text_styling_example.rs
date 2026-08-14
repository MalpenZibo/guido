//! Text Styling Example
//!
//! Text carries content; how it looks is declared on an enclosing container and
//! inherited by everything below it, down to the nearest container that
//! overrides that particular property.
//!
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
                        // Declared once for the whole screen. Every sample
                        // below inherits it through the sections and cards,
                        // and overrides only what it is demonstrating.
                        .text_color(Color::WHITE)
                        .child(
                            container()
                                .font_size(28.0)
                                .bold()
                                .text_color(Color::rgb(0.9, 0.9, 0.95))
                                .child(text("Text Styling Demo")),
                        )
                        .child(create_font_family_section())
                        .child(create_font_weight_section())
                        .child(create_combined_section())
                        .child(create_text_input_section())
                },
            );
        });
}

/// A section heading — the one place its style is written.
fn heading(label: &str) -> Container {
    container()
        .font_size(16.0)
        .bold()
        .text_color(Color::rgb(0.7, 0.7, 0.8))
        .child(text(label))
}

/// The card each section's samples sit in. It declares nothing about text, so
/// the colour set at the root passes straight through it.
fn card() -> Container {
    container()
        .padding(12.0)
        .background(Color::rgb(0.12, 0.12, 0.18))
        .corner_radius(8.0)
        .layout(Flex::column().spacing(8.0))
}

/// Section demonstrating different font families
fn create_font_family_section() -> Container {
    let sample = |label: &'static str, family: FontFamily| {
        container().font_family(family).child(text(label))
    };

    container()
        .layout(Flex::column().spacing(8.0))
        .child(heading("Font Families:"))
        .child(
            card()
                .child(sample("Sans-Serif (default)", FontFamily::SansSerif))
                .child(sample("Serif font family", FontFamily::Serif))
                .child(sample("Monospace font family", FontFamily::Monospace))
                .child(
                    container()
                        .mono()
                        // Overrides only the colour: the family comes from
                        // .mono() here and the size from the root.
                        .text_color(Color::rgb(0.6, 0.9, 0.6))
                        .child(text("Using .mono() shorthand")),
                ),
        )
}

/// Section demonstrating different font weights
fn create_font_weight_section() -> Container {
    let sample = |label: &'static str, weight: FontWeight| {
        container().font_weight(weight).child(text(label))
    };

    container()
        .layout(Flex::column().spacing(8.0))
        .child(heading("Font Weights:"))
        .child(
            card()
                .child(sample("Thin (100)", FontWeight::THIN))
                .child(sample("Light (300)", FontWeight::LIGHT))
                .child(sample("Normal (400)", FontWeight::NORMAL))
                .child(sample("Medium (500)", FontWeight::MEDIUM))
                .child(sample("Semi-Bold (600)", FontWeight::SEMI_BOLD))
                .child(sample("Bold (700)", FontWeight::BOLD))
                .child(
                    container()
                        .bold()
                        .text_color(Color::rgb(0.9, 0.7, 0.4))
                        .child(text("Using .bold() shorthand")),
                ),
        )
}

/// Section demonstrating combined font family and weight
fn create_combined_section() -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(heading("Combined Styling:"))
        .child(
            card()
                .child(
                    container()
                        .mono()
                        .bold()
                        .text_color(Color::rgb(0.4, 0.8, 1.0))
                        .child(text("Bold Monospace")),
                )
                .child(
                    container()
                        .font_family(FontFamily::Serif)
                        .font_weight(FontWeight::LIGHT)
                        .text_color(Color::rgb(0.9, 0.8, 0.7))
                        .child(text("Light Serif")),
                )
                .child(
                    container()
                        .font_family(FontFamily::Serif)
                        .bold()
                        .text_color(Color::rgb(1.0, 0.9, 0.8))
                        .child(text("Bold Serif")),
                ),
        )
}

/// Section demonstrating text input with styling
///
/// The input reads the same declarations a `text` would: there is no separate
/// styling vocabulary for it, and `cursor_color` would be declared alongside.
fn create_text_input_section() -> Container {
    let input_value = create_signal("Type here...".to_string());

    container()
        .layout(Flex::column().spacing(8.0))
        .child(heading("Styled Text Input:"))
        .child(
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .mono()
                .font_size(16.0)
                .text_color(Color::rgb(0.4, 1.0, 0.6))
                .child(text_input(input_value)),
        )
}
