#![cfg(feature = "testing")]
//! A text whose font style or font features are the only thing that changes
//! between frames is measured and drawn in the new ones — by glyphon, by the
//! transformed quad and by the frost's mask alike.
//!
//! Each of those keeps something from the frame before: the measurer its
//! widths, glyphon its shaped buffers, the quad its rasterised textures and
//! the frost its masks, every one of them keyed by what the text looks like.
//! A key that left the style or the features out would hand the new frame the
//! old one's shaping, and nothing about a single frame could tell. So the
//! oracle is a second application that started in the new ones — its own glyphon, quad
//! and mask caches, though the measurer is the thread's and shared: the
//! retained frame has to be its frame, pixel for pixel, band by band. The
//! measurer's own key is watched by the hugging box's edge.

mod common;
use common::headless;
use guido::prelude::*;
use guido::surface::SurfaceId;
use guido::testing::Headless;

/// `assets/make_figures_fixture.py`: an upright face whose digits are boxes
/// of ten widths, with a `tnum` that makes them one, and an italic face of
/// slanted boxes half as tall — so either change moves pixels on every path.
const REGULAR: &[u8] = include_bytes!("assets/GuidoFigures-Regular.ttf");
const ITALIC: &[u8] = include_bytes!("assets/GuidoFigures-Italic.ttf");
const FONT_FAMILY: &str = "Guido Figures";

/// What changes between frames.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    style: FontStyle,
    features: FontFeatures,
}

const PLAIN: Look = Look {
    style: FontStyle::Normal,
    features: FontFeatures::new(),
};
fn tabular() -> Look {
    Look {
        style: FontStyle::Normal,
        features: FontFeatures::new().enable(b"tnum"),
    }
}
const ITALIC_LOOK: Look = Look {
    style: FontStyle::Italic,
    features: FontFeatures::new(),
};

const WIDTH: u32 = 320;
/// Three bands, one per path, each this tall.
const BAND: u32 = 70;
const HEIGHT: u32 = BAND * 3;

const INK: Color = Color::rgb(0.20, 0.30, 0.45);
const BACKDROP: Color = Color::rgb(0.08, 0.08, 0.10);

fn label(look: RwSignal<Look>) -> Text {
    text("1010")
        .font_family(FontFamily::name(FONT_FAMILY))
        .font_size(20.0)
        .color(Color::WHITE)
        .font_style(move || look.get().style)
        .font_features(move || look.get().features)
        // One line box for both faces, whose own lines differ: a box that
        // changed size with the style would miss the quad's and the mask's
        // caches for that reason, and hide a key that left the style out.
        .line_height(LineHeight::Absolute(24.0))
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
/// whatever its features.
///
/// Which is what makes the quad's and the mask's caches answer for the look
/// alone: both key on the size of what they rasterise, and a label whose box
/// grew with its figures would miss them for that reason and hide a key that
/// left the features out.
fn fixed(label: Text) -> Container {
    container()
        .width(200.0)
        .layout(Flex::column().cross_alignment(CrossAlignment::Stretch))
        .child(label)
}

/// The same label three times: upright on a box that hugs it, so the box is
/// the measurement; on a turned card, which is drawn as a quad; and frosted
/// over bars, which cuts a mask.
fn view(look: RwSignal<Look>) -> Container {
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
        .child(band().child(container().background(INK).child(label(look))))
        .child(
            band().child(
                container()
                    .background(INK)
                    .padding(6.0)
                    .rotate(8.0)
                    .child(fixed(label(look))),
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
                        label(look)
                            .color(Color::rgba(1.0, 1.0, 1.0, 0.3))
                            .backdrop_blur(8.0),
                    )),
                ),
        )
}

/// An application drawing the view in `look`, one frame in.
fn app(look: Look) -> Option<(Headless, SurfaceId, RwSignal<Look>)> {
    guido::load_font(REGULAR.to_vec());
    guido::load_font(ITALIC.to_vec());
    let mut app = headless()?;
    let look = create_signal(look);
    let surface = app.surface(
        SurfaceConfig::new()
            .height(HEIGHT)
            .background_color(BACKDROP),
        move || view(look),
    );
    app.configure(surface, WIDTH, HEIGHT, 1.0);
    app.step();
    Some((app, surface, look))
}

/// One frame of an application that has only ever been in `look`.
fn fresh(look: Look) -> Vec<u8> {
    let (app, surface, _) = app(look).expect("an adapter, as the first one found");
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
/// The box is the label's line, 24 pixels, centred in the band, and the
/// figures' boxes start a few pixels under its top: one row into it they have
/// not begun.
fn box_right_edge(frame: &[u8]) -> u32 {
    let y = BAND / 2 - 11;
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

/// Each band of a frame retained through a change to `to` and back is the band
/// of an application that was only ever in `to`, and then in `from` again.
fn changes_alone(from: Look, to: Look) -> Option<(Vec<u8>, Vec<u8>)> {
    let (mut app, surface, look) = app(from)?;
    let before = app.read_frame(surface);

    look.set(to);
    app.step();
    let after = app.read_frame(surface);

    look.set(from);
    app.step();
    let back = app.read_frame(surface);
    drop(app);

    let fresh_after = fresh(to);
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
            rows(&after, band),
            rows(&before, band),
            "{path}: the change drew nothing different at all"
        );
        assert!(
            rows(&after, band) == rows(&fresh_after, band),
            "{path}: the retained frame is not the frame of an application \
             that was only ever in the new look, so something kept the old one"
        );
        assert!(
            rows(&back, band) == rows(&before, band),
            "{path}: going back did not come back"
        );
    }
    Some((before, after))
}

#[test]
fn features_changed_alone_are_measured_and_drawn_afresh_on_every_path() {
    let Some((proportional, tabular)) = changes_alone(PLAIN, tabular()) else {
        return;
    };
    // "1010" is 440 + 400 + 440 + 400 units upright and four of 600 under
    // `tnum`: 33.6 pixels and 48 at 20 to the em.
    let grew = box_right_edge(&tabular) - box_right_edge(&proportional);
    assert!(
        (14..=15).contains(&grew),
        "the box hugging the label grew by {grew}, not by the 14.4 its figures did"
    );
}

#[test]
fn a_style_changed_alone_is_drawn_afresh_on_every_path() {
    changes_alone(PLAIN, ITALIC_LOOK);
}
