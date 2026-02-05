//! Rendering constants to avoid magic numbers throughout the codebase.

/// Padding around text textures in scaled pixels (for SDF anti-aliasing).
pub const TEXT_TEXTURE_PADDING: f32 = 4.0;

/// Extra margin multiplier for text buffer size to account for font rendering differences.
pub const TEXT_BUFFER_MARGIN_MULTIPLIER: f32 = 1.1;

/// Quality multiplier for SVG rendering.
/// Higher values produce sharper SVGs at the cost of texture memory.
pub const SVG_QUALITY_MULTIPLIER: f32 = 2.0;

/// Number of bytes to sample from each section when hashing large images.
/// Used to avoid hashing entire large images for cache keys.
pub const IMAGE_HASH_SAMPLE_SIZE: usize = 256;
