//! A blurred layer whose content runs on past the surface, read back in pixels.
//!
//! A layer's texture is cut to what of its content can reach the surface. A
//! blur reaches past every pixel it draws, so a texture cut at the surface's
//! edge has nothing past it to read there but its own last column, smeared
//! outwards by the clamped sampler — or, by luck, whatever the pooled texture's
//! spare texels happen to hold. A uniform colour hides that, so the content
//! here changes just past the edge, and is compared with the same content on a
//! surface wide enough that nothing near the edge is cut.
//!
//! A golden renders one surface the size of its scene, so it never puts
//! content past an edge; this drives the real loop instead.

#![cfg(feature = "testing")]

mod common;

use guido::layout::Flex;
use guido::prelude::*;
use guido::surface::SurfaceId;
use guido::testing::Headless;

/// The surface's side. Its layer texture is 125 + 3 texels at 1x, exactly two
/// of the pool's 64-texel steps, so there are no spare texels past the cut to
/// hide it; at 2x there are three, against a reach of 24.
const SIDE: u32 = 125;
const RADIUS: f32 = 8.0;
/// Within this many levels per channel, two pixels read the same.
const TOLERANCE: i32 = 3;

/// Which edge of the surface the content runs on past.
#[derive(Clone, Copy, Debug)]
enum Across {
    Right,
    Bottom,
}

/// Red up to the surface's edge, a thin green stripe just past it and blue
/// beyond: the blur near the edge reads the stripe and the blue, and a smear
/// of the edge's last column reads neither.
fn striped_past_the_edge(blur: f32, across: Across) -> Container {
    let band = |color: Color, length: f32| {
        let band = container().background(color);
        match across {
            Across::Right => band.width(length).height(400.0),
            Across::Bottom => band.width(400.0).height(length),
        }
    };
    let flex = match across {
        Across::Right => Flex::row(),
        Across::Bottom => Flex::column(),
    };
    container().blur(blur).child(
        container()
            .width(natural())
            .height(natural())
            .layout(flex)
            .child(band(Color::RED, SIDE as f32))
            .child(band(Color::GREEN, 4.0))
            .child(band(Color::BLUE, 300.0)),
    )
}

fn surface(
    app: &mut Headless,
    (width, height): (u32, u32),
    scale: f32,
    scene: impl FnOnce() -> Container,
) -> SurfaceId {
    let id = app.surface(
        SurfaceConfig::new()
            .width(width)
            .height(height)
            .background_color(Color::BLACK),
        scene,
    );
    app.configure(id, width, height, scale);
    id
}

/// The colour at a logical point, read where it lands in physical pixels.
fn rgb(app: &Headless, id: SurfaceId, scale: f32, (x, y): (u32, u32)) -> [i32; 3] {
    let s = scale as u32;
    let [r, g, b, _] = app.read_pixel(id, x * s + s - 1, y * s + s - 1);
    [r as i32, g as i32, b as i32]
}

fn same(a: [i32; 3], b: [i32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= TOLERANCE)
}

#[test]
fn a_blur_near_the_surface_edge_reads_the_content_past_it() {
    let middle = SIDE / 2;
    let last = SIDE - 1;
    for scale in [1.0, 2.0] {
        for (edge, uncut, points) in [
            (
                Across::Right,
                (SIDE + 64, SIDE),
                [(last, middle), (SIDE - 5, middle)],
            ),
            (
                Across::Bottom,
                (SIDE, SIDE + 64),
                [(middle, last), (middle, SIDE - 5)],
            ),
        ] {
            let Some(mut app) = common::headless() else {
                return;
            };
            let cut = surface(&mut app, (SIDE, SIDE), scale, || {
                striped_past_the_edge(RADIUS, edge)
            });
            let whole = surface(&mut app, uncut, scale, || {
                striped_past_the_edge(RADIUS, edge)
            });
            app.step();

            let [_, green, blue] = rgb(&app, whole, scale, points[0]);
            assert!(
                green > 10 && blue > 10,
                "at {scale}x the blur at the {edge:?} edge of the wide surface does \
                 not reach the stripes past it ({green} green, {blue} blue), so \
                 nothing below says anything"
            );
            for point in points {
                let near = rgb(&app, cut, scale, point);
                let truth = rgb(&app, whole, scale, point);
                assert!(
                    same(near, truth),
                    "at {scale}x, at {point:?} by the {edge:?} edge, the blur reads \
                     {near:?} where the same content on a surface that does not \
                     cut it reads {truth:?}: the layer was cut at the surface's \
                     edge, and the blur read past the cut"
                );
            }
        }
    }
}

/// Where the content really starts — the left and top edges, here at the
/// surface's own — the blur still spreads it into the black beyond.
#[test]
fn a_blur_still_fades_the_content_where_it_ends() {
    let middle = SIDE / 2;
    for scale in [1.0, 2.0] {
        let Some(mut app) = common::headless() else {
            return;
        };
        let plain = surface(&mut app, (SIDE, SIDE), scale, || {
            striped_past_the_edge(0.0, Across::Right)
        });
        let blurred = surface(&mut app, (SIDE, SIDE), scale, || {
            striped_past_the_edge(RADIUS, Across::Right)
        });
        app.step();

        for (edge, point) in [("left", (0, middle)), ("top", (middle, 0))] {
            let [faded, ..] = rgb(&app, blurred, scale, point);
            let [unblurred, ..] = rgb(&app, plain, scale, point);
            assert!(
                faded < unblurred - 40,
                "at {scale}x, at the {edge} edge, where the content starts, the \
                 blur reads {faded} red and the content unblurred {unblurred}: \
                 the blur no longer fades the content where it ends"
            );
        }
    }
}
