//! Tree flattening with world transform computation.

use std::ops::Range;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::transform::Transform;
use crate::widgets::Rect;

use super::commands::{CornerRadii, DrawCommand, EllipticalRadii};
use super::tree::{CachedFlatten, ClipRegion, RenderNode};

/// Render layer for draw command ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RenderLayer {
    /// Backdrop effects, which filter what is already on the target.
    ///
    /// Lowest so that one always opens a draw group: everything it reads must
    /// already have been drawn, which means it must land in a group of its
    /// own, after the groups holding that content.
    #[default]
    Backdrop = 0,
    /// Background shapes (filled rectangles, borders, etc.)
    Shapes = 1,
    /// Image content (after shapes, before text)
    Images = 2,
    /// Text content
    Text = 3,
    /// Overlay effects (ripples, highlights)
    Overlay = 4,
}

/// Clip region transformed to world space (axis-aligned bounding box).
///
/// When a node has a clip region and its parent has rotation, the clip
/// becomes an axis-aligned bounding box in world space.
#[derive(Debug, Clone)]
pub struct WorldClip {
    /// Axis-aligned clip rect in world coordinates (logical pixels).
    pub rect: Rect,
    /// Corner radii for rounded clipping (in logical pixels).
    pub corner_radius: CornerRadii,
    /// Superellipse curvature (K-value).
    pub curvature: f32,
}

/// A draw command with computed world transform.
///
/// This is the flattened representation ready for GPU submission.
/// Uses `Rc<DrawCommand>` so cloning (e.g. for cached flatten reuse)
/// is a reference count bump instead of deep-cloning String/FontFamily.
#[derive(Debug, Clone)]
pub struct FlattenedCommand {
    /// The draw command (shared via Rc to avoid clone overhead)
    pub command: Rc<DrawCommand>,
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

impl FlattenedCommand {
    /// Where a local rounded rect of this command lands in world space: the
    /// axis-aligned box that contains it, and the corner radii grown with it.
    ///
    /// **One implementation, because there are two consumers.** A backdrop blur
    /// is filtered by the renderer and, for the same command, published to the
    /// compositor as a `wl_region` — and the two must describe the same shape.
    /// They did not: one folded four corners and the other subtracted two, so a
    /// container rotated 45° produced a correct region for the renderer and a
    /// zero-width one for the compositor.
    ///
    /// Four corners, because two only bound the box for a transform that keeps
    /// the axes: a rotation moves the extremes onto the other diagonal. And the
    /// radii scale with the box, or a `.scale(2.0)` container reports a shape
    /// whose corners are cut half as deep as the one it draws — each axis by its
    /// own factor, since a corner scaled unevenly is an ellipse and the
    /// geometric mean of the two is neither of them.
    pub fn world_rounded_rect(&self, rect: Rect, radii: CornerRadii) -> (Rect, EllipticalRadii) {
        let (sx, sy) = self.world_transform.extract_scale_components();
        (
            self.world_aabb(rect),
            EllipticalRadii::scaled_xy(radii, sx, sy),
        )
    }

    /// The axis-aligned world box containing a rect of this command's own space.
    fn world_aabb(&self, rect: Rect) -> Rect {
        let corners = [
            self.world_transform.transform_point(rect.x, rect.y),
            self.world_transform
                .transform_point(rect.x + rect.width, rect.y),
            self.world_transform
                .transform_point(rect.x, rect.y + rect.height),
            self.world_transform
                .transform_point(rect.x + rect.width, rect.y + rect.height),
        ];
        let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|c| c.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|c| c.1)
            .fold(f32::NEG_INFINITY, f32::max);
        Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    /// [`world_rounded_rect`](Self::world_rounded_rect), narrowed to what the
    /// command is allowed to show.
    ///
    /// The clip is part of the shape, not a detail of one consumer: a card half
    /// out of a viewport is filtered only where it is on show, and a compositor
    /// region published for the whole card blurs the desktop beside a panel that
    /// is not there. Returns `None` when the clip leaves nothing.
    ///
    /// A corner of the intersection belongs to whichever rectangle supplied both
    /// edges meeting there: the shape's own radius where the clip reached
    /// neither, the *clip's* where it supplied both — a card inside a rounded
    /// scroller is cornered by the scroller, and publishing the scroller's
    /// square bounding box blurs the desktop in the four corners where the panel
    /// is not drawn — and nothing where it supplied one, because a cut edge is
    /// straight.
    ///
    /// An overlay's clip is stored in the command's *local* space so a ripple
    /// follows the shape it belongs to instead of an axis-aligned box around it
    /// — so it is carried into world space here before the two are compared.
    /// Intersecting the two spaces directly reported a region offset by every
    /// translation between the shape and the surface.
    pub fn clipped_world_rounded_rect(
        &self,
        rect: Rect,
        radii: CornerRadii,
    ) -> Option<(Rect, EllipticalRadii)> {
        let (world, world_radii) = self.world_rounded_rect(rect, radii);
        let Some(clip) = self.clip.as_ref() else {
            return Some((world, world_radii));
        };

        // A local clip and the radius that shapes it are in the same space, so
        // both come out through the same transform.
        let (sx, sy) = self.world_transform.extract_scale_components();
        let (clip_rect, clip_radii) = if self.clip_is_local {
            (
                self.world_aabb(clip.rect),
                EllipticalRadii::scaled_xy(clip.corner_radius, sx, sy),
            )
        } else {
            (clip.rect, EllipticalRadii::circular(clip.corner_radius))
        };

        let x = world.x.max(clip_rect.x);
        let y = world.y.max(clip_rect.y);
        let right = (world.x + world.width).min(clip_rect.x + clip_rect.width);
        let bottom = (world.y + world.height).min(clip_rect.y + clip_rect.height);
        if right <= x || bottom <= y {
            return None;
        }

        // Which rectangle supplies each edge — and *both*, where they coincide.
        // Asked as "is this edge at or inside the other's" rather than as "was
        // this edge moved": a child filling its clipping parent exactly shares
        // all four, and read as movement that is no movement at all, so the
        // corners came back as the child's — square, for a child that declares
        // no radius of its own, while the shape on screen is rounded by the
        // parent. Four wedges of blurred desktop outside the panel, which is the
        // artefact this whole computation exists to avoid, in the one case where
        // the two rectangles are equal.
        let (wr, wb) = (world.x + world.width, world.y + world.height);
        let (cr, cb) = (
            clip_rect.x + clip_rect.width,
            clip_rect.y + clip_rect.height,
        );
        let (clip_l, shape_l) = (clip_rect.x >= world.x, world.x >= clip_rect.x);
        let (clip_t, shape_t) = (clip_rect.y >= world.y, world.y >= clip_rect.y);
        let (clip_r, shape_r) = (cr <= wr, wr <= cr);
        let (clip_b, shape_b) = (cb <= wb, wb <= cb);

        // A corner belongs to a rectangle when that rectangle supplies both of
        // the edges meeting there. Both rectangles, where they coincide: the two
        // curves are drawn one on top of the other and the tighter one is what
        // shows. Neither, when one edge comes from each — a cut edge is straight.
        let pick = |shape: f32, clip: f32, from_shape: bool, from_clip: bool| match (
            from_shape, from_clip,
        ) {
            (true, true) => shape.max(clip),
            (true, false) => shape,
            (false, true) => clip,
            (false, false) => 0.0,
        };
        let corners = |shape: CornerRadii, clip: CornerRadii| CornerRadii {
            top_left: pick(
                shape.top_left,
                clip.top_left,
                shape_l && shape_t,
                clip_l && clip_t,
            ),
            top_right: pick(
                shape.top_right,
                clip.top_right,
                shape_r && shape_t,
                clip_r && clip_t,
            ),
            bottom_right: pick(
                shape.bottom_right,
                clip.bottom_right,
                shape_r && shape_b,
                clip_r && clip_b,
            ),
            bottom_left: pick(
                shape.bottom_left,
                clip.bottom_left,
                shape_l && shape_b,
                clip_l && clip_b,
            ),
        };

        Some((
            Rect::new(x, y, right - x, bottom - y),
            EllipticalRadii {
                x: corners(world_radii.x, clip_radii.x),
                y: corners(world_radii.y, clip_radii.y),
            },
        ))
    }
}

/// Draw commands grouped so that batching never reorders drawing.
///
/// Each group buckets its commands by [`RenderLayer`], and the GPU draws a
/// group one bucket at a time — shapes, then images, then text, then overlay.
/// That is what lets commands of a kind share a draw call.
///
/// Bucketing alone would reorder: with a single set of buckets for the whole
/// frame, *every* shape is drawn before *every* image, so a container
/// background painted over an image ends up underneath it however the tree is
/// arranged. A new group is therefore opened when a command's layer goes
/// backwards, and the groups are drawn in order.
///
/// Only when it goes backwards *over something it would cover*, though.
/// Sibling widgets each draw a background and then a label, so a plain
/// "layer went backwards" rule fires between every pair of them — and splits
/// there buy nothing, because siblings do not overlap. Reordering is only
/// observable where the pixels meet, so the group is kept unless the incoming
/// command's bounds actually intersect something already drawn above it. A
/// column of buttons stays one group; a tint over a photo gets two.
struct LayeredCommands {
    groups: Vec<LayerBuckets>,
    /// Whether any command asks the compositor to blur behind it.
    ///
    /// Counted while the commands go past, because the alternative is walking
    /// them all again afterwards to find out — on every painted frame, for the
    /// large majority of surfaces that never ask for it at all.
    compositor_blur: bool,
}

#[derive(Default)]
struct LayerBuckets {
    backdrop: Vec<FlattenedCommand>,
    shapes: Vec<FlattenedCommand>,
    images: Vec<FlattenedCommand>,
    text: Vec<FlattenedCommand>,
    overlay: Vec<FlattenedCommand>,
    /// Highest layer already in this group. Only a command below this can
    /// possibly be drawn too early.
    high_water: RenderLayer,
    /// What each layer of this group covers, so a regression can be tested
    /// for actual overlap.
    bounds: [LayerBounds; LAYER_COUNT],
}

/// What one layer of a group covers.
///
/// A union alone is too coarse. Labels scattered across a panel union into a
/// box that spans the gaps between them, and the next sibling's background
/// lands in one of those gaps and splits for nothing — `examples/showcase.rs`
/// split three times over exactly that, every split a false positive. Keeping
/// the individual rects makes the test exact, up to a cap that stops a huge
/// group turning the scan quadratic; past it the union decides alone, which
/// can only ever over-split.
#[derive(Default)]
struct LayerBounds {
    /// Cheap reject: nothing outside this can intersect anything inside.
    union: Option<Rect>,
    rects: SmallVec<[Rect; 16]>,
    /// Set once `rects` stopped being the whole story.
    overflowed: bool,
}

/// How many rects a layer tracks individually before falling back to the
/// union. Past this the scan costs more than the draw call it saves.
const MAX_TRACKED_RECTS: usize = 64;

impl LayerBounds {
    fn intersects(&self, rect: Rect) -> bool {
        let Some(union) = self.union else {
            return false;
        };
        if !union.intersects(&rect) {
            return false;
        }
        // The union says "maybe"; the rects say for certain, unless there are
        // more of them than we kept.
        self.overflowed || self.rects.iter().any(|held| held.intersects(&rect))
    }

    fn record(&mut self, rect: Option<Rect>) {
        let Some(rect) = rect else {
            // Unbounded: covers anything from here on.
            self.union = Some(EVERYTHING);
            self.overflowed = true;
            return;
        };
        self.union = Some(match self.union {
            Some(union) => union_of(union, rect),
            None => rect,
        });
        if self.rects.len() < MAX_TRACKED_RECTS {
            self.rects.push(rect);
        } else {
            self.overflowed = true;
        }
    }
}

/// Number of [`RenderLayer`] variants.
const LAYER_COUNT: usize = 5;

impl LayerBuckets {
    /// Does `rect` meet anything already drawn above `layer` in this group?
    fn covered_above(&self, layer: RenderLayer, rect: Option<Rect>) -> bool {
        let Some(rect) = rect else {
            // Nothing to test against: assume the worst and split.
            return true;
        };
        (layer as usize + 1..LAYER_COUNT).any(|above| self.bounds[above].intersects(rect))
    }

    fn record(&mut self, layer: RenderLayer, rect: Option<Rect>) {
        self.bounds[layer as usize].record(rect);
    }

    fn bucket_mut(&mut self, layer: RenderLayer) -> &mut Vec<FlattenedCommand> {
        match layer {
            RenderLayer::Backdrop => &mut self.backdrop,
            RenderLayer::Shapes => &mut self.shapes,
            RenderLayer::Images => &mut self.images,
            RenderLayer::Text => &mut self.text,
            RenderLayer::Overlay => &mut self.overlay,
        }
    }
}

impl LayeredCommands {
    fn new() -> Self {
        Self {
            groups: vec![LayerBuckets::default()],
            compositor_blur: false,
        }
    }

    fn push(&mut self, cmd: FlattenedCommand) {
        if let DrawCommand::BackdropBlur { sources, .. } = &*cmd.command
            && sources.contains(crate::backdrop::BackdropSources::COMPOSITOR)
        {
            self.compositor_blur = true;
        }
        let layer = cmd.layer;
        let rect = world_bounds(&cmd);
        // `expect`: `new` seeds one group and nothing ever removes one.
        let current = self.groups.last().expect("at least one group");
        if layer < current.high_water && current.covered_above(layer, rect) {
            self.groups.push(LayerBuckets {
                high_water: layer,
                ..Default::default()
            });
        }
        let current = self.groups.last_mut().expect("at least one group");
        current.high_water = current.high_water.max(layer);
        current.record(layer, rect);
        current.bucket_mut(layer).push(cmd);
    }

    /// Where the buffers stand, so a subtree's own output can be picked out
    /// again afterwards.
    ///
    /// A plain count would not do it. Commands land in the bucket for their
    /// layer, so a shape pushed into a group that already holds text is
    /// inserted *before* that text in draw order — the total is not a
    /// position. Only the last group is ever appended to, so recording its
    /// bucket lengths (and how many groups there were) pins the boundary
    /// exactly.
    fn mark(&self) -> Mark {
        // `expect`: `new` seeds one group and nothing ever removes one.
        let last = self.groups.last().expect("at least one group");
        Mark {
            groups: self.groups.len(),
            buckets: [
                last.backdrop.len(),
                last.shapes.len(),
                last.images.len(),
                last.text.len(),
                last.overlay.len(),
            ],
        }
    }

    /// Every command added since `mark`, in draw order.
    ///
    /// Draw order matters: `CachedFlatten` replays these through [`push`], so
    /// feeding them back in the order they will be drawn reproduces the same
    /// group boundaries next frame.
    ///
    /// [`push`]: Self::push
    fn commands_since(&self, mark: Mark) -> Vec<FlattenedCommand> {
        let mut result = Vec::new();
        for (index, group) in self.groups.iter().enumerate().skip(mark.groups - 1) {
            let buckets = [
                &group.backdrop,
                &group.shapes,
                &group.images,
                &group.text,
                &group.overlay,
            ];
            for (bucket_index, bucket) in buckets.into_iter().enumerate() {
                // Groups opened after the mark are new all through; the group
                // that was current keeps whatever it already held.
                let from = if index == mark.groups - 1 {
                    mark.buckets[bucket_index]
                } else {
                    0
                };
                result.extend_from_slice(&bucket[from..]);
            }
        }
        result
    }

    /// Flatten the groups into one buffer in draw order, recording where each
    /// group's buckets landed.
    fn drain_into(self, out: &mut Vec<FlattenedCommand>, layers: &mut Vec<CommandLayer>) {
        for group in self.groups {
            let mut layer = CommandLayer::default();
            for (bucket, range) in [
                (group.backdrop, &mut layer.backdrop),
                (group.shapes, &mut layer.shapes),
                (group.images, &mut layer.images),
                (group.text, &mut layer.text),
                (group.overlay, &mut layer.overlay),
            ] {
                let start = out.len();
                out.extend(bucket);
                *range = start..out.len();
            }
            // A group left empty by a subtree that drew nothing would cost a
            // pointless set of pipeline switches.
            if !layer.is_empty() {
                layers.push(layer);
            }
        }
    }
}

/// A position in [`LayeredCommands`], for capturing one subtree's output.
#[derive(Debug, Clone, Copy)]
struct Mark {
    /// How many groups existed.
    groups: usize,
    /// Bucket lengths of the group that was current — the only one that can
    /// still grow.
    buckets: [usize; LAYER_COUNT],
}

/// Where one group's commands landed in the flat command buffer.
///
/// Drawn in field order: shapes, images, text, overlay.
#[derive(Debug, Clone, Default)]
pub struct CommandLayer {
    /// Backdrop effects applied to the target before this group draws.
    pub backdrop: Range<usize>,
    pub shapes: Range<usize>,
    pub images: Range<usize>,
    pub text: Range<usize>,
    pub overlay: Range<usize>,
}

impl CommandLayer {
    pub fn is_empty(&self) -> bool {
        self.backdrop.is_empty()
            && self.shapes.is_empty()
            && self.images.is_empty()
            && self.text.is_empty()
            && self.overlay.is_empty()
    }
}

/// Flatten a render tree into existing buffers (cleared and reused).
///
/// Flatten results are cached on nodes (via interior mutability) for
/// incremental reuse in subsequent frames.
///
/// `layers` receives the groups to draw, in order; see [`CommandLayer`].
///
/// Returns whether the frame carries a backdrop blur the *compositor* is asked
/// to apply, which is the only reason to walk the result again and build a
/// `wl_region` from it.
pub fn flatten_root_into(
    root: &RenderNode,
    commands: &mut Vec<FlattenedCommand>,
    layers: &mut Vec<CommandLayer>,
) -> bool {
    commands.clear();
    layers.clear();

    let mut layered = LayeredCommands::new();
    flatten_node(root, Transform::IDENTITY, None, None, &mut layered);

    let compositor_blur = layered.compositor_blur;
    layered.drain_into(commands, layers);
    compositor_blur
}

/// Recursively flatten a node and its children.
///
/// For nodes with `repainted == false` and a valid `cached_flatten`,
/// reuse the cached commands with a translation offset instead of
/// re-flattening the entire subtree.
fn flatten_node(
    node: &RenderNode,
    parent_world_transform: Transform,
    parent_world_origin: Option<(f32, f32)>,
    parent_clip: Option<&WorldClip>,
    out: &mut LayeredCommands,
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

    // Try cached flatten for clean subtrees (translation-only optimization).
    // Clone the Rc out of the RefCell so the borrow isn't held while pushing.
    let cached_flatten = if !node.repainted.get() {
        node.cached_flatten.borrow().clone()
    } else {
        None
    };
    if parent_clip.is_none()
        && node.clip.is_none()
        && let Some(cached) = cached_flatten
        && cached.world_transform.is_translation_only()
        && world_transform.is_translation_only()
    {
        let dx = world_transform.tx() - cached.world_transform.tx();
        let dy = world_transform.ty() - cached.world_transform.ty();
        for cmd in &cached.commands {
            let mut adjusted = cmd.clone();
            adjusted
                .world_transform
                .set_tx(cmd.world_transform.tx() + dx);
            adjusted
                .world_transform
                .set_ty(cmd.world_transform.ty() + dy);
            if let Some(ref mut clip) = adjusted.clip
                && !adjusted.clip_is_local
            {
                clip.rect.x += dx;
                clip.rect.y += dy;
            }
            out.push(adjusted);
        }
        crate::render_stats::record_flatten_cached();
        return;
    }

    // Full flatten — existing logic
    // Track if we should cache this node's flatten output. The mark captures
    // how much has been pushed so far, so everything this subtree adds
    // (including children) can be collected for caching.
    let should_cache =
        node.clip.is_none() && parent_clip.is_none() && world_transform.is_translation_only();
    let mark = if should_cache { Some(out.mark()) } else { None };

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
        let layer = match &**cmd {
            DrawCommand::Text { .. } => RenderLayer::Text,
            DrawCommand::Image { .. } => RenderLayer::Images,
            // Only the half the renderer draws opens a backdrop group. A blur
            // restricted to `COMPOSITOR` is published as a `wl_region` and drawn
            // by nobody here, and `Backdrop` is not a free label: it is the
            // lowest layer, so it splits the draw group, and a non-empty
            // backdrop bucket ends the render pass to store the target the
            // effect would sample. That is a pass break and a group per
            // container per frame, to filter nothing. It still travels with the
            // frame — the region is read off this list — it just travels with
            // the shapes, where it produces no instance.
            DrawCommand::BackdropBlur { sources, .. }
                if !sources.contains(crate::backdrop::BackdropSources::SURFACE) =>
            {
                RenderLayer::Shapes
            }
            DrawCommand::BackdropBlur { .. } | DrawCommand::TextBackdropBlur { .. } => {
                RenderLayer::Backdrop
            }
            _ => RenderLayer::Shapes,
        };
        out.push(FlattenedCommand {
            command: Rc::clone(cmd),
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
            command: Rc::clone(cmd),
            world_transform,
            world_transform_origin: world_origin,
            layer: RenderLayer::Overlay,
            clip: overlay_clip.clone(),
            clip_is_local: overlay_clip_is_local,
        });
    }

    // Cache flatten results for next frame, but only when reuse is possible.
    // The mark captures everything added since the start of this node
    // (including all children).
    *node.cached_flatten.borrow_mut() = mark.map(|mark| {
        Rc::new(CachedFlatten {
            commands: out.commands_since(mark),
            world_transform,
        })
    });
    crate::render_stats::record_flatten_full();
}

/// Stand-in for "this could be anywhere", used when a command's extent is not
/// known: it intersects everything, so the group splits.
const EVERYTHING: Rect = Rect {
    x: f32::NEG_INFINITY,
    y: f32::NEG_INFINITY,
    width: f32::INFINITY,
    height: f32::INFINITY,
};

fn union_of(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    Rect::new(x, y, right - x, bottom - y)
}

/// What a command covers, in world space, or `None` when that cannot be
/// bounded cheaply.
///
/// Only used to decide whether two commands can safely swap places, so it has
/// to be generous: a box that is too small could let something be drawn
/// underneath what it should cover.
fn world_bounds(cmd: &FlattenedCommand) -> Option<Rect> {
    let local = match &*cmd.command {
        DrawCommand::RoundedRect {
            rect,
            border,
            shadow,
            ..
        } => {
            // A shadow spills well outside the rect, and a border straddles
            // its edge.
            let mut grow: f32 = border.map_or(0.0, |b| b.width);
            let mut offset: (f32, f32) = (0.0, 0.0);
            if let Some(shadow) = shadow {
                grow = grow.max(shadow.blur + shadow.spread);
                offset = shadow.offset;
            }
            Rect::new(
                rect.x - grow + offset.0.min(0.0),
                rect.y - grow + offset.1.min(0.0),
                rect.width + grow * 2.0 + offset.0.abs(),
                rect.height + grow * 2.0 + offset.1.abs(),
            )
        }
        DrawCommand::Circle { center, radius, .. } => Rect::new(
            center.0 - radius,
            center.1 - radius,
            radius * 2.0,
            radius * 2.0,
        ),
        DrawCommand::Image { rect, .. } => *rect,
        DrawCommand::BackdropBlur { rect, .. } => *rect,
        DrawCommand::Text {
            rect, font_size, ..
        }
        | DrawCommand::TextBackdropBlur {
            rect, font_size, ..
        } => {
            // Glyphs overshoot their layout box — descenders, italics, marks.
            // A font-size of slack costs an occasional extra group, where too
            // tight a box would let a background cover a letter.
            let slack = font_size * 0.5;
            Rect::new(
                rect.x - slack,
                rect.y - slack,
                rect.width + slack * 2.0,
                rect.height + slack * 2.0,
            )
        }
    };

    if !cmd.world_transform.is_translation_only() {
        // Rotation and scale would need the transformed corners; rather than
        // pay for that on every command, treat it as unbounded and split.
        return None;
    }
    Some(Rect::new(
        local.x + cmd.world_transform.tx(),
        local.y + cmd.world_transform.ty(),
        local.width,
        local.height,
    ))
}

/// Compute axis-aligned bounding box from an array of points.
fn aabb_from_points(points: &[(f32, f32)]) -> Rect {
    let (min_x, max_x, min_y, max_y) = points.iter().fold(
        (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), &(x, y)| {
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    );
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
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

    // Scale corner radius by transform scale
    let scale = transform.extract_scale();

    WorldClip {
        rect: aabb_from_points(&corners),
        corner_radius: CornerRadii {
            top_left: clip.corner_radius.top_left * scale,
            top_right: clip.corner_radius.top_right * scale,
            bottom_right: clip.corner_radius.bottom_right * scale,
            bottom_left: clip.corner_radius.bottom_left * scale,
        },
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

    // Corner by corner, the smaller radius: an intersection can only be
    // rounded where both clips are.
    let corner_radius = CornerRadii {
        top_left: a.corner_radius.top_left.min(b.corner_radius.top_left),
        top_right: a.corner_radius.top_right.min(b.corner_radius.top_right),
        bottom_right: a
            .corner_radius
            .bottom_right
            .min(b.corner_radius.bottom_right),
        bottom_left: a.corner_radius.bottom_left.min(b.corner_radius.bottom_left),
    };
    // The curvature of whichever clip rounds the least, for the same reason.
    let curvature = if a.corner_radius.max() <= b.corner_radius.max() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Color;

    fn command(layer: RenderLayer) -> FlattenedCommand {
        command_at(layer, Rect::new(0.0, 0.0, 100.0, 100.0))
    }

    /// Commands are only reordered where they overlap, so tests that care
    /// about splitting have to place them.
    fn command_at(layer: RenderLayer, rect: Rect) -> FlattenedCommand {
        FlattenedCommand {
            command: Rc::new(DrawCommand::rounded_rect(rect, Color::WHITE, 0.0)),
            world_transform: Transform::IDENTITY,
            world_transform_origin: None,
            layer,
            clip: None,
            clip_is_local: false,
        }
    }

    /// Drive `push` with a sequence of layers and report the resulting groups
    /// as the layers each one holds, in draw order.
    fn groups(sequence: &[RenderLayer]) -> Vec<Vec<RenderLayer>> {
        let mut layered = LayeredCommands::new();
        for layer in sequence {
            layered.push(command(*layer));
        }

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);

        layers
            .iter()
            .map(|layer| {
                [&layer.shapes, &layer.images, &layer.text, &layer.overlay]
                    .iter()
                    .flat_map(|range| commands[(*range).clone()].iter().map(|c| c.layer))
                    .collect()
            })
            .collect()
    }

    use RenderLayer::{Images, Overlay, Shapes, Text};

    /// A blur restricted to the compositor is published as a `wl_region` and
    /// drawn by nobody here, so it must not be filed as a backdrop effect: that
    /// bucket ends the render pass to store the target the effect would sample,
    /// and it is the lowest layer, so it splits the draw group as well. Both,
    /// per container, per frame, to filter nothing.
    #[test]
    fn a_compositor_only_blur_neither_breaks_the_pass_nor_splits_the_group() {
        let frame = |sources| {
            let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
            let mut node = RenderNode::new(1);
            node.bounds = rect;
            // The background first, so a backdrop landing after it is one the
            // group rules have to split for.
            node.commands
                .push(Rc::new(DrawCommand::rounded_rect(rect, Color::WHITE, 0.0)));
            node.commands.push(Rc::new(DrawCommand::BackdropBlur {
                rect,
                sources,
                radius: 24.0,
                corner_radii: CornerRadii::from(0.0),
                curvature: 1.0,
            }));

            let (mut commands, mut layers) = (Vec::new(), Vec::new());
            let told = flatten_root_into(&node, &mut commands, &mut layers);
            let drawn = layers.iter().any(|l| !l.backdrop.is_empty());
            (told, drawn, layers.len())
        };

        let (told, drawn, groups) = frame(crate::backdrop::BackdropSources::COMPOSITOR);
        assert!(told, "the compositor still has to be handed a region");
        assert!(!drawn, "and this renderer has nothing to do for it");
        assert_eq!(groups, 1, "so the frame is not split for it either");

        let (told, drawn, groups) = frame(crate::backdrop::BackdropSources::SURFACE);
        assert!(!told, "nothing for the compositor");
        assert!(drawn, "and a pass of our own to run");
        assert_eq!(groups, 2, "which is what the split is for");
    }

    #[test]
    fn ascending_layers_stay_in_one_group() {
        // The common case: nothing is painted over a higher layer, so the
        // whole frame batches exactly as it did before groups existed.
        assert_eq!(
            groups(&[Shapes, Shapes, Images, Text, Overlay]),
            vec![vec![Shapes, Shapes, Images, Text, Overlay]]
        );
    }

    #[test]
    fn a_shape_after_an_image_opens_a_group() {
        // The bug groups exist to fix: a container background painted over an
        // image must not be batched back underneath it.
        assert_eq!(
            groups(&[Shapes, Images, Shapes]),
            vec![vec![Shapes, Images], vec![Shapes]]
        );
    }

    #[test]
    fn an_image_after_text_opens_a_group() {
        assert_eq!(
            groups(&[Images, Text, Images]),
            vec![vec![Images, Text], vec![Images]]
        );
    }

    #[test]
    fn repeats_of_the_current_layer_do_not_split() {
        assert_eq!(
            groups(&[Images, Images, Images]),
            vec![vec![Images, Images, Images]]
        );
    }

    #[test]
    fn each_regression_opens_exactly_one_group() {
        assert_eq!(
            groups(&[Shapes, Images, Shapes, Images, Shapes]),
            vec![vec![Shapes, Images], vec![Shapes, Images], vec![Shapes]]
        );
    }

    #[test]
    fn a_group_is_reopened_only_below_its_high_water_mark() {
        // Text lifts the group's mark to 2, so the following image regresses
        // even though it is above the shapes already in the group.
        assert_eq!(
            groups(&[Shapes, Text, Images]),
            vec![vec![Shapes, Text], vec![Images]]
        );
    }

    #[test]
    fn replaying_a_cached_subtree_reproduces_its_grouping() {
        // `commands_since` feeds cached commands back through `push` on later
        // frames. If it did not hand them back in draw order, a cached subtree
        // would re-group differently from the one that produced it — the same
        // widget would draw correctly on the frame it was painted and wrongly
        // on every frame it was reused.
        let sequence = [Shapes, Images, Shapes, Text, Images];

        let mut original = LayeredCommands::new();
        for layer in sequence {
            original.push(command(layer));
        }
        let cached = original.commands_since(Mark {
            groups: 1,
            buckets: [0; LAYER_COUNT],
        });

        let mut replayed = LayeredCommands::new();
        for cmd in cached {
            replayed.push(cmd);
        }

        let (mut a_cmds, mut a_layers) = (Vec::new(), Vec::new());
        original.drain_into(&mut a_cmds, &mut a_layers);
        let (mut b_cmds, mut b_layers) = (Vec::new(), Vec::new());
        replayed.drain_into(&mut b_cmds, &mut b_layers);

        assert_eq!(a_layers.len(), b_layers.len());
        let order = |cmds: &[FlattenedCommand], layers: &[CommandLayer]| -> Vec<RenderLayer> {
            layers
                .iter()
                .flat_map(|l| {
                    [&l.shapes, &l.images, &l.text, &l.overlay]
                        .iter()
                        .flat_map(|r| cmds[(*r).clone()].iter().map(|c| c.layer))
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        assert_eq!(order(&a_cmds, &a_layers), order(&b_cmds, &b_layers));
    }

    #[test]
    fn a_regression_that_covers_nothing_stays_in_the_group() {
        // Sibling widgets each paint a background then a label: the next
        // sibling's background is a regression, but it lands somewhere the
        // previous label is not, so splitting there would buy nothing. This
        // is the shape of nearly every real tree, which is why the overlap
        // test is what keeps the group count near one.
        let mut layered = LayeredCommands::new();
        for column in 0..4 {
            let x = column as f32 * 200.0;
            layered.push(command_at(Shapes, Rect::new(x, 0.0, 100.0, 40.0)));
            layered.push(command_at(Text, Rect::new(x, 0.0, 100.0, 40.0)));
        }

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 1, "non-overlapping siblings must not split");
    }

    #[test]
    fn a_regression_that_covers_something_still_splits() {
        let mut layered = LayeredCommands::new();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 100.0, 40.0)));
        // Lands on top of the label, so it has to be drawn after it.
        layered.push(command_at(Shapes, Rect::new(20.0, 10.0, 40.0, 10.0)));

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 2);
    }

    #[test]
    fn an_unbounded_command_forces_a_split() {
        // A rotated or scaled command is not bounded cheaply, so it is
        // assumed to cover anything: correctness over a spare draw call.
        let mut layered = LayeredCommands::new();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 10.0, 10.0)));
        let mut rotated = command_at(Shapes, Rect::new(900.0, 900.0, 10.0, 10.0));
        rotated.world_transform = Transform::rotate(45.0);
        layered.push(rotated);

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 2);
    }

    #[test]
    fn a_mark_survives_a_later_command_landing_in_an_earlier_bucket() {
        // The reason a mark is not a count. Once a group can take a shape
        // while already holding text — which is exactly what the overlap test
        // allows — a later command is inserted *before* that text in draw
        // order. A count would then hand a cached subtree its neighbour's
        // commands, and the neighbour would be drawn twice.
        let mut layered = LayeredCommands::new();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 50.0, 20.0)));

        // Everything from here belongs to the "subtree" being captured.
        let mark = layered.mark();
        // Does not overlap the text, so it joins the same group and slots
        // into the shapes bucket, ahead of the text already there.
        layered.push(command_at(Shapes, Rect::new(500.0, 0.0, 50.0, 20.0)));
        layered.push(command_at(Text, Rect::new(500.0, 0.0, 50.0, 20.0)));

        let captured = layered.commands_since(mark);
        assert_eq!(
            captured.len(),
            2,
            "the mark must not sweep up the earlier text"
        );
        assert_eq!(captured[0].layer, Shapes);
        assert_eq!(captured[1].layer, Text);
    }

    #[test]
    fn a_gap_between_two_labels_is_not_covered() {
        // The union of two labels spans the gap between them. Testing against
        // the union alone would split on a background landing in that gap,
        // which is what showcase did three times over.
        let mut layered = LayeredCommands::new();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 40.0, 20.0)));
        layered.push(command_at(Text, Rect::new(400.0, 0.0, 40.0, 20.0)));
        // Squarely between them, touching neither.
        layered.push(command_at(Shapes, Rect::new(200.0, 0.0, 40.0, 20.0)));

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 1);
    }

    #[test]
    fn past_the_rect_cap_the_union_decides() {
        // Beyond MAX_TRACKED_RECTS the scan would cost more than the draw
        // call it saves, so the union takes over — which can only over-split.
        let mut layered = LayeredCommands::new();
        for i in 0..(MAX_TRACKED_RECTS + 1) {
            let x = i as f32 * 100.0;
            layered.push(command_at(Text, Rect::new(x, 0.0, 10.0, 10.0)));
        }
        // In a gap, so an exact test would keep one group.
        layered.push(command_at(Shapes, Rect::new(50.0, 0.0, 10.0, 10.0)));

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 2, "the union must stay conservative");
    }

    #[test]
    fn empty_groups_are_dropped() {
        // A group that ends up with nothing in it would cost pipeline
        // switches for no draws.
        let mut layered = LayeredCommands::new();
        layered.push(command(Images));
        layered.push(command(Shapes));
        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert!(layers.iter().all(|layer| !layer.is_empty()));
    }
}

#[cfg(test)]
mod world_geometry_tests {
    use super::*;
    use crate::widgets::Color;

    fn command(transform: Transform) -> FlattenedCommand {
        FlattenedCommand {
            command: Rc::new(DrawCommand::RoundedRect {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                color: Color::RED,
                radius: CornerRadii::uniform(16.0),
                curvature: 1.0,
                border: None,
                shadow: None,
                gradient: None,
            }),
            world_transform: transform,
            world_transform_origin: None,
            layer: RenderLayer::Shapes,
            clip: None,
            clip_is_local: false,
        }
    }

    /// Two opposite corners do not bound a rotated box: a 45° rotation puts the
    /// extremes on the *other* diagonal, and subtracting the two you happen to
    /// have gives a zero-width rect — which downstream reads as "nothing to do".
    #[test]
    fn a_rotated_box_reports_the_diagonal_it_actually_covers() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let (world, _) = command(Transform::rotate_degrees(45.0))
            .world_rounded_rect(rect, CornerRadii::from(0.0));

        let diagonal = 100.0 * 2.0f32.sqrt();
        assert!(
            (world.width - diagonal).abs() < 0.1 && (world.height - diagonal).abs() < 0.1,
            "a 100x100 turned 45° covers {diagonal:.1} square, got {:.1}x{:.1}",
            world.width,
            world.height
        );
    }

    /// The radii travel with the box, or a scaled container reports a shape
    /// whose corners are cut a different amount from the one it draws.
    #[test]
    fn the_corner_radii_scale_with_the_box() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let (world, radii) =
            command(Transform::scale(2.0)).world_rounded_rect(rect, CornerRadii::uniform(16.0));

        assert_eq!(world.width, 200.0);
        assert_eq!(
            (radii.x.max(), radii.y.max()),
            (32.0, 32.0),
            "twice the box, twice the corner"
        );
    }

    /// Each axis by its own factor. A corner scaled unevenly is an ellipse, and
    /// the geometric mean the shared scale used to return — 22.6 for 2x/1x — is
    /// neither of its axes, so the region cut a curve the shape does not have.
    #[test]
    fn an_uneven_scale_gives_the_corner_two_axes() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let (_, radii) = command(Transform::scale_xy(2.0, 1.0))
            .world_rounded_rect(rect, CornerRadii::uniform(16.0));

        assert_eq!(radii.x.top_left, 32.0, "twice as wide");
        assert_eq!(radii.y.top_left, 16.0, "and as tall as it was");
        assert_eq!(
            radii.to_circular().top_left,
            32.0,
            "a consumer taking one radius gets the larger, so it cuts at least \
             as much as the ellipse and stays inside the shape"
        );
    }

    /// The clip is part of the shape. A card half out of a viewport is filtered
    /// only where it is on show, and the region published to the compositor has
    /// to agree — otherwise it blurs the desktop beside a panel that is not
    /// there, which is the last way the two halves of one command could
    /// describe different things.
    #[test]
    fn a_clip_narrows_the_shape_for_both_halves() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut cmd = command(Transform::translate(0.0, 0.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 40.0, 100.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });

        let (world, _) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::from(0.0))
            .expect("still on show");
        assert_eq!((world.x, world.width), (0.0, 40.0), "cut to the clip");
        assert_eq!(world.height, 100.0, "and untouched on the other axis");
    }

    /// And the corners go with it. A card cut in half by a viewport has a
    /// straight edge where the cut is, so the two corners along it are square —
    /// published as round, the region loses a wedge the size of the radius at
    /// each of them, and the desktop shows through beside the panel.
    #[test]
    fn a_clip_squares_off_the_corners_it_cuts() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut cmd = command(Transform::translate(0.0, 0.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 40.0, 100.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });

        let (_, radii) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::uniform(16.0))
            .expect("still on show");
        assert_eq!(
            (radii.x.top_left, radii.x.bottom_left),
            (16.0, 16.0),
            "the left edge was not moved, so its corners are as they were drawn"
        );
        assert_eq!(
            (radii.x.top_right, radii.x.bottom_right),
            (0.0, 0.0),
            "and the ones along the cut are square"
        );
    }

    /// A corner where the clip supplied *both* edges is the clip's corner. A
    /// frosted card filling a rounded scroller published the scroller's square
    /// bounding box, so the compositor blurred the desktop in the four corners
    /// where the panel is not drawn — the function's own doc, applied to itself.
    #[test]
    fn a_corner_the_clip_supplies_belongs_to_the_clip() {
        // The card overhangs the scroller on every side, so all four corners of
        // the intersection come from the clip.
        let rect = Rect::new(-10.0, -10.0, 120.0, 120.0);
        let mut cmd = command(Transform::translate(0.0, 0.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        });

        let (world, radii) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::uniform(4.0))
            .expect("still on show");
        assert_eq!(
            (world.x, world.y, world.width, world.height),
            (0.0, 0.0, 100.0, 100.0)
        );
        assert_eq!(
            radii.x.to_array(),
            [16.0; 4],
            "every corner is the scroller's, not the card's and not square"
        );
    }

    /// And when the two rectangles are *equal*, both corners are there. A child
    /// declared `width(fill()).height(fill())` inside a clipping parent with no
    /// padding shares all four edges, so reading the corner as "was this edge
    /// moved" found no movement and handed back the child's own radius — square,
    /// where the shape on screen is rounded by the parent, and the compositor
    /// blurs four wedges of desktop outside the panel.
    #[test]
    fn coincident_edges_keep_the_tighter_curve() {
        // Exactly the clip: a fill/fill child of a zero-padding parent.
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut cmd = command(Transform::translate(0.0, 0.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        });

        let (world, radii) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::from(0.0))
            .expect("nothing is cut away");
        assert_eq!(
            (world.x, world.y, world.width, world.height),
            (0.0, 0.0, 100.0, 100.0),
            "the intersection is either of them"
        );
        assert_eq!(
            radii.x.to_array(),
            [16.0; 4],
            "the child declares none, so what shows is the parent's"
        );

        // The other way round: the tighter of the two is the one that shows.
        let (_, radii) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::uniform(24.0))
            .expect("nothing is cut away");
        assert_eq!(
            radii.x.to_array(),
            [24.0; 4],
            "a child rounded harder than its clip keeps its own curve"
        );
    }

    /// Clipped away entirely is nothing to publish, not an empty rectangle
    /// somewhere.
    #[test]
    fn a_shape_outside_its_clip_has_no_region() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut cmd = command(Transform::translate(500.0, 0.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });

        assert!(
            cmd.clipped_world_rounded_rect(rect, CornerRadii::from(0.0))
                .is_none()
        );
    }

    /// An overlay keeps its clip in local space so a ripple follows the shape
    /// through a rotation. Compared against a world rect without being carried
    /// over first, a ripple inside a translated subtree reports a region
    /// somewhere else entirely — or none at all, for a shape wholly on show.
    #[test]
    fn a_local_clip_is_carried_into_world_space_before_it_cuts() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut cmd = command(Transform::translate(500.0, 300.0));
        cmd.clip = Some(WorldClip {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });
        cmd.clip_is_local = true;

        let (world, _) = cmd
            .clipped_world_rounded_rect(rect, CornerRadii::from(0.0))
            .expect("the clip covers the whole shape, so nothing is cut");
        assert_eq!(
            (world.x, world.y, world.width, world.height),
            (500.0, 300.0, 100.0, 100.0)
        );
    }

    /// A translation moves it and changes nothing else.
    #[test]
    fn a_translation_only_moves_it() {
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let (world, radii) = command(Transform::translate(20.0, 30.0))
            .world_rounded_rect(rect, CornerRadii::uniform(8.0));

        assert_eq!((world.x, world.y), (20.0, 30.0));
        assert_eq!((world.width, world.height), (100.0, 60.0));
        assert_eq!((radii.x.max(), radii.y.max()), (8.0, 8.0));
    }
}
