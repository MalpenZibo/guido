//! Paint context for the hierarchical render tree.

use crate::renderer::primitives::{Gradient, Shadow};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::image::{ContentFit, ImageSource};
use crate::widgets::{Color, Rect};

use super::commands::{Border, DrawCommand};
use super::tree::{ClipRegion, NodeId, RenderNode};

/// Painting context for the V2 renderer.
///
/// Widgets use this to build their render node. Unlike the V1 PaintContext,
/// there's no push/pop for transforms - they're set once per node
/// and inherited automatically by children.
///
/// All drawing is done in LOCAL coordinates (0,0 is the widget's top-left).
/// Positioning is handled via transforms:
/// - Parent sets child's position via `set_transform` before calling `paint_v2`
/// - Child applies its own user transform (rotation, scale) via `apply_transform`
///
/// # Example
///
/// ```ignore
/// fn paint_v2(&self, ctx: &mut PaintContextV2) {
///     // Local bounds (0,0 origin with widget's own width/height)
///     let local_bounds = Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height);
///     ctx.set_bounds(local_bounds);
///
///     // Apply user transform (rotation, scale) - composes with parent's position transform
///     // Parent already set our position via set_transform before calling paint_v2
///     if !self.user_transform.is_identity() {
///         ctx.apply_transform_with_origin(self.user_transform, self.transform_origin);
///     }
///
///     // Draw background in LOCAL coordinates
///     ctx.draw_rounded_rect(local_bounds, Color::BLUE, 8.0);
///
///     // Paint children - set their position, then let them apply their own transforms
///     for child in &self.children {
///         let child_global = child.bounds();
///         let child_local = Rect::new(0.0, 0.0, child_global.width, child_global.height);
///         let child_offset_x = child_global.x - self.bounds.x;
///         let child_offset_y = child_global.y - self.bounds.y;
///
///         let mut child_ctx = ctx.add_child(child.id(), child_local);
///         child_ctx.set_transform(Transform::translate(child_offset_x, child_offset_y));
///         child.paint_v2(&mut child_ctx);  // Child will apply its own user transform
///     }
///
///     // Draw overlay effects (after children) in LOCAL coords
///     ctx.draw_overlay_circle(cx, cy, radius, color);
/// }
/// ```
pub struct PaintContextV2<'a> {
    /// The node being built
    node: &'a mut RenderNode,
}

impl<'a> PaintContextV2<'a> {
    /// Create a context for painting to a node.
    pub fn new(node: &'a mut RenderNode) -> Self {
        Self { node }
    }

    // -------------------------------------------------------------------------
    // Node Properties
    // -------------------------------------------------------------------------

    /// Set this node's bounds (for transform origin resolution).
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.node.bounds = bounds;
    }

    /// Set this node's local transform (replaces any existing transform).
    pub fn set_transform(&mut self, transform: Transform) {
        self.node.local_transform = transform;
    }

    /// Apply a transform by composing it with the existing transform.
    ///
    /// The new transform is applied AFTER the existing transform:
    /// `result = existing.then(transform)`
    ///
    /// Use this when a child widget needs to add its own transform (rotation, scale)
    /// on top of the position transform set by its parent.
    pub fn apply_transform(&mut self, transform: Transform) {
        if !transform.is_identity() {
            self.node.local_transform = self.node.local_transform.then(&transform);
        }
    }

    /// Apply a transform with origin by composing it with the existing transform.
    pub fn apply_transform_with_origin(&mut self, transform: Transform, origin: TransformOrigin) {
        if !transform.is_identity() {
            self.node.local_transform = self.node.local_transform.then(&transform);
            self.node.transform_origin = origin;
        }
    }

    /// Set this node's transform origin.
    pub fn set_transform_origin(&mut self, origin: TransformOrigin) {
        self.node.transform_origin = origin;
    }

    /// Set this node's local transform with origin.
    pub fn set_transform_with_origin(&mut self, transform: Transform, origin: TransformOrigin) {
        self.node.local_transform = transform;
        self.node.transform_origin = origin;
    }

    // -------------------------------------------------------------------------
    // Clipping
    // -------------------------------------------------------------------------

    /// Set a clip region for this node and its children.
    ///
    /// The clip rect is in local coordinates (0,0 = node origin).
    /// All children of this node will be clipped to this region.
    ///
    /// # Arguments
    /// * `rect` - The clip rectangle in local coordinates
    /// * `corner_radius` - Corner radius for rounded clipping
    /// * `curvature` - Superellipse curvature (K-value: 1.0=circle, 2.0=squircle)
    pub fn set_clip(&mut self, rect: Rect, corner_radius: f32, curvature: f32) {
        self.node.clip = Some(ClipRegion {
            rect,
            corner_radius,
            curvature,
        });
    }

    /// Set a rectangular clip (no rounded corners).
    ///
    /// This is a convenience method for `set_clip(rect, 0.0, 1.0)`.
    pub fn set_clip_rect(&mut self, rect: Rect) {
        self.set_clip(rect, 0.0, 1.0);
    }

    /// Set a clip region only for overlay commands (doesn't clip children).
    ///
    /// Use this for effects like ripples that need to be clipped to the
    /// container's rounded corners without affecting child content.
    ///
    /// # Arguments
    /// * `rect` - The clip rectangle in local coordinates
    /// * `corner_radius` - Corner radius for rounded clipping
    /// * `curvature` - Superellipse curvature (K-value: 1.0=circle, 2.0=squircle)
    pub fn set_overlay_clip(&mut self, rect: Rect, corner_radius: f32, curvature: f32) {
        self.node.overlay_clip = Some(ClipRegion {
            rect,
            corner_radius,
            curvature,
        });
    }

    // -------------------------------------------------------------------------
    // Draw Commands (Main Layer)
    // -------------------------------------------------------------------------

    /// Draw a rounded rectangle in local coordinates.
    pub fn draw_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature: 1.0,
            border: None,
            shadow: None,
            gradient: None,
        });
    }

    /// Draw a rounded rectangle with curvature in local coordinates.
    pub fn draw_rounded_rect_with_curvature(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border: None,
            shadow: None,
            gradient: None,
        });
    }

    /// Draw a rounded rectangle with gradient.
    pub fn draw_gradient_rect(
        &mut self,
        rect: Rect,
        gradient: Gradient,
        radius: f32,
        curvature: f32,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color: gradient.start_color, // Fallback color
            radius,
            curvature,
            border: None,
            shadow: None,
            gradient: Some(gradient),
        });
    }

    /// Draw a border frame (no fill).
    pub fn draw_border_frame(
        &mut self,
        rect: Rect,
        border_color: Color,
        radius: f32,
        border_width: f32,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color: Color::TRANSPARENT,
            radius,
            curvature: 1.0,
            border: Some(Border::new(border_width, border_color)),
            shadow: None,
            gradient: None,
        });
    }

    /// Draw a border frame with curvature.
    pub fn draw_border_frame_with_curvature(
        &mut self,
        rect: Rect,
        border_color: Color,
        radius: f32,
        border_width: f32,
        curvature: f32,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color: Color::TRANSPARENT,
            radius,
            curvature,
            border: Some(Border::new(border_width, border_color)),
            shadow: None,
            gradient: None,
        });
    }

    /// Draw a rounded rectangle with shadow.
    pub fn draw_rounded_rect_with_shadow(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
        shadow: Shadow,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border: None,
            shadow: Some(shadow),
            gradient: None,
        });
    }

    /// Draw a fully configured rounded rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_rounded_rect_full(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
        border: Option<Border>,
        shadow: Option<Shadow>,
        gradient: Option<Gradient>,
    ) {
        self.node.commands.push(DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border,
            shadow,
            gradient,
        });
    }

    /// Draw a circle in local coordinates.
    pub fn draw_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.node.commands.push(DrawCommand::Circle {
            center: (cx, cy),
            radius,
            color,
        });
    }

    // -------------------------------------------------------------------------
    // Text Commands
    // -------------------------------------------------------------------------

    /// Draw text with default font settings.
    pub fn draw_text(&mut self, text: &str, rect: Rect, color: Color, font_size: f32) {
        self.draw_text_styled(
            text,
            rect,
            color,
            font_size,
            FontFamily::default(),
            FontWeight::NORMAL,
        );
    }

    /// Draw text with custom font family and weight.
    pub fn draw_text_styled(
        &mut self,
        text: &str,
        rect: Rect,
        color: Color,
        font_size: f32,
        font_family: FontFamily,
        font_weight: FontWeight,
    ) {
        // Skip empty text
        if text.is_empty() {
            return;
        }
        self.node.commands.push(DrawCommand::Text {
            text: text.to_string(),
            rect,
            color,
            font_size,
            font_family,
            font_weight,
        });
    }

    // -------------------------------------------------------------------------
    // Image Commands
    // -------------------------------------------------------------------------

    /// Draw an image in local coordinates.
    pub fn draw_image(&mut self, source: ImageSource, rect: Rect, content_fit: ContentFit) {
        self.node.commands.push(DrawCommand::Image {
            source,
            rect,
            content_fit,
        });
    }

    // -------------------------------------------------------------------------
    // Children
    // -------------------------------------------------------------------------

    /// Add a child node and get a context to paint into it.
    ///
    /// The child will inherit transforms from this node automatically
    /// during tree flattening.
    pub fn add_child(&mut self, id: NodeId, bounds: Rect) -> PaintContextV2<'_> {
        self.node.children.push(RenderNode::with_bounds(id, bounds));
        let child = self.node.children.last_mut().expect("child was just pushed");
        PaintContextV2::new(child)
    }

    /// Add a child node with a pre-built node.
    pub fn add_child_node(&mut self, node: RenderNode) {
        self.node.children.push(node);
    }

    // -------------------------------------------------------------------------
    // Overlay Commands (After Children)
    // -------------------------------------------------------------------------

    /// Draw a circle as overlay (rendered after children).
    /// Used for ripple effects that should appear on top of child content.
    pub fn draw_overlay_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        self.node.overlay_commands.push(DrawCommand::Circle {
            center: (cx, cy),
            radius,
            color,
        });
    }

    /// Draw a rounded rectangle as overlay (rendered after children).
    pub fn draw_overlay_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        self.node.overlay_commands.push(DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature: 1.0,
            border: None,
            shadow: None,
            gradient: None,
        });
    }
}
