//! Rendering constants to avoid magic numbers throughout the codebase.

/// Padding around text textures in scaled pixels (for SDF anti-aliasing).
pub const TEXT_TEXTURE_PADDING: f32 = 4.0;

/// Extra margin multiplier for text buffer size to account for font rendering differences.
pub const TEXT_BUFFER_MARGIN_MULTIPLIER: f32 = 1.1;

/// How much finer than the surface a text is rasterized when it is drawn from
/// a texture rather than by glyphon: at a 2x scale animation's peak the letters
/// are still sampled from something denser than the screen.
///
/// The same number serves the frost's coverage mask of a transformed text, so
/// the hole and the letters that sit in it are rasterized at one resolution —
/// and, like the glyph texture, the mask is made in the text's own space and
/// left alone as the transform moves, rather than remade for every size an
/// animation passes through. A text no transform stretches needs none of it
/// and is measurably better without.
pub const TEXT_SUPERSAMPLE: f32 = 2.0;

/// Quality multiplier for SVG rendering.
/// Higher values produce sharper SVGs at the cost of texture memory.
pub const SVG_QUALITY_MULTIPLIER: f32 = 2.0;
