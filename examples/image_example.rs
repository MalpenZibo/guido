//! Image widget example demonstrating raster and SVG image support.
//!
//! Run with: cargo run --example image_example

use guido::prelude::*;

fn main() {
    // Helper to create a labeled image card
    fn image_card(label: &'static str, img: Image) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(
                container()
                    .background(Color::rgb(0.2, 0.2, 0.25))
                    .corner_radius(4.0)
                    .child(img),
            )
            .child(text(label).font_size(12.0).color(Color::rgb(0.7, 0.7, 0.7)))
    }

    // Helper to create a transformed image card
    fn transformed_card(label: &'static str, img: Image, transform: Container) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(
                transform
                    .background(Color::rgb(0.2, 0.2, 0.25))
                    .corner_radius(4.0)
                    .child(img),
            )
            .child(text(label).font_size(12.0).color(Color::rgb(0.7, 0.7, 0.7)))
    }

    // Panel with two columns: raster images and SVG images
    let view = container()
        .padding(24.0)
        .layout(Flex::row().spacing(48.0))
        .child(
            // Left column: Raster images
            container()
                .layout(Flex::column().spacing(32.0))
                .child(text("Raster Image").font_size(16.0).color(Color::WHITE))
                .child(
                    container()
                        .layout(Flex::row().spacing(32.0))
                        .child(image_card(
                            "Contain",
                            image("examples/assets/photo.webp")
                                .width(90.0)
                                .height(90.0)
                                .content_fit(ContentFit::Contain),
                        ))
                        .child(image_card(
                            "Cover",
                            image("examples/assets/photo.webp")
                                .width(90.0)
                                .height(90.0)
                                .content_fit(ContentFit::Cover),
                        ))
                        .child(image_card(
                            "Fill",
                            image("examples/assets/photo.webp")
                                .width(90.0)
                                .height(90.0)
                                .content_fit(ContentFit::Fill),
                        )),
                )
                .child(
                    container()
                        .layout(Flex::row().spacing(48.0))
                        .child(transformed_card(
                            "Rotated 10°",
                            image("examples/assets/photo.webp")
                                .width(90.0)
                                .height(90.0)
                                .content_fit(ContentFit::Cover),
                            container().rotate(10.0),
                        ))
                        .child(transformed_card(
                            "Scaled 1.5x",
                            image("examples/assets/photo.webp")
                                .width(90.0)
                                .height(90.0)
                                .content_fit(ContentFit::Cover),
                            container().scale(1.5),
                        )),
                ),
        )
        .child(
            // Right column: SVG images
            container()
                .layout(Flex::column().spacing(32.0))
                .child(text("SVG Image").font_size(16.0).color(Color::WHITE))
                .child(
                    container()
                        .layout(Flex::row().spacing(32.0))
                        .child(image_card(
                            "Normal",
                            image("examples/assets/logo.svg").width(80.0).height(60.0),
                        ))
                        .child(transformed_card(
                            "Rotated 15°",
                            image("examples/assets/logo.svg").width(80.0).height(60.0),
                            container().rotate(15.0),
                        ))
                        .child(transformed_card(
                            "Scaled 1.5x",
                            image("examples/assets/logo.svg").width(80.0).height(60.0),
                            container().scale(1.5),
                        )),
                ),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(820)
            .height(400)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .layer(Layer::Top)
            .namespace("image-example")
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        |_| view,
    );
    app.run();
}
