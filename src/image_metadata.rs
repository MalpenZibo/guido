//! Image metadata utilities for determining intrinsic dimensions.
//!
//! This module provides functions to get image dimensions without loading
//! the full image data, enabling correct layout calculations before rendering.

use std::path::Path;

use crate::widgets::image::ImageSource;

/// Get the intrinsic dimensions of an image source without loading the full image.
///
/// This is used during layout to determine the natural size of an image
/// before the renderer loads it to a GPU texture.
///
/// Returns `Some((width, height))` if the dimensions can be determined,
/// or `None` if the image cannot be read or parsed.
pub fn get_intrinsic_size(source: &ImageSource) -> Option<(u32, u32)> {
    match source {
        ImageSource::Path(path) => image::image_dimensions(path).ok(),
        // The header, not the image: a full decode here would put back on the
        // frame exactly what `image_decode` takes off it.
        ImageSource::Bytes(bytes) => image::ImageReader::new(std::io::Cursor::new(&bytes[..]))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok(),
        ImageSource::Rgba { width, height, .. } => Some((*width, *height)),
        ImageSource::SvgPath(path) => get_svg_size_from_file(path),
        ImageSource::SvgBytes(bytes) => get_svg_size_from_bytes(bytes),
    }
}

pub(crate) fn get_layout_size(source: &ImageSource) -> Option<(f64, f64)> {
    match source {
        #[cfg(feature = "svg")]
        ImageSource::SvgPath(path) => {
            get_svg_layout_size_from_file(path).map(|(width, height)| (width as f64, height as f64))
        }
        #[cfg(feature = "svg")]
        ImageSource::SvgBytes(bytes) => get_svg_layout_size_from_bytes(bytes)
            .map(|(width, height)| (width as f64, height as f64)),
        _ => get_intrinsic_size(source).map(|(width, height)| (width as f64, height as f64)),
    }
}

/// Get SVG dimensions from a file path.
#[cfg(feature = "svg")]
fn get_svg_size_from_file(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    get_svg_size_from_bytes(&data)
}

/// Get SVG dimensions from raw bytes.
#[cfg(feature = "svg")]
fn get_svg_size_from_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    get_svg_layout_size_from_bytes(bytes).map(svg_pixel_extent)
}

#[cfg(feature = "svg")]
fn get_svg_layout_size_from_file(path: &Path) -> Option<(f32, f32)> {
    let data = std::fs::read(path).ok()?;
    get_svg_layout_size_from_bytes(&data)
}

#[cfg(feature = "svg")]
fn get_svg_layout_size_from_bytes(bytes: &[u8]) -> Option<(f32, f32)> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    Some((size.width(), size.height()))
}

#[cfg(feature = "svg")]
fn svg_pixel_extent((width, height): (f32, f32)) -> (u32, u32) {
    (width.ceil() as u32, height.ceil() as u32)
}

#[cfg(not(feature = "svg"))]
fn get_svg_size_from_file(_path: &Path) -> Option<(u32, u32)> {
    log::warn!("SVG image used but the `svg` feature is disabled");
    None
}

#[cfg(not(feature = "svg"))]
fn get_svg_size_from_bytes(_bytes: &[u8]) -> Option<(u32, u32)> {
    log::warn!("SVG image used but the `svg` feature is disabled");
    None
}
