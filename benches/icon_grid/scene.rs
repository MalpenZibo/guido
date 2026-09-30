//! A scroller full of small icons, each from a source of its own.
//!
//! The workload #546 asks about: a launcher grid, or a list with an icon on
//! every row. `benches/scroll_list` and `benches/static_clip` paint no images
//! at all, so neither can say what drawing a few hundred of them costs.
//!
//! Half the icons are raw RGBA and half are SVG, because the two reach the
//! texture cache differently — an SVG is rasterized at the size it is shown —
//! and the atlas takes both. There are [`DISTINCT`] sources, more than a
//! screen holds, so no two icons in view share a source and nothing on screen
//! could be batched by drawing one texture twice.

use std::sync::Arc;

use guido::prelude::*;

/// The surface, in logical pixels.
pub const VIEWPORT: (u32, u32) = (600, 800);

/// An icon's side.
const ICON: f32 = 28.0;
const GAP: f32 = 8.0;
const PAD: f32 = 8.0;
/// Icons in a row: `16 * 28 + 15 * 8 + 2 * 8` is 584, inside the 600 the
/// surface is wide, with the scrollbar's strip to spare.
pub const PER_ROW: usize = 16;

/// Distinct sources, half raster and half SVG. A screen holds about 23 rows
/// of 16, which is 368, so every icon in view is a source no other icon in
/// view shares.
pub const DISTINCT: usize = 512;

/// A grid of `rows` rows of [`PER_ROW`] icons, in a scroller the size of the
/// surface.
pub fn scene(rows: usize) -> Container {
    let sources: Arc<[ImageSource]> = (0..DISTINCT).map(source).collect();
    container()
        .width(VIEWPORT.0 as f32)
        .height(VIEWPORT.1 as f32)
        .scroll(Scroll::vertical())
        .background(Color::rgb(0.10, 0.10, 0.14))
        .child(
            container()
                .layout(Flex::column().spacing(GAP))
                .padding(PAD)
                .children((0..rows).map(move |row| {
                    let sources = sources.clone();
                    container()
                        .layout(Flex::row().spacing(GAP))
                        .children((0..PER_ROW).map(move |column| {
                            let index = (row * PER_ROW + column) % DISTINCT;
                            icon(sources[index].clone())
                        }))
                })),
        )
}

fn icon(source: ImageSource) -> Container {
    container()
        .width(ICON)
        .height(ICON)
        .child(image(source).content_fit(ContentFit::Fill))
}

/// The `index`th source: raster for an even index, SVG for an odd one, each
/// in a colour of its own so no two share a texture.
fn source(index: usize) -> ImageSource {
    let (r, g, b) = colour(index);
    if index.is_multiple_of(2) {
        let side = ICON as u32;
        ImageSource::Rgba {
            width: side,
            height: side,
            pixels: [r, g, b, 255].repeat((side * side) as usize).into(),
        }
    } else {
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10" fill="#{r:02x}{g:02x}{b:02x}"/><rect x="7" y="7" width="10" height="10" fill="#ffffff"/></svg>"##
        );
        ImageSource::SvgBytes(svg.into_bytes().into())
    }
}

/// A colour per index, distinct across [`DISTINCT`].
fn colour(index: usize) -> (u8, u8, u8) {
    let r = (index % 8) as u8 * 32;
    let g = (index / 8 % 8) as u8 * 32;
    let b = (index / 64 % 8) as u8 * 32;
    (r, g, b)
}
