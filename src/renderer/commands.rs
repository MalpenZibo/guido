//! Draw command definitions for the render tree.

use super::types::{Gradient, Shadow};
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::{Color, Rect};

/// Border definition for shapes.
#[derive(Debug, Clone, Copy)]
pub struct Border {
    /// Border width in logical pixels
    pub width: f32,
    /// Border color
    pub color: Color,
}

impl Border {
    /// Create a new border.
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }
}

/// A single draw operation in local coordinates.
///
/// All coordinates and sizes are in the node's local coordinate space.
/// World transforms are applied during tree flattening.
#[derive(Debug, Clone)]
pub enum DrawCommand {
    /// Draw a rounded rectangle with optional gradient, border, shadow.
    RoundedRect {
        /// Rectangle bounds in local coordinates
        rect: Rect,
        /// Fill color
        color: Color,
        /// Corner radius in logical pixels
        radius: f32,
        /// Superellipse curvature (K-value: 1.0 = circle, 2.0 = squircle)
        curvature: f32,
        /// Optional border
        border: Option<Border>,
        /// Optional shadow
        shadow: Option<Shadow>,
        /// Optional gradient (overrides solid color)
        gradient: Option<Gradient>,
    },

    /// Draw a circle (used for ripple effects).
    Circle {
        /// Center point in local coordinates
        center: (f32, f32),
        /// Radius in logical pixels
        radius: f32,
        /// Fill color
        color: Color,
    },

    /// Draw text.
    Text {
        /// The text string to render
        text: String,
        /// The bounding rectangle for the text in local coordinates
        rect: Rect,
        /// The text color
        color: Color,
        /// The font size in logical pixels
        font_size: f32,
        /// The font family
        font_family: FontFamily,
        /// The font weight
        font_weight: FontWeight,
    },

    /// Draw an image.
    Image {
        /// Image source (path or bytes)
        source: ImageSource,
        /// Bounding rectangle in local coordinates
        rect: Rect,
        /// How the image content fits within the rect
        content_fit: ContentFit,
    },
}

impl DrawCommand {
    /// Create a simple rounded rectangle.
    pub fn rounded_rect(rect: Rect, color: Color, radius: f32) -> Self {
        Self::RoundedRect {
            rect,
            color,
            radius,
            curvature: 1.0,
            border: None,
            shadow: None,
            gradient: None,
        }
    }

    /// Create a rounded rectangle with curvature.
    pub fn rounded_rect_with_curvature(
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
    ) -> Self {
        Self::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border: None,
            shadow: None,
            gradient: None,
        }
    }

    /// Create a circle.
    pub fn circle(center: (f32, f32), radius: f32, color: Color) -> Self {
        Self::Circle {
            center,
            radius,
            color,
        }
    }
}
