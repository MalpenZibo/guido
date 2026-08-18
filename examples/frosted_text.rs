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
//! The last panel is the trap, and it is a shadow: shadows are still copies of
//! the glyphs drawn *under* the fill, so they cover the letter's own area and
//! not only its edge. Under an opaque fill that is invisible and free; over
//! frost it is an opaque letter where the picture should be.

use guido::prelude::*;

const LABEL: &str = "09:41";

fn photo() -> Container {
    container().width(fill()).height(fill()).child(
        image(ImageSource::Path("examples/assets/photo.webp".into()))
            .content_fit(ContentFit::Cover),
    )
}

/// One sample: the same reading over the same photograph, styled differently.
fn panel(caption: &'static str, label: Text) -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(
            container()
                .width(240.0)
                .height(120.0)
                .corner_radius(8.0)
                .overflow(Overflow::Hidden)
                .layout(ZStack::new())
                .child(photo())
                .child(
                    container()
                        .width(fill())
                        .height(fill())
                        .layout(
                            Flex::column()
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(label.font_size(52.0).nowrap()),
                ),
        )
        .child(
            container()
                .font_size(12.0)
                .text_color(Color::rgb(0.7, 0.7, 0.75))
                .child(text(caption)),
        )
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let view = container()
            .padding(24.0)
            .layout(Flex::row().spacing(20.0))
            .child(panel("nothing", text(LABEL).color(Color::WHITE)))
            .child(panel(
                "frost 16",
                text(LABEL)
                    .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
                    .backdrop_blur(16.0),
            ))
            .child(panel(
                "frost 16, no tint",
                text(LABEL).color(Color::TRANSPARENT).backdrop_blur(16.0),
            ))
            .child(panel(
                "frost + stroke 2",
                text(LABEL)
                    .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                    .backdrop_blur(16.0)
                    .text_stroke(TextStroke::new(2.0, Color::BLACK)),
            ))
            .child(panel(
                "frost under a shadow — buried",
                text(LABEL)
                    .color(Color::rgba(1.0, 1.0, 1.0, 0.35))
                    .backdrop_blur(16.0)
                    .text_shadow(TextShadow::new(
                        0.0,
                        2.0,
                        10.0,
                        Color::rgba(0.0, 0.0, 0.0, 0.6),
                    )),
            ));

        app.add_surface(
            SurfaceConfig::new()
                .width(1360)
                .height(210)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("frosted-text")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
