//! Tree flattening with world transform computation.

use std::ops::Range;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::transform::Transform;
use crate::widgets::Rect;

use super::commands::DrawCommand;
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
    /// Corner radius for rounded clipping (in logical pixels).
    pub corner_radius: f32,
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
        }
    }

    fn push(&mut self, cmd: FlattenedCommand) {
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
/// `layers` receives the groups to draw, in order; see [`LayeredCommands`].
pub fn flatten_root_into(
    root: &RenderNode,
    commands: &mut Vec<FlattenedCommand>,
    layers: &mut Vec<CommandLayer>,
) {
    commands.clear();
    layers.clear();

    let mut layered = LayeredCommands::new();
    flatten_node(root, Transform::IDENTITY, None, None, &mut layered);

    layered.drain_into(commands, layers);
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
            DrawCommand::BackdropBlur { .. } => RenderLayer::Backdrop,
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
        corner_radius: clip.corner_radius * scale,
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
