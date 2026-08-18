//! Draw command definitions for the render tree.

use super::types::{Gradient, Shadow};
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::text_style::TextStroke;
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

/// Per-corner radii for rounded rectangles (logical pixels).
///
/// Most shapes use a uniform radius — `f32` converts directly. Per-corner
/// values enable accordion-style lists where the first row rounds only its
/// top corners and the last only its bottom ones.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    /// The same radius on all four corners.
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// `radius` on the top corners, zero on the bottom ones.
    pub fn top(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: 0.0,
            bottom_left: 0.0,
        }
    }

    /// `radius` on the bottom corners, zero on the top ones.
    pub fn bottom(radius: f32) -> Self {
        Self {
            top_left: 0.0,
            top_right: 0.0,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// The largest of the four radii (uniform approximations: clip, blur).
    pub fn max(&self) -> f32 {
        self.top_left
            .max(self.top_right)
            .max(self.bottom_right)
            .max(self.bottom_left)
    }

    /// As `[top_left, top_right, bottom_right, bottom_left]`.
    pub fn to_array(self) -> [f32; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }

    /// Multiply every radius (logical → physical pixels).
    pub fn scaled(self, factor: f32) -> Self {
        Self {
            top_left: self.top_left * factor,
            top_right: self.top_right * factor,
            bottom_right: self.bottom_right * factor,
            bottom_left: self.bottom_left * factor,
        }
    }
}

impl From<f32> for CornerRadii {
    fn from(radius: f32) -> Self {
        Self::uniform(radius)
    }
}

impl From<[f32; 4]> for CornerRadii {
    /// `[top_left, top_right, bottom_right, bottom_left]` — clockwise from
    /// the top left, CSS `border-radius` order.
    fn from(r: [f32; 4]) -> Self {
        Self {
            top_left: r[0],
            top_right: r[1],
            bottom_right: r[2],
            bottom_left: r[3],
        }
    }
}

impl From<(f32, f32, f32, f32)> for CornerRadii {
    /// `(top_left, top_right, bottom_right, bottom_left)`.
    fn from(r: (f32, f32, f32, f32)) -> Self {
        Self {
            top_left: r.0,
            top_right: r.1,
            bottom_right: r.2,
            bottom_left: r.3,
        }
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
        /// Corner radii in logical pixels
        radius: CornerRadii,
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

    /// Filter what has already been drawn beneath `rect`, in place.
    ///
    /// Ordered before the container's own background so the container paints
    /// over its own blurred backdrop. See [`crate::backdrop`].
    BackdropBlur {
        /// Region to filter, in local coordinates.
        rect: Rect,
        /// Blur radius in logical pixels.
        radius: f32,
        /// Corner radii of the region, so the result is masked to the
        /// container's shape rather than its bounding box.
        corner_radii: CornerRadii,
        /// Superellipse curvature of those corners.
        curvature: f32,
    },

    /// Filter what has already been drawn beneath the glyphs of `text`.
    ///
    /// The rectangular sibling above filters a box; this one filters the shape
    /// of the letters, which no formula describes — the renderer rasterizes
    /// them into a coverage mask. Ordered before the text itself, so the glyphs
    /// are painted over their own blurred backdrop.
    ///
    /// Everything needed to shape the mask is repeated here, because the mask
    /// has to come out identical to the text drawn after it.
    TextBackdropBlur {
        /// The text whose coverage is the mask.
        text: String,
        /// A contour to draw around that coverage, once the blur is composited
        /// and before the glyphs land on it. Carried here because the mask it
        /// needs is the one this command already rasterizes — and because the
        /// cheap stroke, copies of the glyphs under the fill, would fill the
        /// letter the frost just cut out.
        stroke: Option<TextStroke>,
        /// The text's layout box in local coordinates.
        rect: Rect,
        /// Blur radius in logical pixels.
        radius: f32,
        /// The font size in logical pixels.
        font_size: f32,
        /// The font family.
        font_family: FontFamily,
        /// The font weight.
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
    pub fn rounded_rect(rect: Rect, color: Color, radius: impl Into<CornerRadii>) -> Self {
        Self::RoundedRect {
            rect,
            color,
            radius: radius.into(),
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
        radius: impl Into<CornerRadii>,
        curvature: f32,
    ) -> Self {
        Self::RoundedRect {
            rect,
            color,
            radius: radius.into(),
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
