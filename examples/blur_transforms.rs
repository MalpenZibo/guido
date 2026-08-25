//! Does a transform move the blur with the shape, or only the shape?
//!
//! A backdrop blur is resolved to a region twice from the same draw command:
//! the renderer filters the surface's own content inside it, and the compositor
//! is handed a `wl_region` for the desktop behind it. Both are computed as the
//! **axis-aligned bounding box** of the container's world shape.
//!
//! A bounding box equals the shape for any transform that keeps the axes, and
//! is larger than it for one that does not. This lays the four cases side by
//! side over a striped backdrop this surface draws itself, so the blur is
//! visible without a compositor implementing `ext-background-effect-v1`:
//!
//! - each card's **border marks the shape** — it is transformed with the card;
//! - the **blurred area is the region**.
//!
//! Wherever the two come apart, they have come apart. Expect them to agree for
//! the first three and not for the fourth.

use guido::prelude::*;

const CARD: Color = Color::rgba(0.10, 0.10, 0.16, 0.35);
const FRAME: Color = Color::rgba(1.0, 1.0, 1.0, 0.55);

/// A high-contrast backdrop, drawn by this surface so the *surface* half of the
/// blur has something to filter. `SURFACE` blurs what has already been drawn —
/// over a transparent background there is nothing to see.
fn stripes() -> impl Widget {
    let bars: Vec<AnyWidget> = (0..26)
        .map(|i| {
            let warm = i % 2 == 0;
            container()
                .width(fill())
                .height(20.0)
                .background(if warm {
                    Color::rgb(0.85, 0.35, 0.30)
                } else {
                    Color::rgb(0.15, 0.45, 0.85)
                })
                .into_any()
        })
        .collect();

    container()
        .width(fill())
        .height(fill())
        .layout(Flex::column())
        .children(bars)
}

/// A card whose blur comes from this surface's own content, so the case is
/// visible whatever the compositor does.
fn card() -> Container {
    container()
        .width(150.0)
        .height(90.0)
        .corners(18.0)
        .background(CARD)
        .border(2.0, FRAME)
        .backdrop_blur(BackdropBlur::new(14.0).sources(BackdropSources::SURFACE))
}

fn case(caption: &'static str, body: impl Widget + 'static) -> AnyWidget {
    container()
        .width(210.0)
        .layout(
            Flex::column()
                .spacing(10.0)
                .main_alignment(MainAlignment::Center)
                .cross_alignment(CrossAlignment::Center),
        )
        .children([
            container()
                .width(210.0)
                .height(190.0)
                .layout(
                    Flex::row()
                        .main_alignment(MainAlignment::Center)
                        .cross_alignment(CrossAlignment::Center),
                )
                .child(body)
                .into_any(),
            container()
                .child(text(caption).font_size(12.0).color(Color::WHITE))
                .into_any(),
        ])
        .into_any()
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(900)
                .height(240)
                .anchor(Anchor::TOP)
                .margin([100, 0, 0, 0])
                .namespace("guido-blur-transforms")
                .layer(Layer::Overlay)
                .exclusive_zone(ExclusiveZone::None)
                .background_color(Color::TRANSPARENT),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .layout(ZStack::new())
                    .children([
                        stripes().into_any(),
                        container()
                            .width(fill())
                            .height(fill())
                            .layout(
                                Flex::row()
                                    .main_alignment(MainAlignment::Center)
                                    .cross_alignment(CrossAlignment::Center),
                            )
                            .children([
                                // A translated box is the same box somewhere
                                // else, so its bounding box is itself.
                                case("translated 30, 12", card().translate((30.0, 12.0))),
                                // A uniformly scaled one likewise — and its
                                // corners grow with it, or the region cuts a
                                // curve the card does not have.
                                case("scaled 1.4x", card().scale(1.4)),
                                // Unevenly scaled: still axis-aligned, so still
                                // exact, but the corner is now an ellipse.
                                case("scaled 1.6x / 0.8x", card().scale((1.6, 0.8))),
                                // A rotation is the one that does not keep the
                                // axes: the bounding box gains the four corner
                                // triangles, and the mask has no transform to
                                // cut them back out with.
                                case("rotated 20°", card().rotate(20.0)),
                            ])
                            .into_any(),
                    ])
            },
        );
    });
}
