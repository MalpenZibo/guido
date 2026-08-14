//! Stroke and shadow on text over a photograph.
//!
//! Run with: cargo run --example text_decoration_example
//!
//! The problem both solve: over a picture there is no single text colour that
//! works, because the picture is light in some places and dark in others. The
//! left column is the control — plain white, illegible wherever the photo is
//! bright. The others separate the glyphs from whatever is behind them.

use guido::prelude::*;

const LABEL: &str = "09:41";

fn photo() -> Container {
    container().width(fill()).height(fill()).child(
        image(ImageSource::Path("examples/assets/photo.webp".into()))
            .content_fit(ContentFit::Cover),
    )
}

/// One sample: the same text over the same photo, styled differently.
fn panel(caption: &'static str, styled: Container) -> Container {
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
                    styled
                        .width(fill())
                        .height(fill())
                        .font_size(52.0)
                        .text_color(Color::WHITE)
                        .layout(
                            Flex::column()
                                .main_alignment(MainAlignment::Center)
                                .cross_alignment(CrossAlignment::Center),
                        )
                        .child(text(LABEL).nowrap()),
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
            .child(panel("nothing", container()))
            .child(panel(
                "stroke 1.5",
                container().text_stroke(TextStroke::new(1.5, Color::BLACK)),
            ))
            .child(panel(
                "shadow 0,2 blur 10",
                container().text_shadow(TextShadow::new(
                    0.0,
                    2.0,
                    10.0,
                    Color::rgba(0.0, 0.0, 0.0, 0.75),
                )),
            ))
            .child(panel(
                "both",
                container()
                    .text_stroke(TextStroke::new(1.0, Color::rgba(0.0, 0.0, 0.0, 0.6)))
                    .text_shadow(TextShadow::new(
                        0.0,
                        2.0,
                        8.0,
                        Color::rgba(0.0, 0.0, 0.0, 0.6),
                    )),
            ));

        app.add_surface(
            SurfaceConfig::new()
                .width(1080)
                .height(210)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("text-decoration-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
