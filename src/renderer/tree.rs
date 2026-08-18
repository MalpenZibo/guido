//! Render tree data structures.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use smallvec::SmallVec;

use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::widgets::Rect;

use super::commands::DrawCommand;
use super::flatten::FlattenedCommand;

/// Clip region for a render node (in local coordinates).
///
/// When set on a node, this clips the node and all its children
/// to the specified rectangle with optional rounded corners.
#[derive(Debug, Clone)]
pub struct ClipRegion {
    /// The clip rectangle in local coordinates (0,0 = node origin).
    pub rect: Rect,
    /// Corner radius for rounded clipping.
    pub corner_radius: f32,
    /// Superellipse curvature (K-value: 1.0=circle, 2.0=squircle).
    pub curvature: f32,
}

/// Unique identifier for a render node (typically matches widget ID).
pub type NodeId = u64;

/// Cached flattened commands from a previous frame.
///
/// Stored on each RenderNode after flattening, enabling incremental
/// flatten: clean subtrees reuse their cached commands with a
/// translation offset instead of re-flattening.
#[derive(Debug, Clone)]
pub struct CachedFlatten {
    /// The flattened commands produced by this subtree.
    pub commands: Vec<FlattenedCommand>,
    /// The world transform at the time of caching.
    pub world_transform: Transform,
}

/// A node in the render tree representing a widget's visual output.
///
/// Each node contains:
/// - Local transform relative to parent
/// - Draw commands for this node
/// - Child nodes (nested widgets)
/// - Overlay commands rendered after children
///
/// # Sharing model
///
/// Children are `Rc`-shared. The paint cache (`Tree::cache_paint`) stores an
/// `Rc` to the same node that sits in the frame's render tree, so caching a
/// subtree is a refcount bump and "cloning" a cached node copies only the
/// node header (child `Rc`s and `Rc<DrawCommand>`s are bumped, never deep-
/// copied). This is what makes per-frame paint caching O(n) instead of
/// O(n·depth).
///
/// Because cached nodes are shared, the per-frame flags use interior
/// mutability: `repainted` is a `Cell` (flipped to false once the node's
/// output has been cached) and `cached_flatten` is a `RefCell` (written
/// during flatten, which otherwise only needs `&RenderNode`).
#[derive(Debug, Clone)]
pub struct RenderNode {
    /// Unique identifier for this node (matches widget ID)
    pub id: NodeId,

    /// Transform relative to parent (identity by default)
    pub local_transform: Transform,

    /// The position transform set by the parent (before user transforms).
    /// Used for cache reuse: when reusing a cached node with a new parent
    /// position, we can extract the user transform part and recompose.
    pub parent_position: Transform,

    /// Transform origin for local_transform
    pub transform_origin: TransformOrigin,

    /// Bounds in local coordinates (for transform origin resolution)
    pub bounds: Rect,

    /// Draw commands for this node (shapes, text, etc.).
    /// These are in LOCAL coordinates - world transform applied during flatten.
    /// Wrapped in Rc so flatten can share them via Rc::clone() instead of deep-cloning.
    /// SmallVec: most nodes have 1-2 commands (background + border).
    pub commands: SmallVec<[Rc<DrawCommand>; 2]>,

    /// Child nodes (nested widgets), Rc-shared with the paint cache.
    pub children: Vec<Rc<RenderNode>>,

    /// Overlay commands - drawn AFTER all children (for ripples, effects).
    /// These are also in local coordinates.
    /// SmallVec sized for `MAX_LIVE_RIPPLES`: presses overlap, so a container
    /// being clicked repeatedly holds one command per live ripple.
    pub overlay_commands: SmallVec<[Rc<DrawCommand>; 4]>,

    /// Optional clip region that applies to this node and children.
    /// The clip rect is in local coordinates (0,0 = node origin).
    pub clip: Option<ClipRegion>,

    /// Optional clip region that applies only to overlay commands (not children).
    /// Used for effects like ripples that need clipping to rounded corners
    /// without affecting child content.
    pub overlay_clip: Option<ClipRegion>,

    /// Whether this node was freshly painted this frame (true) or reused
    /// from cache (false). The flattener uses this to decide whether to
    /// reuse cached flatten output. Cleared by `cache_paint_results` after
    /// the node's output has been cached — which is why it is a `Cell`:
    /// by then the node is already Rc-shared with the paint cache.
    pub repainted: Cell<bool>,

    /// Whether some children were skipped (culled by cull_rect) during paint.
    /// Partial subtrees are not cached (see `cache_paint_results`, which
    /// propagates partial-ness to ancestors) because their paint is
    /// incomplete — reusing them later would permanently hide the culled
    /// children.
    pub partial: bool,

    /// Cached flattened commands from a previous flatten pass, shared via Rc
    /// so shallow node clones inherit it for free. Interior-mutable because
    /// flatten writes it while the tree is Rc-shared.
    pub cached_flatten: RefCell<Option<Rc<CachedFlatten>>>,
}

impl RenderNode {
    /// Create a new empty render node with the given ID.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            local_transform: Transform::IDENTITY,
            parent_position: Transform::IDENTITY,
            transform_origin: TransformOrigin::CENTER,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            commands: SmallVec::new(),
            children: Vec::new(),
            overlay_commands: SmallVec::new(),
            clip: None,
            overlay_clip: None,
            repainted: Cell::new(true),
            partial: false,
            cached_flatten: RefCell::new(None),
        }
    }

    /// Create a new render node with bounds.
    pub fn with_bounds(id: NodeId, bounds: Rect) -> Self {
        Self {
            bounds,
            ..Self::new(id)
        }
    }

    /// Clear all commands and children for reuse.
    pub fn clear(&mut self) {
        self.local_transform = Transform::IDENTITY;
        self.parent_position = Transform::IDENTITY;
        self.transform_origin = TransformOrigin::CENTER;
        self.commands.clear();
        self.children.clear();
        self.overlay_commands.clear();
        self.clip = None;
        self.overlay_clip = None;
        self.repainted.set(true);
        self.partial = false;
        *self.cached_flatten.borrow_mut() = None;
    }
}
