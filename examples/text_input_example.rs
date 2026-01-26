//! Text Input Example
//!
//! Demonstrates the TextInput widget with:
//! - Basic text input
//! - Password field with masking
//! - Real-time display of input values
//! - Submit handling with Enter key
//! - Focused state styling on input containers
//! - Clipboard support (Ctrl+C/V/X)
//! - Undo/redo history (Ctrl+Z/Y)

use guido::prelude::*;

fn main() {
    let username = create_signal(String::new());
    let password = create_signal(String::new());
    let submitted = create_signal(String::new());

    let view = container()
        .background(Color::rgb(0.12, 0.12, 0.18))
        .padding(24.0)
        .layout(Flex::column().spacing(16.0))
        .child(
            // Title
            text("Text Input Demo").color(Color::WHITE).font_size(20.0),
        )
        .child(
            // Username section
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Username")
                        .color(Color::rgb(0.7, 0.7, 0.8))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(6.0)
                        // Highlight border when text input is focused
                        .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                        .child(
                            text_input(username)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                                .font_size(14.0),
                        ),
                ),
        )
        .child(
            // Password section
            container()
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Password")
                        .color(Color::rgb(0.7, 0.7, 0.8))
                        .font_size(12.0),
                )
                .child(
                    container()
                        .width(at_least(300.0))
                        .padding(8.0)
                        .background(Color::rgb(0.18, 0.18, 0.24))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(6.0)
                        // Highlight border when text input is focused
                        .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                        .child(
                            text_input(password)
                                .text_color(Color::WHITE)
                                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                                .font_size(14.0)
                                .password(true)
                                .on_submit(move |_| {
                                    let msg = format!("Login attempt: {}", username.get());
                                    submitted.set(msg);
                                }),
                        ),
                ),
        )
        .child(
            // Current values display
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(6.0)
                .layout(Flex::column().spacing(8.0))
                .child(
                    text("Current Values:")
                        .color(Color::rgb(0.6, 0.6, 0.7))
                        .font_size(12.0),
                )
                .child(
                    text(move || format!("Username: {}", username.get()))
                        .color(Color::rgb(0.8, 0.8, 0.9))
                        .font_size(13.0),
                )
                .child(
                    text(move || format!("Password: {} chars", password.get().len()))
                        .color(Color::rgb(0.8, 0.8, 0.9))
                        .font_size(13.0),
                ),
        )
        .child(
            // Submit status
            text(move || {
                let msg = submitted.get();
                if msg.is_empty() {
                    "Press Enter in password field to submit".to_string()
                } else {
                    msg
                }
            })
            .color(Color::rgb(0.5, 0.8, 0.5))
            .font_size(13.0),
        )
        .child(
            // Instructions
            container()
                .padding(12.0)
                .background(Color::rgb(0.1, 0.1, 0.14))
                .corner_radius(6.0)
                .layout(Flex::column().spacing(4.0))
                .child(
                    text("Keyboard shortcuts:")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Click to focus and position cursor")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Arrow keys to move cursor")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Shift+Arrow to select text")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Ctrl+A to select all")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Ctrl+Arrow for word jump")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Home/End to go to start/end")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Enter to submit (in password field)")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Ctrl+C/X/V to copy/cut/paste")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                )
                .child(
                    text("• Ctrl+Z to undo, Ctrl+Y to redo")
                        .color(Color::rgb(0.5, 0.5, 0.6))
                        .font_size(11.0),
                ),
        );

    App::new()
        .width(400)
        .height(500)
        .anchor(Anchor::TOP | Anchor::LEFT)
        .layer(Layer::Top)
        .namespace("text-input-example")
        .background_color(Color::rgb(0.12, 0.12, 0.18))
        .run(view);
}
