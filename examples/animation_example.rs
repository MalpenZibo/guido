use guido::prelude::*;

fn main() {
    // State signals for animations
    let expanded = create_signal(false);
    let hovered = create_signal(false);
    let bg_hovered = create_signal(false);

    // Main view with animated cards
    let view = container()
        .background(Color::rgb(0.08, 0.08, 0.12))
        .padding(20.0)
        .layout(Flex::column().spacing(20.0))
        .children([
            // Card 1: Width animation with spring
            create_width_animation_card(expanded, hovered),
            // Card 2: Background color animation
            create_color_animation_card(bg_hovered),
            // Card 3: Combined animations
            create_combined_animation_card(),
        ]);

    App::new()
        .width(800)
        .height(600)
        .background_color(Color::rgb(0.08, 0.08, 0.12))
        .run(view);
}

/// Card demonstrating width animation with spring physics
fn create_width_animation_card(expanded: Signal<bool>, hovered: Signal<bool>) -> Container {
    container()
        .min_width(move || if expanded.get() { 600.0 } else { 300.0 })
        .animate_width(Transition::spring(SpringConfig::DEFAULT))
        .min_height(80.0)
        .background(move || {
            if hovered.get() {
                Color::rgb(0.25, 0.3, 0.4)
            } else {
                Color::rgb(0.2, 0.25, 0.35)
            }
        })
        .corner_radius(12.0)
        .padding(20.0)
        .on_click(move || expanded.update(|e| *e = !*e))
        .on_hover(move |h| hovered.set(h))
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

/// Card demonstrating background color animation
fn create_color_animation_card(hovered: Signal<bool>) -> Container {
    container()
        .min_width(400.0)
        .min_height(80.0)
        .background(move || {
            if hovered.get() {
                Color::rgb(0.4, 0.25, 0.35)
            } else {
                Color::rgb(0.25, 0.2, 0.3)
            }
        })
        .animate_background(Transition::spring(SpringConfig::DEFAULT))
        .corner_radius(12.0)
        .padding(20.0)
        .on_hover(move |h| hovered.set(h))
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
    let local_hovered = create_signal(false);

    container()
        .min_width(move || if clicked.get() { 500.0 } else { 350.0 })
        .animate_width(Transition::spring(SpringConfig::SNAPPY))
        .min_height(100.0)
        .background(move || {
            if local_hovered.get() {
                Color::rgb(0.3, 0.35, 0.25)
            } else {
                Color::rgb(0.2, 0.25, 0.2)
            }
        })
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .corner_radius(move || if clicked.get() { 20.0 } else { 12.0 })
        .animate_corner_radius(Transition::new(250.0, TimingFunction::EaseInOut))
        .padding(20.0)
        .on_click(move || clicked.update(|c| *c = !*c))
        .on_hover(move |h| local_hovered.set(h))
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
