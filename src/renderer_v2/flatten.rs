//! Tree flattening with world transform computation.

use crate::renderer::primitives::ClipRegion;
use crate::transform::Transform;
use crate::widgets::Rect;

use super::commands::DrawCommand;
use super::tree::{RenderNode, RenderTree};

/// Render layer for draw command ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderLayer {
    /// Background shapes (filled rectangles, borders, etc.)
    Shapes = 0,
    /// Text content
    Text = 1,
    /// Overlay effects (ripples, highlights)
    Overlay = 2,
}

/// A draw command with computed world transform and clip.
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
    /// Intersected clip region in world coordinates (None = no clip)
    pub clip: Option<ClipRegion>,
    /// Render layer for ordering
    pub layer: RenderLayer,
}

/// Flatten a render tree into a list of commands ready for GPU submission.
///
/// This walks the tree depth-first, computing world transforms and intersecting
/// clips as it goes. Commands are sorted by layer for correct render order.
pub fn flatten_tree(tree: &RenderTree) -> Vec<FlattenedCommand> {
    let mut commands = Vec::new();

    for root in &tree.roots {
        flatten_node(root, Transform::IDENTITY, None, None, &mut commands);
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
    parent_clip: Option<&ClipRegion>,
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

    // Compute effective clip (intersection with parent)
    let effective_clip = compute_effective_clip(node, &world_transform, parent_clip);

    // Add main commands with appropriate layers
    for cmd in &node.commands {
        let layer = match cmd {
            DrawCommand::Text { .. } => RenderLayer::Text,
            _ => RenderLayer::Shapes,
        };
        out.push(FlattenedCommand {
            command: transform_command(cmd, &world_transform),
            world_transform,
            world_transform_origin: world_origin,
            clip: effective_clip.clone(),
            layer,
        });
    }

    // Recurse to children
    for child in &node.children {
        flatten_node(
            child,
            world_transform,
            world_origin,
            effective_clip.as_ref(),
            out,
        );
    }

    // Add overlay commands (layer = Overlay)
    for cmd in &node.overlay_commands {
        out.push(FlattenedCommand {
            command: transform_command(cmd, &world_transform),
            world_transform,
            world_transform_origin: world_origin,
            clip: effective_clip.clone(),
            layer: RenderLayer::Overlay,
        });
    }
}

/// Compute the effective clip region for a node.
fn compute_effective_clip(
    node: &RenderNode,
    world_transform: &Transform,
    parent_clip: Option<&ClipRegion>,
) -> Option<ClipRegion> {
    match (&node.clip, parent_clip) {
        (Some(local_clip), Some(parent)) => {
            // Transform local clip to world coords and intersect with parent
            let world_clip = transform_clip(local_clip, world_transform);
            Some(intersect_clips(&world_clip, parent))
        }
        (Some(local_clip), None) => Some(transform_clip(local_clip, world_transform)),
        (None, Some(parent)) => Some(parent.clone()),
        (None, None) => None,
    }
}

/// Transform a clip region by a transform.
///
/// For now, we only handle translation. Rotation/scale of clips is complex
/// and will be addressed in a later phase.
fn transform_clip(clip: &ClipRegion, transform: &Transform) -> ClipRegion {
    // Extract translation from transform
    let tx = transform.tx();
    let ty = transform.ty();

    ClipRegion {
        rect: Rect::new(
            clip.rect.x + tx,
            clip.rect.y + ty,
            clip.rect.width,
            clip.rect.height,
        ),
        radius: clip.radius,
        curvature: clip.curvature,
    }
}

/// Intersect two clip regions (axis-aligned bounding box intersection).
fn intersect_clips(a: &ClipRegion, b: &ClipRegion) -> ClipRegion {
    let a_right = a.rect.x + a.rect.width;
    let a_bottom = a.rect.y + a.rect.height;
    let b_right = b.rect.x + b.rect.width;
    let b_bottom = b.rect.y + b.rect.height;

    let left = a.rect.x.max(b.rect.x);
    let top = a.rect.y.max(b.rect.y);
    let right = a_right.min(b_right);
    let bottom = a_bottom.min(b_bottom);

    let width = (right - left).max(0.0);
    let height = (bottom - top).max(0.0);

    // Use the smaller radius and curvature (more restrictive)
    ClipRegion {
        rect: Rect::new(left, top, width, height),
        radius: a.radius.min(b.radius),
        curvature: a.curvature.min(b.curvature),
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
