//! Tree flattening with world transform computation.

use crate::transform::Transform;

use super::commands::DrawCommand;
use super::tree::{RenderNode, RenderTree};

/// Render layer for draw command ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
    /// Background shapes (filled rectangles, borders, etc.)
    Shapes = 0,
    /// Image content (after shapes, before text)
    Images = 1,
    /// Text content
    Text = 2,
    /// Overlay effects (ripples, highlights)
    Overlay = 3,
}

/// A draw command with computed world transform.
///
/// This is the flattened representation ready for GPU submission.
#[derive(Debug, Clone)]
pub struct FlattenedCommand {
    /// The draw command
    pub command: DrawCommand,
    /// World transform (composed from all ancestors)
    pub world_transform: Transform,
    /// World transform origin in screen coordinates
    pub world_transform_origin: Option<(f32, f32)>,
    /// Render layer for ordering
    pub layer: RenderLayer,
}

/// Flatten a render tree into a list of commands ready for GPU submission.
///
/// This walks the tree depth-first, computing world transforms as it goes.
/// Commands are sorted by layer for correct render order.
pub fn flatten_tree(tree: &RenderTree) -> Vec<FlattenedCommand> {
    let mut commands = Vec::new();

    for root in &tree.roots {
        flatten_node(root, Transform::IDENTITY, None, &mut commands);
    }

    // Sort by layer for correct render order
    commands.sort_by_key(|c| c.layer);
    commands
}

/// Recursively flatten a node and its children.
fn flatten_node(
    node: &RenderNode,
    parent_world_transform: Transform,
    parent_world_origin: Option<(f32, f32)>,
    out: &mut Vec<FlattenedCommand>,
) {
    // Compute this node's world transform
    let (origin_x, origin_y) = node.transform_origin.resolve(node.bounds);

    // Compose transforms: parent first, then local centered at origin
    let local_centered = if node.local_transform.is_identity() {
        Transform::IDENTITY
    } else {
        node.local_transform.center_at(origin_x, origin_y)
    };
    let world_transform = parent_world_transform.then(&local_centered);

    // Compute world transform origin (for shapes that need it)
    let world_origin = if !node.local_transform.is_identity() {
        let (world_ox, world_oy) = parent_world_transform.transform_point(origin_x, origin_y);
        Some((world_ox, world_oy))
    } else {
        parent_world_origin
    };

    // Add main commands with appropriate layers
    for cmd in &node.commands {
        let layer = match cmd {
            DrawCommand::Text { .. } => RenderLayer::Text,
            DrawCommand::Image { .. } => RenderLayer::Images,
            _ => RenderLayer::Shapes,
        };
        out.push(FlattenedCommand {
            command: transform_command(cmd, &world_transform),
            world_transform,
            world_transform_origin: world_origin,
            layer,
        });
    }

    // Recurse to children
    for child in &node.children {
        flatten_node(child, world_transform, world_origin, out);
    }

    // Add overlay commands (layer = Overlay)
    for cmd in &node.overlay_commands {
        out.push(FlattenedCommand {
            command: transform_command(cmd, &world_transform),
            world_transform,
            world_transform_origin: world_origin,
            layer: RenderLayer::Overlay,
        });
    }
}

/// Clone a draw command (no coordinate transformation).
///
/// All coordinate transformation is handled by the GPU shader using the
/// world_transform stored in FlattenedCommand. This avoids double-transformation
/// issues when rotation/scale is involved.
fn transform_command(cmd: &DrawCommand, _transform: &Transform) -> DrawCommand {
    cmd.clone()
}
