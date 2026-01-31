//! Shared types for the renderer.

use crate::transform::Transform;
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::{Color, Rect};

/// Gradient direction for linear gradients
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientDir {
    Horizontal,
    Vertical,
    Diagonal,
    DiagonalReverse,
}

/// Optional gradient for shapes
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub start_color: Color,
    pub end_color: Color,
    pub direction: GradientDir,
}

/// Shadow configuration for shapes
#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    /// Shadow offset in logical pixels (x, y)
    pub offset: (f32, f32),
    /// Blur radius in logical pixels
    pub blur: f32,
    /// Spread amount in logical pixels (expands shadow)
    pub spread: f32,
    /// Shadow color
    pub color: Color,
}

impl Shadow {
    /// Create a shadow with the given parameters
    pub fn new(offset: (f32, f32), blur: f32, spread: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread,
            color,
        }
    }

    /// Create a shadow with no spread
    pub fn simple(offset: (f32, f32), blur: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread: 0.0,
            color,
        }
    }

    /// Create a default shadow (no shadow)
    pub fn none() -> Self {
        Self {
            offset: (0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
        }
    }
}

/// A text entry for rendering, containing all information needed to render text.
#[derive(Debug, Clone)]
pub struct TextEntry {
    /// The text string to render
    pub text: String,
    /// The bounding rectangle for the text in logical pixels
    pub rect: Rect,
    /// The text color
    pub color: Color,
    /// The font size in logical pixels
    pub font_size: f32,
    /// The font family
    pub font_family: FontFamily,
    /// The font weight
    pub font_weight: FontWeight,
    /// Optional clip rectangle to constrain text rendering
    pub clip_rect: Option<Rect>,
    /// Transform to apply to this text
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
}

/// An image entry for rendering.
#[derive(Clone)]
pub struct ImageEntry {
    /// The image source
    pub source: ImageSource,
    /// The bounding rectangle for the image in logical pixels
    pub rect: Rect,
    /// How the image content should fit within its bounds
    pub content_fit: ContentFit,
    /// Optional clip rectangle to constrain image rendering
    pub clip_rect: Option<Rect>,
    /// Transform to apply to this image
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
}
