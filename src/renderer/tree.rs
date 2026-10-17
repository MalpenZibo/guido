//! Render tree data structures.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use smallvec::SmallVec;

use crate::pivot::Pivot;
use crate::transform::Transform;
use crate::widgets::Rect;

use super::clip::CachedClip;
use super::commands::DrawCommand;
use super::flatten::FlattenedCommand;
use super::merge::MergeChildren;

/// Clip region for a render node (in local coordinates).
///
/// When set on a node, this clips the node and all its children
/// to the specified rectangle with optional rounded corners.
#[derive(Debug, Clone)]
pub struct ClipRegion {
    /// The clip rectangle in local coordinates (0,0 = node origin).
    pub rect: Rect,
    /// Corner radii for rounded clipping — one per corner, so a clip cuts the
    /// shape its container drew rather than an approximation of it.
    pub corner_radius: crate::renderer::CornerRadii,
    /// Superellipse curvature (K-value: 1.0=circle, 2.0=squircle).
    pub curvature: f32,
}

/// Unique identifier for a render node (typically matches widget ID).
pub type NodeId = u64;

/// How an [`opacity`](RenderNode::opacity) below one is applied to a subtree.
///
/// Compose's `CompositingStrategy` makes the same choice under the same two
/// names' worth of meaning, and CSS has only the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpacityMode {
    /// The subtree fades as one picture: where two of its draws overlap, the
    /// overlap fades with the rest instead of showing through.
    ///
    /// The subtree is drawn into a texture of its own and composited once —
    /// but only when two of its draws actually overlap. Where none does,
    /// fading each draw on its own is the same picture, and that is what is
    /// drawn instead, for nothing. A subtree holding a backdrop blur is faded
    /// draw by draw too, so its frost keeps frosting what guido drew beneath
    /// it in the surface rather than only what the subtree drew.
    #[default]
    Auto,
    /// Every draw of the subtree fades on its own, so an overlap shows
    /// through darker. Never drawn offscreen: what a container that knows its
    /// children never overlap, or that wants the old picture, asks for.
    Modulate,
}

/// Cached flattened commands from a previous frame.
///
/// Stored on each RenderNode after flattening, enabling incremental
/// flatten: clean subtrees reuse their cached commands with a
/// translation offset instead of re-flattening.
#[derive(Debug, Clone)]
pub struct CachedFlatten {
    /// The flattened commands produced by this subtree.
    ///
    /// The clip each one names is the subtree's own; a command cut by the clip
    /// the subtree *inherited* names nothing, and is given whatever it inherits
    /// where it is replayed.
    pub commands: Vec<FlattenedCommand>,
    /// The clips this subtree placed, so a replay can place them again where
    /// the subtree now is — see [`ClipTree::replay`](super::clip::ClipTree::replay).
    pub clips: Vec<CachedClip>,
    /// The world transform at the time of caching.
    pub world_transform: Transform,
    /// The opacity the subtree inherited at the time of caching — what every
    /// command in it was multiplied by from above. See
    /// [`replay_fade`](Self::replay_fade).
    pub opacity: f32,
    /// Whether the subtree composites a layer. Its content is not among the
    /// commands kept here — it is drawn from the layer's texture, which a
    /// replay leaves where it is — so the entry stands in only where that
    /// texture is still right: moved by whole device pixels, see
    /// [`replays_layers_by`](Self::replays_layers_by).
    pub holds_layers: bool,
    /// Whether a layer in the subtree was cut by the surface's edge, or put
    /// back as plain commands because none of it was on the surface. What was
    /// kept is then true only where the subtree stood: moved, more of the
    /// layer may be in view than its texture holds, or it may be in view at
    /// all. Such an entry replays only where it was made.
    pub cut_by_surface: bool,
}

impl CachedFlatten {
    /// How far this entry would have to move to stand in for the frame being
    /// flattened — or `None` if it cannot stand in for it at all.
    ///
    /// Replay shifts each command by one `(dx, dy)`, so a subtree that did
    /// anything but translate is refused outright. That is the whole of the
    /// question now. It used to have a second half — whether the clip inherited
    /// from above had made the same journey the content did — because every
    /// command carried that clip resolved into it and replaying one shifted the
    /// viewport along with the rows. A command names its clip instead, and a
    /// clip it did not place itself is looked up where the frame finds it, so
    /// scrolling is the ordinary case rather than the excluded one.
    pub fn replay_offset(&self, world_transform: Transform) -> Option<(f32, f32)> {
        if !self.world_transform.is_translation_only() || !world_transform.is_translation_only() {
            return None;
        }
        Some((
            world_transform.tx() - self.world_transform.tx(),
            world_transform.ty() - self.world_transform.ty(),
        ))
    }

    /// Whether a replay by `(dx, dy)` keeps every layer this entry composites
    /// right: always, for an entry that composites none; for one that does,
    /// only a move by whole device pixels, since a layer's content is drawn on
    /// the device grid where it stands and a new fraction of a pixel draws it
    /// again.
    ///
    /// And an entry whose layer the surface's edge cut, or left out entirely,
    /// only where it was made — see
    /// [`cut_by_surface`](Self::cut_by_surface).
    pub fn replays_layers_by(&self, (dx, dy): (f32, f32), scale: f32) -> bool {
        if self.cut_by_surface {
            return dx.abs() < 1e-6 && dy.abs() < 1e-6;
        }
        let whole = |at: f32| ((at * scale) - (at * scale).round()).abs() < 1e-3;
        !self.holds_layers || (whole(dx) && whole(dy))
    }

    /// What this entry's commands have to be multiplied by to stand in for a
    /// frame that hands the subtree `opacity` from above — or `None` if they
    /// cannot be.
    ///
    /// The opacity half of the same question [`replay_offset`](Self::replay_offset)
    /// asks. A fade on an ancestor repaints that ancestor and nothing under
    /// it, so the subtree below comes back clean under a new inherited
    /// opacity, and its cached commands carry the old one multiplied in. One
    /// ratio puts every one of them right, because each is the inherited
    /// opacity times what the subtree itself declared.
    ///
    /// An entry made under an opacity of zero is refused: every command in it
    /// is zero, and zero says nothing about what the subtree declared.
    pub fn replay_fade(&self, opacity: f32) -> Option<f32> {
        if opacity == self.opacity {
            return Some(1.0);
        }
        (self.opacity > 0.0).then(|| opacity / self.opacity)
    }
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
    pub pivot: Pivot,

    /// How opaque this node and everything under it is drawn, from 0 to 1.
    ///
    /// Multiplied down during flatten the way the world transform is composed
    /// down, and into every colour the subtree draws — each draw on its own,
    /// so where two children overlap, the overlap shows through. `1.0` by
    /// default, which multiplies nothing.
    ///
    /// That is [`OpacityMode::Modulate`], and only what happens when the
    /// subtree is not composited as a layer — see
    /// [`opacity_mode`](Self::opacity_mode).
    pub opacity: f32,

    /// Whether an `opacity` below one fades the subtree as one picture or each
    /// of its draws on its own.
    pub opacity_mode: OpacityMode,

    /// A gaussian blur over everything the subtree draws, as a radius in
    /// logical pixels — the same radius a backdrop blur takes.
    ///
    /// Declaring one, zero included, composites the subtree as a layer: the
    /// blur needs the subtree as one picture to run over, as a CSS `filter`
    /// does. `None` by default.
    pub blur: Option<f32>,

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

    /// Optional clip region that applies only to the children, placed under
    /// `clip`. What a scroller or an `Overflow::Hidden` container cuts its
    /// content to: CSS's overflow clip, which is the padding box and leaves the
    /// box's own fill, border and shadow uncut (#764).
    pub children_clip: Option<ClipRegion>,

    /// Set by a container that merges its children's backgrounds into one
    /// shape — see [`MergeChildren`]. Boxed, because a node is made for every
    /// widget painted and almost none of them merge.
    pub merge: Option<Box<MergeChildren>>,

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
    /// children. Nor does the entry from the last complete paint survive: the
    /// widget painted this frame and painted something else, so that entry is
    /// a picture of it as it no longer is, and it is dropped.
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
            pivot: Pivot::CENTER,
            opacity: 1.0,
            opacity_mode: OpacityMode::Auto,
            blur: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            commands: SmallVec::new(),
            children: Vec::new(),
            overlay_commands: SmallVec::new(),
            clip: None,
            overlay_clip: None,
            children_clip: None,
            merge: None,
            repainted: Cell::new(true),
            partial: false,
            cached_flatten: RefCell::new(None),
        }
    }

    /// This node's own space to its parent's: its local transform, about its
    /// pivot. What flatten composes onto the parent's world transform.
    pub fn placement(&self) -> Transform {
        if self.local_transform.is_identity() {
            Transform::IDENTITY
        } else {
            self.local_transform.about(self.pivot, self.bounds)
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
        self.pivot = Pivot::CENTER;
        self.opacity = 1.0;
        self.opacity_mode = OpacityMode::Auto;
        self.blur = None;
        self.commands.clear();
        self.children.clear();
        self.overlay_commands.clear();
        self.clip = None;
        self.overlay_clip = None;
        self.children_clip = None;
        self.merge = None;
        self.repainted.set(true);
        self.partial = false;
        *self.cached_flatten.borrow_mut() = None;
    }
}
