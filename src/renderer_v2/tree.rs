//! Render tree data structures.

use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::widgets::Rect;

use super::commands::DrawCommand;

/// Unique identifier for a render node (typically matches widget ID).
pub type NodeId = u64;

/// A node in the render tree representing a widget's visual output.
///
/// Each node contains:
/// - Local transform relative to parent
/// - Draw commands for this node
/// - Child nodes (nested widgets)
/// - Overlay commands rendered after children
#[derive(Debug, Clone)]
pub struct RenderNode {
    /// Unique identifier for this node (matches widget ID)
    pub id: NodeId,

    /// Transform relative to parent (identity by default)
    pub local_transform: Transform,

    /// Transform origin for local_transform
    pub transform_origin: TransformOrigin,

    /// Bounds in local coordinates (for transform origin resolution)
    pub bounds: Rect,

    /// Draw commands for this node (shapes, text, etc.).
    /// These are in LOCAL coordinates - world transform applied during flatten.
    pub commands: Vec<DrawCommand>,

    /// Child nodes (nested widgets)
    pub children: Vec<RenderNode>,

    /// Overlay commands - drawn AFTER all children (for ripples, effects).
    /// These are also in local coordinates.
    pub overlay_commands: Vec<DrawCommand>,
}

impl RenderNode {
    /// Create a new empty render node with the given ID.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            local_transform: Transform::IDENTITY,
            transform_origin: TransformOrigin::CENTER,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            commands: Vec::new(),
            children: Vec::new(),
            overlay_commands: Vec::new(),
        }
    }

    /// Create a new render node with bounds.
    pub fn with_bounds(id: NodeId, bounds: Rect) -> Self {
        Self {
            id,
            bounds,
            ..Self::new(id)
        }
    }

    /// Clear all commands and children for reuse.
    pub fn clear(&mut self) {
        self.local_transform = Transform::IDENTITY;
        self.transform_origin = TransformOrigin::CENTER;
        self.commands.clear();
        self.children.clear();
        self.overlay_commands.clear();
    }
}

/// The complete render tree for a frame.
///
/// Contains root nodes (one per surface or top-level widget).
#[derive(Debug, Default)]
pub struct RenderTree {
    /// Root nodes
    pub roots: Vec<RenderNode>,
}

impl RenderTree {
    /// Create a new empty render tree.
    pub fn new() -> Self {
        Self { roots: Vec::new() }
    }

    /// Add a root node to the tree.
    pub fn add_root(&mut self, node: RenderNode) {
        self.roots.push(node);
    }

    /// Clear the tree for reuse (preserves capacity).
    pub fn clear(&mut self) {
        self.roots.clear();
    }

    /// Check if the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}
