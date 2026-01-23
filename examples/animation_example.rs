use guido::prelude::*;

fn main() {
    // State signals for animations
    let expanded = create_signal(false);

    // Main view with animated cards in a 2-column grid
    let view = container()
        .background(Color::rgb(0.08, 0.08, 0.12))
        .padding(20.0)
        .layout(Flex::column().spacing(20.0))
        .children([
            // Row 1
            container().layout(Flex::row().spacing(20.0)).children([
                create_width_animation_card(expanded),
                create_color_animation_card(),
            ]),
            // Row 2
            container().layout(Flex::row().spacing(20.0)).children([
                create_combined_animation_card(),
                create_border_animation_card(),
            ]),
        ]);

    App::new()
        .width(800)
        .height(400)
        .background_color(Color::rgb(0.08, 0.08, 0.12))
        .run(view);
}

/// Card demonstrating width animation with spring physics
fn create_width_animation_card(expanded: Signal<bool>) -> Container {
    container()
        .width(move || at_least(if expanded.get() { 600.0 } else { 50.0 }))
        .animate_width(Transition::spring(SpringConfig::DEFAULT))
        .height(at_least(80.0))
        .background(Color::rgb(0.2, 0.25, 0.35))
        .corner_radius(12.0)
        .padding(20.0)
        .animate_background(Transition::new(150.0, TimingFunction::EaseOut))
        .hover_state(|s| s.lighter(0.08))
        .pressed_state(|s| s.ripple())
        .on_click(move || expanded.update(|e| *e = !*e))
        .child(
            container().layout(Flex::column().spacing(8.0)).children([
                text("Width Animation with Spring")
                    .font_size(18.0)
                    .color(Color::rgb(0.9, 0.9, 0.95)),
                text(move || {
                    if expanded.get() {
                        "Click to collapse (watch the spring bounce!)".to_string()
                    } else {
                        "Click to expand (watch the spring bounce!)".to_string()
                    }
                })
                .font_size(14.0)
                .color(Color::rgb(0.6, 0.6, 0.7)),
            ]),
        )
}

/// Card demonstrating background color animation with state layer
fn create_color_animation_card() -> Container {
    container()
        .width(at_least(400.0))
        .height(at_least(80.0))
        .background(Color::rgb(0.25, 0.2, 0.3))
        .animate_background(Transition::spring(SpringConfig::DEFAULT))
        .corner_radius(12.0)
        .padding(20.0)
        .hover_state(|s| s.background(Color::rgb(0.4, 0.25, 0.35)))
        .pressed_state(|s| s.ripple())
        .child(
            container().layout(Flex::column().spacing(8.0)).children([
                text("Background Color Animation")
                    .font_size(18.0)
                    .color(Color::rgb(0.9, 0.9, 0.95)),
                text("Hover to see smooth color transition")
                    .font_size(14.0)
                    .color(Color::rgb(0.6, 0.6, 0.7)),
            ]),
        )
}

/// Card demonstrating combined animations
fn create_combined_animation_card() -> Container {
    let clicked = create_signal(false);

    container()
        .width(move || at_least(if clicked.get() { 500.0 } else { 350.0 }))
        .animate_width(Transition::spring(SpringConfig::SNAPPY))
        .height(at_least(100.0))
        .background(Color::rgb(0.2, 0.25, 0.2))
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .corner_radius(move || if clicked.get() { 20.0 } else { 12.0 })
        .animate_corner_radius(Transition::new(250.0, TimingFunction::EaseInOut))
        .padding(20.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .on_click(move || clicked.update(|c| *c = !*c))
        .child(
            container().layout(Flex::column().spacing(8.0)).children([
                text("Combined Animations")
                    .font_size(18.0)
                    .color(Color::rgb(0.9, 0.9, 0.95)),
                text(move || {
                    if clicked.get() {
                        "Width, color, and corner radius all animating!".to_string()
                    } else {
                        "Click to see multiple properties animate together".to_string()
                    }
                })
                .font_size(14.0)
                .color(Color::rgb(0.6, 0.6, 0.7)),
            ]),
        )
}

/// Card demonstrating border animations
fn create_border_animation_card() -> Container {
    let clicked = create_signal(false);

    container()
        .width(at_least(400.0))
        .height(at_least(100.0))
        .background(Color::rgb(0.15, 0.15, 0.2))
        // Reactive border width: 2px normally, 6px when clicked
        .border(
            move || if clicked.get() { 6.0 } else { 2.0 },
            Color::rgb(0.4, 0.5, 0.7),
        )
        .animate_border_width(Transition::spring(SpringConfig::BOUNCY))
        .animate_border_color(Transition::new(300.0, TimingFunction::EaseOut))
        .corner_radius(12.0)
        .padding(20.0)
        .hover_state(|s| s.border(3.0, Color::rgb(0.4, 0.8, 0.6)))
        .pressed_state(|s| s.ripple())
        .on_click(move || clicked.update(|c| *c = !*c))
        .child(
            container().layout(Flex::column().spacing(8.0)).children([
                text("Border Animation")
                    .font_size(18.0)
                    .color(Color::rgb(0.9, 0.9, 0.95)),
                text(move || {
                    if clicked.get() {
                        "Border width and color animating! Click to reset.".to_string()
                    } else {
                        "Hover for color change, click for width + color".to_string()
                    }
                })
                .font_size(14.0)
                .color(Color::rgb(0.6, 0.6, 0.7)),
            ]),
        )
}
