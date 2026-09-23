//! Does a transform move the blur with the shape, or only the shape?
//!
//! A backdrop blur is resolved twice from the same draw command: the renderer
//! filters the surface's own content behind the card, and the compositor is
//! handed a `wl_region` for the desktop behind the surface. Both are cut to the
//! shape the container drew — the renderer masks its fragments in the
//! container's own space, and the region is tessellated from the same placed
//! shape — so a transform carries the blur along with the card.
//!
//! This lays the four cases side by side over a striped backdrop this surface
//! draws itself, so the blur is visible without a compositor implementing
//! `ext-background-effect-v1`:
//!
//! - each card's **border marks the shape** — it is transformed with the card;
//! - the **blurred area is what the effect reached**.
//!
//! Expect them to agree in all four. Until #198 the fourth was the one that
//! came apart: both halves took the axis-aligned bounding box, which is the
//! shape only while the transform keeps the axes, so a turned card blurred the
//! box around itself — 73% more area, corner triangles and all.

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
        .layout(Flex::column().spacing(10.0).center())
        .children([
            container()
                .width(210.0)
                .height(190.0)
                .layout(Flex::row().center())
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
                            .layout(Flex::row().center())
                            .children([
                                // A translated box is the same box somewhere
                                // else: the easy case, and the one that was
                                // never wrong.
                                case("translated 30, 12", card().translate((30.0, 12.0))),
                                // A uniformly scaled one likewise — and its
                                // corners grow with it, or the region cuts a
                                // curve the card does not have.
                                case("scaled 1.4x", card().scale(1.4)),
                                // Unevenly scaled: the corner is now an
                                // ellipse, which the region rounds to the
                                // circle inscribed in it (#400).
                                case("scaled 1.6x / 0.8x", card().scale((1.6, 0.8))),
                                // A rotation is the one that does not keep the
                                // axes. The viewport is still the bounding box
                                // — a viewport is four integers — but the mask
                                // is tested in the card's own space, so the
                                // corner triangles are cut back off.
                                case("rotated 20°", card().rotate(20.0)),
                            ])
                            .into_any(),
                    ])
            },
        );
    });
}
