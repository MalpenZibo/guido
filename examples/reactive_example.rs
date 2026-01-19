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

    // Clone signals for the background thread
    let count_for_thread = count.clone();

    // Clone signals for the UI closures
    let count_for_text = count.clone();
    let count_for_click = count.clone();
    let scroll_for_text = scroll_offset.clone();
    let scroll_for_callback = scroll_offset.clone();
    let hover_for_bg = hover_color.clone();
    let hover_for_callback = hover_color.clone();

    // Spawn a background thread that increments count every 2 seconds
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(2));
        count_for_thread.update(|c| *c += 1);
    });

    // Build the reactive UI with event handlers
    let view = row![
        // Clickable container with ripple effect - click to increment count
        container()
            .padding(8.0)
            .background(hover_for_bg)
            .corner_radius(4.0)
            .ripple() // Enable ripple effect on hover
            .on_click(move || {
                count_for_click.update(|c| *c += 10);
            })
            .on_hover(move |hovered| {
                if hovered {
                    hover_for_callback.set(Color::rgb(0.3, 0.3, 0.4));
                } else {
                    hover_for_callback.set(Color::rgb(0.2, 0.2, 0.3));
                }
            })
            .child(
                text(move || format!("Count: {} (click me!)", count_for_text.get()))
                    .color(Color::WHITE),
            ),
        // Scrollable container
        container()
            .padding(8.0)
            .background(Color::rgb(0.2, 0.3, 0.2))
            .corner_radius(4.0)
            .on_scroll(move |_dx, dy, _source| {
                scroll_for_callback.update(|offset| {
                    *offset += dy;
                });
            })
            .child(
                text(move || format!("Scroll: {:.0}px", scroll_for_text.get())).color(Color::WHITE),
            ),
        // Container with border
        container()
            .padding(8.0)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .border(2.0, Color::rgb(0.4, 0.6, 0.8))
            .child(text("With border").color(Color::WHITE)),
        // Container with gradient
        container()
            .padding(8.0)
            .gradient_horizontal(Color::rgb(0.3, 0.1, 0.4), Color::rgb(0.1, 0.3, 0.5),)
            .corner_radius(4.0)
            .child(text("Gradient!").color(Color::WHITE)),
    ]
    .spacing(8.0)
    .main_axis_alignment(MainAxisAlignment::SpaceBetween);

    // Run the app
    App::new()
        .height(32)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
