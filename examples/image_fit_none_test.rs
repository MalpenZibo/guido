//! Test ContentFit::None for images.
//!
//! Expected CSS-like behavior: Images with ContentFit::None should size
//! themselves to their intrinsic dimensions without requiring explicit
//! width/height.
//!
//! Current status: The widget layout defaults to 100x100 because intrinsic
//! size isn't available during layout. The renderer draws at intrinsic size,
//! causing overflow. This needs to be fixed by making intrinsic size available
//! to the widget before layout.
//!
//! Run with: cargo run --example image_fit_none_test

use guido::prelude::*;

fn main() {
    // Helper to create a labeled image card
    fn labeled(label: &'static str, widget: impl Widget + 'static) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(
                container()
                    .background(Color::rgb(0.2, 0.2, 0.25))
                    .corner_radius(4.0)
                    .child(widget),
            )
            .child(text(label).font_size(11.0).color(Color::rgb(0.7, 0.7, 0.7)))
    }

    // Test: Images without explicit dimensions should use intrinsic size
    // The SVG logo has intrinsic size ~257x248 pixels

    let view = container()
        .padding(24.0)
        .layout(Flex::column().spacing(20.0))
        .child(
            text("ContentFit::None Test")
                .font_size(18.0)
                .color(Color::WHITE),
        )
        .child(
            text("No explicit size - should use intrinsic dimensions")
                .font_size(12.0)
                .color(Color::rgb(0.6, 0.6, 0.6)),
        )
        .child(
            container()
                .layout(Flex::row().spacing(24.0))
                .child(labeled(
                    "ContentFit::None",
                    image("examples/assets/logo.svg").content_fit(ContentFit::None),
                ))
                .child(labeled(
                    "ContentFit::Contain",
                    image("examples/assets/logo.svg").content_fit(ContentFit::Contain),
                )),
        );

    App::new()
        .width(700)
        .height(400)
        .anchor(Anchor::TOP | Anchor::LEFT)
        .layer(Layer::Top)
        .namespace("image-fit-none-test")
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
