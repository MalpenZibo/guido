//! Does attaching a size animation change how a capped container lays out?
//!
//! Each row below is the SAME container twice: on the left as written, on the
//! right with `.animate_width(...)` added and nothing else. The animation never
//! runs — there is no signal to move it — so the two ought to be identical.
//!
//! Run it on a build with the fix and the pairs match. Run it on one without
//! and the right-hand box of each pair is wrong: `at_most` bounded the
//! container but not what it told its children, so they were laid out against
//! the surface's full width and then the box was cut back to the cap.
//!
//! The dashed-looking outline is the cap. Anything drawn past it is content
//! that was measured against a width that never existed.
//!
//!     cargo run --example at_most_animation

use guido::prelude::*;

const CAP: f32 = 160.0;
const SENTENCE: &str = "una frase lunga che deve andare a capo dentro il box";

fn label(s: &str) -> Container {
    container()
        .text_color(Color::rgb(0.75, 0.78, 0.85))
        .font_size(12.0)
        .child(text(s))
}

fn heading(s: &str) -> Container {
    container()
        .text_color(Color::WHITE)
        .font_size(15.0)
        .child(text(s))
}

/// The cap, drawn as an outline so overflowing content is visible against it.
fn capped(animated: bool, body: impl Widget + 'static) -> Container {
    let c = container()
        .width(at_most(CAP))
        .border(1.0, Color::rgb(0.45, 0.5, 0.65))
        .corner_radius(4.0)
        .padding(6.0)
        // Visible, not Hidden: clipping would conceal exactly what we are here
        // to look at.
        .overflow(Overflow::Visible)
        .child(body);
    if animated {
        c.animate_width(Transition::new(200, TimingFunction::EaseOut))
    } else {
        c
    }
}

/// One case, shown twice, with its measured size printed underneath.
fn pair(title: &str, build: impl Fn(bool) -> Container + 'static) -> Container {
    let plain_ref = create_widget_ref();
    let anim_ref = create_widget_ref();

    let side = |caption: &str, w: Container, r: WidgetRef| {
        container()
            .width(240.0)
            .layout(Flex::column().spacing(6.0))
            .child(label(caption))
            .child(w.widget_ref(r))
            .child(
                container()
                    .text_color(Color::rgb(0.55, 0.85, 0.6))
                    .font_size(12.0)
                    .child(text(move || {
                        let b = r.rect().get();
                        format!("{:.1} x {:.1}", b.width, b.height)
                    })),
            )
    };

    container()
        .layout(Flex::column().spacing(10.0))
        .child(heading(title))
        .child(
            container()
                .layout(Flex::row().spacing(24.0))
                .child(side("come scritto", build(false), plain_ref))
                .child(side("+ .animate_width()", build(true), anim_ref)),
        )
}

fn main() {
    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(content())
                .height(content())
                .anchor(Anchor::TOP | Anchor::LEFT)
                .margin(40, 0, 0, 40)
                .layer(Layer::Overlay)
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .namespace("guido-at-most")
                .background_color(Color::rgb(0.09, 0.09, 0.13)),
            move || {
                container()
                    .padding(20.0)
                    .layout(Flex::column().spacing(28.0))
                    .child(
                        container()
                            .text_color(Color::WHITE)
                            .font_size(17.0)
                            .child(text(
                                "at_most + animazione: le coppie devono essere identiche",
                            )),
                    )
                    .child(pair("testo che va a capo", |animated| {
                        capped(
                            animated,
                            container()
                                .text_color(Color::WHITE)
                                .font_size(13.0)
                                .child(text(SENTENCE)),
                        )
                    }))
                    .child(pair("figlio con fill()", |animated| {
                        capped(
                            animated,
                            container()
                                .width(fill())
                                .height(22.0)
                                .background(Color::rgb(0.85, 0.35, 0.35))
                                .corner_radius(3.0),
                        )
                    }))
                    .child(label("Esc per chiudere").text_color(Color::rgb(0.5, 0.53, 0.6)))
                    .on_key_down(|key, _| {
                        if key == Key::Escape {
                            quit_app();
                        }
                    })
            },
        );
    });
}
