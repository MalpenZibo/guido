use crate::{Result, VisualTestError};
use image::{Rgba, RgbaImage};
use image_compare::Algorithm;
use std::path::Path;

/// Result of comparing two images
pub struct CompareResult {
    /// Similarity score from 0.0 to 1.0
    pub similarity: f64,
}

/// Compare two images using SSIM algorithm
pub fn compare_images(reference: &Path, captured: &Path) -> Result<CompareResult> {
    let ref_img = image::open(reference)?;
    let cap_img = image::open(captured)?;

    // Convert to RGB for comparison (SSIM works on grayscale or RGB)
    let ref_rgb = ref_img.to_rgb8();
    let cap_rgb = cap_img.to_rgb8();

    // Check dimensions match
    if ref_rgb.dimensions() != cap_rgb.dimensions() {
        return Err(VisualTestError::Compare(format!(
            "Image dimensions don't match: reference {:?} vs captured {:?}",
            ref_rgb.dimensions(),
            cap_rgb.dimensions()
        )));
    }

    // Use SSIM for comparison
    let result =
        image_compare::rgb_similarity_structure(&Algorithm::MSSIMSimple, &ref_rgb, &cap_rgb)
            .map_err(|e| VisualTestError::Compare(format!("SSIM comparison failed: {}", e)))?;

    Ok(CompareResult {
        similarity: result.score,
    })
}

/// Generate a diff image highlighting differences between two images
pub fn generate_diff_image(reference: &Path, captured: &Path, output: &Path) -> Result<()> {
    let ref_img = image::open(reference)?;
    let cap_img = image::open(captured)?;

    let ref_rgba = ref_img.to_rgba8();
    let cap_rgba = cap_img.to_rgba8();

    let (width, height) = ref_rgba.dimensions();

    // Create diff image
    let mut diff_img = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let ref_pixel = ref_rgba.get_pixel(x, y);
            let cap_pixel = cap_rgba.get_pixel(x, y);

            // Calculate pixel difference
            let diff = pixel_difference(ref_pixel, cap_pixel);

            if diff > 10 {
                // Highlight differences in red
                let intensity = (diff as f32 / 255.0 * 200.0 + 55.0) as u8;
                diff_img.put_pixel(x, y, Rgba([intensity, 0, 0, 255]));
            } else {
                // Show original with reduced opacity
                let r = (cap_pixel[0] as u16 / 3) as u8;
                let g = (cap_pixel[1] as u16 / 3) as u8;
                let b = (cap_pixel[2] as u16 / 3) as u8;
                diff_img.put_pixel(x, y, Rgba([r, g, b, 255]));
            }
        }
    }

    diff_img.save(output)?;
    Ok(())
}

/// Calculate the maximum channel difference between two pixels
fn pixel_difference(a: &Rgba<u8>, b: &Rgba<u8>) -> u8 {
    let dr = (a[0] as i16 - b[0] as i16).unsigned_abs() as u8;
    let dg = (a[1] as i16 - b[1] as i16).unsigned_abs() as u8;
    let db = (a[2] as i16 - b[2] as i16).unsigned_abs() as u8;
    dr.max(dg).max(db)
}
