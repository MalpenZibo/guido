//! Example demonstrating reactive widget properties and input events.
//!
//! This example shows:
//! - Reactive text content that updates automatically
//! - State layer API for hover/pressed states with ripple effects
//! - Scroll event handling
//! - Borders and gradient backgrounds
//! - Direct signal updates from callbacks and background threads

use std::thread;
use std::time::Duration;

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        // Create signals for reactive state
        let count = create_signal(0i32);
        let scroll_offset = create_signal(0.0f32);

        // Spawn a background thread that increments count every 2 seconds
        // Use .writer() to get a Send-able handle for the background thread
        let count_w = count.writer();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_secs(2));
                count_w.update(|c| *c += 1);
            }
        });

        // Run the app
        app.add_surface(
            SurfaceConfig::new()
                .height(32)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .layout(
                        Flex::row()
                            .spacing(8.0)
                            .main_alignment(MainAlignment::SpaceBetween),
                    )
                    .child(
                        // Clickable container with state layer - click to increment count
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.2, 0.2, 0.3))
                            .corner_radius(4.0)
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
                            .on_click(move || {
                                count.update(|c| *c += 10);
                            })
                            .child(
                                text(move || format!("Count: {} (click me!)", count.get()))
                                    .color(Color::WHITE),
                            ),
                    )
                    .child(
                        // Scrollable container with hover state
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.2, 0.3, 0.2))
                            .corner_radius(4.0)
                            .hover_state(|s| s.lighter(0.05))
                            .on_scroll(move |_dx, dy, _source| {
                                scroll_offset.update(|offset| {
                                    *offset += dy;
                                });
                            })
                            .child(
                                text(move || format!("Scroll: {:.0}px", scroll_offset.get()))
                                    .color(Color::WHITE),
                            ),
                    )
                    .child(
                        // Container with border and hover state
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.15, 0.15, 0.2))
                            .border(2.0, Color::rgb(0.4, 0.6, 0.8))
                            .hover_state(|s| s.lighter(0.1))
                            .child(text("With border").color(Color::WHITE)),
                    )
                    .child(
                        // Container with gradient and hover state
                        container()
                            .padding(8.0)
                            .gradient_horizontal(
                                Color::rgb(0.3, 0.1, 0.4),
                                Color::rgb(0.1, 0.3, 0.5),
                            )
                            .corner_radius(4.0)
                            .hover_state(|s| s.lighter(0.1))
                            .child(text("Gradient!").color(Color::WHITE)),
                    )
            },
        );
    });
}
