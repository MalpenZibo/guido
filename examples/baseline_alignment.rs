//! The four cross alignments on the same row, so the difference is visible.
//!
//! The row is the ordinary bar module: a big number, a small unit, a longer
//! label, and a box with no text at all.
//!
//! `End` and `Baseline` are the pair worth looking at, and they are only a
//! few pixels apart: `End` lines up the children's boxes, `Baseline` lines up
//! the line their text is written on. "umidità 41%" has a taller box than its
//! visible glyphs because it has to hold the descenders, so under `End` it
//! rides higher than the rest — under `Baseline` it does not. The dot has no
//! text at all, so it aligns by its bottom edge either way, which is what CSS
//! does with a baseline-less box.

use guido::prelude::*;

fn row(label: &str, align: CrossAlignment) -> Container {
    container()
        .layout(Flex::row().spacing(10.0).cross_alignment(align))
        .padding([6.0, 10.0])
        .background(Color::rgb(0.16, 0.16, 0.22))
        .corners(8.0)
        .children([
            container()
                .width(96.0)
                .child(text(label.to_string()).color(Color::rgb(0.55, 0.55, 0.65)))
                .into_any(),
            container().child(text("28").color(Color::WHITE)).into_any(),
            container()
                .child(text("°C").color(Color::rgb(0.9, 0.7, 0.3)))
                .into_any(),
            container()
                .child(
                    text("umidità 41%")
                        .font_size(18.0)
                        .font_size(30.0)
                        .font_size(12.0)
                        .font_size(13.0)
                        .color(Color::rgb(0.6, 0.8, 1.0)),
                )
                .into_any(),
            container()
                .width(22.0)
                .height(22.0)
                .corners(11.0)
                .background(Color::rgb(0.4, 0.8, 0.5))
                .into_any(),
        ])
}

fn main() {
    App::new().run(|app| {
        let view = container()
            .background(Color::rgb(0.10, 0.10, 0.14))
            .padding(14.0)
            .layout(Flex::column().spacing(10.0))
            .children([
                row("Start", CrossAlignment::Start),
                row("Center", CrossAlignment::Center),
                row("End", CrossAlignment::End),
                row("Baseline", CrossAlignment::Baseline),
            ]);

        app.add_surface(
            SurfaceConfig::new()
                .width(460)
                .height(320)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Overlay)
                .namespace("zz-baseline"),
            move || view,
        );
    });
}
