//! Text as frosted glass: the glyphs are the window.
//!
//! Run with: cargo run --example frosted_text
//!
//! A container's `backdrop_blur` softens what is behind a box. On a text the
//! same declaration cuts the shape of the letters out of the blur instead, so
//! the picture shows through them, out of focus — CSS's `backdrop-filter`
//! together with `background-clip: text`.
//!
//! The colour is the tint laid over the glass, which is why every panel here
//! but the first is translucent. The fourth adds a stroke, which over frost is
//! drawn as a true contour — outside the letter, leaving the glass alone.
//!
//! The fifth panel is the trap, and it is a shadow: shadows are still copies of
//! the glyphs drawn *under* the fill, so they cover the letter's own area and
//! not only its edge. Under an opaque fill that is invisible and free; over
//! frost it is an opaque letter where the picture should be.
//!
//! The second row is the same declaration under a transform. The coverage is
//! cut in the text's own space and read back through the transform, so the
//! frost turns and stretches with the letters — which is what a hover that
//! grows a card, or an `animate_transform` on a frosted label, needs. Until
//! #199 all four of these showed sharp letters over a sharp photograph: the
//! effect was switched off for the length of the animation and came back when
//! it settled.

use guido::prelude::*;

const LABEL: &str = "09:41";

fn photo() -> Container {
    container().width(fill()).height(fill()).child(
        image(ImageSource::Path("examples/assets/photo.webp".into()))
            .content_fit(ContentFit::Cover),
    )
}

/// The frosted reading, at the size every panel shows it.
fn reading(label: Text) -> Container {
    container()
        .width(fill())
        .height(fill())
        .layout(
            Flex::column()
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(label.font_size(52.0).nowrap())
}

/// One sample: the same reading over the same photograph, styled differently.
fn panel(caption: &'static str, content: Container) -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            container()
                .width(240.0)
                .height(120.0)
                .corners(8.0)
                .overflow(Overflow::Hidden)
                .layout(ZStack::new())
                .child(photo())
                .child(content),
        )
        .child(
            container().child(
                text(caption)
                    .font_size(12.0)
                    .color(Color::rgb(0.7, 0.7, 0.75)),
            ),
        )
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        // The declaration every panel in the second row shares, so what they
        // differ by is the transform and nothing else.
        let glass = || {
            text(LABEL)
                .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
                .backdrop_blur(16.0)
        };

        let styles = container()
            .layout(Flex::row().spacing(20.0))
            .child(panel("nothing", reading(text(LABEL).color(Color::WHITE))))
            .child(panel("frost 16", reading(glass())))
            .child(panel(
                "frost 16, no tint",
                reading(text(LABEL).color(Color::TRANSPARENT).backdrop_blur(16.0)),
            ))
            .child(panel(
                "frost + stroke 2",
                reading(
                    text(LABEL)
                        .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                        .backdrop_blur(16.0)
                        .text_stroke(TextStroke::new(2.0, Color::BLACK)),
                ),
            ))
            .child(panel(
                "frost under a shadow — buried",
                reading(glass().text_shadow(TextShadow::new(
                    0.0,
                    2.0,
                    10.0,
                    Color::rgba(0.0, 0.0, 0.0, 0.6),
                ))),
            ));

        let transformed = container()
            .layout(Flex::row().spacing(20.0))
            .child(panel("turned 20°", reading(glass()).rotate(20.0)))
            .child(panel("grown 1.4", reading(glass()).scale(1.4)))
            .child(panel(
                "stretched 0.7 x 1.6",
                reading(glass()).scale((0.7, 1.6)),
            ))
            .child(panel(
                "turned, with a contour",
                reading(
                    glass()
                        .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                        .text_stroke(TextStroke::new(2.0, Color::BLACK)),
                )
                .rotate(-12.0),
            ));

        let view = container()
            .padding(24.0)
            .layout(Flex::column().spacing(20.0))
            .child(styles)
            .child(transformed);

        app.add_surface(
            SurfaceConfig::new()
                .width(1360)
                .height(400)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("frosted-text")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
