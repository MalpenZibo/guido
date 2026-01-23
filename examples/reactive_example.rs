//! Example demonstrating reactive widget properties and input events.
//!
//! This example shows:
//! - Reactive text content that updates automatically
//! - Reactive colors that change based on signals
//! - Click, hover, and scroll event handling
//! - Borders and gradient backgrounds
//! - Direct signal updates from callbacks and background threads

use std::thread;
use std::time::Duration;

use guido::prelude::*;

fn main() {
    // Create signals for reactive state
    let count = create_signal(0i32);
    let scroll_offset = create_signal(0.0f32);
    let hover_color = create_signal(Color::rgb(0.2, 0.2, 0.3));

    // Spawn a background thread that increments count every 2 seconds
    // No need to clone signals anymore - they implement Copy!
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        count.update(|c| *c += 1);
    });

    // Build the reactive UI with event handlers
    let view = container()
        .layout(
            Flex::row()
                .spacing(8.0)
                .main_axis_alignment(MainAxisAlignment::SpaceBetween),
        )
        .child(
            // Clickable container - click to increment count
            container()
                .padding(8.0)
                .background(hover_color)
                .corner_radius(4.0)
                .on_click(move || {
                    count.update(|c| *c += 10);
                })
                .on_hover(move |hovered| {
                    if hovered {
                        hover_color.set(Color::rgb(0.3, 0.3, 0.4));
                    } else {
                        hover_color.set(Color::rgb(0.2, 0.2, 0.3));
                    }
                })
                .child(
                    text(move || format!("Count: {} (click me!)", count.get())).color(Color::WHITE),
                ),
        )
        .child(
            // Scrollable container
            container()
                .padding(8.0)
                .background(Color::rgb(0.2, 0.3, 0.2))
                .corner_radius(4.0)
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
            // Container with border
            container()
                .padding(8.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .border(2.0, Color::rgb(0.4, 0.6, 0.8))
                .child(text("With border").color(Color::WHITE)),
        )
        .child(
            // Container with gradient
            container()
                .padding(8.0)
                .gradient_horizontal(Color::rgb(0.3, 0.1, 0.4), Color::rgb(0.1, 0.3, 0.5))
                .corner_radius(4.0)
                .child(text("Gradient!").color(Color::WHITE)),
        );

    // Run the app
    App::new()
        .height(32)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
