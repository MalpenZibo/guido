//! Tree flattening with world transform computation.

use crate::transform::Transform;
use crate::widgets::Rect;

use super::commands::DrawCommand;
use super::tree::{ClipRegion, RenderNode, RenderTree};

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

/// Clip region transformed to world space (axis-aligned bounding box).
///
/// When a node has a clip region and its parent has rotation, the clip
/// becomes an axis-aligned bounding box in world space.
#[derive(Debug, Clone)]
pub struct WorldClip {
    /// Axis-aligned clip rect in world coordinates (logical pixels).
    pub rect: Rect,
    /// Corner radius for rounded clipping (in logical pixels).
    pub corner_radius: f32,
    /// Superellipse curvature (K-value).
    pub curvature: f32,
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
    /// Clip region in world coordinates (if any).
    pub clip: Option<WorldClip>,
    /// Whether the clip is in local coordinates (use frag_pos in shader instead of world_pos).
    /// This is true for overlay clips on transformed containers.
    pub clip_is_local: bool,
}

/// Flatten a render tree into a list of commands ready for GPU submission.
///
/// This walks the tree depth-first, computing world transforms as it goes.
/// Commands are sorted by layer for correct render order.
pub fn flatten_tree(tree: &RenderTree) -> Vec<FlattenedCommand> {
    let mut commands = Vec::new();
    flatten_tree_into(tree, &mut commands);
    commands
}

/// Flatten a render tree into an existing buffer (clears and reuses capacity).
///
/// This is more efficient than `flatten_tree` when called repeatedly,
/// as it avoids reallocating the output vector each frame.
pub fn flatten_tree_into(tree: &RenderTree, commands: &mut Vec<FlattenedCommand>) {
    commands.clear();

    for root in &tree.roots {
        flatten_node(root, Transform::IDENTITY, None, None, commands);
    }

    // Sort by layer for correct render order
    commands.sort_by_key(|c| c.layer);
}

/// Recursively flatten a node and its children.
fn flatten_node(
    node: &RenderNode,
    parent_world_transform: Transform,
    parent_world_origin: Option<(f32, f32)>,
    parent_clip: Option<&WorldClip>,
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

    // Compute this node's world clip (if any)
    let node_world_clip = node
        .clip
        .as_ref()
        .map(|clip| transform_clip_to_world(clip, &world_transform));

    // Effective clip = intersection of parent clip and node clip
    let effective_clip: Option<WorldClip> = match (parent_clip, &node_world_clip) {
        (Some(parent), Some(node_clip)) => Some(intersect_clips(parent, node_clip)),
        (Some(parent), None) => Some(parent.clone()),
        (None, Some(node_clip)) => Some(node_clip.clone()),
        (None, None) => None,
    };

    // Add main commands with appropriate layers and clip
    for cmd in &node.commands {
        let layer = match cmd {
            DrawCommand::Text { .. } => RenderLayer::Text,
            DrawCommand::Image { .. } => RenderLayer::Images,
            _ => RenderLayer::Shapes,
        };
        out.push(FlattenedCommand {
            command: clone_command(cmd),
            world_transform,
            world_transform_origin: world_origin,
            layer,
            clip: effective_clip.clone(),
            clip_is_local: false,
        });
    }

    // Recurse to children with effective clip
    for child in &node.children {
        flatten_node(
            child,
            world_transform,
            world_origin,
            effective_clip.as_ref(),
            out,
        );
    }

    // Compute overlay-specific clip (if set)
    // For overlay clips (ripples), keep the clip in LOCAL space so it follows the shape's transform.
    // This ensures ripples are clipped to the rotated/scaled container, not an AABB.
    let (overlay_clip, overlay_clip_is_local): (Option<WorldClip>, bool) =
        if let Some(ref clip) = node.overlay_clip {
            // Keep overlay clip in LOCAL space - don't transform to world AABB
            let local_clip = WorldClip {
                rect: clip.rect,
                corner_radius: clip.corner_radius,
                curvature: clip.curvature,
            };
            (Some(local_clip), true)
        } else {
            // Fall back to effective_clip (which is in world space)
            (effective_clip.clone(), false)
        };

    // Add overlay commands (layer = Overlay) with overlay-specific clip
    for cmd in &node.overlay_commands {
        out.push(FlattenedCommand {
            command: clone_command(cmd),
            world_transform,
            world_transform_origin: world_origin,
            layer: RenderLayer::Overlay,
            clip: overlay_clip.clone(),
            clip_is_local: overlay_clip_is_local,
        });
    }
}

/// Transform a local clip region to world space (axis-aligned bounding box).
///
/// When the transform includes rotation, the clip becomes the AABB of
/// the rotated rectangle. This is a conservative approximation that
/// ensures no clipped content is visible outside the clip region.
fn transform_clip_to_world(clip: &ClipRegion, transform: &Transform) -> WorldClip {
    // Transform all 4 corners and compute AABB
    let corners = [
        transform.transform_point(clip.rect.x, clip.rect.y),
        transform.transform_point(clip.rect.x + clip.rect.width, clip.rect.y),
        transform.transform_point(clip.rect.x, clip.rect.y + clip.rect.height),
        transform.transform_point(
            clip.rect.x + clip.rect.width,
            clip.rect.y + clip.rect.height,
        ),
    ];

    let min_x = corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|(x, _)| *x)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|(_, y)| *y)
        .fold(f32::NEG_INFINITY, f32::max);

    // Scale corner radius by transform scale
    let scale = transform.extract_scale();
    let scaled_radius = clip.corner_radius * scale;

    WorldClip {
        rect: Rect::new(min_x, min_y, max_x - min_x, max_y - min_y),
        corner_radius: scaled_radius,
        curvature: clip.curvature,
    }
}

/// Compute the intersection of two clip regions.
///
/// Returns the tighter of the two clips. For simplicity, we use the
/// intersection of the AABBs and take the smaller corner radius.
fn intersect_clips(a: &WorldClip, b: &WorldClip) -> WorldClip {
    // Compute AABB intersection
    let min_x = a.rect.x.max(b.rect.x);
    let min_y = a.rect.y.max(b.rect.y);
    let max_x = (a.rect.x + a.rect.width).min(b.rect.x + b.rect.width);
    let max_y = (a.rect.y + a.rect.height).min(b.rect.y + b.rect.height);

    // Clamp to non-negative dimensions
    let width = (max_x - min_x).max(0.0);
    let height = (max_y - min_y).max(0.0);

    // Use the smaller corner radius (more conservative)
    let corner_radius = a.corner_radius.min(b.corner_radius);
    // Use the curvature from the clip with the smaller radius
    let curvature = if a.corner_radius <= b.corner_radius {
        a.curvature
    } else {
        b.curvature
    };

    WorldClip {
        rect: Rect::new(min_x, min_y, width, height),
        corner_radius,
        curvature,
    }
}

/// Clone a draw command without applying any coordinate transformation.
///
/// All coordinate transformation is handled by the GPU shader using the
/// world_transform stored in FlattenedCommand. This avoids double-transformation
/// issues when rotation/scale is involved.
fn clone_command(cmd: &DrawCommand) -> DrawCommand {
    cmd.clone()
}
