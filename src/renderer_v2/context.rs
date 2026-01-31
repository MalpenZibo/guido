//! Paint context for the hierarchical render tree.

use crate::renderer::primitives::{Gradient, Shadow};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::{Color, Rect};

use super::commands::{Border, DrawCommand};
use super::tree::{NodeId, RenderNode};

/// Painting context for the V2 renderer.
///
/// Widgets use this to build their render node. Unlike the V1 PaintContext,
/// there's no push/pop for transforms - they're set once per node
/// and inherited automatically by children.
///
/// # Example
///
/// ```ignore
/// fn paint_v2(&self, ctx: &mut PaintContextV2) {
///     // Set node properties
///     ctx.set_bounds(self.bounds);
///     ctx.set_transform(self.transform, TransformOrigin::CENTER);
///
///     // Draw background
///     ctx.draw_rounded_rect(self.bounds, Color::BLUE, 8.0);
///
///     // Paint children - each gets its own node
///     for child in &self.children {
///         let mut child_ctx = ctx.add_child(child.id(), child.bounds());
///         child.paint_v2(&mut child_ctx);
///     }
///
///     // Draw overlay effects (after children)
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

    /// Set this node's local transform.
    pub fn set_transform(&mut self, transform: Transform) {
        self.node.local_transform = transform;
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
    // Children
    // -------------------------------------------------------------------------

    /// Add a child node and get a context to paint into it.
    ///
    /// The child will inherit transforms from this node automatically
    /// during tree flattening.
    pub fn add_child(&mut self, id: NodeId, bounds: Rect) -> PaintContextV2<'_> {
        self.node.children.push(RenderNode::with_bounds(id, bounds));
        let child = self.node.children.last_mut().unwrap();
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
