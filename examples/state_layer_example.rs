//! State Layer Example
//!
//! Demonstrates the state layer API for hover and pressed style overrides,
//! including ripple effects.
//! Run with: cargo run --example state_layer_example

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(700)
                .height(400)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .background_color(Color::rgb(0.08, 0.08, 0.12)),
            || {
                container()
                .background(Color::rgb(0.08, 0.08, 0.12))
                .padding(24.0)
                .layout(Flex::column().spacing(16.0))
                .child(
                    // Title section
                    container().layout(Flex::column().spacing(8.0)).children([
                        container().font_size(24.0).text_color(Color::rgb(0.9, 0.9, 0.95)).child(text("State Layer Demo")),
                        container().font_size(14.0).text_color(Color::rgb(0.6, 0.6, 0.7)).child(text("Hover and click the buttons to see state changes and ripple effects")),
                    ]),
                )
                .child(
                    // Buttons container - two columns
                    container().layout(Flex::row().spacing(16.0)).children([
                        // Left column - basic effects
                        container().layout(Flex::column().spacing(12.0)).children([
                            create_lighter_button(),
                            create_explicit_colors_button(),
                            create_transform_button(),
                            create_animated_button(),
                            create_border_button(),
                        ]),
                        // Right column - ripple effects
                        container().layout(Flex::column().spacing(12.0)).children([
                            create_ripple_button(),
                            create_colored_ripple_button(),
                            create_ripple_with_scale_button(),
                            create_rotated_ripple_button(),
                        ]),
                    ]),
                )
            },
        );
    });
}

/// Button with lighter() hover effect
fn create_lighter_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(8.0)
        .when_hovered(|s| s.lighter(0.1).text_color(Color::WHITE))
        // The state layer reaches the glyphs, not just the box: the label
        // brightens with the background, and eases there with it.
        .text_color(Color::rgb(0.6, 0.6, 0.66))
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_text_color(Transition::new(200.0, TimingFunction::EaseOut))
        .child(text("Hover me (lighter + text)"))
}

/// Button with explicit hover and pressed colors
fn create_explicit_colors_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .when_hovered(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))
        .when_pressed(|s| s.background(Color::rgb(0.2, 0.4, 0.7)))
        .text_color(Color::WHITE)
        .child(text("Click me (explicit colors)"))
}

/// Button with transform on press
fn create_transform_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.8, 0.3, 0.3))
        .corner_radius(8.0)
        .when_hovered(|s| s.lighter(0.05))
        .when_pressed(|s| s.darker(0.1).transform(Transform::scale(0.98)))
        .text_color(Color::WHITE)
        .child(text("Press me (scale down)"))
}

/// Button with smooth animated transitions
fn create_animated_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.6, 0.4))
        .corner_radius(8.0)
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .when_hovered(|s| s.lighter(0.15))
        .when_pressed(|s| s.darker(0.1))
        .text_color(Color::WHITE)
        .child(text("Animated transitions"))
}

/// Button with border changes on hover/press
fn create_border_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.15, 0.15, 0.2))
        .corner_radius(8.0)
        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))
        .animate_border_color(Transition::new(150.0, TimingFunction::EaseOut))
        .when_hovered(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
        .when_pressed(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))
        .text_color(Color::rgb(0.8, 0.8, 0.85))
        .child(text("Border changes"))
}

/// Button with default ripple effect
fn create_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(8.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
        .text_color(Color::rgb(0.9, 0.9, 0.95))
        .child(text("Default ripple"))
}

/// Button with colored ripple effect
fn create_colored_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
        .text_color(Color::WHITE)
        .child(text("Yellow ripple"))
}

/// Button with ripple and scale transform
fn create_ripple_with_scale_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.6, 0.3, 0.5))
        .corner_radius(8.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple().transform(Transform::scale(0.98)))
        .text_color(Color::WHITE)
        .child(text("Ripple + scale"))
}

/// Button with rotation and translation to test transformed ripple
fn create_rotated_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.4, 0.6, 0.4))
        .corner_radius(8.0)
        .transform(Transform::rotate_degrees(5.0).then(&Transform::translate(10.0, 15.0)))
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 1.0, 1.0, 0.5)))
        .text_color(Color::WHITE)
        .child(text("Rotated + translated"))
}
