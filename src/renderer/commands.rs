//! Draw command definitions for the render tree.

use super::merge::{MergeMember, MergedOutline};
use super::path::{Cap, Fill, Path, Stroke};
use super::text_measurer::LineFit;
use super::types::{Gradient, Shadow};
use crate::image_decode::DecodedImage;
use crate::widgets::font::{FontFamily, FontFeatures, FontStyle, FontWeight, LineHeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::text_style::TextStroke;
use crate::widgets::{Color, Point, Rect, TextAlign};

/// Border definition for shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// These radii after `permutation` has moved each corner.
    ///
    /// `permutation[i]` says where corner `i` lands, so the radius that was at
    /// `i` is the radius at `permutation[i]` afterwards — see
    /// [`Transform::corner_permutation`](crate::transform::Transform::corner_permutation),
    /// which is the only thing that produces one.
    ///
    /// Four numbers in a fixed order is what made a quarter turn between two
    /// clips unanswerable until #397: the rect survived the turn and the
    /// corners had nowhere to go.
    pub fn permuted(&self, permutation: [usize; 4]) -> Self {
        let from = self.to_array();
        let mut to = [0.0; 4];
        for (i, &dest) in permutation.iter().enumerate() {
            to[dest] = from[i];
        }
        Self {
            top_left: to[0],
            top_right: to[1],
            bottom_right: to[2],
            bottom_left: to[3],
        }
    }

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

    /// The largest of the four radii.
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

/// A single radius on all four corners.
impl From<f32> for CornerRadii {
    fn from(radius: f32) -> Self {
        Self::uniform(radius)
    }
}

impl From<f64> for CornerRadii {
    fn from(radius: f64) -> Self {
        Self::uniform(radius as f32)
    }
}

impl From<i32> for CornerRadii {
    fn from(radius: i32) -> Self {
        Self::uniform(radius as f32)
    }
}

impl From<u32> for CornerRadii {
    fn from(radius: u32) -> Self {
        Self::uniform(radius as f32)
    }
}

impl From<u16> for CornerRadii {
    fn from(radius: u16) -> Self {
        Self::uniform(radius as f32)
    }
}

/// `[top, bottom]` — the top pair and the bottom pair.
///
/// CSS pairs its two-value shorthand diagonally (`top-left & bottom-right`,
/// then the other two), which is a curiosity almost nobody writes on purpose;
/// what people write is "round the top of this row". It deliberately does not
/// mirror `padding([a, b])`, which pairs *opposite sides* — corners have no
/// opposite pairs to give. The four-value form below *is* CSS.
impl From<[f32; 2]> for CornerRadii {
    fn from(v: [f32; 2]) -> Self {
        Self {
            top_left: v[0],
            top_right: v[0],
            bottom_right: v[1],
            bottom_left: v[1],
        }
    }
}

/// `[top-left, top-right, bottom-right, bottom-left]` — clockwise, as CSS.
impl From<[f32; 4]> for CornerRadii {
    fn from(v: [f32; 4]) -> Self {
        Self {
            top_left: v[0],
            top_right: v[1],
            bottom_right: v[2],
            bottom_left: v[3],
        }
    }
}

/// `[top, bottom]`, integer.
impl From<[i32; 2]> for CornerRadii {
    fn from(v: [i32; 2]) -> Self {
        [v[0] as f32, v[1] as f32].into()
    }
}

/// `[top-left, top-right, bottom-right, bottom-left]`, integer.
impl From<[i32; 4]> for CornerRadii {
    fn from(v: [i32; 4]) -> Self {
        [v[0] as f32, v[1] as f32, v[2] as f32, v[3] as f32].into()
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
///
/// Comparable, because a layer's texture is kept for as long as what it was
/// drawn from is the same: a container that repaints only to change its own
/// opacity draws the same background again, as a new value.
#[derive(Debug, Clone, PartialEq)]
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
        /// Whether this is the box a container draws for itself — its fill
        /// or its border frame. A parent that merges its children's shapes
        /// takes these into the merged shape and draws them there instead;
        /// see [`Container::merge_children`](crate::widgets::Container::merge_children).
        background: bool,
        /// Drawn for its shadow alone, beneath a box drawn with the same fill:
        /// a shadow past a box's first. The fill is not drawn here, but it says
        /// how much of the shadow the box over it lets through.
        shadow_only: bool,
    },

    /// The backgrounds of a container's direct children, drawn as one shape
    /// whose members join where they come within `distance` of each other.
    ///
    /// Never painted: flatten builds it, in place of the members' own
    /// backgrounds, from the children of a node that declared
    /// [`MergeChildren`](super::merge::MergeChildren).
    MergedShape {
        /// Each member, placed in this command's own space.
        members: Vec<MergeMember>,
        /// How close two members come before they reach for each other, in
        /// logical pixels; they join at half of it.
        distance: f32,
        /// Drawn along the merged outline in place of the members' own.
        border: Option<Border>,
        /// Cast by the merged shape, all outer.
        shadows: super::types::Shadows,
        /// What the members and the necks between them can cover, in this
        /// command's own space.
        bounds: Rect,
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

    /// Stroke a circular arc: `sweep` degrees of the circle of `radius`
    /// around `center`, from `start`. Angles are measured clockwise from
    /// twelve o'clock, and a negative sweep runs the other way.
    ///
    /// Drawn exactly, as one shape — not tessellated — so a sweep that moves
    /// every frame costs the frame nothing more than a sweep that stands
    /// still.
    Arc {
        /// Centre of the circle, in local coordinates.
        center: Point,
        /// Radius of the line's centre, in logical pixels.
        radius: f32,
        /// Where the arc begins, in degrees clockwise from twelve o'clock.
        start: f32,
        /// How far it runs, in degrees. A full turn or more is a ring.
        sweep: f32,
        /// Width of the line, in logical pixels.
        width: f32,
        /// Colour of the line.
        color: Color,
        /// How both ends of an arc short of a ring are drawn.
        cap: Cap,
    },

    /// Fill a path, stroke it, or both — the fill first.
    Path {
        /// The path, in local coordinates.
        path: Path,
        fill: Option<Fill>,
        stroke: Option<Stroke>,
    },

    /// Draw text.
    Text {
        /// The text string to render. Shared rather than owned: the renderer
        /// takes it into a [`TextEntry`](super::TextEntry) every frame the
        /// text is drawn, and a decorated text pushes it once per copy.
        text: std::rc::Rc<str>,
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
        /// Upright, italic or oblique
        font_style: FontStyle,
        /// The OpenType features it is shaped with
        font_features: FontFeatures,
        /// How tall each line is
        line_height: LineHeight,
        /// Extra advance after every glyph, in logical pixels
        letter_spacing: f32,
        /// Where each line sits across `rect`.
        align: TextAlign,
        /// The lines it is cut to, when it is cut. `None` draws it whole in
        /// its box.
        fit: Option<LineFit>,
    },

    /// Where pointer and touch input reaches this surface, or stops reaching
    /// it. Draws nothing: it rides the frame so that the region published to
    /// the compositor is read off the same list the pixels came from, which is
    /// what keeps a hidden, culled or cached container from leaving a stale
    /// hole behind it.
    InputRegion {
        /// The area declared, in local coordinates.
        rect: Rect,
        /// Corner radii of that area, so the region follows the shape drawn
        /// rather than its bounding box.
        corner_radii: CornerRadii,
        /// Superellipse curvature of those corners. A bevel is smaller than
        /// the circle of the same radius, so a region cut as a circle would
        /// take clicks past the diagonal the container actually drew.
        curvature: f32,
        /// Whether input reaches this area. `false` is a hole in a surface
        /// that otherwise takes input; `true` is an island in one that does
        /// not.
        takes: bool,
    },
    /// Filter what has already been drawn beneath `rect`, in place.
    ///
    /// Ordered before the container's own background so the container paints
    /// over its own blurred backdrop. See [`crate::backdrop`].
    BackdropBlur {
        /// Region to filter, in local coordinates.
        rect: Rect,
        /// Which backdrops this asks for. The surface's own is filtered by the
        /// renderer; the compositor's is published as a `wl_region` collected
        /// off this very command after flattening — which is what keeps the two
        /// halves from disagreeing about whether a container still wants it.
        sources: crate::backdrop::BackdropSources,
        /// Blur radius in logical pixels.
        radius: f32,
        /// Corner radii of the region, so the result is masked to the
        /// container's shape rather than its bounding box.
        corner_radii: CornerRadii,
        /// Superellipse curvature of those corners.
        curvature: f32,
        /// The outline the blur is cut to instead, when it is the blur of a
        /// merging container's members: their smooth union, neck included.
        /// `rect` is then the box around it, and the corners are none.
        merged: Option<std::rc::Rc<MergedOutline>>,
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
        /// Upright, italic or oblique.
        font_style: FontStyle,
        /// The OpenType features it is shaped with.
        font_features: FontFeatures,
        /// How tall each line is.
        line_height: LineHeight,
        /// Extra advance after every glyph, in logical pixels.
        letter_spacing: f32,
        /// Where each line sits across `rect` — the frost has to put its
        /// lines where the letters put theirs.
        align: TextAlign,
        /// The lines the text it frosts is cut to, so the mask is cut there too.
        fit: Option<LineFit>,
    },

    /// A layer's texture, laid over what is beneath it: a subtree drawn
    /// offscreen and composited as one picture.
    ///
    /// No widget paints this. Flatten writes it in place of the subtree, once
    /// it has decided the subtree is a layer — see
    /// [`OpacityMode`](super::tree::OpacityMode) and
    /// [`RenderNode::blur`](super::tree::RenderNode::blur) — and the renderer
    /// draws it as a textured quad, at the opacity, through the clips and by
    /// the transform the subtree was drawn under.
    Layer {
        /// The node composited, which is also what the texture is kept by.
        id: super::tree::NodeId,
        /// What the texture covers, in the node's own space, before any blur
        /// spreads it.
        frame: Rect,
        /// The blur run over the texture before it is laid down, in logical
        /// pixels; zero for none.
        blur: f32,
    },

    /// Draw an image.
    Image {
        /// Image source (path or bytes)
        source: ImageSource,
        /// Where a raster `Path` or `Bytes` source's decoded pixels, or an
        /// SVG's rasters, wait, off the frame, until the renderer takes them
        /// to upload — after which it is empty and the texture is the image.
        /// The renderer never decodes or rasterizes on the render path; `None`
        /// for a source that needs neither.
        decoded: Option<DecodedImage>,
        /// Bounding rectangle in local coordinates
        rect: Rect,
        /// How the image content fits within the rect
        content_fit: ContentFit,
        /// The colour every pixel is drawn in, keeping its alpha. `None`
        /// draws the image's own colours.
        tint: Option<Color>,
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
            background: false,
            shadow_only: false,
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
            background: false,
            shadow_only: false,
        }
    }

    /// Create a border frame: a rounded rectangle that draws only its border.
    pub fn border_frame(
        rect: Rect,
        color: Color,
        radius: impl Into<CornerRadii>,
        width: f32,
        curvature: f32,
    ) -> Self {
        Self::RoundedRect {
            rect,
            color: Color::TRANSPARENT,
            radius: radius.into(),
            curvature,
            border: Some(Border::new(width, color)),
            shadow: None,
            gradient: None,
            background: false,
            shadow_only: false,
        }
    }

    /// `shadow` alone, beneath a rounded rectangle drawn next with the same
    /// `color` and `gradient`: the fill is not drawn, but the shader reads it
    /// to leave the shadow out from under that rectangle.
    pub fn shadow_beneath(
        rect: Rect,
        color: Color,
        radius: CornerRadii,
        curvature: f32,
        shadow: Shadow,
        gradient: Option<Gradient>,
    ) -> Self {
        Self::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border: None,
            shadow: Some(shadow),
            gradient,
            background: false,
            shadow_only: true,
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
