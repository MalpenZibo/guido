//! Image metadata utilities for determining intrinsic dimensions.
//!
//! This module provides functions to get image dimensions without loading
//! the full image data, enabling correct layout calculations before rendering.

use std::path::Path;

use image::GenericImageView;

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
        ImageSource::Bytes(bytes) => image::load_from_memory(bytes)
            .ok()
            .map(|img| img.dimensions()),
        ImageSource::SvgPath(path) => get_svg_size_from_file(path),
        ImageSource::SvgBytes(bytes) => get_svg_size_from_bytes(bytes),
    }
}

/// Get SVG dimensions from a file path.
fn get_svg_size_from_file(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    get_svg_size_from_bytes(&data)
}

/// Get SVG dimensions from raw bytes.
fn get_svg_size_from_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    Some((size.width() as u32, size.height() as u32))
}
