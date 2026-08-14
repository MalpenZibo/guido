//! Draw order across primitive kinds.
//!
//! Run with: cargo run --example draw_order_example
//!
//! Commands are batched by kind — all shapes together, then all images, then
//! all text — because that is what lets them share a draw call. Batching alone
//! would reorder them: a container background painted *over* an image would be
//! drawn first and vanish underneath it. The renderer therefore opens a new
//! draw group whenever a command's kind would go backwards.
//!
//! Every panel below paints something over an image. If the ordering ever
//! regresses, the overlays disappear rather than degrade — the failure is
//! obvious on sight.

use guido::prelude::*;

fn photo() -> Image {
    image(ImageSource::Path("examples/assets/photo.webp".into()))
        .width(220.0)
        .height(140.0)
        .content_fit(ContentFit::Cover)
}

fn panel(label: &'static str, content: Container) -> Container {
    container()
        .layout(Flex::column().spacing(8.0))
        .child(content)
        .child(
            container()
                .font_size(12.0)
                .text_color(Color::rgb(0.7, 0.7, 0.75))
                .child(text(label)),
        )
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let view = container()
            .padding(24.0)
            .layout(Flex::row().spacing(24.0))
            .child(panel(
                "shape over image",
                container()
                    .width(220.0)
                    .height(140.0)
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    .layout(ZStack::new())
                    .child(photo())
                    // Translucent tint: batched with the frame's other shapes
                    // it would be painted before the photo and never seen.
                    .child(
                        container()
                            .width(fill())
                            .height(fill())
                            .background(Color::rgba(0.1, 0.05, 0.4, 0.55)),
                    ),
            ))
            .child(panel(
                "shape over image over shape",
                container()
                    .width(220.0)
                    .height(140.0)
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    // Two regressions in one subtree, so two extra groups.
                    .background(Color::rgb(0.8, 0.2, 0.2))
                    .layout(ZStack::new())
                    .child(photo())
                    .child(
                        container()
                            .width(80.0)
                            .height(80.0)
                            .corner_radius(40.0)
                            .background(Color::rgba(1.0, 1.0, 1.0, 0.85)),
                    ),
            ))
            .child(panel(
                "backdrop blur",
                container()
                    .width(220.0)
                    .height(140.0)
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
                            .child(
                                container()
                                    .width(150.0)
                                    .height(70.0)
                                    .corner_radius(16.0)
                                    .squircle()
                                    // Blurs the photo beneath it, then paints
                                    // its own translucent tint over the result.
                                    .backdrop_blur(18.0)
                                    .background(Color::rgba(0.1, 0.1, 0.15, 0.45))
                                    .layout(
                                        Flex::column()
                                            .main_alignment(MainAlignment::Center)
                                            .cross_alignment(CrossAlignment::Center),
                                    )
                                    .font_size(18.0)
                                    .text_color(Color::rgb(0.95, 0.95, 1.0))
                                    .child(text("frosted")),
                            ),
                    ),
            ))
            .child(panel(
                "image over text",
                container()
                    .width(220.0)
                    .height(140.0)
                    .corner_radius(8.0)
                    .overflow(Overflow::Hidden)
                    .background(Color::rgb(0.15, 0.15, 0.2))
                    .layout(ZStack::new())
                    .child(
                        container()
                            .font_size(40.0)
                            .text_color(Color::rgb(0.9, 0.3, 0.3))
                            .child(text("BEHIND").nowrap()),
                    )
                    // Text sits above images in the batch order, so this photo
                    // needs a group of its own to land on top of the label.
                    // Narrower than the panel and pushed right, so the label
                    // is covered where they overlap and legible where they do
                    // not. A photo that filled the panel would look identical
                    // whether the order held or not. The row wrapper is what
                    // keeps the photo at its own width — a stack child sized
                    // directly would be stretched to the panel's tight bounds.
                    .child(
                        container()
                            .width(fill())
                            .height(fill())
                            .layout(Flex::row().main_alignment(MainAlignment::End))
                            .child(photo().width(120.0)),
                    ),
            ));

        app.add_surface(
            SurfaceConfig::new()
                .width(1040)
                .height(230)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("draw-order-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
