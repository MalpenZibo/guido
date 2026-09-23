//! Tree flattening with world transform computation.

use std::ops::Range;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::transform::Transform;
use crate::widgets::Rect;

use super::clip::{ClipIndex, ClipRef, ClipTree, Under};
use super::commands::{CornerRadii, DrawCommand};
use super::tree::{CachedFlatten, RenderNode};
use crate::shape::PlacedShape;

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
    /// Render layer for ordering
    pub layer: RenderLayer,
    /// How opaque it is drawn: the product of every opacity from the root down
    /// to the node that painted it. Multiplied into each colour it draws —
    /// see [`RenderNode::opacity`](super::tree::RenderNode::opacity).
    pub opacity: f32,
    /// The clip this command is cut to, if any — named rather than copied, so
    /// that a subtree replayed somewhere else does not drag it along. Read it
    /// with [`clip`](Self::clip).
    pub clip: Option<ClipRef>,
}

impl FlattenedCommand {
    /// The shape this command is cut to, where it is this frame.
    ///
    /// The one step between a command and every pipeline that draws one: the
    /// clip arrives here as a [`PlacedShape`] exactly as it did when each
    /// command carried its own copy.
    pub fn clip(&self) -> Option<PlacedShape> {
        self.clip.as_ref().map(ClipRef::shape)
    }
}

/// Where one frame's draw commands are grouped, before they are handed on as
/// one buffer.
///
/// It belongs to the surface rather than to the frame, and
/// [`flatten_root_into`] empties it on the way in instead of building one: a
/// frame's groups, buckets, bounds and clips are the same shapes frame after
/// frame, so what the widest frame needed is what the next one fills. The
/// output buffers already made that bargain — this is the intermediate they
/// are drained out of making it too.
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
pub struct FlattenScratch {
    /// Every group this scratch has ever held, the live ones first. Dropping
    /// the ones a narrower frame does not need would free the five buffers
    /// inside each, which is the whole of what is being kept.
    groups: Vec<LayerBuckets>,
    /// How many of `groups` this frame has opened. The rest are empty and
    /// waiting; nothing outside this file sees past it.
    used: usize,
    /// The clips this frame has placed, which the commands above name.
    clips: ClipTree,
    /// What this frame carries for the compositor, counted while the commands
    /// go past — because the alternative is walking them all again afterwards
    /// to find out, on every painted frame, for the large majority of surfaces
    /// that carry neither.
    carried: RegionsCarried,
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

    /// Back to what [`Default`] gives, without giving back the rects' room.
    ///
    /// Every field, and that is the point: a `union` or an `overflowed` left
    /// over from the last frame is a group split this frame cannot account
    /// for.
    fn clear(&mut self) {
        // Destructured so that a field added here is a compile error rather
        // than a value that quietly survives into the next frame.
        let Self {
            union,
            rects,
            overflowed,
        } = self;
        *union = None;
        rects.clear();
        *overflowed = false;
    }

    /// How many rects this can hold before it allocates — counted only once it
    /// has spilled off the stack, since the inline room is not a frame's doing.
    fn heap_rects(&self) -> usize {
        if self.rects.spilled() {
            self.rects.capacity()
        } else {
            0
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

    /// Every bucket, in draw order, for the one place that walks all five
    /// without naming them.
    fn buckets_mut(&mut self) -> [&mut Vec<FlattenedCommand>; LAYER_COUNT] {
        [
            &mut self.backdrop,
            &mut self.shapes,
            &mut self.images,
            &mut self.text,
            &mut self.overlay,
        ]
    }

    /// Back to what [`Default`] gives, without giving back the buffers.
    ///
    /// `high_water` too: a group that starts at anything but the lowest layer
    /// would split on a command that belongs in it.
    fn clear(&mut self) {
        // Destructured, as in [`LayerBounds::clear`]: a bucket or a mark left
        // out here is a group boundary the next frame did not ask for.
        let Self {
            backdrop,
            shapes,
            images,
            text,
            overlay,
            high_water,
            bounds,
        } = self;
        for bucket in [backdrop, shapes, images, text, overlay] {
            bucket.clear();
        }
        *high_water = RenderLayer::default();
        for bounds in bounds {
            bounds.clear();
        }
    }

    /// How many commands this group's buckets can hold before they allocate.
    fn command_capacity(&self) -> usize {
        self.backdrop.capacity()
            + self.shapes.capacity()
            + self.images.capacity()
            + self.text.capacity()
            + self.overlay.capacity()
    }
}

impl Default for FlattenScratch {
    fn default() -> Self {
        Self {
            groups: vec![LayerBuckets::default()],
            used: 1,
            clips: ClipTree::default(),
            carried: RegionsCarried::default(),
        }
    }
}

/// What a [`FlattenScratch`] is holding onto between frames.
///
/// Four numbers rather than one, because they are four different buffers and a
/// sum could hide one of them being rebuilt behind another one growing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScratchCapacity {
    /// Draw groups the scratch can hold without allocating.
    pub groups: usize,
    /// Commands its buckets can hold, summed over every group it kept.
    pub commands: usize,
    /// Bounds rectangles they can hold, counting only what spilled onto the
    /// heap — the inline room comes with the group and says nothing about
    /// whether it is being reused.
    pub rects: usize,
    /// Clips its clip tree can hold.
    pub clips: usize,
}

impl FlattenScratch {
    /// What this scratch can take before it asks for more memory.
    ///
    /// Read by `tests/paint_cache_across_frames.rs`, which is the only thing
    /// that can say a frame refilled the buffers the frame before it left.
    pub fn capacity(&self) -> ScratchCapacity {
        ScratchCapacity {
            groups: self.groups.capacity(),
            commands: self.groups.iter().map(LayerBuckets::command_capacity).sum(),
            rects: self
                .groups
                .iter()
                .flat_map(|group| &group.bounds)
                .map(LayerBounds::heap_rects)
                .sum(),
            clips: self.clips.capacity(),
        }
    }

    /// Empty every buffer, keeping what each one is holding.
    ///
    /// The one thing that must not happen here is a field surviving: a
    /// `high_water`, a `bounds` or an `overflowed` left over from the last
    /// frame is a group boundary this frame did not ask for, and the groups
    /// past `used` are already in this state.
    fn clear(&mut self) {
        // Destructured, as the two above are. `used` goes back to one *after*
        // the loop: it is what says which groups were written, so emptying
        // them is the last thing it is good for.
        let Self {
            groups,
            used,
            clips,
            carried,
        } = self;
        for group in &mut groups[..*used] {
            group.clear();
        }
        *used = 1;
        clips.clear();
        *carried = RegionsCarried::default();
    }

    /// The groups this frame has opened, in draw order.
    fn live(&self) -> &[LayerBuckets] {
        &self.groups[..self.used]
    }

    fn live_mut(&mut self) -> &mut [LayerBuckets] {
        &mut self.groups[..self.used]
    }

    /// The group commands are landing in. `used` is never zero: `Default` and
    /// `clear` both leave one group open and nothing ever closes one.
    fn current(&self) -> &LayerBuckets {
        &self.groups[self.used - 1]
    }

    fn current_mut(&mut self) -> &mut LayerBuckets {
        &mut self.groups[self.used - 1]
    }

    /// Open a group starting at `high_water`, reusing the next one this
    /// scratch kept if there is one. Everything past `used` was emptied by
    /// [`clear`](Self::clear), so where the group starts is all there is to
    /// say about it.
    fn open_group(&mut self, high_water: RenderLayer) {
        if self.used == self.groups.len() {
            self.groups.push(LayerBuckets::default());
        }
        self.groups[self.used].high_water = high_water;
        self.used += 1;
    }

    fn push(&mut self, cmd: FlattenedCommand) {
        match &*cmd.command {
            DrawCommand::BackdropBlur { sources, .. }
                if sources.contains(crate::backdrop::BackdropSources::COMPOSITOR) =>
            {
                self.carried.compositor_blur = true;
            }
            DrawCommand::InputRegion { .. } => {
                self.carried.input_region = true;
                // No pixels, so no group. Splitting one for a command that
                // draws nothing costs a pipeline bind per frame, and recording
                // its bounds spends one of the tracked rectangles a real
                // overlap test needs. Its place in the list is all the region
                // is read for.
                self.current_mut().bucket_mut(cmd.layer).push(cmd);
                return;
            }
            _ => {}
        }
        let layer = cmd.layer;
        let rect = world_bounds(&cmd);
        let current = self.current();
        if layer < current.high_water && current.covered_above(layer, rect) {
            self.open_group(layer);
        }
        let current = self.current_mut();
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
        let last = self.current();
        Mark {
            groups: self.used,
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
    /// The clip each command names is rewritten on the way out: one the
    /// subtree placed itself is kept, since the same reference is written
    /// through when the subtree is replayed, and the one it inherited becomes
    /// `None` — "whatever is above me when I am put back", which is the whole
    /// of what stops a replayed row carrying its viewport away with it.
    ///
    /// [`CachedParent::Inherited`] is that same sentence about a cached
    /// *clip*, and the two are read by the two halves of one replay: this one
    /// by the loop that pushes the commands, that one by
    /// [`ClipTree::replay`](super::clip::ClipTree::replay). They mean the same
    /// thing and have to keep meaning it.
    ///
    /// [`CachedParent::Inherited`]: super::clip::CachedParent::Inherited
    ///
    /// [`push`]: Self::push
    fn commands_since(&self, mark: Mark, inherited: Option<&ClipRef>) -> Vec<FlattenedCommand> {
        // Lengths first, then one allocation. A subtree is captured on every
        // frame it is flattened in full, and `FlattenedCommand` is large
        // enough that letting the vector find its own size copies real bytes.
        let mut result = Vec::with_capacity(self.tails_since(mark).map(<[_]>::len).sum());
        // One `extend` per tail, not one over the tails flattened together: a
        // mapped slice is `TrustedLen` and `Vec` fills from it without asking
        // after each command whether it still fits. Flattening loses that, and
        // it is worth 0.9us a frame on `benches/scroll_list` at 5000 rows.
        for tail in self.tails_since(mark) {
            result.extend(tail.iter().map(|cmd| {
                let mut kept = cmd.clone();
                if kept
                    .clip
                    .as_ref()
                    .is_some_and(|clip| inherited.is_some_and(|base| base.is(clip)))
                {
                    kept.clip = None;
                }
                kept
            }));
        }
        result
    }

    /// The part of each bucket that was added after `mark`, in draw order.
    fn tails_since(&self, mark: Mark) -> impl Iterator<Item = &[FlattenedCommand]> {
        self.live()
            .iter()
            .enumerate()
            .skip(mark.groups - 1)
            .flat_map(move |(index, group)| {
                let buckets = [
                    &group.backdrop,
                    &group.shapes,
                    &group.images,
                    &group.text,
                    &group.overlay,
                ];
                buckets
                    .into_iter()
                    .enumerate()
                    .map(move |(bucket_index, bucket)| {
                        // Groups opened after the mark are new all through;
                        // the group that was current keeps whatever it
                        // already held.
                        let from = if index == mark.groups - 1 {
                            mark.buckets[bucket_index]
                        } else {
                            0
                        };
                        &bucket[from..]
                    })
            })
    }

    /// Move the groups into one buffer in draw order, recording where each
    /// group's buckets landed.
    ///
    /// The buckets are emptied rather than consumed — `append` takes the
    /// commands and leaves the buffer that held them, which is what the next
    /// frame fills.
    fn drain_into(&mut self, out: &mut Vec<FlattenedCommand>, layers: &mut Vec<CommandLayer>) {
        for group in self.live_mut() {
            let mut layer = CommandLayer::default();
            let ranges = [
                &mut layer.backdrop,
                &mut layer.shapes,
                &mut layer.images,
                &mut layer.text,
                &mut layer.overlay,
            ];
            for (bucket, range) in group.buckets_mut().into_iter().zip(ranges) {
                let start = out.len();
                out.append(bucket);
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

/// A position in a [`FlattenScratch`], for capturing one subtree's output.
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
/// `scratch` is where the frame is built before it lands in those two, and is
/// the surface's rather than the frame's for the same reason they are — it is
/// emptied here, not constructed; see [`FlattenScratch`].
///
/// Returns what the frame carries that becomes a `wl_region` — a backdrop blur
/// the *compositor* is asked to apply, and a declaration about where input
/// reaches the surface — which is the only reason to walk the result again and
/// build one from it.
pub fn flatten_root_into(
    root: &RenderNode,
    commands: &mut Vec<FlattenedCommand>,
    layers: &mut Vec<CommandLayer>,
    scratch: &mut FlattenScratch,
) -> RegionsCarried {
    commands.clear();
    layers.clear();
    scratch.clear();

    flatten_node(root, Transform::IDENTITY, None, 1.0, scratch);

    let carried = scratch.carried;
    scratch.drain_into(commands, layers);
    carried
}

/// What a flattened frame carries for the compositor, counted on the way past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RegionsCarried {
    /// A backdrop blur the compositor is asked to apply.
    pub compositor_blur: bool,
    /// A declaration about where input reaches the surface.
    pub input_region: bool,
}

/// Recursively flatten a node and its children.
///
/// For nodes with `repainted == false` and a valid `cached_flatten`,
/// reuse the cached commands with a translation offset instead of
/// re-flattening the entire subtree.
///
/// `parent_opacity` is what everything above has multiplied together, and
/// travels down the way `parent_world_transform` does.
fn flatten_node(
    node: &RenderNode,
    parent_world_transform: Transform,
    parent_clip: Option<ClipIndex>,
    parent_opacity: f32,
    out: &mut FlattenScratch,
) -> bool {
    // Compose transforms: parent first, then local about its own pivot
    let local_centered = if node.local_transform.is_identity() {
        Transform::IDENTITY
    } else {
        node.local_transform.about(node.pivot, node.bounds)
    };
    let world_transform = parent_world_transform.then(&local_centered);
    let opacity = parent_opacity * node.opacity;

    // What this node is cut by before it says anything of its own — read once,
    // because three things want it: a replay, the commands of a node that sets
    // no clip, and the entry left for next frame.
    let inherited = parent_clip.map(|index| out.clips.reference(index));

    // Try cached flatten for clean subtrees (translation-only optimization).
    // Clone the Rc out of the RefCell so the borrow isn't held while pushing.
    let cached_flatten = if !node.repainted.get() {
        node.cached_flatten.borrow().clone()
    } else {
        None
    };
    if let Some(cached) = cached_flatten
        && let Some((dx, dy)) = cached.replay_offset(world_transform)
        && let Some(fade) = cached.replay_fade(parent_opacity)
    {
        // The clips this subtree placed for itself move with it; the one it
        // inherits is the one it finds here, wherever that has got to. Placed
        // first, because the commands below read what they name.
        out.clips.replay(&cached.clips, dx, dy, parent_clip);
        for cmd in &cached.commands {
            let mut replayed = cmd.clone();
            replayed.world_transform = cmd.world_transform.translated(dx, dy);
            replayed.opacity = cmd.opacity * fade;
            // A command that named no clip named the one its subtree inherited,
            // which is this frame's and not the one it was cached under.
            if replayed.clip.is_none() {
                replayed.clip = inherited.clone();
            }
            out.push(replayed);
        }
        crate::render_stats::record_flatten_cached();
        // Whatever was replayed came out of the paint cache, which keeps only
        // complete subtrees.
        return false;
    }

    // Full flatten — existing logic
    // Where this subtree's own output starts, so that everything it adds
    // (including its children, and including the clip it is about to place)
    // can be picked out again and kept. A subtree that did anything but
    // translate can be put back by no offset at all, so there is nothing to
    // mark for it; whether the rest is worth keeping is decided at the bottom,
    // once the children have said whether any of them was cut short.
    let marks = world_transform
        .is_translation_only()
        .then(|| (out.mark(), out.clips.mark()));

    // This node's clip, placed under the one it inherits. The intersection is
    // still computed here and once — what changed is where it is written: into
    // the frame's clip tree, which every command below names, instead of into
    // each of those commands.
    let (clip_index, effective_clip) = match &node.clip {
        Some(clip) => {
            let own = PlacedShape::from_clip(clip, world_transform);
            let index = out.clips.place(own, Under::Inherited(parent_clip));
            (Some(index), Some(out.clips.reference(index)))
        }
        None => (parent_clip, inherited.clone()),
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
            // Draws nothing, for the same reason and by the same route: it
            // travels with the shapes, where it produces no instance.
            DrawCommand::InputRegion { .. } => RenderLayer::Shapes,
            DrawCommand::BackdropBlur { .. } | DrawCommand::TextBackdropBlur { .. } => {
                RenderLayer::Backdrop
            }
            _ => RenderLayer::Shapes,
        };
        out.push(FlattenedCommand {
            command: Rc::clone(cmd),
            world_transform,
            layer,
            opacity,
            clip: effective_clip.clone(),
        });
    }

    // Recurse to children with effective clip
    let mut subtree_partial = node.partial;
    for child in &node.children {
        subtree_partial |= flatten_node(child, world_transform, clip_index, opacity, out);
    }

    // Add overlay commands (layer = Overlay) with overlay-specific clip.
    //
    // An overlay's own clip, if it declared one — a ripple is cut to the shape
    // it belongs to, which is this node's, so it is placed by this node's
    // transform like any other clip. It used to need a flag of its own saying
    // "test this one in local coordinates"; now every clip says where its
    // coordinates are and the flag has nothing left to distinguish.
    //
    // It narrows nothing, which is why it is placed under no parent: what a
    // replay must do with it is carry it, not cut it again by an ancestor it
    // never answered to.
    if !node.overlay_commands.is_empty() {
        let overlay_clip = match node.overlay_clip {
            Some(ref clip) => {
                let own = PlacedShape::from_clip(clip, world_transform);
                let index = out.clips.place(own, Under::Nothing);
                Some(out.clips.reference(index))
            }
            None => effective_clip,
        };
        for cmd in &node.overlay_commands {
            out.push(FlattenedCommand {
                command: Rc::clone(cmd),
                world_transform,
                layer: RenderLayer::Overlay,
                opacity,
                clip: overlay_clip.clone(),
            });
        }
    }

    // Cache flatten results for next frame, but only when reuse is possible,
    // which is two questions.
    //
    // Can the entry be replayed? A replay shifts everything it kept by one
    // offset, so a subtree that did anything but translate cannot be put back
    // by one. A clip is no longer part of that question: the clips a subtree
    // places travel with it and the one it inherits is looked up where it now
    // is, so a row under a scroller is as replayable as a card on a desktop.
    //
    // Will it ever be *read*? Only a subtree the paint cache hands back clean
    // is ever offered to the replay path, and `cache_paint_results` keeps only
    // complete paints — a subtree with anything culled under it is dropped
    // from the paint cache and repaints next frame. Collecting an entry for
    // one is a copy of everything it drew, made for a frame that cannot ask
    // for it. That is most of the cost of caching in a scrolling list, where
    // the list culls and every ancestor of it inherits the culling.
    *node.cached_flatten.borrow_mut() =
        marks.filter(|_| !subtree_partial).map(|(mark, clip_mark)| {
            Rc::new(CachedFlatten {
                commands: out.commands_since(mark, inherited.as_ref()),
                clips: out.clips.since(clip_mark),
                world_transform,
                opacity: parent_opacity,
            })
        });
    crate::render_stats::record_flatten_full();
    subtree_partial
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
        DrawCommand::BackdropBlur { rect, .. } | DrawCommand::InputRegion { rect, .. } => *rect,
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

/// The two clips, as one.
///
/// **In a shared space where there is one, the enclosing box where there is
/// not.** Both clips describe a rounded rect, and where the second can be
/// written in the first's coordinates the intersection is another rounded rect
/// in those coordinates — exact, and still turned with whatever turned the
/// first. Where it cannot, one rect and one matrix are not enough to say what
/// the overlap of two differently-turned shapes is, and both fall back to
/// [`flattened`](PlacedShape::flattened): the box around each, in world space.
/// Over-generous, and exactly what every case here produced before any of this
/// carried a transform.
///
/// The fork is the one GTK draws too — `gsk_gpu_clip_transform` folds a clip
/// through a translation or an axis-aligned scale and refuses anything else,
/// leaving its caller to rasterise a mask. There is no mask here; there is the
/// box.
pub(super) fn intersect_clips(a: &PlacedShape, b: &PlacedShape) -> PlacedShape {
    match rebase(b, a) {
        Some(b) => tighten(a, &b),
        None => tighten(&a.flattened(), &b.flattened()),
    }
}

/// `clip` rewritten in `onto`'s coordinates, when that can be done without
/// losing the shape.
///
/// It can when the transform between the two spaces sends a rect to a rect,
/// which is [`Transform::rect_stays_rect`]. A shared placement — two clips
/// under the one transformed node, or the ordinary case of two untransformed
/// ones — is the identity and passes trivially; a scroller inside a scroller
/// differs by a translation and passes too.
///
/// A quarter turn and a mirror also send a rect to a rect. They used to be
/// refused because each moves the top-left corner somewhere else and nothing
/// permuted the radii to follow; the corners are permuted now, so the answer is
/// exact there as well rather than the box around both (#397). What is still
/// refused is a turn that is not a quarter of one, and a skew: the image is not
/// an axis-aligned rect at all, and no reordering makes it one.
fn rebase(clip: &PlacedShape, onto: &PlacedShape) -> Option<PlacedShape> {
    if clip.placement == onto.placement {
        return Some(*clip);
    }
    let between = onto.placement.inverse()?.then(&clip.placement);
    if !between.rect_stays_rect() {
        return None;
    }
    Some(PlacedShape {
        rect: between.map_rect(clip.rect),
        // One radius per corner, so an unevenly scaled corner — an ellipse —
        // is approximated by the geometric mean of its two axes. Exactly what
        // this did before a clip carried a transform, and the one place in
        // this type where "the radii stay circles" is not true.
        //
        // The permutation is the other half: a quarter turn leaves the corner
        // sizes alone and moves which corner each belongs to.
        radii: clip
            .radii
            .scaled(between.extract_scale())
            .permuted(between.corner_permutation()),
        curvature: clip.curvature,
        placement: onto.placement,
    })
}

/// `base`, cut down by a second clip written in the same coordinates.
fn tighten(base: &PlacedShape, other: &PlacedShape) -> PlacedShape {
    let min_x = base.rect.x.max(other.rect.x);
    let min_y = base.rect.y.max(other.rect.y);
    let max_x = (base.rect.x + base.rect.width).min(other.rect.x + other.rect.width);
    let max_y = (base.rect.y + base.rect.height).min(other.rect.y + other.rect.height);

    // Corner by corner, the smaller radius: an intersection can only be
    // rounded where both clips are.
    let radii = CornerRadii {
        top_left: base.radii.top_left.min(other.radii.top_left),
        top_right: base.radii.top_right.min(other.radii.top_right),
        bottom_right: base.radii.bottom_right.min(other.radii.bottom_right),
        bottom_left: base.radii.bottom_left.min(other.radii.bottom_left),
    };
    // The curvature of whichever clip rounds the least, for the same reason.
    let curvature = if base.radii.max() <= other.radii.max() {
        base.curvature
    } else {
        other.curvature
    };

    PlacedShape {
        // Clamped to non-negative: two clips that miss each other leave nothing.
        rect: Rect::new(
            min_x,
            min_y,
            (max_x - min_x).max(0.0),
            (max_y - min_y).max(0.0),
        ),
        radii,
        curvature,
        placement: base.placement,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::NodeId;
    use crate::renderer::tree::ClipRegion;
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
            layer,
            opacity: 1.0,
            clip: None,
        }
    }

    /// Drive `push` with a sequence of layers and report the resulting groups
    /// as the layers each one holds, in draw order.
    fn groups(sequence: &[RenderLayer]) -> Vec<Vec<RenderLayer>> {
        let mut layered = FlattenScratch::default();
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
            let told = flatten_root_into(
                &node,
                &mut commands,
                &mut layers,
                &mut FlattenScratch::default(),
            )
            .compositor_blur;
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

    /// The other region a frame carries, and the same two questions: the
    /// compositor has to be told it is there, and a command that draws nothing
    /// must not cost a draw group. This one is stricter than the blur above —
    /// it is declared *over* a background rather than under one, which is the
    /// order `Container::paint` emits, and the order that would split a group
    /// for a command with no pixels in it.
    #[test]
    fn an_input_declaration_is_carried_without_costing_a_group() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let mut node = RenderNode::new(1);
        node.bounds = rect;
        node.commands
            .push(Rc::new(DrawCommand::rounded_rect(rect, Color::WHITE, 0.0)));
        node.commands.push(Rc::new(DrawCommand::InputRegion {
            rect,
            corner_radii: CornerRadii::from(0.0),
            curvature: 1.0,
            takes: true,
        }));

        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        let carried = flatten_root_into(
            &node,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );

        assert!(
            carried.input_region,
            "the frame carries a declaration, and the loop is told so"
        );
        assert_eq!(layers.len(), 1, "and it splits nothing to say it");
        assert_eq!(commands.len(), 2, "while still travelling with the frame");
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

        let mut original = FlattenScratch::default();
        for layer in sequence {
            original.push(command(layer));
        }
        let cached = original.commands_since(
            Mark {
                groups: 1,
                buckets: [0; LAYER_COUNT],
            },
            None,
        );

        let mut replayed = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 50.0, 20.0)));

        // Everything from here belongs to the "subtree" being captured.
        let mark = layered.mark();
        // Does not overlap the text, so it joins the same group and slots
        // into the shapes bucket, ahead of the text already there.
        layered.push(command_at(Shapes, Rect::new(500.0, 0.0, 50.0, 20.0)));
        layered.push(command_at(Text, Rect::new(500.0, 0.0, 50.0, 20.0)));

        let captured = layered.commands_since(mark, None);
        assert_eq!(
            captured.len(),
            2,
            "the mark must not sweep up the earlier text"
        );
        assert_eq!(captured[0].layer, Shapes);
        assert_eq!(captured[1].layer, Text);
    }

    #[test]
    fn a_captured_subtree_is_allocated_once() {
        // The length is known from the bucket lengths before a command is
        // cloned, and a cached entry is collected for every clean subtree of
        // every frame. Letting the vector find its own size instead copies a
        // large struct over as it fills: eight shapes take the buffer to
        // eight, and the label after them doubles it to sixteen to hold nine.
        let mut layered = FlattenScratch::default();
        let mark = layered.mark();
        for column in 0..8 {
            let x = column as f32 * 200.0;
            layered.push(command_at(Shapes, Rect::new(x, 0.0, 100.0, 40.0)));
        }
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 100.0, 40.0)));

        let captured = layered.commands_since(mark, None);
        assert_eq!(captured.len(), 9);
        assert_eq!(
            captured.capacity(),
            captured.len(),
            "the capture must be allocated to its final size, not grown into it"
        );
    }

    #[test]
    fn a_gap_between_two_labels_is_not_covered() {
        // The union of two labels spans the gap between them. Testing against
        // the union alone would split on a background landing in that gap,
        // which is what showcase did three times over.
        let mut layered = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
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
        let mut layered = FlattenScratch::default();
        layered.push(command(Images));
        layered.push(command(Shapes));
        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert!(layers.iter().all(|layer| !layer.is_empty()));
    }

    // -- a scratch that is filled again, frame after frame -------------

    /// Where the labels and the badge sit.
    const NEAR: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    };
    /// The card, and what is drawn over it. Far enough from `NEAR` that the
    /// half a font-size of slack `world_bounds` allows around a label does not
    /// reach it, which is what makes a shape here a regression covering
    /// nothing.
    const CARD: Rect = Rect {
        x: 100.0,
        y: 100.0,
        width: 40.0,
        height: 40.0,
    };

    fn box_node(id: NodeId, rect: Rect) -> RenderNode {
        let mut node = RenderNode::new(id);
        node.bounds = rect;
        node.commands
            .push(Rc::new(DrawCommand::rounded_rect(rect, Color::WHITE, 0.0)));
        node
    }

    fn text_node(id: NodeId, rect: Rect) -> RenderNode {
        let mut node = RenderNode::new(id);
        node.bounds = rect;
        node.commands.push(Rc::new(DrawCommand::Text {
            text: "hi".into(),
            rect,
            color: Color::WHITE,
            font_size: 10.0,
            font_family: crate::widgets::FontFamily::default(),
            font_weight: crate::widgets::FontWeight::default(),
        }));
        node
    }

    fn frame_of(children: Vec<RenderNode>) -> RenderNode {
        let mut node = RenderNode::new(1);
        node.bounds = Rect::new(0.0, 0.0, 200.0, 200.0);
        node.children = children.into_iter().map(Rc::new).collect();
        node
    }

    /// One frame through `scratch`, as everything it produced written down:
    /// what the frame carries, every command in draw order, and the groups
    /// over them. Two frames that differ anywhere differ here.
    fn frame_through(node: &RenderNode, scratch: &mut FlattenScratch) -> (String, usize) {
        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        let carried = flatten_root_into(node, &mut commands, &mut layers, scratch);
        (
            format!("{carried:?}\n{commands:?}\n{layers:?}"),
            layers.len(),
        )
    }

    /// What `capacity` reports is the room the buffers actually hold.
    ///
    /// The three tests below read those numbers to decide whether a frame
    /// refilled the last frame's buffers, and `capacity`, `command_capacity`
    /// and `heap_rects` exist for nothing else — no frame reads them, so this
    /// is the only thing that will ever ask whether they are right. Their
    /// assertions are "not zero" and "not changed", which a `capacity` that
    /// answered a constant, or added its five buckets up with the wrong
    /// operator, would satisfy as happily as a correct one.
    ///
    /// So the buffers are given room by hand, in distinct amounts across two
    /// groups, and the answer is the one arithmetic anybody can check.
    #[test]
    fn the_capacity_reported_is_the_room_the_buffers_hold() {
        let mut scratch = FlattenScratch::default();
        scratch.groups.push(LayerBuckets::default());

        // Distinct primes, so that a sum taken with the wrong operator lands
        // nowhere near this one: 2*3*5*7*11 is 2310, and going down from 2
        // underflows rather than arriving at 28.
        let rooms = [[2, 3, 5, 7, 11], [13, 17, 19, 23, 29]];
        for (group, rooms) in scratch.groups.iter_mut().zip(rooms) {
            for (bucket, room) in group.buckets_mut().into_iter().zip(rooms) {
                bucket.reserve_exact(room);
                assert_eq!(
                    bucket.capacity(),
                    room,
                    "the allocator gave a bucket more than it asked for, so \
                     the sum below is arithmetic about the wrong numbers"
                );
            }
        }
        let commands: usize = rooms.iter().flatten().sum();

        // Two of the ten `LayerBounds` spill off the stack and the rest do
        // not, which is the other half of `heap_rects`: inline room belongs to
        // the group and is not a frame's doing.
        for (group, room) in scratch.groups.iter_mut().zip([31, 37]) {
            let bounds = &mut group.bounds[RenderLayer::Shapes as usize];
            bounds.rects.reserve_exact(room);
            assert!(
                bounds.rects.spilled() && bounds.rects.capacity() == room,
                "the rects were meant to spill onto the heap with exactly the \
                 room asked for"
            );
        }

        let clip = PlacedShape::from_clip(
            &ClipRegion {
                rect: CARD,
                corner_radius: CornerRadii::from(0.0),
                curvature: 1.0,
            },
            Transform::IDENTITY,
        );
        let placed = 10;
        for _ in 0..placed {
            scratch.clips.place(clip, Under::Nothing);
        }

        assert_eq!(
            commands, 129,
            "the primes above add up to this and to nothing else, which is \
             what the sum below is being held to"
        );

        let reported = scratch.capacity();
        assert_eq!(
            reported.commands, commands,
            "the commands reported are not the five buckets of both groups \
             added together"
        );
        assert_eq!(
            reported.rects,
            31 + 37,
            "the rects reported are not the two spilled buffers added \
             together — the eight that never left the stack are not room a \
             frame won"
        );
        assert_eq!(
            reported.groups,
            scratch.groups.capacity(),
            "the groups reported are not the groups the scratch can hold"
        );
        assert!(
            reported.clips >= placed,
            "the clip tree holds {placed} clips and reports room for \
             {} of them",
            reported.clips
        );
    }

    /// A scratch that has already drawn a frame flattens the next one exactly
    /// as a fresh scratch would.
    ///
    /// This is the whole of what makes the scratch reusable, and every field
    /// of it is a way to get this wrong: a `high_water` left high splits the
    /// next frame's first command off on its own, bounds left behind split it
    /// against rectangles nothing drew, a stale `carried` tells the loop to
    /// publish a region this frame does not declare. None of them is visible
    /// in the frame that left them — they are read by the frame after.
    ///
    /// So two different frames, and the second one is the subject. The first
    /// leaves every field dirty: a clip, an input region, and an overlay over
    /// the card, which is the highest layer there is.
    ///
    /// The second is four commands that belong in one group, each of which
    /// something left behind would split. The card goes down first, so that a
    /// split after it has a group to leave behind — split on the *first*
    /// command and the group in front is empty, and an empty group is dropped
    /// on the way out. Then the badge, turned: a rotation has no bounds a
    /// rectangle can hold, so `covered_above` assumes the worst and a
    /// `high_water` still at overlay splits on it. Then the label, which lifts
    /// the mark to text. Then the tint over the card, a regression under the
    /// label that covers nothing the label covers — and exactly what the
    /// previous frame's overlay covered, so bounds left behind split on it.
    #[test]
    fn a_scratch_that_drew_a_frame_flattens_the_next_like_a_fresh_one() {
        let first_frame = || {
            let mut node = box_node(1, CARD);
            node.clip = Some(ClipRegion {
                rect: CARD,
                corner_radius: CornerRadii::uniform(4.0),
                curvature: 1.0,
            });
            node.commands.push(Rc::new(DrawCommand::InputRegion {
                rect: CARD,
                corner_radii: CornerRadii::from(0.0),
                curvature: 1.0,
                takes: true,
            }));
            node.overlay_commands
                .push(Rc::new(DrawCommand::rounded_rect(CARD, Color::WHITE, 0.0)));
            node
        };

        let second_frame = || {
            let mut badge = box_node(3, NEAR);
            badge.local_transform = Transform::rotate_degrees(45.0);
            frame_of(vec![
                box_node(6, CARD),
                badge,
                text_node(4, NEAR),
                box_node(7, CARD),
            ])
        };

        // A fresh tree each time: what is under test is the scratch, and a
        // node flattened twice would be answered out of its own flatten cache.
        let mut reused = FlattenScratch::default();
        frame_through(&first_frame(), &mut reused);
        let after_a_frame = frame_through(&second_frame(), &mut reused);
        let from_scratch = frame_through(&second_frame(), &mut FlattenScratch::default());

        assert_eq!(
            from_scratch.1, 1,
            "the second frame is meant to be one group of its own accord — \
             split it here and the comparison below stops asking anything"
        );
        assert_eq!(
            after_a_frame.0, from_scratch.0,
            "the second frame came out differently because the first one had \
             been through the same scratch"
        );
    }

    /// A group opened again is as empty as one opened for the first time.
    ///
    /// The test above only ever fills the group a frame starts in. A frame
    /// that splits leaves a *second* group behind, and the next frame that
    /// splits is handed that one back — with its bounds and its mark from the
    /// frame before unless `clear` reached it. It is the one buffer that is
    /// recycled rather than refilled, and `clear` reaches it only because
    /// `used` still says how many groups were written when the loop runs.
    ///
    /// Both frames split on a tint over a label. What differs is what lands in
    /// the second group afterwards: a label over the card in the first frame,
    /// a shape over the card in the second. That shape regresses under the
    /// label beside it and covers nothing the label covers — but it covers
    /// exactly what the *previous* frame put there, so a group handed back
    /// with its bounds intact splits the frame a third time.
    #[test]
    fn a_group_opened_again_is_as_empty_as_a_new_one() {
        let splitting =
            |last: RenderNode| frame_of(vec![text_node(2, NEAR), box_node(3, NEAR), last]);
        let first_frame = || splitting(text_node(4, CARD));
        let second_frame = || splitting(frame_of(vec![text_node(5, NEAR), box_node(6, CARD)]));

        let mut reused = FlattenScratch::default();
        let first = frame_through(&first_frame(), &mut reused);
        let after_a_frame = frame_through(&second_frame(), &mut reused);
        let from_scratch = frame_through(&second_frame(), &mut FlattenScratch::default());

        assert_eq!(
            (first.1, from_scratch.1),
            (2, 2),
            "both frames are meant to split once and only once — a frame that \
             does not split never reaches the second group, and one that \
             splits twice reached it for a reason this test cannot name"
        );
        assert_eq!(
            after_a_frame.0, from_scratch.0,
            "the second frame came out differently because it was given the \
             first frame's second group back"
        );
    }

    /// Three frames leave the scratch the size of one.
    ///
    /// Groups and clips are what a frame *appends* to rather than fills: a
    /// group is opened, a clip is placed, and the buckets and the bounds
    /// inside them are emptied and refilled. So either one that is not given
    /// back at the start of a frame accumulates for as long as the surface
    /// lives, and the frames keep coming out right while it does — an empty
    /// group is dropped on the way to `layers`, and a clip nobody names is
    /// read by nobody.
    ///
    /// A tint over a label, under a clip: the tint regresses over something it
    /// covers, so the frame is two groups and one clip, three times over.
    #[test]
    fn three_frames_leave_the_scratch_the_size_of_one() {
        let scene = || {
            let mut node = frame_of(vec![text_node(2, CARD), box_node(3, CARD)]);
            node.clip = Some(ClipRegion {
                rect: CARD,
                corner_radius: CornerRadii::uniform(4.0),
                curvature: 1.0,
            });
            node
        };

        let mut scratch = FlattenScratch::default();
        frame_through(&scene(), &mut scratch);
        frame_through(&scene(), &mut scratch);
        let (third, groups) = frame_through(&scene(), &mut scratch);
        let from_scratch = frame_through(&scene(), &mut FlattenScratch::default());

        assert_eq!(
            groups, 2,
            "the tint is meant to regress over the label — one group here and \
             this test is about a scratch that never opens a second one"
        );
        assert_eq!(
            (scratch.groups.len(), scratch.clips.mark()),
            (groups, 1),
            "three frames of the same two groups and the same one clip left \
             the scratch holding more than one frame's worth"
        );
        assert_eq!(
            third, from_scratch.0,
            "the third frame's groups came back holding something a fresh \
             scratch does not put in them"
        );
    }
}

#[cfg(test)]
mod world_geometry_tests {
    use super::*;
    use crate::renderer::tree::ClipRegion;
    use crate::widgets::Color;
    // -- the clip as a shape, not as the box around it ------------------

    /// A clip declared inside a turned widget is kept as declared, and the
    /// transform is what says where it went.
    ///
    /// Mapping it into world space here is what the bug was: the image of a
    /// rect under a rotation is not a rect, so the only thing that could be
    /// stored was the box, and the box is bigger than the shape.
    #[test]
    fn a_turned_clip_keeps_its_shape_and_its_box_does_not() {
        let declared = ClipRegion {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        };
        let placement = Transform::rotate_degrees(45.0).center_at(40.0, 40.0);
        let clip = PlacedShape::from_clip(&declared, placement);

        assert_eq!(clip.rect, declared.rect, "the declaration, untouched");
        assert_eq!(
            clip.radii.top_left, 16.0,
            "and the radii with it — in the clip's own space a turned corner is \
             still a circle, so nothing has to be approximated"
        );

        let boxed = clip.world_aabb();
        assert!(
            boxed.width > 112.0 && boxed.width < 114.0,
            "80·√2 ≈ 113.1, and this is what the clip used to be reduced to: \
             wide enough that a 100-wide child is not touched, which is how \
             `Overflow::Hidden` stopped hiding — got {}",
            boxed.width
        );
    }

    /// The point the shader tests, back where the clip was written.
    ///
    /// A corner of the declared rect must come back to that corner, and a
    /// point the box contains but the turned shape does not must land outside
    /// it — which is the whole difference between clipping the shape and
    /// clipping its box.
    #[test]
    fn a_fragment_comes_back_to_the_space_the_clip_was_written_in() {
        let placement = Transform::rotate_degrees(45.0).center_at(40.0, 40.0);
        let clip = PlacedShape {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement,
        };
        let back = clip.to_local(1.0).expect("a rotation is invertible");

        let (wx, wy) = placement.transform_point(80.0, 80.0);
        let (cx, cy) = back.transform_point(wx, wy);
        assert!(
            (cx - 80.0).abs() < 0.01 && (cy - 80.0).abs() < 0.01,
            "the far corner comes back to the far corner, got ({cx}, {cy})"
        );

        // The left-hand corner of the bounding box, which the turned square
        // misses by the whole of its own diagonal overhang.
        let boxed = clip.world_aabb();
        let (cx, cy) = back.transform_point(boxed.x + 1.0, boxed.y + 1.0);
        assert!(
            cx < 0.0 || cy < 0.0 || cx > 80.0 || cy > 80.0,
            "a point in the box but not in the shape lands outside the clip \
             rect, so the shader cuts it — got ({cx}, {cy})"
        );
    }

    /// The surface scale is undone in the matrix and not in the rect, because
    /// the rect is in logical units and the fragment is in physical ones.
    ///
    /// Scaling both would apply the scale twice and clip a HiDPI surface to a
    /// quarter of its viewport.
    #[test]
    fn the_surface_scale_is_undone_once() {
        let clip = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::translate(10.0, 20.0),
        };
        let back = clip.to_local(2.0).expect("a translation is invertible");

        // The bottom-right of the clip is at (110, 70) logical, (220, 140)
        // physical.
        let (cx, cy) = back.transform_point(220.0, 140.0);
        assert!(
            (cx - 100.0).abs() < 0.01 && (cy - 50.0).abs() < 0.01,
            "the far corner in physical pixels comes back to the far corner of \
             the logical rect, got ({cx}, {cy})"
        );
    }

    /// The identity fast path carries a fragment the same distance the general
    /// one would.
    ///
    /// `to_local` short-circuits a placement of identity — most clips
    /// in most frames — instead of inverting it. It was added for speed after
    /// the test above was written, and that test uses a *translated* placement,
    /// so it exercises the other branch: the whole fast path shipped unwatched
    /// until the mutation job said so.
    ///
    /// The scales are not 1.0 on purpose. At 1.0 a shrink, a stretch and a
    /// remainder all leave the point where it was, and every mutant lives.
    #[test]
    fn the_identity_fast_path_still_undoes_the_scale() {
        let unplaced = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 50.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };

        for scale in [2.0, 1.5, 0.5] {
            let back = unplaced.to_local(scale).expect("invertible");
            let (x, y) = back.transform_point(100.0 * scale, 50.0 * scale);
            assert!(
                (x - 100.0).abs() < 0.01 && (y - 50.0).abs() < 0.01,
                "the far corner in physical pixels comes back to the far corner \
                 of the logical rect at scale {scale}, got ({x}, {y})"
            );

            // And it agrees with the route a placement that is *not* the
            // identity takes, which is the only reason the branch is allowed
            // to exist.
            let placed = PlacedShape {
                placement: Transform::translate(11.0, -3.0),
                ..unplaced
            };
            let long = placed.to_local(scale).expect("invertible");
            assert_eq!(
                (back.a(), back.b(), back.c(), back.d()),
                (long.a(), long.b(), long.c(), long.d()),
                "the two routes shrink by the same factor at scale {scale}; \
                 only the offset differs, because one clip has moved"
            );
        }
    }

    /// The intersection is curved by whichever clip rounds the *least*.
    ///
    /// Every other test here uses a curvature of 1.0 on both sides, so the
    /// comparison that picks between them could be reversed and none of them
    /// would move. A squircle clip inside a circular one draws the circular
    /// corner, because that is the corner that is actually there.
    #[test]
    fn the_flatter_corner_supplies_the_curve() {
        let circular = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(8.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };
        let squircular = PlacedShape {
            radii: CornerRadii::uniform(24.0),
            curvature: 2.0,
            ..circular
        };

        assert_eq!(
            intersect_clips(&circular, &squircular).curvature,
            1.0,
            "the rounder clip cuts less deeply, so its curve is the one on show"
        );
        assert_eq!(
            intersect_clips(&squircular, &circular).curvature,
            1.0,
            "and the answer does not depend on which was asked first"
        );
    }

    /// A placement that collapses has no inverse, and a clip with no inside is
    /// not a clip that lets everything through.
    #[test]
    fn a_collapsed_clip_has_no_way_back() {
        let clip = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::scale(0.0),
        };
        assert!(clip.to_local(1.0).is_none());
    }

    /// A ripple is cut to the shape it belongs to, through a rotation.
    ///
    /// This is the path the retired `clip_is_local` flag existed for. An
    /// overlay's clip used to be marked "test this one in local coordinates"
    /// and every other clip tested in world ones; now every clip says where its
    /// coordinates are, and the overlay case is the node's own transform.
    /// Equivalent by algebra, which is not the same as watched — nothing in the
    /// suite builds an `overlay_clip`, and no golden draws a ripple, so the one
    /// arrangement the flag was for was also the one nothing would have caught.
    #[test]
    fn an_overlay_is_clipped_in_the_space_of_the_node_it_belongs_to() {
        let mut node = RenderNode::new(1);
        node.bounds = Rect::new(0.0, 0.0, 80.0, 80.0);
        node.local_transform = Transform::rotate_degrees(45.0);
        node.overlay_clip = Some(ClipRegion {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        });
        node.overlay_commands
            .push(Rc::new(DrawCommand::rounded_rect(
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Color::WHITE,
                0.0,
            )));

        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        flatten_root_into(
            &node,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );

        let overlay = commands
            .iter()
            .find(|c| c.layer == RenderLayer::Overlay)
            .expect("the ripple is in the frame");
        let clip = overlay.clip().expect("and it is clipped");

        assert_eq!(
            clip.rect,
            Rect::new(0.0, 0.0, 80.0, 80.0),
            "the clip as declared, in the node's own coordinates — not a box \
             around it in world ones"
        );
        assert_eq!(clip.radii.top_left, 16.0, "and its corners, uncrushed");
        assert_eq!(
            clip.placement, overlay.world_transform,
            "placed by the very transform the ripple is drawn through, which is \
             what makes the ripple follow the shape instead of an axis-aligned \
             box around it"
        );

        // The thing the flag was guarding against, stated as a number: the box
        // is half as wide again as the shape, and a ripple tested against it
        // spills past the corners of what it belongs to.
        assert!(
            clip.world_aabb().width > 112.0,
            "80·√2 ≈ 113.1, got {}",
            clip.world_aabb().width
        );
    }

    /// A cached subtree replayed at a new position carries its clip with it —
    /// on both axes.
    ///
    /// The replay path skips re-flattening and shifts what it kept by the
    /// distance the subtree moved. The clip's rect is in its owner's
    /// coordinates and has not moved relative to the owner, so what is shifted
    /// is the placement.
    ///
    /// Both axes, and both the clip and the command, because the existing
    /// watch on this path is a vertical scroller whose own transform does not
    /// move: `dx` is zero there and the replay's `dy` never fires, so every way
    /// of getting either shift wrong produced the same answer as getting it
    /// right. The mutation job found five of those, one of them in code older
    /// than this change.
    #[test]
    fn a_replayed_clip_moves_on_both_axes() {
        let mut child = RenderNode::new(2);
        child.bounds = Rect::new(0.0, 0.0, 40.0, 40.0);
        child.clip = Some(ClipRegion {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });
        child.commands.push(Rc::new(DrawCommand::rounded_rect(
            Rect::new(0.0, 0.0, 40.0, 40.0),
            Color::WHITE,
            0.0,
        )));

        let mut root = RenderNode::new(1);
        root.bounds = Rect::new(0.0, 0.0, 200.0, 200.0);
        root.local_transform = Transform::translate(10.0, 20.0);
        root.children.push(Rc::new(child));

        // The clip and the command it cuts, which move together or the clip
        // stops describing the thing it is cutting.
        let replayed = |node: &RenderNode| {
            let (mut commands, mut layers) = (Vec::new(), Vec::new());
            flatten_root_into(
                node,
                &mut commands,
                &mut layers,
                &mut FlattenScratch::default(),
            );
            let cmd = commands.first().expect("the child draws");
            let clip = cmd.clip().expect("the child clips");
            (
                (clip.placement.tx(), clip.placement.ty()),
                (cmd.world_transform.tx(), cmd.world_transform.ty()),
            )
        };

        // First pass populates the subtree's cached flatten.
        assert_eq!(replayed(&root), ((10.0, 20.0), (10.0, 20.0)));

        // Now move it in *both* axes and replay from the cache rather than
        // re-flattening: nothing is dirty, and the transform is a translation.
        root.local_transform = Transform::translate(35.0, 65.0);
        root.repainted.set(false);
        for c in &root.children {
            c.repainted.set(false);
        }

        assert_eq!(
            replayed(&root),
            ((35.0, 65.0), (35.0, 65.0)),
            "the clip and the command are both where the subtree now is, on \
             each axis independently"
        );
    }

    /// A replayed subtree puts its clips back under each other, not under
    /// whatever the arithmetic happens to land on.
    ///
    /// Everything above watches one clip inside a cached subtree. The links
    /// *between* several of them are a different thing: an entry writes them
    /// down as offsets into its own run, a replay reads them back as indices
    /// into this frame's tree, and the two pieces of arithmetic have to be
    /// inverses. A scene with one clip above the subtree, two side by side
    /// inside it and a third under the second is the smallest one where every
    /// way of getting that wrong lands somewhere different: the offset is not
    /// zero, the run does not start at zero, and the three candidate parents
    /// cut in three visibly different places.
    ///
    /// The oracle is a full flatten of the same scene at the same place. What a
    /// replay produces and what flattening produces are the same frame or one
    /// of them is a bug — which is the whole promise of the cache, stated once
    /// rather than as a list of coordinates.
    #[test]
    fn a_replayed_subtree_puts_its_clips_back_under_each_other() {
        fn clipped(id: u64, rect: Rect, children: Vec<Rc<RenderNode>>) -> RenderNode {
            let mut node = RenderNode::new(id);
            node.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
            node.clip = Some(ClipRegion {
                rect,
                corner_radius: CornerRadii::uniform(0.0),
                curvature: 1.0,
            });
            node.children = children;
            node
        }

        // root, clipped -> the subtree that is replayed -> [a clip of its own,
        // and a clip under a clip]. The last one draws.
        let scene = |dx: f32, dy: f32| {
            let mut drawn = RenderNode::new(5);
            drawn.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
            drawn.commands.push(Rc::new(DrawCommand::rounded_rect(
                Rect::new(0.0, 0.0, 100.0, 100.0),
                Color::WHITE,
                0.0,
            )));

            let inner = clipped(4, Rect::new(0.0, 20.0, 100.0, 60.0), vec![Rc::new(drawn)]);
            let outer = clipped(3, Rect::new(0.0, 0.0, 100.0, 60.0), vec![Rc::new(inner)]);
            // Beside the other two rather than above them, so the run holds
            // three clips and the link that matters points at the middle one.
            let beside = clipped(2, Rect::new(50.0, 0.0, 50.0, 100.0), Vec::new());

            let mut subtree = RenderNode::new(1);
            subtree.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
            subtree.children = vec![Rc::new(beside), Rc::new(outer)];

            let mut root = clipped(0, Rect::new(0.0, 0.0, 100.0, 100.0), vec![Rc::new(subtree)]);
            root.local_transform = Transform::translate(dx, dy);
            root
        };

        let cut = |node: &RenderNode| {
            let (mut commands, mut layers) = (Vec::new(), Vec::new());
            flatten_root_into(
                node,
                &mut commands,
                &mut layers,
                &mut FlattenScratch::default(),
            );
            commands
                .iter()
                .find(|c| matches!(&*c.command, DrawCommand::RoundedRect { .. }))
                .expect("the box draws")
                .clip()
                .expect("under three clips")
        };

        let mut replayed = scene(10.0, 20.0);
        let at_rest = cut(&replayed);
        assert_eq!(
            at_rest.rect,
            Rect::new(0.0, 20.0, 100.0, 40.0),
            "the box is cut by the two clips over it and not by the one beside \
             them"
        );

        // The subtree below the root is clean; the root is not, so it places
        // its own clip afresh and the subtree comes back out of the cache.
        replayed.local_transform = Transform::translate(35.0, 65.0);
        fn mark_clean(node: &RenderNode) {
            node.repainted.set(false);
            for child in &node.children {
                mark_clean(child);
            }
        }
        for child in &replayed.children {
            mark_clean(child);
        }

        let moved = cut(&replayed);
        assert_ne!(
            moved.placement, at_rest.placement,
            "the subtree was replayed where it already was, so this says \
             nothing about putting it back somewhere else"
        );
        assert_eq!(
            moved,
            cut(&scene(35.0, 65.0)),
            "the replayed subtree's clips came back under the wrong ones: a \
             replay and a flatten of the same scene in the same place are the \
             same frame, or the cache is not a cache"
        );
    }

    /// A replayed overlay is cut to the shape it belongs to and to nothing
    /// above it.
    ///
    /// The other half of what a replay has to know about a clip, and the one
    /// with no second witness anywhere else. A cached clip that recorded no
    /// parent means one of two things, and they differ only a frame later:
    /// *nothing was above me then*, which a replay must ask again because
    /// something may be above the subtree now, and *nothing is ever above me*,
    /// which is what an overlay's clip is — a ripple is cut to the shape it
    /// belongs to, and the full-flatten path has always replaced the inherited
    /// clip with it rather than narrowing it. `Under::Nothing` is the second,
    /// and reading it as the first would narrow a replayed ripple by a
    /// scroller it never answered to.
    ///
    /// So the overlay sits under a clip that would cut it in half, in a subtree
    /// that is replayed rather than flattened, and comes out whole.
    #[test]
    fn a_replayed_overlay_answers_to_no_clip_above_it() {
        let mut child = RenderNode::new(3);
        child.bounds = Rect::new(0.0, 0.0, 40.0, 40.0);
        child.overlay_clip = Some(ClipRegion {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });
        child
            .overlay_commands
            .push(Rc::new(DrawCommand::rounded_rect(
                Rect::new(0.0, 0.0, 40.0, 40.0),
                Color::WHITE,
                0.0,
            )));

        let mut middle = RenderNode::new(2);
        middle.bounds = Rect::new(0.0, 0.0, 40.0, 40.0);
        middle.children.push(Rc::new(child));

        let mut root = RenderNode::new(1);
        root.bounds = Rect::new(0.0, 0.0, 200.0, 200.0);
        // Half the overlay's height: if the overlay ever answered to it, the
        // rect below would be 20 tall rather than 40.
        root.clip = Some(ClipRegion {
            rect: Rect::new(0.0, 0.0, 40.0, 20.0),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        });
        root.children.push(Rc::new(middle));

        let overlay_clip = |node: &RenderNode| {
            let (mut commands, mut layers) = (Vec::new(), Vec::new());
            flatten_root_into(
                node,
                &mut commands,
                &mut layers,
                &mut FlattenScratch::default(),
            );
            commands
                .iter()
                .find(|c| c.layer == RenderLayer::Overlay)
                .expect("the ripple is in the frame")
                .clip()
                .expect("and it is clipped")
                .rect
        };

        assert_eq!(
            overlay_clip(&root),
            Rect::new(0.0, 0.0, 40.0, 40.0),
            "flattened in full, the overlay takes its own clip and not the \
             one above it"
        );

        // The subtree below the clip is clean and the clip's own node is not,
        // so the middle node is replayed while the clip above it is placed
        // afresh — which is the arrangement that asks the question.
        for c in &root.children {
            c.repainted.set(false);
        }

        assert_eq!(
            overlay_clip(&root),
            Rect::new(0.0, 0.0, 40.0, 40.0),
            "replayed, the overlay came back cut by the clip above it, which \
             it never answered to when it was flattened"
        );
    }

    // -- two clips ------------------------------------------------------

    /// Two clips under one transform intersect in that transform's space, and
    /// come out still turned.
    ///
    /// Falling back to the world box here would undo the fix for every
    /// scroller that happens to sit inside another one.
    #[test]
    fn two_clips_in_one_space_stay_in_it() {
        let placement = Transform::rotate_degrees(30.0);
        let outer = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(8.0),
            curvature: 1.0,
            placement,
        };
        let inner = PlacedShape {
            rect: Rect::new(20.0, 0.0, 100.0, 60.0),
            radii: CornerRadii::uniform(16.0),
            curvature: 1.0,
            placement,
        };

        let both = intersect_clips(&outer, &inner);
        assert_eq!(both.placement, placement, "still turned by the same thing");
        assert_eq!(
            (both.rect.x, both.rect.width, both.rect.height),
            (20.0, 80.0, 60.0),
            "the overlap, exactly, in the space they were both written in"
        );
        assert_eq!(
            both.radii.top_left, 8.0,
            "an intersection is only as round as its rounder edge allows"
        );
    }

    /// A clip inside a scroller inside a scroller: the two spaces differ by a
    /// translation, which keeps the axes, so the overlap is still exact.
    #[test]
    fn a_clip_is_rebased_through_a_translation() {
        let outer = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };
        let inner = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::translate(40.0, 0.0),
        };

        let both = intersect_clips(&outer, &inner);
        assert_eq!(
            both.placement,
            Transform::IDENTITY,
            "rebased onto the first"
        );
        assert_eq!(
            (both.rect.x, both.rect.width),
            (40.0, 60.0),
            "the inner one, moved into the outer's coordinates, and cut there"
        );
    }

    /// A pixel is covered by the published region.
    fn covers(rects: &[crate::region::RegionRect], x: i32, y: i32) -> bool {
        rects
            .iter()
            .any(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)
    }

    /// What the compositor is told a surface covers follows the clip chain, and
    /// the chain is collapsed before it gets here. So widening that collapse
    /// changes the published region, not only the pixels — and for a quarter
    /// turn it changes *only* the corners, since the rect survives the turn
    /// either way. A rounded corner on the wrong side is a wedge of the surface
    /// that stops being ours: the compositor sends the click to whatever is
    /// behind, which from inside the application looks like nothing happening.
    #[test]
    fn a_region_under_two_clips_a_quarter_turn_apart_rounds_the_right_corner() {
        let outer = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            CornerRadii::uniform(40.0),
            1.0,
            Transform::IDENTITY,
        );
        // The same square, a quarter turn round and moved back over the outer,
        // rounded at one corner only.
        let inner = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            CornerRadii {
                top_left: 40.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            1.0,
            Transform::translate(100.0, 0.0).then(&Transform::rotate_degrees(90.0)),
        );

        let both = intersect_clips(&outer, &inner);
        assert!(
            (both.rect.width - 100.0).abs() < 0.01 && (both.rect.height - 100.0).abs() < 0.01,
            "the square survives the turn whichever way this is answered: {:?}",
            both.rect
        );

        let rects = crate::region::placed_shape_to_rects(both, None);
        // A rounded corner is the one cut *away*, so the square corner is the
        // covered one. Without the permutation these two swap: the rounding
        // stays on the top-left and the region gives up the wrong wedge.
        assert!(
            covers(&rects, 3, 3),
            "the inner's rounding turned a quarter clockwise off the top-left, \
             so this corner is square and the surface still claims it"
        );
        assert!(
            !covers(&rects, 96, 3),
            "and the top-right is where the rounding landed, so the region \
             gives that wedge up — claim it instead and a click there is sent \
             to a surface that does not draw it"
        );
    }

    /// A quarter turn sends a rect to a rect — the axes swap rather than lean —
    /// so the overlap is still exact, and the corners follow the turn.
    #[test]
    fn a_clip_a_quarter_turn_away_is_rebased_not_boxed() {
        // Rounded generously, because `tighten` takes the smaller radius per
        // corner: against a square outer clip the intersection is square
        // whatever the inner one does, and the permutation would be invisible.
        let outer = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 60.0),
            radii: CornerRadii::uniform(20.0),
            curvature: 1.0,
            placement: Transform::rotate_degrees(30.0),
        };
        let inner = PlacedShape {
            rect: Rect::new(0.0, 0.0, 60.0, 100.0),
            radii: CornerRadii {
                top_left: 12.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            curvature: 1.0,
            // A further quarter turn, so `between` swaps the axes.
            placement: Transform::rotate_degrees(30.0).then(&Transform::rotate_degrees(90.0)),
        };

        let both = intersect_clips(&outer, &inner);
        assert_eq!(
            both.placement, outer.placement,
            "rebased onto the first, not flattened to the box around both"
        );
        assert!(
            (both.radii.top_right - 12.0).abs() < 1e-3,
            "the inner's top-left corner turned a quarter clockwise, so it is \
             the top-right one now: {:?}",
            both.radii
        );
        assert_eq!(
            (
                both.radii.top_left,
                both.radii.bottom_right,
                both.radii.bottom_left
            ),
            (0.0, 0.0, 0.0),
            "and nothing else picked up a radius"
        );
    }

    /// A mirror sends a rect to a rect too, and `Container::scale` takes a
    /// negative factor, so this is reachable rather than hypothetical.
    #[test]
    fn a_mirrored_clip_is_rebased_not_boxed() {
        // Turned, so that "rebased onto the first" is a claim the flattening
        // fallback cannot also satisfy — it would answer the identity.
        let outer = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radii: CornerRadii::uniform(20.0),
            curvature: 1.0,
            placement: Transform::rotate_degrees(20.0),
        };
        let inner = PlacedShape {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            radii: CornerRadii {
                top_left: 9.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 5.0,
            },
            curvature: 1.0,
            placement: outer
                .placement
                .then(&Transform::scale_xy(-1.0, 1.0).then_translate(100.0, 0.0)),
        };

        let both = intersect_clips(&outer, &inner);
        assert_eq!(
            both.placement, outer.placement,
            "rebased onto the first, not flattened to the box around both"
        );
        assert!(
            (both.radii.top_right - 9.0).abs() < 1e-3
                && (both.radii.bottom_right - 5.0).abs() < 1e-3,
            "mirrored across x: the left corners are the right ones now, {:?}",
            both.radii
        );
        assert_eq!((both.radii.top_left, both.radii.bottom_left), (0.0, 0.0));
    }

    /// Two clips turned differently cannot both be a rect in one space, so the
    /// answer is the box around each — over-generous, and what every case here
    /// produced before any of this carried a transform.
    ///
    /// The wall [`intersect_clips`] documents at length.
    /// And the corners of those boxes cut at least as deep as the ellipses they
    /// stand for, so the fallback stays inside the shape rather than reaching
    /// past it. The larger axis, not the mean of the two — under a uniform
    /// scale they agree, which is why every other test here cannot tell.
    #[test]
    fn a_flattened_clip_rounds_by_the_larger_axis() {
        let clip = PlacedShape::placed(
            Rect::new(0.0, 0.0, 100.0, 100.0),
            CornerRadii::uniform(10.0),
            1.0,
            Transform::scale_xy(3.0, 1.0),
        );

        assert_eq!(
            clip.world_radii().top_left,
            30.0,
            "the corner is 30 wide and 10 tall; cutting 30 stays inside it, and \
             cutting the mean of 17.3 would round less than the shape does"
        );
    }

    #[test]
    fn two_clips_turned_differently_fall_back_to_their_boxes() {
        let a = PlacedShape {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::rotate_degrees(45.0),
        };
        let b = PlacedShape {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::rotate_degrees(10.0),
        };

        let both = intersect_clips(&a, &b);
        assert_eq!(
            both.placement,
            Transform::IDENTITY,
            "world coordinates, where a box is a box"
        );
        let (a_box, b_box) = (a.world_aabb(), b.world_aabb());
        let expected_right = (a_box.x + a_box.width).min(b_box.x + b_box.width);
        assert!(
            (both.rect.x + both.rect.width - expected_right).abs() < 0.01,
            "the two boxes, intersected"
        );
    }

    /// A clip that misses its parent's entirely leaves nothing, not a rect of
    /// negative width that would read as covering the plane.
    #[test]
    fn two_clips_that_miss_leave_nothing() {
        let here = PlacedShape {
            rect: Rect::new(0.0, 0.0, 50.0, 50.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };
        let far = PlacedShape {
            rect: Rect::new(500.0, 500.0, 50.0, 50.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        };

        let both = intersect_clips(&here, &far);
        assert_eq!((both.rect.width, both.rect.height), (0.0, 0.0));
    }

    /// A translation moves the box it is cut from and changes nothing else.
    #[test]
    fn a_translation_only_moves_it() {
        let clip = PlacedShape {
            rect: Rect::new(0.0, 0.0, 100.0, 60.0),
            radii: CornerRadii::uniform(8.0),
            curvature: 1.0,
            placement: Transform::translate(20.0, 30.0),
        };

        let world = clip.world_aabb();
        assert_eq!((world.x, world.y), (20.0, 30.0));
        assert_eq!((world.width, world.height), (100.0, 60.0));
        assert_eq!(clip.world_radii().max(), 8.0, "and the corners with it");
    }
}

#[cfg(test)]
mod opacity_tests {
    use super::*;
    use crate::renderer::NodeId;
    use crate::widgets::Color;

    fn with_box(id: NodeId, opacity: f32) -> RenderNode {
        let mut node = RenderNode::with_bounds(id, Rect::new(0.0, 0.0, 40.0, 40.0));
        node.opacity = opacity;
        node.commands.push(Rc::new(DrawCommand::rounded_rect(
            Rect::new(0.0, 0.0, 40.0, 40.0),
            Color::WHITE,
            0.0,
        )));
        node
    }

    fn flatten(root: &RenderNode) -> Vec<FlattenedCommand> {
        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        flatten_root_into(
            root,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );
        commands
    }

    /// A frame of `child` under a parent at `opacity`, as a fading container
    /// paints one: the parent is new, the child is the same `Rc`.
    fn frame_under(child: &Rc<RenderNode>, opacity: f32) -> RenderNode {
        let mut fading = RenderNode::new(2);
        fading.opacity = opacity;
        fading.children.push(Rc::clone(child));
        let mut root = RenderNode::new(1);
        root.children.push(Rc::new(fading));
        root
    }

    fn opacities(commands: &[FlattenedCommand]) -> Vec<f32> {
        commands.iter().map(|c| c.opacity).collect()
    }

    /// An opacity is the product of every one above it: a half inside a half
    /// draws at a quarter, and an overlay drawn after the children is drawn at
    /// its own node's opacity, not at the last child's.
    #[test]
    fn nested_opacities_multiply() {
        let mut inner = with_box(3, 0.5);
        inner
            .overlay_commands
            .push(Rc::new(DrawCommand::circle((5.0, 5.0), 5.0, Color::WHITE)));
        let mut outer = with_box(2, 0.5);
        outer.children.push(Rc::new(inner));
        outer
            .overlay_commands
            .push(Rc::new(DrawCommand::circle((5.0, 5.0), 5.0, Color::WHITE)));
        let mut root = RenderNode::new(1);
        root.children.push(Rc::new(outer));

        let commands = flatten(&root);
        let by_layer = |layer| {
            commands
                .iter()
                .filter(|c| c.layer == layer)
                .map(|c| c.opacity)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            by_layer(RenderLayer::Shapes),
            [0.5, 0.25],
            "the outer box at a half, the inner one at a half of that"
        );
        let mut overlays = by_layer(RenderLayer::Overlay);
        overlays.sort_by(f32::total_cmp);
        assert_eq!(overlays, [0.25, 0.5], "each overlay at its own node's");
    }

    /// A clean subtree under an ancestor whose opacity changed is replayed,
    /// not flattened again, and drawn at the opacity it now inherits.
    ///
    /// This is the frame a fade is made of: the fading container repaints, so
    /// its node is new each frame, and everything under it comes back clean
    /// out of the paint cache with the old opacity multiplied into its cached
    /// commands.
    #[test]
    fn a_clean_subtree_is_replayed_at_the_opacity_it_now_inherits() {
        let child = Rc::new(with_box(3, 0.5));
        let frame_under = |opacity| frame_under(&child, opacity);

        assert_eq!(opacities(&flatten(&frame_under(0.8))), [0.4]);
        let entry = child.cached_flatten.borrow().clone().expect("cached");

        // Served from the paint cache from here on.
        child.repainted.set(false);
        assert_eq!(
            opacities(&flatten(&frame_under(0.4))),
            [0.2],
            "replayed at the old opacity: the fade stopped at the child"
        );
        assert!(
            Rc::ptr_eq(&entry, &child.cached_flatten.borrow().clone().unwrap()),
            "flattened again rather than replayed — a replay writes nothing back"
        );
    }

    /// An entry made while the subtree was invisible cannot say what it looks
    /// like visible: every command in it is at zero. It is flattened again.
    #[test]
    fn an_entry_made_at_zero_is_not_replayed() {
        let child = Rc::new(with_box(3, 1.0));
        let frame_under = |opacity| frame_under(&child, opacity);

        assert_eq!(opacities(&flatten(&frame_under(0.0))), [0.0]);
        child.repainted.set(false);
        assert_eq!(opacities(&flatten(&frame_under(0.5))), [0.5]);
    }
}
