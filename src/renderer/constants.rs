//! Rendering constants to avoid magic numbers throughout the codebase.

/// Padding around text textures in scaled pixels (for SDF anti-aliasing).
pub const TEXT_TEXTURE_PADDING: f32 = 4.0;

/// Extra margin multiplier for text buffer size to account for font rendering differences.
pub const TEXT_BUFFER_MARGIN_MULTIPLIER: f32 = 1.1;

/// How much finer than the surface a text is rasterized when it is drawn from
/// a texture rather than by glyphon: at a 2x scale animation's peak the letters
/// are still sampled from something denser than the screen.
///
/// Fixed rather than taken from the transform, so a scale animation stretches
/// one texture instead of asking for a new one every frame.
pub const TEXT_SUPERSAMPLE: f32 = 2.0;

/// Quality multiplier for SVG rendering.
/// Higher values produce sharper SVGs at the cost of texture memory.
pub const SVG_QUALITY_MULTIPLIER: f32 = 2.0;

/// Number of bytes to sample from each section when hashing large images.
/// Used to avoid hashing entire large images for cache keys.
pub const IMAGE_HASH_SAMPLE_SIZE: usize = 256;
