//! Hold a key and watch it repeat.
//!
//! Every key-down is printed with its repeat flag, and the panel counts the
//! presses and the repeats apart. Holding a key should print one
//! `repeat=false` and then a run of `repeat=true`, and releasing it should end
//! the run.
//!
//! A repeat comes from one of two places: the toolkit's own timer, armed from
//! the compositor's `repeat_info`, or the compositor itself sending the key
//! again in `wl_keyboard`'s repeated state (version 10). `WAYLAND_DEBUG=1`
//! tells them apart — a protocol repeat is a `wl_keyboard.key` event whose
//! state is 2, a timer repeat puts nothing on the wire.
//!
//!     WAYLAND_DEBUG=1 cargo run --example key_repeat 2>&1 | grep -E 'wl_keyboard|repeat='
//!
//! Escape quits.

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        let presses = create_signal(0u32);
        let repeats = create_signal(0u32);
        app.add_surface(
            SurfaceConfig::new()
                .width(content())
                .height(content())
                .anchor(Anchor::TOP | Anchor::LEFT)
                .margin([40, 0, 0, 40])
                .layer(Layer::Overlay)
                .keyboard_interactivity(KeyboardInteractivity::Exclusive)
                .namespace("guido-key-repeat")
                .background_color(Color::rgb(0.09, 0.09, 0.13)),
            move || {
                container()
                    .padding(20.0)
                    .child(
                        text(move || {
                            format!(
                                "presses {}  repeats {}  (hold a key, Esc quits)",
                                presses.get(),
                                repeats.get()
                            )
                        })
                        .font_size(15.0)
                        .color(Color::WHITE),
                    )
                    .on_key_down(move |key, _, repeat| {
                        println!("{key:?} repeat={repeat}");
                        if key == Key::Escape {
                            quit_app();
                        } else if repeat {
                            repeats.update(|n| *n += 1);
                        } else {
                            presses.update(|n| *n += 1);
                        }
                    })
            },
        );
    });
}
