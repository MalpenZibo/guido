//! Image widget example demonstrating raster and SVG image support.
//!
//! Run with: cargo run --example image_example

use guido::prelude::*;

fn main() {
    // Helper to create a labeled image card.
    //
    // The card decides the box; the image only decides how its pixels land in
    // it. That is why the sizes moved here from the image calls below.
    fn image_card(label: &'static str, (w, h): (f32, f32), img: Image) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(
                container()
                    .width(w)
                    .height(h)
                    .background(Color::rgb(0.2, 0.2, 0.25))
                    .corner_radius(4.0)
                    .child(img),
            )
            .child(container().child(text(label).color(Color::rgb(0.7, 0.7, 0.7))))
    }

    // Helper to create a transformed image card
    fn transformed_card(
        label: &'static str,
        (w, h): (f32, f32),
        img: Image,
        transform: Container,
    ) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(
                transform
                    .width(w)
                    .height(h)
                    .background(Color::rgb(0.2, 0.2, 0.25))
                    .corner_radius(4.0)
                    .child(img),
            )
            .child(container().child(text(label).color(Color::rgb(0.7, 0.7, 0.7))))
    }

    // Panel with two columns: raster images and SVG images
    let view = container()
        .padding(24.0)
        .layout(Flex::row().spacing(48.0))
        .child(
            // Left column: Raster images
            container()
                .layout(Flex::column().spacing(32.0))
                .child(container().child(text("Raster Image").color(Color::WHITE)))
                .child(
                    container()
                        .layout(Flex::row().spacing(32.0))
                        .child(image_card(
                            "Contain",
                            (90.0, 90.0),
                            image("examples/assets/photo.webp").content_fit(ContentFit::Contain),
                        ))
                        .child(image_card(
                            "Cover",
                            (90.0, 90.0),
                            image("examples/assets/photo.webp").content_fit(ContentFit::Cover),
                        ))
                        .child(image_card(
                            "Fill",
                            (90.0, 90.0),
                            image("examples/assets/photo.webp").content_fit(ContentFit::Fill),
                        )),
                )
                .child(
                    container()
                        .layout(Flex::row().spacing(48.0))
                        .child(transformed_card(
                            "Rotated 10°",
                            (90.0, 90.0),
                            image("examples/assets/photo.webp").content_fit(ContentFit::Cover),
                            container().transform(Transform::rotate_degrees(10.0)),
                        ))
                        .child(transformed_card(
                            "Scaled 1.5x",
                            (90.0, 90.0),
                            image("examples/assets/photo.webp").content_fit(ContentFit::Cover),
                            container().transform(Transform::scale(1.5)),
                        )),
                ),
        )
        .child(
            // Right column: SVG images
            container()
                .layout(Flex::column().spacing(32.0))
                .child(
                    container().child(
                        text("SVG Image")
                            .font_size(16.0)
                            .font_size(12.0)
                            .font_size(12.0)
                            .font_size(16.0)
                            .color(Color::WHITE),
                    ),
                )
                .child(
                    container()
                        .layout(Flex::row().spacing(32.0))
                        .child(image_card(
                            "Normal",
                            (80.0, 60.0),
                            image("examples/assets/logo.svg"),
                        ))
                        .child(transformed_card(
                            "Rotated 15°",
                            (80.0, 60.0),
                            image("examples/assets/logo.svg"),
                            container().transform(Transform::rotate_degrees(15.0)),
                        ))
                        .child(transformed_card(
                            "Scaled 1.5x",
                            (80.0, 60.0),
                            image("examples/assets/logo.svg"),
                            container().transform(Transform::scale(1.5)),
                        )),
                ),
        );

    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(820)
                .height(400)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("image-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            || view,
        );
    });
}
