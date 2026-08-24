//! Session lock example (`ext-session-lock-v1`).
//!
//! **WARNING: running this locks your session.** Unlock by typing the
//! password `guido` (plus Enter) in the password field, or wait for the
//! 30-second auto-unlock safety timer.
//!
//! A small bar hosts a "Lock session" button. Locking creates one lock
//! surface per output with a password field; the compositor blanks every
//! output and routes keyboard input to the lock surfaces.

use std::time::Duration;

use guido::prelude::*;

const PASSWORD: &str = "guido";

fn lock_screen(output: OutputInfo) -> Container {
    let attempt = create_signal(String::new());
    let error = create_signal(false);

    // Safety net for an example: never leave the user locked out.
    let auto_unlock = create_signal(false);
    let auto_unlock_w = auto_unlock.writer();
    create_task(move |ctx| async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        if ctx.is_running() {
            auto_unlock_w.set(true);
        }
    });
    create_effect(move || {
        if auto_unlock.get() {
            unlock_session();
        }
    });

    container()
        .width(fill())
        .height(fill())
        .background(Color::rgb(0.07, 0.07, 0.1))
        .layout(
            Flex::column()
                .spacing(16.0)
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(
            container().child(
                text(format!(
                    "Locked — {}",
                    output.name.unwrap_or_else(|| "output".into())
                ))
                .color(Color::WHITE),
            ),
        )
        .child(container().child(
            text("password: guido (auto-unlocks after 30s)").color(Color::rgb(0.6, 0.6, 0.7)),
        ))
        .child(
            container()
                .width(at_least(280.0))
                .padding(10.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
                .child(
                    text_input(attempt)
                        .color(Color::WHITE)
                        .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                        .password(true)
                        .on_submit(move |s| {
                            if s == PASSWORD {
                                unlock_session();
                            } else {
                                error.set(true);
                            }
                        }),
                ),
        )
        .child(text(move || {
            if error.get() {
                "Wrong password".to_string()
            } else {
                String::new()
            }
        }))
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .height(40)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                container()
                    .width(fill())
                    .layout(
                        Flex::row()
                            .spacing(12.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        container()
                            .padding([6.0, 16.0])
                            .background(Color::rgb(0.3, 0.2, 0.2))
                            .corner_radius(6.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(|| lock_session(lock_screen))
                            .child(text("Lock session")),
                    )
                    .child(
                        text(move || format!("state: {:?}", lock_state().get()))
                            .font_size(13.0)
                            .font_size(24.0),
                    )
            },
        );
    });
}
