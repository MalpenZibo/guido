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
    App::new().run(|app| {
        let username = create_signal(String::new());
        let password = create_signal(String::new());
        let submitted = create_signal(String::new());

        let view = container()
            .background(Color::rgb(0.12, 0.12, 0.18))
            .padding(24.0)
            .layout(Flex::column().spacing(16.0))
            .child(
                // Title
                container().child(text("Text Input Demo").color(Color::WHITE)),
            )
            .child(
                // Username section
                container()
                    .layout(Flex::column().spacing(4.0))
                    .child(container().child(text("Username").color(Color::rgb(0.7, 0.7, 0.8))))
                    .child(
                        container()
                            .width(at_least(300.0))
                            .padding(8.0)
                            .background(Color::rgb(0.18, 0.18, 0.24))
                            .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                            .corner_radius(6.0)
                            // Highlight border when text input is focused
                            .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                            .child(
                                text_input(username)
                                    .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                                    .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                    .color(Color::WHITE),
                            ),
                    ),
            )
            .child(
                // Password section
                container()
                    .layout(Flex::column().spacing(4.0))
                    .child(container().child(text("Password").color(Color::rgb(0.7, 0.7, 0.8))))
                    .child(
                        container()
                            .width(at_least(300.0))
                            .padding(8.0)
                            .background(Color::rgb(0.18, 0.18, 0.24))
                            .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                            .corner_radius(6.0)
                            // Highlight border when text input is focused
                            .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                            .child(
                                text_input(password)
                                    .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                                    .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                                    .color(Color::WHITE)
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
                        container().child(text("Current Values:").color(Color::rgb(0.6, 0.6, 0.7))),
                    )
                    .child(
                        container().child(
                            text(move || format!("Username: {}", username.get()))
                                .color(Color::rgb(0.8, 0.8, 0.9)),
                        ),
                    )
                    .child(
                        container().child(
                            text(move || format!("Password: {} chars", password.get().len()))
                                .color(Color::rgb(0.8, 0.8, 0.9)),
                        ),
                    ),
            )
            .child(
                // Submit status
                container().child(
                    text(move || {
                        let msg = submitted.get();
                        if msg.is_empty() {
                            "Press Enter in password field to submit".to_string()
                        } else {
                            msg
                        }
                    })
                    .color(Color::rgb(0.5, 0.8, 0.5)),
                ),
            )
            .child(
                // Instructions
                container()
                    .padding(12.0)
                    .background(Color::rgb(0.1, 0.1, 0.14))
                    .corner_radius(6.0)
                    .layout(Flex::column().spacing(4.0))
                    .child(
                        container()
                            .child(text("Keyboard shortcuts:").color(Color::rgb(0.5, 0.5, 0.6))),
                    )
                    .child(
                        container().child(
                            text("• Click to focus and position cursor")
                                .color(Color::rgb(0.5, 0.5, 0.6)),
                        ),
                    )
                    .child(container().child(
                        text("• Arrow keys to move cursor").color(Color::rgb(0.5, 0.5, 0.6)),
                    ))
                    .child(container().child(
                        text("• Shift+Arrow to select text").color(Color::rgb(0.5, 0.5, 0.6)),
                    ))
                    .child(
                        container()
                            .child(text("• Ctrl+A to select all").color(Color::rgb(0.5, 0.5, 0.6))),
                    )
                    .child(
                        container().child(
                            text("• Ctrl+Arrow for word jump").color(Color::rgb(0.5, 0.5, 0.6)),
                        ),
                    )
                    .child(container().child(
                        text("• Home/End to go to start/end").color(Color::rgb(0.5, 0.5, 0.6)),
                    ))
                    .child(
                        container().child(
                            text("• Enter to submit (in password field)")
                                .color(Color::rgb(0.5, 0.5, 0.6)),
                        ),
                    )
                    .child(container().child(
                        text("• Ctrl+C/X/V to copy/cut/paste").color(Color::rgb(0.5, 0.5, 0.6)),
                    ))
                    .child(
                        container().child(
                            text("• Ctrl+Z to undo, Ctrl+Y to redo")
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(13.0)
                                .font_size(13.0)
                                .font_size(14.0)
                                .font_size(14.0)
                                .font_size(20.0)
                                .font_size(12.0)
                                .font_size(12.0)
                                .font_size(12.0)
                                .font_size(13.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .font_size(11.0)
                                .color(Color::rgb(0.5, 0.5, 0.6)),
                        ),
                    ),
            );

        app.add_surface(
            SurfaceConfig::new()
                .width(400)
                .height(500)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("text-input-example")
                .background_color(Color::rgb(0.12, 0.12, 0.18)),
            move || view,
        );
    });
}
