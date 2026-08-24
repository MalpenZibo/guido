//! Where a box answers a click, at a size where you can see it.
//!
//! Run with: cargo run --example hit_testing
//!
//! Each square lights up while the pointer is inside its *shape* — not its
//! bounding box. Drag the pointer slowly into a corner: the moment the colour
//! drops is the edge of the hit region, and it should sit exactly on the drawn
//! curve. A corner declared square must answer right up to the point.
//!
//! The counter under each square is clicks that landed, so a corner that
//! looks cut but still answers shows up as a number going up where nothing
//! should have been hit.

use guido::prelude::*;

const SIDE: f32 = 220.0;

fn probe(caption: &'static str, corners: Corners) -> Container {
    let hovered = create_signal(false);
    let clicks = create_signal(0u32);

    container()
        .layout(Flex::column().spacing(6.0))
        .child(
            container()
                .width(SIDE)
                .height(SIDE)
                .corners(corners)
                .background(move || {
                    if hovered.get() {
                        Color::rgb(0.30, 0.55, 0.95)
                    } else {
                        Color::rgb(0.20, 0.22, 0.30)
                    }
                })
                .border(2.0, Color::rgb(0.55, 0.60, 0.75))
                .on_hover(move |inside| hovered.set(inside))
                .on_click(move || clicks.update(|c| *c += 1)),
        )
        .child(
            text(caption)
                .font_size(12.0)
                .color(Color::rgb(0.75, 0.78, 0.85)),
        )
        .child(
            text(move || format!("clicks: {}", clicks.get()))
                .font_size(12.0)
                .color(move || {
                    if hovered.get() {
                        Color::rgb(0.55, 0.85, 1.0)
                    } else {
                        Color::rgb(0.45, 0.48, 0.55)
                    }
                }),
        )
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(1180)
                .height(340)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .keyboard_interactivity(KeyboardInteractivity::None)
                .namespace("hit-testing")
                .background_color(Color::rgb(0.09, 0.09, 0.13)),
            || {
                container()
                    .padding(16.0)
                    .layout(Flex::row().spacing(16.0))
                    // Square: the whole rectangle answers, corners included.
                    .child(probe("square", Corners::SQUARE))
                    // Uniform: all four corners cut by the same arc.
                    .child(probe("radius 60, all four", Corners::rounded(60.0)))
                    // The one the four-radius work is about: cut on top,
                    // square at the bottom. The bottom corners must answer to
                    // the very point.
                    .child(probe("[60, 0] — top only", Corners::rounded([60.0, 0.0])))
                    // A different radius per corner, clockwise from top-left.
                    .child(probe(
                        "[80, 10, 80, 10]",
                        Corners::rounded([80.0, 10.0, 80.0, 10.0]),
                    ))
                    // Curvature changes the shape, so it must change the
                    // region too: a bevel is a straight diagonal cut.
                    .child(probe("bevel 80", Corners::bevel(80.0)))
            },
        );
    });
}
