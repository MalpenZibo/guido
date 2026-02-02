//! State Layer Example
//!
//! Demonstrates the state layer API for hover and pressed style overrides,
//! including ripple effects.
//! Run with: cargo run --example state_layer_example

use guido::prelude::*;

fn main() {
    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(700)
            .height(400)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.08, 0.08, 0.12)),
        |_| {
            container()
                .background(Color::rgb(0.08, 0.08, 0.12))
                .padding(24.0)
                .layout(Flex::column().spacing(16.0))
                .child(
                    // Title section
                    container().layout(Flex::column().spacing(8.0)).children([
                        text("State Layer Demo")
                            .font_size(24.0)
                            .color(Color::rgb(0.9, 0.9, 0.95)),
                        text("Hover and click the buttons to see state changes and ripple effects")
                            .font_size(14.0)
                            .color(Color::rgb(0.6, 0.6, 0.7)),
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
    app.run();
}

/// Button with lighter() hover effect
fn create_lighter_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.1))
        .child(text("Hover me (lighter)").color(Color::rgb(0.9, 0.9, 0.95)))
}

/// Button with explicit hover and pressed colors
fn create_explicit_colors_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .hover_state(|s| s.background(Color::rgb(0.4, 0.6, 0.9)))
        .pressed_state(|s| s.background(Color::rgb(0.2, 0.4, 0.7)))
        .child(text("Click me (explicit colors)").color(Color::WHITE))
}

/// Button with transform on press
fn create_transform_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.8, 0.3, 0.3))
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.05))
        .pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
        .child(text("Press me (scale down)").color(Color::WHITE))
}

/// Button with smooth animated transitions
fn create_animated_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.6, 0.4))
        .corner_radius(8.0)
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .hover_state(|s| s.lighter(0.15))
        .pressed_state(|s| s.darker(0.1))
        .child(text("Animated transitions").color(Color::WHITE))
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
        .hover_state(|s| s.border(2.0, Color::rgb(0.5, 0.5, 0.6)))
        .pressed_state(|s| s.border(3.0, Color::rgb(0.7, 0.7, 0.8)))
        .child(text("Border changes").color(Color::rgb(0.8, 0.8, 0.85)))
}

/// Button with default ripple effect
fn create_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .child(text("Default ripple").color(Color::rgb(0.9, 0.9, 0.95)))
}

/// Button with colored ripple effect
fn create_colored_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple_with_color(Color::rgba(1.0, 0.8, 0.0, 0.4)))
        .child(text("Yellow ripple").color(Color::WHITE))
}

/// Button with ripple and scale transform
fn create_ripple_with_scale_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.6, 0.3, 0.5))
        .corner_radius(8.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple().transform(Transform::scale(0.98)))
        .child(text("Ripple + scale").color(Color::WHITE))
}

/// Button with rotation and translation to test transformed ripple
fn create_rotated_ripple_button() -> Container {
    container()
        .padding(16.0)
        .background(Color::rgb(0.4, 0.6, 0.4))
        .corner_radius(8.0)
        .transform(Transform::rotate_degrees(5.0).then(&Transform::translate(10.0, 15.0)))
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple_with_color(Color::rgba(1.0, 1.0, 1.0, 0.5)))
        .child(text("Rotated + translated").color(Color::WHITE))
}
