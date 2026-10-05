#![cfg(feature = "testing")]
//! A text whose letter spacing is the only thing that changes between frames
//! is measured and drawn at the new spacing — by glyphon, by the transformed
//! quad and by the frost's mask alike.
//!
//! Each of those keeps something from the frame before: the measurer its
//! widths, glyphon its shaped buffers, the quad its rasterised textures and
//! the frost its masks, every one of them keyed by what the text looks like.
//! A key that left the spacing out would hand the new frame the old one's
//! shaping, and nothing about a single frame could tell. So the oracle is a
//! second application that started at the new spacing — its own glyphon, quad
//! and mask caches, though the measurer is the thread's and shared: the
//! retained frame has to be its frame, pixel for pixel, band by band. The
//! measurer's own key is watched by the hugging box's edge.

mod common;
use common::headless;
use guido::prelude::*;
use guido::surface::SurfaceId;
use guido::testing::Headless;

/// The goldens' font, so the frames do not depend on what the machine has.
const FONT: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");
const FONT_FAMILY: &str = "DejaVu Sans Mono";

const WIDTH: u32 = 320;
/// Three bands, one per path, each this tall.
const BAND: u32 = 70;
const HEIGHT: u32 = BAND * 3;

const INK: Color = Color::rgb(0.20, 0.30, 0.45);
const BACKDROP: Color = Color::rgb(0.08, 0.08, 0.10);

fn label(spacing: RwSignal<f32>) -> Text {
    text("spacing")
        .font_family(FontFamily::name(FONT_FAMILY))
        .font_size(20.0)
        .color(Color::WHITE)
        .letter_spacing(spacing)
        .nowrap()
}

fn band() -> Container {
    container()
        .width(WIDTH as f32)
        .height(BAND as f32)
        .padding(10.0)
        .layout(Flex::row().cross_alignment(CrossAlignment::Center))
}

/// A box the label is stretched across, so its own box is the same width
/// whatever its spacing.
///
/// Which is what makes the quad's and the mask's caches answer for the
/// spacing alone: both key on the size of what they rasterise, and a label
/// whose box grew with its spacing would miss them for that reason and hide a
/// key that left the spacing out.
fn fixed(label: Text) -> Container {
    container()
        .width(200.0)
        .layout(Flex::column().cross_alignment(CrossAlignment::Stretch))
        .child(label)
}

/// The same label three times: upright on a box that hugs it, so the box is
/// the measurement; on a turned card, which is drawn as a quad; and frosted
/// over bars, which cuts a mask.
fn view(spacing: RwSignal<f32>) -> Container {
    let bars: Vec<AnyWidget> = (0..16)
        .map(|i| {
            let colour = if i % 2 == 0 {
                Color::rgb(0.85, 0.35, 0.30)
            } else {
                Color::rgb(0.15, 0.45, 0.85)
            };
            container()
                .width(20.0)
                .height(BAND as f32)
                .background(colour)
                .into_any()
        })
        .collect();

    container()
        .layout(Flex::column())
        .child(band().child(container().background(INK).child(label(spacing))))
        .child(
            band().child(
                container()
                    .background(INK)
                    .padding(6.0)
                    .rotate(8.0)
                    .child(fixed(label(spacing))),
            ),
        )
        .child(
            container()
                .width(WIDTH as f32)
                .height(BAND as f32)
                .layout(ZStack::new())
                .child(container().layout(Flex::row()).children(bars))
                .child(
                    band().child(fixed(
                        label(spacing)
                            .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                            .backdrop_blur(8.0),
                    )),
                ),
        )
}

/// An application drawing the view at `spacing`, one frame in.
fn app(spacing: f32) -> Option<(Headless, SurfaceId, RwSignal<f32>)> {
    guido::load_font(FONT.to_vec());
    let mut app = headless()?;
    let spacing = create_signal(spacing);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(HEIGHT)
            .background_color(BACKDROP),
        move || view(spacing),
    );
    app.configure(surface, WIDTH, HEIGHT, 1.0);
    app.step();
    Some((app, surface, spacing))
}

/// One frame of an application that has only ever been at `spacing`.
fn fresh(spacing: f32) -> Vec<u8> {
    let (app, surface, _) = app(spacing).expect("an adapter, as the first one found");
    app.read_frame(surface)
}

/// The rows of one band of a frame.
fn rows(frame: &[u8], band: u32) -> &[u8] {
    let row = (WIDTH * 4) as usize;
    &frame[band as usize * BAND as usize * row..(band + 1) as usize * BAND as usize * row]
}

/// How far right the hugging box reaches, along a row of the top band above
/// the letters: the measured width of the upright label, read off the pixels.
///
/// The box is one line tall, 23.3 pixels, centred in the band; two rows into
/// it the glyphs have not begun.
fn box_right_edge(frame: &[u8]) -> u32 {
    let y = BAND / 2 - 9;
    let ink = [51u8, 77, 115];
    (0..WIDTH)
        .rev()
        .find(|x| {
            let at = ((y * WIDTH + x) * 4) as usize;
            frame[at..at + 3]
                .iter()
                .zip(ink)
                .all(|(a, b)| a.abs_diff(b) <= 1)
        })
        .expect("the hugging box is drawn")
}

#[test]
fn a_spacing_changed_alone_is_measured_and_drawn_afresh_on_every_path() {
    let Some((mut app, surface, spacing)) = app(0.0) else {
        return;
    };
    let unspaced = app.read_frame(surface);

    spacing.set(6.0);
    app.step();
    let spaced = app.read_frame(surface);

    spacing.set(0.0);
    app.step();
    let back = app.read_frame(surface);
    drop(app);

    let fresh_spaced = fresh(6.0);

    // Six pixels after each of seven letters.
    assert_eq!(
        box_right_edge(&spaced),
        box_right_edge(&unspaced) + 42,
        "the box hugging the label was not measured at the new spacing"
    );
    for (band, path) in [
        "upright, through glyphon",
        "turned, through the quad",
        "frosted, through the mask",
    ]
    .into_iter()
    .enumerate()
    {
        let band = band as u32;
        assert_ne!(
            rows(&spaced, band),
            rows(&unspaced, band),
            "{path}: the spacing changed nothing at all"
        );
        assert!(
            rows(&spaced, band) == rows(&fresh_spaced, band),
            "{path}: the retained frame is not the frame of an application \
             that was only ever spaced, so something kept the old spacing"
        );
        assert!(
            rows(&back, band) == rows(&unspaced, band),
            "{path}: going back to no spacing did not come back"
        );
    }
}
