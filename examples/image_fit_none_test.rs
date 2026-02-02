//! Test ContentFit::None for images and image clipping.
//!
//! Expected CSS-like behavior:
//! 1. Images with ContentFit::None should size themselves to their intrinsic
//!    dimensions without requiring explicit width/height.
//! 2. Images should clip to their container bounds when the image is larger
//!    than its container.
//!
//! Run with: cargo run --example image_fit_none_test

use guido::prelude::*;

fn main() {
    // Helper to create a labeled widget
    fn labeled(label: &'static str, widget: impl Widget + 'static) -> Container {
        container()
            .layout(Flex::column().spacing(8.0))
            .child(widget)
            .child(text(label).font_size(11.0).color(Color::rgb(0.7, 0.7, 0.7)))
    }

    // Helper to create a clipped container with visible border
    fn clipped_box(width: f32, height: f32, widget: impl Widget + 'static) -> Container {
        container()
            .width(width)
            .height(height)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corner_radius(12.0)
            .border(2.0, Color::rgb(0.4, 0.4, 0.5))
            .overflow(Overflow::Hidden)
            .child(widget)
    }

    // Helper to create an unclipped container (for comparison)
    fn unclipped_box(width: f32, height: f32, widget: impl Widget + 'static) -> Container {
        container()
            .width(width)
            .height(height)
            .background(Color::rgb(0.15, 0.15, 0.2))
            .corner_radius(12.0)
            .border(2.0, Color::rgb(0.6, 0.3, 0.3))
            .overflow(Overflow::Visible)
            .child(widget)
    }

    let view = container()
        .padding(24.0)
        .layout(Flex::column().spacing(24.0))
        .child(
            text("Image Clipping Test")
                .font_size(20.0)
                .color(Color::WHITE),
        )
        // Row 1: Clipping comparison
        .child(
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    text("Clipping: 120x120 container with 257x248 image (ContentFit::None)")
                        .font_size(13.0)
                        .color(Color::rgb(0.7, 0.7, 0.7)),
                )
                .child(
                    container()
                        .layout(Flex::row().spacing(40.0))
                        .child(labeled(
                            "Overflow::Hidden (clipped)",
                            clipped_box(
                                120.0,
                                120.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        ))
                        .child(labeled(
                            "Overflow::Visible (not clipped)",
                            unclipped_box(
                                120.0,
                                120.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        )),
                ),
        )
        // Row 2: Different sizes
        .child(
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    text("Clipped containers at different sizes")
                        .font_size(13.0)
                        .color(Color::rgb(0.7, 0.7, 0.7)),
                )
                .child(
                    container()
                        .layout(
                            Flex::row()
                                .spacing(24.0)
                                .cross_axis_alignment(CrossAxisAlignment::End),
                        )
                        .child(labeled(
                            "60x60",
                            clipped_box(
                                60.0,
                                60.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        ))
                        .child(labeled(
                            "100x100",
                            clipped_box(
                                100.0,
                                100.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        ))
                        .child(labeled(
                            "150x150",
                            clipped_box(
                                150.0,
                                150.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        ))
                        .child(labeled(
                            "200x200",
                            clipped_box(
                                200.0,
                                200.0,
                                image("examples/assets/logo.svg").content_fit(ContentFit::None),
                            ),
                        )),
                ),
        )
        // Row 3: Intrinsic size (no container constraint)
        .child(
            container()
                .layout(Flex::column().spacing(12.0))
                .child(
                    text("Intrinsic size layout (no explicit dimensions)")
                        .font_size(13.0)
                        .color(Color::rgb(0.7, 0.7, 0.7)),
                )
                .child(labeled(
                    "ContentFit::None at intrinsic size (~257x248)",
                    container()
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .corner_radius(8.0)
                        .border(1.0, Color::rgb(0.3, 0.5, 0.3))
                        .child(image("examples/assets/logo.svg").content_fit(ContentFit::None)),
                )),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(900)
            .height(700)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .layer(Layer::Top)
            .namespace("image-fit-none-test")
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        |_| view,
    );
    app.run();
}
