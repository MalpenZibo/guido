//! The *shape* of a compositor blur region.
//!
//! `ext-background-effect-v1` carries no radius — the compositor picks how
//! strong its blur is — so all a client controls is **where** it applies. That
//! region is a `wl_region`, a union of rectangles, and guido derives it from the
//! same draw command the renderer filters its own backdrop from, tessellating
//! the rounded corners into slabs.
//!
//! Everything below is a case where the published region and the shape on
//! screen can disagree. Each card is translucent, so the desktop shows through
//! it; wherever the compositor blurs *outside* the card, or leaves a corner of
//! it sharp, the two have come apart.
//!
//! Requires a compositor implementing `ext-background-effect-v1` with its blur
//! capability, and with blur actually enabled — the log says
//! "Compositor blur capability available" when the protocol is there. Without
//! it these are plain translucent cards.

use guido::prelude::*;

const GLASS: Color = Color::rgba(0.10, 0.10, 0.16, 0.45);
const FRAME: Color = Color::rgba(1.0, 1.0, 1.0, 0.25);

/// A translucent card that asks the compositor to blur behind it.
///
/// No border: where one of these is clipped, the shape on screen is the
/// clip's and a border drawn on the child would be cut away at every curve —
/// which looks like a rendering fault and is only the child being square. The
/// border goes on whatever supplies the visible outline.
fn glass() -> Container {
    container()
        .backdrop_blur(BackdropBlur::new(24.0).sources(BackdropSources::COMPOSITOR))
        .background(GLASS)
}

fn label(s: &'static str) -> impl Widget {
    container().child(
        text(s)
            .font_size(11.0)
            .color(Color::rgba(1.0, 1.0, 1.0, 0.75)),
    )
}

fn case(caption: &'static str, body: impl Widget + 'static) -> impl Widget {
    container()
        .layout(
            Flex::column()
                .spacing(6.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .children([body.into_any(), label(caption).into_any()])
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        // The on/off that took the longest to get right: a radius reaching zero
        // has to withdraw the region, not keep blurring for the life of the
        // surface.
        let frosted = create_signal(true);

        app.add_surface(
            SurfaceConfig::new()
                .width(900)
                .height(420)
                .anchor(Anchor::TOP)
                .margin([80, 0, 0, 0])
                .namespace("guido-blur")
                .layer(Layer::Overlay)
                .exclusive_zone(ExclusiveZone::None)
                .background_color(Color::TRANSPARENT),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .padding(24.0)
                    .layout(
                        Flex::column()
                            .spacing(20.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .children([
                        container()
                            .layout(Flex::row().spacing(20.0))
                            .children([
                                // Four different corners. The region used to be
                                // tessellated from `radii.max()`, so the sharp
                                // corners here were cut as though they were the
                                // round ones — a wedge of blur outside the card
                                // at every corner that is not the largest.
                                case(
                                    "per-corner radii",
                                    glass()
                                        .width(200.0)
                                        .height(120.0)
                                        .corners([36.0, 0.0, 36.0, 0.0])
                                        .border(1.0, FRAME),
                                )
                                .into_any(),
                                // A blurred child filling a rounded clip
                                // exactly. Both rectangles supply all four
                                // edges, and the corners on screen are the
                                // parent's — reading them as the child's leaves
                                // four sharp wedges of blurred desktop outside
                                // the panel.
                                case(
                                    "child fills a rounded clip",
                                    container()
                                        .width(200.0)
                                        .height(120.0)
                                        .corners(28.0)
                                        .border(1.0, FRAME)
                                        .overflow(Overflow::Hidden)
                                        .child(glass().width(fill()).height(fill())),
                                )
                                .into_any(),
                                // Half out of the same clip. The cut edge is
                                // straight, so the two corners along it are
                                // square — published as round, the region gives
                                // back a wedge the size of the radius at each.
                                case(
                                    "child half out of a clip",
                                    container()
                                        .width(200.0)
                                        .height(120.0)
                                        .corners(28.0)
                                        .border(1.0, FRAME)
                                        .overflow(Overflow::Hidden)
                                        .layout(Flex::row())
                                        .child(glass().width(320.0).height(fill()).corners(20.0)),
                                )
                                .into_any(),
                            ])
                            .into_any(),
                        container()
                            .layout(Flex::row().spacing(20.0))
                            .children([
                                // An unevenly scaled corner is an ellipse. One
                                // radius for both axes — the geometric mean —
                                // is neither of them, so the region cut a curve
                                // this card does not have.
                                case(
                                    "scaled 1.6x / 1.0x",
                                    container().width(200.0).height(120.0).child(
                                        glass()
                                            .width(120.0)
                                            .height(120.0)
                                            .corners(28.0)
                                            .border(1.0, FRAME)
                                            .scale((1.6, 1.0)),
                                    ),
                                )
                                .into_any(),
                                // A rotation moves the extremes onto the other
                                // diagonal; the region is the bounding box of
                                // the turned card, which is wider than the card.
                                case(
                                    "rotated 20°",
                                    container().width(200.0).height(120.0).child(
                                        glass()
                                            .width(150.0)
                                            .height(90.0)
                                            .corners(16.0)
                                            .border(1.0, FRAME)
                                            .rotate(20.0),
                                    ),
                                )
                                .into_any(),
                                // Click: the radius goes to zero and the region
                                // has to be withdrawn. Turning it back on has to
                                // work too — the fix for the first bug inverted
                                // it, so it switched off and never came back.
                                case(
                                    "click to switch off and on",
                                    container()
                                        .width(200.0)
                                        .height(120.0)
                                        .corners(24.0)
                                        .background(GLASS)
                                        .border(1.0, FRAME)
                                        .backdrop_blur(move || {
                                            BackdropBlur::new(if frosted.get() {
                                                24.0
                                            } else {
                                                0.0
                                            })
                                            .sources(BackdropSources::COMPOSITOR)
                                        })
                                        .when_hovered(|s| s.lighter(0.06))
                                        .on_click(move || frosted.update(|f| *f = !*f))
                                        .layout(
                                            Flex::column()
                                                .main_alignment(MainAlignment::Center)
                                                .cross_alignment(CrossAlignment::Center),
                                        )
                                        .child(text(move || {
                                            if frosted.get() {
                                                "blur on".to_owned()
                                            } else {
                                                "blur off".to_owned()
                                            }
                                        })),
                                )
                                .into_any(),
                            ])
                            .into_any(),
                    ])
            },
        );
    });
}
