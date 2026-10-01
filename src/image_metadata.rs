//! Image metadata utilities for determining intrinsic dimensions.
//!
//! This module provides functions to get image dimensions without loading
//! the full image data, enabling correct layout calculations before rendering.

use crate::widgets::image::ImageSource;

/// The size an image source lays out at, in logical pixels, read without
/// decoding or rasterizing it.
///
/// A raster image's is the header's, in whole pixels. An SVG's is what its
/// root element declares, which can be fractional and is kept that way: a
/// 0.5 × 1.5 icon is not a 0 × 1 one.
///
/// Returns `None` if the source cannot be read or parsed.
pub fn get_intrinsic_size(source: &ImageSource) -> Option<(f32, f32)> {
    let (width, height) = match source {
        ImageSource::Path(path) => image::image_dimensions(path).ok()?,
        // The header, not the image: a full decode here would put back on the
        // frame exactly what `image_decode` takes off it.
        ImageSource::Bytes(bytes) => image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?,
        ImageSource::Rgba { width, height, .. } => (*width, *height),
        ImageSource::SvgPath(_) | ImageSource::SvgBytes(_) => {
            return parse_svg(source).map(|tree| svg_size(&tree));
        }
    };
    Some((width as f32, height as f32))
}

/// A parsed SVG document.
#[cfg(feature = "svg")]
pub(crate) type SvgTree = resvg::usvg::Tree;

/// Without the `svg` feature there is no document to hold: nothing parses one.
#[cfg(not(feature = "svg"))]
pub(crate) enum SvgTree {}

/// An SVG source's document, parsed — read from disk first for a path. The
/// one place either is done: the decode cache keeps what this returns, so its
/// rasters are drawn from the parse that gave the size.
#[cfg(feature = "svg")]
pub(crate) fn parse_svg(source: &ImageSource) -> Option<SvgTree> {
    let read;
    let bytes = match source {
        ImageSource::SvgPath(path) => {
            read = std::fs::read(path).ok()?;
            &read[..]
        }
        ImageSource::SvgBytes(bytes) => &bytes[..],
        _ => return None,
    };
    SvgTree::from_data(bytes, &resvg::usvg::Options::default()).ok()
}

/// Without the `svg` feature an SVG source fails with a warning instead of
/// failing to compile.
#[cfg(not(feature = "svg"))]
pub(crate) fn parse_svg(_source: &ImageSource) -> Option<SvgTree> {
    log::warn!("SVG image used but the `svg` feature is disabled");
    None
}

/// The size an SVG document declares.
pub(crate) fn svg_size(tree: &SvgTree) -> (f32, f32) {
    #[cfg(feature = "svg")]
    {
        let size = tree.size();
        (size.width(), size.height())
    }
    #[cfg(not(feature = "svg"))]
    match *tree {}
}
