//! Image metadata utilities for determining intrinsic dimensions.
//!
//! This module provides functions to get image dimensions without loading
//! the full image data, enabling correct layout calculations before rendering.

use std::io::{BufRead, Cursor, Seek};

use image::metadata::Orientation;
use image::{ImageDecoder, ImageReader, ImageResult};

use crate::widgets::image::ImageSource;

/// The size an image source lays out at, in logical pixels, read without
/// decoding or rasterizing it.
///
/// A raster image's is the header's, in whole pixels, turned as its EXIF
/// orientation says: a photo stored landscape and marked a quarter turn lays
/// out portrait, as it is drawn. An SVG's is what its root element declares,
/// which can be fractional and is kept that way: a 0.5 × 1.5 icon is not a
/// 0 × 1 one.
///
/// Returns `None` if the source cannot be read or parsed.
pub fn get_intrinsic_size(source: &ImageSource) -> Option<(f32, f32)> {
    let (width, height) = match source {
        ImageSource::Path(path) => header_size(ImageReader::open(path).ok()?)?,
        // The header, not the image: a full decode here would put back on the
        // frame exactly what `image_decode` takes off it.
        ImageSource::Bytes(bytes) => header_size(ImageReader::new(Cursor::new(&bytes[..])))?,
        ImageSource::Rgba { width, height, .. } => (*width, *height),
        ImageSource::SvgPath(_) | ImageSource::SvgBytes(_) => {
            return parse_svg(source).map(|tree| svg_size(&tree));
        }
    };
    Some((width as f32, height as f32))
}

/// A raster's size as it is shown, read from its header.
fn header_size<R: BufRead + Seek>(reader: ImageReader<R>) -> Option<(u32, u32)> {
    let (decoder, orientation) = raster_decoder(reader).ok()?;
    Some(oriented_size(decoder.dimensions(), orientation))
}

/// A raster's decoder, its format guessed from its contents, and its EXIF
/// orientation — `NoTransforms` when the format reads none or the metadata
/// cannot be read. Sizing and decoding both open it here, so they turn alike.
pub(crate) fn raster_decoder<'a, R: 'a + BufRead + Seek>(
    reader: ImageReader<R>,
) -> ImageResult<(impl ImageDecoder + 'a, Orientation)> {
    let mut decoder = reader.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    Ok((decoder, orientation))
}

/// `(width, height)` stored, as shown once turned by `orientation`: a quarter
/// turn, mirrored or not, swaps the two.
fn oriented_size((width, height): (u32, u32), orientation: Orientation) -> (u32, u32) {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => (width, height),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    /// A 2×1 JPEG whose EXIF says to show it a quarter turn clockwise: a
    /// portrait photo as a phone stores it.
    fn sideways_jpeg() -> Vec<u8> {
        // A little-endian TIFF header and one IFD with one entry: Orientation
        // (tag 0x0112), a SHORT, 6.
        let exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        let mut encoded = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut encoded);
        encoder.set_exif_metadata(exif).unwrap();
        encoder
            .write_image(&[0; 6], 2, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        encoded
    }

    /// The size laid out and the pixels drawn turn alike, whatever the turn.
    #[test]
    fn every_orientation_sizes_as_it_turns_the_pixels() {
        for tag in 1..=8 {
            let orientation = Orientation::from_exif(tag).expect("a TIFF orientation");
            let mut pixels = image::DynamicImage::new_rgb8(3, 2);
            pixels.apply_orientation(orientation);
            assert_eq!(
                oriented_size((3, 2), orientation),
                (pixels.width(), pixels.height()),
                "{orientation:?}"
            );
        }
    }

    #[test]
    fn a_quarter_turn_in_the_exif_lays_the_image_out_turned() {
        let source = ImageSource::Bytes(sideways_jpeg().into());
        assert_eq!(get_intrinsic_size(&source), Some((1.0, 2.0)));
    }
}
