//! Multi-surface example demonstrating multiple layer shell surfaces.
//!
//! This example shows how to create multiple surfaces at startup:
//! - A top status bar
//! - A bottom dock
//!
//! Both surfaces share the same reactive signals.

use guido::prelude::*;

fn main() {
    // Shared signal that can be updated from any surface
    let count = create_signal(0);

    let (app, _bar_id) = App::new().add_surface(
        // Top status bar
        SurfaceConfig::new()
            .height(32)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .layer(Layer::Top)
            .namespace("status-bar")
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move |_| {
            container()
                .height(fill())
                .layout(
                    Flex::row()
                        .spacing(8.0)
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .padding_xy(16.0, 0.0)
                .child(
                    text("Multi-Surface Demo")
                        .color(Color::WHITE)
                        .font_size(14.0),
                )
                .child(
                    text(move || format!("Count: {}", count.get()))
                        .color(Color::WHITE)
                        .font_size(14.0),
                )
        },
    );
    // Bottom dock
    let (app, _dock_id) = app.add_surface(
        SurfaceConfig::new()
            .height(48)
            .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
            .layer(Layer::Top)
            .namespace("dock")
            .background_color(Color::rgb(0.15, 0.15, 0.2)),
        move |_| {
            container()
                .height(fill())
                .layout(
                    Flex::row()
                        .spacing(16.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .children([
                    container()
                        .background(Color::rgb(0.3, 0.3, 0.4))
                        .padding_xy(16.0, 8.0)
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || count.update(|c| *c += 1))
                        .child(text("+").color(Color::WHITE).font_size(16.0)),
                    container()
                        .background(Color::rgb(0.3, 0.3, 0.4))
                        .padding_xy(16.0, 8.0)
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || count.update(|c| *c -= 1))
                        .child(text("-").color(Color::WHITE).font_size(16.0)),
                    container()
                        .background(Color::rgb(0.4, 0.2, 0.2))
                        .padding_xy(16.0, 8.0)
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || count.set(0))
                        .child(text("Reset").color(Color::WHITE).font_size(16.0)),
                ])
        },
    );
    app.run();
}
