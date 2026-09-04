//! A panel that can be switched off, and what that does to everything in it.
//!
//! **Busy** is outside the panel and always works. Turn it on and the panel
//! goes dead from one declaration: the button stops responding, the field stops
//! taking keys, and both grey themselves out from the same signal that stopped
//! the events — that is what `when_disabled` is for.
//!
//! Two things to look for while it is off. The button does not light up under
//! the pointer, and the field's focus ring goes out: a disabled subtree is
//! neither under the pointer nor where the keyboard is aimed.
//!
//! The **Frozen** field below is the other half, and the contrast is the point.
//! It is `readonly`, not disabled: it refuses every edit and keeps its ring, so
//! it goes on saying the keyboard is aimed at it. That is what a lock screen
//! wants while PAM answers.
//!
//! The nested `enabled(true)` on the button is deliberate: it re-enables
//! nothing. Disabling propagates down and a descendant cannot undo it.

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(420)
                .height(250)
                .anchor(Anchor::TOP)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || {
                let busy = create_signal(false);
                let value = create_signal(String::new());
                let frozen = create_signal("you cannot change this".to_owned());
                let sent = create_signal(0);

                container()
                    .padding(16.0)
                    .layout(Flex::column().spacing(12.0))
                    .child(
                        container()
                            .padding(8.0)
                            .corners(6.0)
                            .background(Color::rgb(0.25, 0.25, 0.3))
                            .when_hovered(|s| s.lighter(0.1))
                            .on_click(move || busy.update(|b| *b = !*b))
                            .child(
                                text(move || {
                                    if busy.get() {
                                        "Busy — click to let the form work".to_owned()
                                    } else {
                                        "Idle — click to switch the form off".to_owned()
                                    }
                                })
                                .color(Color::WHITE),
                            ),
                    )
                    .child(
                        container()
                            .padding(12.0)
                            .corners(8.0)
                            .background(Color::rgb(0.16, 0.16, 0.22))
                            .layout(Flex::column().spacing(10.0))
                            // The one declaration the whole panel hangs from.
                            .enabled(move || !busy.get())
                            .child(
                                container()
                                    .padding(8.0)
                                    .corners(6.0)
                                    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                                    .when_focused(|s| s.border(1.0, Color::rgb(0.4, 0.8, 1.0)))
                                    .child(
                                        text_input(value)
                                            .placeholder("Say something")
                                            .when_disabled(|s| {
                                                s.color(Color::rgb(0.45, 0.45, 0.5))
                                            }),
                                    ),
                            )
                            .child(
                                container()
                                    .padding(10.0)
                                    .corners(6.0)
                                    .background(Color::rgb(0.2, 0.4, 0.7))
                                    // Says nothing: the panel above already said no.
                                    .enabled(true)
                                    .when_hovered(|s| s.lighter(0.15))
                                    .when_disabled(|s| s.darker(0.4))
                                    .on_click(move || sent.update(|n| *n += 1))
                                    .child(
                                        text("Send").color(Color::WHITE).when_disabled(|s| {
                                            s.color(Color::rgb(0.55, 0.55, 0.6))
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        container()
                            .padding(8.0)
                            .corners(6.0)
                            .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                            .when_focused(|s| s.border(1.0, Color::rgb(0.4, 0.8, 1.0)))
                            .child(
                                text_input(frozen)
                                    .placeholder("Frozen — click in, then type")
                                    .readonly(true),
                            ),
                    )
                    .child(text(move || format!("sent {} times", sent.get())))
            },
        );
    });
}
