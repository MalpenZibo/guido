//! Tree flattening with world transform computation.

use std::ops::Range;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::transform::Transform;
use crate::widgets::Rect;

use super::clip::{ClipChains, ClipIndex, ClipRef, ClipTree, ResolvedClip};
use super::commands::DrawCommand;
use super::layer_blur::blur_margin;
use super::tree::{CachedFlatten, ClipRegion, NodeId, OpacityMode, RenderNode};
use crate::backdrop::BackdropSources;
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
    /// Background shapes (filled rectangles, borders, arcs, etc.)
    Shapes = 1,
    /// Filled and stroked paths, drawn as triangles by a pipeline of their
    /// own — a layer rather than a share of the shapes' bucket, so a group's
    /// shapes stay one call and its paths one more, however the two alternate.
    Paths = 2,
    /// Image content (after shapes, before text)
    Images = 3,
    /// Text content
    Text = 4,
    /// Overlay effects (ripples, highlights)
    Overlay = 5,
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
    /// What this command is cut to, where it is this frame: a shape, and the
    /// chain of shapes it could not be merged with, in the frame's
    /// [`ClipChains`].
    ///
    /// The one step between a command and every pipeline that draws one.
    pub fn clip(&self) -> Option<ResolvedClip> {
        self.clip.as_ref().map(ClipRef::resolved)
    }
}

/// One flattened frame, as the renderer is handed it: the commands, the
/// groups they are drawn in, and the chains their clips hang from.
///
/// Three things that are only meaningful together — a command's clip names a
/// link in `clips` by index, and `layers` addresses `commands` by range — so
/// they travel as one rather than as three arguments in a row that a caller
/// could take from two different frames.
///
/// A frame that composites a subtree as a layer carries that layer's content
/// in the same two buffers, ahead of the surface's own: `passes` says which
/// ranges are whose, and the surface's groups are the ones after the last of
/// them — [`surface_layers`](Self::surface_layers).
#[derive(Clone, Copy)]
pub struct FlatFrame<'a> {
    pub commands: &'a [FlattenedCommand],
    pub layers: &'a [CommandLayer],
    /// [`FlattenScratch::clip_chains`] of the scratch `commands` came out of.
    pub clips: &'a ClipChains,
    /// The layers drawn offscreen, a layer inside another before the one it
    /// is in — [`FlattenScratch::passes`] of the same scratch.
    pub passes: &'a [LayerPass],
    /// The layers composited from a texture an earlier frame drew, with no
    /// pass this frame: a cached subtree replayed, composite and all.
    pub kept: &'a [NodeId],
    /// The surface's root node, which layer textures idle by.
    pub surface: NodeId,
}

impl<'a> FlatFrame<'a> {
    /// A frame with no layers in it — what a caller that builds its own
    /// commands, rather than flattening a tree, hands the renderer.
    pub fn of(
        commands: &'a [FlattenedCommand],
        layers: &'a [CommandLayer],
        clips: &'a ClipChains,
    ) -> Self {
        Self {
            commands,
            layers,
            clips,
            passes: &[],
            kept: &[],
            surface: 0,
        }
    }

    /// Where the surface's own groups start in `layers`: after every layer's.
    pub fn surface_start(&self) -> usize {
        self.passes.last().map_or(0, |pass| pass.groups.end)
    }

    /// The groups drawn onto the surface itself, in order.
    pub fn surface_layers(&self) -> &'a [CommandLayer] {
        &self.layers[self.surface_start()..]
    }
}

/// A subtree the frame draws into a texture of its own and composites once.
///
/// Its content sits in the frame's buffers like the surface's does, already in
/// the texture's space: the texture's top-left texel is the origin, a logical
/// pixel is `scale` texels, and nothing above the layer clips it there — the
/// clips around the layer cut the composite instead.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerPass {
    /// The node composited, which is what the texture is kept by from one
    /// frame to the next.
    pub id: NodeId,
    /// Its content, in [`FlatFrame::commands`].
    pub commands: Range<usize>,
    /// The groups that content is drawn in, in [`FlatFrame::layers`].
    pub groups: Range<usize>,
    /// How large its texture has to be, in texels.
    pub size: (u32, u32),
    /// The blur its texture is composited through, in logical pixels; zero
    /// for none.
    pub blur: f32,
    /// The layers drawn inside it, in this list — all before it, since a layer
    /// inside another is finished first. One of them drawn again is a new
    /// picture in this one.
    pub inner: Range<usize>,
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
/// group one bucket at a time — shapes, then paths, then images, then text,
/// then overlay.
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
    /// the ones a narrower frame does not need would free the six buffers
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
    /// The layers this frame has finished, a layer inside another first.
    passes: Vec<LayerPass>,
    /// Their content and groups, in the same order, which is where they land
    /// in the frame: ahead of the surface's own.
    layer_commands: Vec<FlattenedCommand>,
    layer_groups: Vec<CommandLayer>,
    /// The layers whose content is being flattened, the innermost last.
    open: Vec<OpenLayer>,
    /// How many layers this frame has opened, which numbers each one's clips.
    opened: u32,
    /// Where a layer's content goes back to when it turns out not to need a
    /// texture after all — kept so that, frame after frame, it is not a new
    /// buffer.
    elided: Vec<FlattenedCommand>,
    /// Physical pixels per logical one, and the surface's own rect, for the
    /// frame being flattened — what a layer's texture is sized against.
    scale: f32,
    surface: Option<Rect>,
    /// The surface's root node, which the renderer keeps a layer's texture
    /// idle-counting by.
    root: NodeId,
    /// The layers composited this frame from the texture an earlier frame
    /// drew, replayed with a cached subtree and named by no pass.
    kept: Vec<NodeId>,
    /// How many layers this frame has composited, drawn or kept — what says a
    /// cached subtree holds one.
    composites: usize,
    /// How many layers this frame has cut by the surface's edge, or put back
    /// as plain commands for being wholly off it — what says a cached subtree
    /// holds one that only stands where it was flattened.
    cut_by_surface: usize,
}

/// A layer whose content is being flattened.
#[derive(Debug, Clone, Copy)]
struct OpenLayer {
    /// The first of its groups in [`FlattenScratch::groups`].
    base: usize,
    /// Where its clips and chain links start in the clip tree.
    clip_mark: usize,
    link_mark: usize,
    /// Its number in the clip tree, and the number of what it is in.
    segment: u32,
    outer_segment: u32,
    /// What of its space can reach the surface at all, where that is known.
    visible: Option<Rect>,
    /// Whether a layer inside it was composited — which makes it one too.
    holds_layer: bool,
    /// How many layers the frame had finished when it opened: the ones
    /// finished after are inside it.
    first_pass: usize,
}

/// What a layer's content turned out to be, read once it has been flattened.
struct LayerScan {
    /// The box everything it draws stays inside, in its own space.
    bounds: Option<Rect>,
    /// Whether two of those draws touch the same pixel.
    overlaps: bool,
    /// Whether any of it is for the compositor rather than for the texture.
    speaks_to_compositor: bool,
    /// Whether a layer inside it was composited offscreen.
    holds_layer: bool,
    /// Whether any of it filters what is beneath it: a backdrop blur.
    frosts: bool,
}

#[derive(Default)]
struct LayerBuckets {
    backdrop: Vec<FlattenedCommand>,
    shapes: Vec<FlattenedCommand>,
    paths: Vec<FlattenedCommand>,
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
const LAYER_COUNT: usize = 6;

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
            RenderLayer::Paths => &mut self.paths,
            RenderLayer::Images => &mut self.images,
            RenderLayer::Text => &mut self.text,
            RenderLayer::Overlay => &mut self.overlay,
        }
    }

    /// Every bucket, in draw order, for the one place that walks all six
    /// without naming them.
    fn buckets_mut(&mut self) -> [&mut Vec<FlattenedCommand>; LAYER_COUNT] {
        [
            &mut self.backdrop,
            &mut self.shapes,
            &mut self.paths,
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
            paths,
            images,
            text,
            overlay,
            high_water,
            bounds,
        } = self;
        for bucket in [backdrop, shapes, paths, images, text, overlay] {
            bucket.clear();
        }
        *high_water = RenderLayer::default();
        for bounds in bounds {
            bounds.clear();
        }
    }

    /// Move this group's commands into `out` in draw order, each bucket by
    /// `take`, and its ranges into `layers` — unless it ended up empty, which
    /// would cost a pointless set of pipeline switches.
    ///
    /// The buckets are emptied rather than consumed, keeping what they held
    /// for the next frame to fill.
    fn drain_into(
        &mut self,
        out: &mut Vec<FlattenedCommand>,
        layers: &mut Vec<CommandLayer>,
        mut take: impl FnMut(&mut Vec<FlattenedCommand>, &mut Vec<FlattenedCommand>),
    ) {
        let mut layer = CommandLayer::default();
        // Sized by the bucket count, so a bucket added without its range here
        // is a compile error rather than a bucket silently left undrained.
        let ranges: [&mut Range<usize>; LAYER_COUNT] = [
            &mut layer.backdrop,
            &mut layer.shapes,
            &mut layer.paths,
            &mut layer.images,
            &mut layer.text,
            &mut layer.overlay,
        ];
        for (bucket, range) in self.buckets_mut().into_iter().zip(ranges) {
            let start = out.len();
            take(bucket, out);
            *range = start..out.len();
        }
        if !layer.is_empty() {
            layers.push(layer);
        }
    }

    /// How many commands this group's buckets can hold before they allocate.
    fn command_capacity(&self) -> usize {
        self.backdrop.capacity()
            + self.shapes.capacity()
            + self.paths.capacity()
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
            passes: Vec::new(),
            layer_commands: Vec::new(),
            layer_groups: Vec::new(),
            open: Vec::new(),
            opened: 0,
            elided: Vec::new(),
            scale: 1.0,
            surface: None,
            root: 0,
            kept: Vec::new(),
            composites: 0,
            cut_by_surface: 0,
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
    /// Links the chains those clips hang from can hold.
    pub clip_links: usize,
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
            clip_links: self.clips.link_capacity(),
        }
    }

    /// The chains the clips of the frame just flattened hang from, which the
    /// [`ResolvedClip`](super::clip::ResolvedClip) each command's clip resolves
    /// to points into. Good until the next frame is flattened through this
    /// scratch, and no longer.
    pub fn clip_chains(&self) -> &ClipChains {
        self.clips.chains()
    }

    /// The layers the frame just flattened draws offscreen, a layer inside
    /// another first. Good for as long as [`clip_chains`](Self::clip_chains)
    /// is.
    pub fn passes(&self) -> &[LayerPass] {
        &self.passes
    }

    /// The frame just flattened through this scratch into `commands` and
    /// `layers`, as the renderer takes it.
    pub fn frame<'a>(
        &'a self,
        commands: &'a [FlattenedCommand],
        layers: &'a [CommandLayer],
    ) -> FlatFrame<'a> {
        FlatFrame {
            commands,
            layers,
            clips: self.clip_chains(),
            passes: self.passes(),
            kept: &self.kept,
            surface: self.root,
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
            passes,
            layer_commands,
            layer_groups,
            open,
            opened,
            elided,
            scale: _,
            surface: _,
            root: _,
            kept,
            composites,
            cut_by_surface,
        } = self;
        for group in &mut groups[..*used] {
            group.clear();
        }
        *used = 1;
        clips.clear();
        *carried = RegionsCarried::default();
        passes.clear();
        layer_commands.clear();
        layer_groups.clear();
        open.clear();
        *opened = 0;
        elided.clear();
        kept.clear();
        *composites = 0;
        *cut_by_surface = 0;
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
        let rect = world_bounds(&cmd, self.scale);
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
                last.paths.len(),
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

    /// Remove the commands at `taken`, by address, from what was added after
    /// `mark`.
    ///
    /// Their bounds stay recorded in the group they landed in: a group that
    /// split for one of them stays split, which can only cost a draw call.
    fn take_out_since(&mut self, mark: Mark, taken: &[*const DrawCommand]) {
        let groups = &mut self.groups[mark.groups - 1..self.used];
        for (index, group) in groups.iter_mut().enumerate() {
            for (bucket_index, bucket) in group.buckets_mut().into_iter().enumerate() {
                let from = if index == 0 {
                    mark.buckets[bucket_index]
                } else {
                    0
                };
                // One pass, keeping order: what stays is moved down over what
                // goes, and the tail cut off.
                let mut kept = from;
                for at in from..bucket.len() {
                    if !taken.contains(&Rc::as_ptr(&bucket[at].command)) {
                        bucket.swap(kept, at);
                        kept += 1;
                    }
                }
                bucket.truncate(kept);
            }
        }
    }

    /// Whether `id` was composited as a layer after `mark` — drawn this frame
    /// or laid down again from a replay, both of which leave its
    /// [`DrawCommand::Layer`] in the frame.
    fn composited_since(&self, mark: Mark, id: NodeId) -> bool {
        self.tails_since(mark)
            .flatten()
            .any(|cmd| matches!(&*cmd.command, DrawCommand::Layer { id: at, .. } if *at == id))
    }

    /// Put `with` in place of the command at `at`, by address, among what was
    /// added after `mark`.
    fn replace_since(&mut self, mark: Mark, at: *const DrawCommand, with: &Rc<DrawCommand>) {
        let groups = &mut self.groups[mark.groups - 1..self.used];
        for (index, group) in groups.iter_mut().enumerate() {
            for (bucket_index, bucket) in group.buckets_mut().into_iter().enumerate() {
                let from = if index == 0 {
                    mark.buckets[bucket_index]
                } else {
                    0
                };
                for cmd in &mut bucket[from..] {
                    if Rc::as_ptr(&cmd.command) == at {
                        cmd.command = Rc::clone(with);
                    }
                }
            }
        }
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
                    &group.paths,
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
    /// The buckets are emptied rather than consumed, and the next frame
    /// fills what they held.
    fn drain_into(&mut self, out: &mut Vec<FlattenedCommand>, layers: &mut Vec<CommandLayer>) {
        // The layers' content first, at the positions their passes recorded.
        out.append(&mut self.layer_commands);
        layers.append(&mut self.layer_groups);
        for group in self.live_mut() {
            // `append` takes the commands whole, and leaves the buffer.
            group.drain_into(out, layers, |bucket, out| out.append(bucket));
        }
    }
}

/// A layer's texture, once its content says how large it is.
struct LayerTexture {
    id: NodeId,
    /// The texture's top-left texel, in physical pixels of the space the
    /// content was flattened in.
    origin: (f32, f32),
    size: (u32, u32),
    /// The content's space — the layer node's own, nudged onto the device
    /// grid — to the one it is composited into.
    to_world: Transform,
    /// What the composite is drawn by: the layer node's own world transform.
    world_transform: Transform,
    /// The texture's extent in the layer node's own space.
    frame: Rect,
    blur: f32,
    opacity: f32,
}

/// One texel of transparent edge round a layer's content, so the clamped
/// sampler that reads past it — a blur's taps, a turned composite's edge —
/// reads nothing rather than smearing the outermost colour outwards.
const LAYER_EDGE: f32 = 1.0;

/// The widest a layer's texture is allowed to be, in texels. A layer is cut to
/// what of it can reach the surface, so this is only ever met by a surface
/// larger than any screen, or a layer turned so far that the box round the
/// surface in its space is.
const MAX_LAYER_TEXELS: f32 = 8192.0;

impl FlattenScratch {
    /// What of the space being flattened in can reach the surface, if that is
    /// known: the surface itself, or a layer's view of it.
    fn visible(&self) -> Option<Rect> {
        match self.open.last() {
            Some(layer) => layer.visible,
            None => self.surface,
        }
    }

    /// Begin a layer's content: a group of its own, and clips numbered as its.
    fn open_layer(&mut self, visible: Option<Rect>) {
        self.opened += 1;
        let segment = self.opened;
        let layer = OpenLayer {
            base: self.used,
            clip_mark: self.clips.mark(),
            link_mark: self.clips.link_mark(),
            segment,
            outer_segment: self.clips.enter_segment(segment),
            visible,
            holds_layer: false,
            first_pass: self.passes.len(),
        };
        self.open_group(RenderLayer::default());
        self.open.push(layer);
    }

    /// The innermost layer being flattened — the one the steps below close.
    /// Read off the stack rather than handed around, because a layer inside it
    /// writes `holds_layer` there.
    fn innermost(&self) -> OpenLayer {
        *self.open.last().expect("a layer is open")
    }

    /// Everything the layer's content pushed, in draw order.
    fn layer_tails(&self, layer: &OpenLayer) -> impl Iterator<Item = &[FlattenedCommand]> {
        self.tails_since(Mark {
            groups: layer.base + 1,
            buckets: [0; LAYER_COUNT],
        })
    }

    /// Read what the layer's content turned out to be. Asked while it is still
    /// the innermost one open.
    fn scan_layer(&self) -> LayerScan {
        let layer = self.innermost();
        let mut drawn = LayerBounds::default();
        let mut scan = LayerScan {
            bounds: None,
            overlaps: false,
            speaks_to_compositor: false,
            holds_layer: layer.holds_layer,
            frosts: false,
        };
        for cmd in self.layer_tails(&layer).flatten() {
            if speaks_to_compositor(cmd) {
                scan.speaks_to_compositor = true;
            }
            if cmd.layer == RenderLayer::Backdrop {
                scan.frosts = true;
            }
            // A stroke drawn over its own fill is two draws on the same pixels.
            if let DrawCommand::Path {
                fill: Some(_),
                stroke: Some(_),
                ..
            } = &*cmd.command
            {
                scan.overlaps = true;
            }
            let Some(rect) = extent(cmd, self.scale) else {
                continue;
            };
            scan.overlaps |= drawn.intersects(rect);
            drawn.record(Some(rect));
            // A shape's antialiased edge is drawn past the box its extent
            // says, and the texture holds that fringe too, inside the empty
            // texel kept round it. Two shapes whose fringes alone meet do not
            // overlap: on the pixel grid the fringes cover nothing, and off it
            // they share the pixel the two edges fall in.
            let drawn_to = rect.outset(antialiased_past(cmd) / self.scale);
            scan.bounds = Some(
                scan.bounds
                    .map_or(drawn_to, |held| union_of(held, drawn_to)),
            );
        }
        scan
    }

    /// The layer needs no texture: put its content back where the layer
    /// stands, faded draw by draw, as though it had been flattened there.
    ///
    /// It was flattened in the layer's own space and under none of the clips
    /// around it, so it is carried out of that space by `to_world` and cut by
    /// `parent_clip` on the way — the replay a cached subtree takes, with a
    /// transform for an offset.
    fn elide_layer(
        &mut self,
        to_world: &Transform,
        opacity: f32,
        parent_clip: Option<ClipIndex>,
        inherited: Option<ClipRef>,
    ) {
        let layer = self.innermost();
        let mut content = std::mem::take(&mut self.elided);
        content.clear();
        for tail in self.layer_tails(&layer) {
            content.extend_from_slice(tail);
        }
        let clips = self.clips.since(layer.clip_mark);
        self.close_layer_groups(&layer);
        // Nothing outside the layer was cut against its clips — the first of
        // them was placed under none — so they can go, and be placed again.
        self.clips.truncate(layer.clip_mark, layer.link_mark);
        self.clips.enter_segment(layer.outer_segment);
        self.clips.replay(&clips, to_world, parent_clip);
        for mut cmd in content.drain(..) {
            cmd.world_transform = to_world.then(&cmd.world_transform);
            cmd.opacity *= opacity;
            if cmd.clip.is_none() {
                cmd.clip = inherited.clone();
            }
            self.push(cmd);
        }
        self.elided = content;
    }

    /// The layer is drawn offscreen: move its content into its texture's space
    /// and out of the surface's groups, and composite it where it stands.
    fn finish_layer(
        &mut self,
        texture: LayerTexture,
        scan: &LayerScan,
        parent_clip: Option<ClipIndex>,
        inherited: Option<ClipRef>,
    ) {
        let layer = self.innermost();
        self.clips.enter_segment(layer.outer_segment);

        // What the content says to the compositor is said where the layer
        // stands on the surface, cut by the clips around it too — a texture
        // has no input region and no window behind it.
        let mut speaking = Vec::new();
        let mut twins = Vec::new();
        if scan.speaks_to_compositor {
            speaking.extend(
                self.layer_tails(&layer)
                    .flatten()
                    .filter(|cmd| speaks_to_compositor(cmd))
                    .cloned(),
            );
            twins = self.clips.twin_segment(
                layer.segment,
                layer.clip_mark,
                &texture.to_world,
                parent_clip,
            );
        }

        let shift = (
            -texture.origin.0 / self.scale,
            -texture.origin.1 / self.scale,
        );
        self.clips
            .translate_segment(layer.segment, layer.clip_mark, layer.link_mark, shift);

        let commands_start = self.layer_commands.len();
        let groups_start = self.layer_groups.len();
        for group in &mut self.groups[layer.base..self.used] {
            group.drain_into(
                &mut self.layer_commands,
                &mut self.layer_groups,
                |bucket, out| {
                    out.extend(bucket.drain(..).filter_map(|mut cmd| {
                        cmd.command = drawn_in_texture(&cmd)?;
                        cmd.world_transform = cmd.world_transform.translated(shift.0, shift.1);
                        Some(cmd)
                    }));
                },
            );
        }
        self.close_layer_groups(&layer);
        self.passes.push(LayerPass {
            id: texture.id,
            commands: commands_start..self.layer_commands.len(),
            groups: groups_start..self.layer_groups.len(),
            size: texture.size,
            blur: texture.blur,
            inner: layer.first_pass..self.passes.len(),
        });
        if let Some(outer) = self.open.last_mut() {
            outer.holds_layer = true;
        }
        self.composites += 1;

        self.push(FlattenedCommand {
            command: Rc::new(DrawCommand::Layer {
                id: texture.id,
                frame: texture.frame,
                blur: texture.blur,
            }),
            world_transform: texture.world_transform,
            layer: RenderLayer::Images,
            opacity: texture.opacity,
            clip: inherited.clone(),
        });
        for cmd in speaking {
            let Some(said) = said_to_compositor(&cmd) else {
                continue;
            };
            let clip = match &cmd.clip {
                Some(clip) => twins
                    .iter()
                    .find(|(original, _)| original.is(clip))
                    .map(|(_, twin)| twin.clone())
                    .or_else(|| inherited.clone()),
                None => inherited.clone(),
            };
            self.push(FlattenedCommand {
                command: said,
                world_transform: texture.to_world.then(&cmd.world_transform),
                layer: RenderLayer::Shapes,
                opacity: cmd.opacity * texture.opacity,
                clip,
            });
        }
    }

    /// Empty the layer's groups and hand the group before them back as the
    /// current one, which is where the composite lands.
    fn close_layer_groups(&mut self, layer: &OpenLayer) {
        for group in &mut self.groups[layer.base..self.used] {
            group.clear();
        }
        self.used = layer.base;
        self.open.pop();
    }
}

/// Whether a command tells the compositor something: an input region, or a
/// blur the compositor is asked to run.
fn speaks_to_compositor(cmd: &FlattenedCommand) -> bool {
    match &*cmd.command {
        DrawCommand::InputRegion { .. } => true,
        DrawCommand::BackdropBlur { sources, .. } => sources.contains(BackdropSources::COMPOSITOR),
        _ => false,
    }
}

/// What of a command a layer's texture draws: all of it, the surface's half of
/// a blur that asks for both, or nothing.
fn drawn_in_texture(cmd: &FlattenedCommand) -> Option<Rc<DrawCommand>> {
    match &*cmd.command {
        DrawCommand::InputRegion { .. } => None,
        DrawCommand::BackdropBlur { sources, .. }
            if sources.contains(BackdropSources::COMPOSITOR) =>
        {
            sources
                .contains(BackdropSources::SURFACE)
                .then(|| with_sources(&cmd.command, BackdropSources::SURFACE))
        }
        _ => Some(Rc::clone(&cmd.command)),
    }
}

/// The half of a command the compositor is told about — the inverse of
/// [`drawn_in_texture`].
fn said_to_compositor(cmd: &FlattenedCommand) -> Option<Rc<DrawCommand>> {
    match &*cmd.command {
        DrawCommand::InputRegion { .. } => Some(Rc::clone(&cmd.command)),
        DrawCommand::BackdropBlur { sources, .. }
            if sources.contains(BackdropSources::COMPOSITOR) =>
        {
            Some(with_sources(&cmd.command, BackdropSources::COMPOSITOR))
        }
        _ => None,
    }
}

/// A backdrop blur asking for `sources` and nothing else.
fn with_sources(command: &DrawCommand, only: BackdropSources) -> Rc<DrawCommand> {
    let mut command = command.clone();
    if let DrawCommand::BackdropBlur { sources, .. } = &mut command {
        *sources = only;
    }
    Rc::new(command)
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
/// Drawn in field order: shapes, paths, images, text, overlay.
#[derive(Debug, Clone, Default)]
pub struct CommandLayer {
    /// Backdrop effects applied to the target before this group draws.
    pub backdrop: Range<usize>,
    pub shapes: Range<usize>,
    pub paths: Range<usize>,
    pub images: Range<usize>,
    pub text: Range<usize>,
    pub overlay: Range<usize>,
}

impl CommandLayer {
    pub fn is_empty(&self) -> bool {
        self.backdrop.is_empty()
            && self.shapes.is_empty()
            && self.paths.is_empty()
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
/// `scale` is the surface's, in physical pixels per logical one: a subtree
/// composited as a layer is drawn into a texture at that density. The root's
/// `bounds` are taken as the surface, which a layer's texture is cut to; a
/// root with none cuts nothing.
///
/// Returns what the frame carries that becomes a `wl_region` — a backdrop blur
/// the *compositor* is asked to apply, and a declaration about where input
/// reaches the surface — which is the only reason to walk the result again and
/// build one from it.
pub fn flatten_root_into(
    root: &RenderNode,
    scale: f32,
    commands: &mut Vec<FlattenedCommand>,
    layers: &mut Vec<CommandLayer>,
    scratch: &mut FlattenScratch,
) -> RegionsCarried {
    commands.clear();
    layers.clear();
    scratch.clear();
    scratch.scale = scale;
    scratch.root = root.id;
    scratch.surface = (root.bounds.width > 0.0 && root.bounds.height > 0.0).then_some(root.bounds);

    // Not replayable: the root is cleared before every paint (see `replayable`).
    flatten_node(root, Transform::IDENTITY, None, 1.0, false, scratch);

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

/// A node's clip placed under `under`, and the reference its commands name —
/// or, where it declares none, what it inherits unchanged.
fn place_clip(
    out: &mut FlattenScratch,
    clip: Option<&ClipRegion>,
    world_transform: Transform,
    under: Option<ClipIndex>,
    inherited: &Option<ClipRef>,
) -> (Option<ClipIndex>, Option<ClipRef>) {
    match clip {
        Some(clip) => {
            let own = PlacedShape::from_clip(clip, world_transform);
            let index = out.clips.place(own, under);
            (Some(index), Some(out.clips.reference(index)))
        }
        None => (under, inherited.clone()),
    }
}

/// Recursively flatten a node and its children.
///
/// For nodes with `repainted == false` and a valid `cached_flatten`,
/// reuse the cached commands with a translation offset instead of
/// re-flattening the entire subtree.
///
/// `parent_opacity` is what everything above has multiplied together, and
/// travels down the way `parent_world_transform` does.
///
/// `replayable` says whether a later frame could ever be offered this node's
/// entry. Every node below the root could; the root could not, and so is
/// flattened without one.
///
/// A node that is a layer — see [`flatten_as_layer`] — is kept like any
/// other, its composite among the commands: the entry says it holds a layer,
/// and is replayed only by whole device pixels, or only where it stands if the
/// surface's edge cut the layer.
///
/// Returns whether this subtree's output must not be kept for a later frame:
/// something under it was culled.
fn flatten_node(
    node: &RenderNode,
    parent_world_transform: Transform,
    parent_clip: Option<ClipIndex>,
    parent_opacity: f32,
    replayable: bool,
    out: &mut FlattenScratch,
) -> bool {
    // Compose transforms: parent first, then local about its own pivot
    let world_transform = parent_world_transform.then(&node.placement());
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
        && cached.replays_layers_by((dx, dy), out.scale)
        && let Some(fade) = cached.replay_fade(parent_opacity)
    {
        // The clips this subtree placed for itself move with it; the one it
        // inherits is the one it finds here, wherever that has got to. Placed
        // first, because the commands below read what they name.
        out.clips
            .replay(&cached.clips, &Transform::translate(dx, dy), parent_clip);
        for cmd in &cached.commands {
            let mut replayed = cmd.clone();
            replayed.world_transform = cmd.world_transform.translated(dx, dy);
            replayed.opacity = cmd.opacity * fade;
            // A command that named no clip named the one its subtree inherited,
            // which is this frame's and not the one it was cached under.
            if replayed.clip.is_none() {
                replayed.clip = inherited.clone();
            }
            // A layer's composite, laid down from the texture an earlier frame
            // drew: the renderer is told, since no pass this frame names it.
            if let DrawCommand::Layer { id, .. } = &*replayed.command {
                out.kept.push(*id);
                out.composites += 1;
            }
            out.push(replayed);
        }
        // Replayed where it was made, so whatever the surface cut is cut the
        // same — but an entry made over this one has to say so too.
        if cached.cut_by_surface {
            out.cut_by_surface += 1;
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
    // mark for it, nor for one no frame will ask to replay; whether the rest
    // is worth keeping is decided at the bottom, once the children have said
    // whether any of them was cut short.
    let marks = (replayable && world_transform.is_translation_only()).then(|| {
        (
            out.mark(),
            out.clips.mark(),
            (out.composites, out.cut_by_surface),
        )
    });

    let subtree_partial =
        match flatten_as_layer(node, world_transform, parent_clip, parent_opacity, out) {
            // Kept like any other subtree, the composite with the rest: its
            // content is in a pass of its own, drawn into the texture a replay
            // lays down again — see `CachedFlatten::holds_layers`. Put back
            // where it stands, an elided one is the same.
            Layered::Composited { partial } | Layered::Elided { partial } => partial,
            Layered::No => flatten_contents(
                node,
                world_transform,
                parent_clip,
                inherited.clone(),
                opacity,
                out,
            ),
        };

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
        marks
            .filter(|_| !subtree_partial)
            .map(|(mark, clip_mark, composites)| {
                Rc::new(CachedFlatten {
                    commands: out.commands_since(mark, inherited.as_ref()),
                    clips: out.clips.since(clip_mark),
                    world_transform,
                    opacity: parent_opacity,
                    holds_layers: out.composites > composites.0,
                    cut_by_surface: out.cut_by_surface > composites.1,
                })
            });
    crate::render_stats::record_flatten_full();
    subtree_partial
}

/// Everything a node draws: its clip, its commands, its children and its
/// overlays, at `world_transform` and `opacity`. Returns whether anything
/// under it was cut short — see [`flatten_node`].
fn flatten_contents(
    node: &RenderNode,
    world_transform: Transform,
    parent_clip: Option<ClipIndex>,
    inherited: Option<ClipRef>,
    opacity: f32,
    out: &mut FlattenScratch,
) -> bool {
    // This node's clip, placed under the one it inherits. The intersection is
    // still computed here and once — what changed is where it is written: into
    // the frame's clip tree, which every command below names, instead of into
    // each of those commands.
    let (clip_index, effective_clip) = place_clip(
        out,
        node.clip.as_ref(),
        world_transform,
        parent_clip,
        &inherited,
    );

    // What the children are cut to, under the node's own clip: the node's
    // commands above are not, so a container's border is outside the clip its
    // content stops at (#764).
    let (children_index, children_clip) = place_clip(
        out,
        node.children_clip.as_ref(),
        world_transform,
        clip_index,
        &effective_clip,
    );

    // Add main commands with appropriate layers and clip
    for cmd in &node.commands {
        let layer = match &**cmd {
            DrawCommand::Text { .. } => RenderLayer::Text,
            DrawCommand::Image { .. } | DrawCommand::Layer { .. } => RenderLayer::Images,
            DrawCommand::Path { .. } => RenderLayer::Paths,
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
                if !sources.contains(BackdropSources::SURFACE) =>
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

    // The children's backgrounds, merged into one shape at this node's place
    // in paint order: after what it drew itself, before what they draw.
    let merged = node.merge.as_deref().and_then(|merge| {
        super::merge::gather(node, merge, |why| {
            crate::reactive::diagnostics::merge_member_left_out(node.id, why.reason());
        })
    });
    let merge_mark = merged.as_ref().map(|_| out.mark());
    if let Some(merged) = &merged {
        for (index, shadow) in &merged.shadows {
            let child = &node.children[*index];
            out.push(FlattenedCommand {
                command: Rc::clone(shadow),
                world_transform: world_transform.then(&child.placement()),
                layer: RenderLayer::Shapes,
                opacity: opacity * child.opacity,
                clip: children_clip.clone(),
            });
        }
        out.push(FlattenedCommand {
            command: Rc::clone(&merged.command),
            world_transform,
            layer: RenderLayer::Shapes,
            opacity,
            clip: children_clip.clone(),
        });
    }

    // Recurse to children with effective clip
    let mut subtree_partial = node.partial;
    let mut layers = SmallVec::<[usize; 2]>::new();
    for (index, child) in node.children.iter().enumerate() {
        let taken = merged
            .as_ref()
            .map(|merged| &merged.taken[index])
            .filter(|taken| !taken.is_empty());
        let mark = taken.map(|_| out.mark());
        subtree_partial |= flatten_node(child, world_transform, children_index, opacity, true, out);
        // Taken back out of what the child drew rather than kept from it: the
        // child's own entry is made inside that call and has to hold its
        // background, because the next frame may replay it under a parent
        // that no longer merges it.
        //
        // Unless the child was composited: its background went into its layer,
        // to be blurred or faded with the rest of it, so it is the merged
        // shape that has to let it go.
        if let (Some(taken), Some(mark)) = (taken, mark) {
            if out.composited_since(mark, child.id) {
                layers.push(index);
            } else {
                out.take_out_since(mark, taken);
            }
        }
    }
    if let (Some(merged), Some(mark)) = (&merged, merge_mark)
        && !layers.is_empty()
    {
        crate::reactive::diagnostics::merge_member_left_out(
            node.id,
            super::merge::LeftOut::Layer.reason(),
        );
        let mut gone: SmallVec<[*const DrawCommand; 4]> = merged
            .shadows
            .iter()
            .filter(|(child, _)| layers.contains(child))
            .map(|(_, shadow)| Rc::as_ptr(shadow))
            .collect();
        match merged.without(&layers) {
            Some(rest) => out.replace_since(mark, Rc::as_ptr(&merged.command), &rest),
            None => gone.push(Rc::as_ptr(&merged.command)),
        }
        out.take_out_since(mark, &gone);
    }

    // Add overlay commands (layer = Overlay) with overlay-specific clip.
    //
    // An overlay's own clip, if it declared one — a ripple is cut to the shape
    // it belongs to, which is this node's, so it is placed by this node's
    // transform like any other clip. It used to need a flag of its own saying
    // "test this one in local coordinates"; now every clip says where its
    // coordinates are and the flag has nothing left to distinguish.
    //
    // And it is cut by every clip above it, as everything else this node draws
    // is: a ripple in a scrollbar handle stops at its scroller's rounded
    // corner, not at the handle's pill (#708). It used to be placed under no
    // parent at all, which was the only answer while two clips could only be
    // folded into one by taking the smaller corner — the handle's pill would
    // have lost the scroller's arc. They meet exactly now.
    //
    // Only those drawn after the clip was set, though: one drawn before it — an
    // outline, which stands outside the box a ripple is cut to — is cut by the
    // clips above alone (#767).
    if !node.overlay_commands.is_empty() {
        let overlay_clip = match node.overlay_clip {
            Some(ref clip) => {
                let own = PlacedShape::from_clip(clip, world_transform);
                let index = out.clips.place(own, clip_index);
                Some(out.clips.reference(index))
            }
            None => effective_clip.clone(),
        };
        let (unclipped, clipped) = node.overlay_commands.split_at(node.overlay_clip_from);
        let commands = unclipped
            .iter()
            .map(|cmd| (cmd, &effective_clip))
            .chain(clipped.iter().map(|cmd| (cmd, &overlay_clip)));
        for (cmd, clip) in commands {
            out.push(FlattenedCommand {
                command: Rc::clone(cmd),
                world_transform,
                layer: RenderLayer::Overlay,
                opacity,
                clip: clip.clone(),
            });
        }
    }

    subtree_partial
}

/// What [`flatten_as_layer`] made of a node.
enum Layered {
    /// It does not ask to be a layer, and is flattened as usual.
    No,
    /// It asked and needed no texture: its content is in the frame where it
    /// stands, as though it had been flattened there. `partial` is what
    /// [`flatten_node`] returns.
    Elided { partial: bool },
    /// It is drawn offscreen, and composited where it stands.
    Composited { partial: bool },
}

/// Flatten `node` as a layer, if it asks to be one.
///
/// A node asks when it declares a blur, or fades below one with
/// [`OpacityMode::Auto`]. Its content is flattened in the node's own space —
/// after its own transform, before every one above it — under none of the
/// clips around it and at full opacity. What it turned out to be decides the
/// rest:
///
/// - **A fade whose draws never overlap is no layer.** Fading each draw on its
///   own is then the same picture, so the content is put back where the node
///   stands, faded draw by draw, and the frame is as it would have been.
/// - **Nor is a fade over a backdrop blur.** Inside a layer a frost sees only
///   what the layer drew beneath it, so a frosted panel fading as one picture
///   would frost nothing until the fade ended and then pop back to frosting
///   what is behind it. It fades draw by draw instead, its frost with it. A
///   declared blur still makes a layer, frost or not.
/// - **Anything else is.** The content moves into its texture's space and out
///   of the frame's groups, into a [`LayerPass`], and the node draws one
///   [`DrawCommand::Layer`] in its place — at the node's opacity and every
///   ancestor's, by its world transform, cut by the clips around it.
///
/// The content is drawn on the device's pixel grid where the node stands, the
/// way Chromium rasterizes a layer at its fraction of a pixel: a node that is
/// only moved has its content nudged by that fraction, so the texture's texels
/// land on the surface's pixels and the layer is as sharp as the content drawn
/// without one. A move by whole device pixels changes nothing in the content,
/// and the texture is only laid down again; a move to a new fraction draws it
/// again. A node turned or stretched is resampled whatever is done, so its
/// content is drawn unnudged and a transform of its own only re-composites.
///
/// A layer holding a layer is always one, so that a texture's content is
/// never put back somewhere a frame's inner texture has already been made for.
fn flatten_as_layer(
    node: &RenderNode,
    world_transform: Transform,
    parent_clip: Option<ClipIndex>,
    parent_opacity: f32,
    out: &mut FlattenScratch,
) -> Layered {
    let opacity = parent_opacity * node.opacity;
    let fades = node.opacity_mode == OpacityMode::Auto && node.opacity < 1.0;
    // Invisible either way, and a texture would only be filled to be thrown
    // away. Drawn faded to nothing, as without a layer.
    if !(fades || node.blur.is_some()) || opacity.is_nan() || opacity <= 0.0 {
        return Layered::No;
    }
    let scale = out.scale;
    let nudge = if world_transform.is_translation_only() {
        (
            (world_transform.tx() * scale).rem_euclid(1.0) / scale,
            (world_transform.ty() * scale).rem_euclid(1.0) / scale,
        )
    } else {
        (0.0, 0.0)
    };
    // The content's space — the node's own, nudged — to the one the node is
    // composited into.
    let to_world = world_transform.then(&Transform::translate(-nudge.0, -nudge.1));
    // A node squashed to nothing draws nothing, and there is no space of its
    // own to draw it in.
    let Some(back) = to_world.inverse() else {
        return Layered::No;
    };
    // A blur carries what lies past the surface onto it, so the layer holds
    // that much more of its content than the surface shows.
    let reach = node.blur.map_or(0.0, |radius| blur_margin(radius, scale));
    let visible = out
        .visible()
        .map(|surface| back.map_rect(surface).outset(reach));

    let inherited = parent_clip.map(|index| out.clips.reference(index));
    out.open_layer(visible);
    let partial = flatten_contents(
        node,
        Transform::translate(nudge.0, nudge.1),
        None,
        None,
        1.0,
        out,
    );
    let scan = out.scan_layer();

    let needed = node.blur.is_some() || scan.holds_layer || (scan.overlaps && !scan.frosts);
    let shown = needed.then_some(scan.bounds).flatten().map(|bounds| {
        let shown = match visible {
            Some(visible) => bounds.intersection(&visible),
            None => Some(bounds),
        };
        // Cut by the surface's edge, or off it altogether: what is made of
        // the layer now holds only where it stands.
        if shown != Some(bounds) {
            out.cut_by_surface += 1;
        }
        shown
    });
    let texels = shown.flatten().map(|bounds| {
        let even = |at: f32| at - at.rem_euclid(2.0);
        let x0 = even((bounds.x * scale).floor() - LAYER_EDGE);
        let y0 = even((bounds.y * scale).floor() - LAYER_EDGE);
        let x1 = ((bounds.x + bounds.width) * scale).ceil() + LAYER_EDGE;
        let y1 = ((bounds.y + bounds.height) * scale).ceil() + LAYER_EDGE;
        (
            (x0, y0),
            (
                (x1 - x0).min(MAX_LAYER_TEXELS),
                (y1 - y0).min(MAX_LAYER_TEXELS),
            ),
        )
    });
    let Some((origin, (width, height))) = texels else {
        out.elide_layer(&to_world, opacity, parent_clip, inherited);
        return Layered::Elided { partial };
    };

    let texture = LayerTexture {
        id: node.id,
        origin,
        size: (width as u32, height as u32),
        to_world,
        world_transform,
        frame: Rect::new(
            origin.0 / scale - nudge.0,
            origin.1 / scale - nudge.1,
            width / scale,
            height / scale,
        ),
        blur: node.blur.unwrap_or(0.0).max(0.0),
        opacity,
    };
    out.finish_layer(texture, &scan, parent_clip, inherited);
    Layered::Composited { partial }
}

/// Stand-in for "this could be anywhere", used when a command's extent is not
/// known: it intersects everything, so the group splits.
///
/// Finite on purpose: `Rect::intersects` reads the far edge as `x + width`,
/// and from an infinite origin that is `-∞ + ∞`, a NaN that intersects
/// nothing. Half of `f32::MAX` either way keeps both edges finite, through
/// `union_of` too, and still past any surface.
const EVERYTHING: Rect = Rect {
    x: f32::MIN / 2.0,
    y: f32::MIN / 2.0,
    width: f32::MAX,
    height: f32::MAX,
};

pub(super) fn union_of(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);
    Rect::new(x, y, right - x, bottom - y)
}

/// What a command covers in its own space, before its transform.
///
/// Only used to decide whether two commands can safely swap places, and how
/// large a layer's texture is, so it has to be generous: a box that is too
/// small could let something be drawn underneath what it should cover, or cut
/// a layer short.
fn local_bounds(cmd: &FlattenedCommand, scale: f32) -> Rect {
    match &*cmd.command {
        DrawCommand::RoundedRect {
            rect,
            border,
            shadow,
            ..
        } => spilled(*rect, border.map_or(0.0, |b| b.width), shadow.as_ref()),
        // Its border runs inside the merged outline, which `bounds` already
        // holds.
        DrawCommand::MergedShape {
            bounds, shadows, ..
        } => shadows.as_slice().iter().fold(*bounds, |held, shadow| {
            union_of(held, spilled(*bounds, 0.0, Some(shadow)))
        }),
        DrawCommand::Circle { center, radius, .. } => Rect::new(
            center.0 - radius,
            center.1 - radius,
            radius * 2.0,
            radius * 2.0,
        ),
        DrawCommand::Arc {
            center,
            radius,
            width,
            ..
        } => {
            let reach = super::gpu::arc_reach(*radius, width * 0.5);
            Rect::new(center.x - reach, center.y - reach, reach * 2.0, reach * 2.0)
        }
        DrawCommand::Path { path, stroke, .. } => {
            // The control points hold the curve, and a stroke reaches past
            // them by its half width, its miters and its caps — and the fringe
            // by half a pixel more.
            path.bounds()
                .outset(stroke.as_ref().map_or(0.0, |s| s.reach()) + 1.0)
        }
        DrawCommand::Image { rect, .. } => *rect,
        DrawCommand::BackdropBlur { rect, .. } | DrawCommand::InputRegion { rect, .. } => *rect,
        // A blur spreads the texture past what it covers, by its reach.
        DrawCommand::Layer { frame, blur, .. } => frame.outset(blur_margin(*blur, scale)),
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
    }
}

/// What a command covers, in world space, or `None` when that cannot be
/// bounded cheaply.
fn world_bounds(cmd: &FlattenedCommand, scale: f32) -> Option<Rect> {
    if !cmd.world_transform.is_translation_only() {
        // Rotation and scale would need the transformed corners; rather than
        // pay for that on every command, treat it as unbounded and split.
        return None;
    }
    let local = local_bounds(cmd, scale);
    Some(Rect::new(
        local.x + cmd.world_transform.tx(),
        local.y + cmd.world_transform.ty(),
        local.width,
        local.height,
    ))
}

/// `rect` grown by what spills past it: a shadow well outside, and a border
/// that straddles its edge.
fn spilled(rect: Rect, border: f32, shadow: Option<&crate::renderer::Shadow>) -> Rect {
    let mut grow = border;
    let mut offset: (f32, f32) = (0.0, 0.0);
    // An inset one is drawn inside the rect, and reaches nothing.
    if let Some(shadow) = shadow.filter(|s| !s.inset) {
        grow = grow.max(shadow.spill());
        offset = shadow.offset;
    }
    Rect::new(
        rect.x - grow + offset.0.min(0.0),
        rect.y - grow + offset.1.min(0.0),
        rect.width + grow * 2.0 + offset.0.abs(),
        rect.height + grow * 2.0 + offset.1.abs(),
    )
}

/// How far past its extent a command draws, in device pixels: the fringe the
/// shape shader's antialiasing lays outside a shape, which nothing else draws.
fn antialiased_past(cmd: &FlattenedCommand) -> f32 {
    match &*cmd.command {
        DrawCommand::RoundedRect { .. }
        | DrawCommand::MergedShape { .. }
        | DrawCommand::Circle { .. }
        | DrawCommand::Arc { .. } => super::ANTIALIASING_REACH,
        _ => 0.0,
    }
}

/// The box a command's pixels land in, in the space it was flattened in — the
/// box round its transformed bounds — or `None` for a command that draws no
/// pixels at all.
///
/// What a layer is sized by and asked about overlap with, once per command
/// and once per frame: worth the four corners [`world_bounds`] will not pay
/// for on every push.
fn extent(cmd: &FlattenedCommand, scale: f32) -> Option<Rect> {
    match &*cmd.command {
        DrawCommand::InputRegion { .. } => None,
        DrawCommand::BackdropBlur { sources, .. }
            if !sources.contains(BackdropSources::SURFACE) =>
        {
            None
        }
        _ => Some(cmd.world_transform.map_rect(local_bounds(cmd, scale))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::CornerRadii;
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

    use RenderLayer::{Backdrop, Images, Overlay, Shapes, Text};

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
                1.0,
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
            1.0,
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

    /// A shadow is drawn two blurs and its spread out, and that is where it
    /// can cover a label. A box whose shadow reaches the label only with its
    /// outer ring still has to be drawn after it: kept in the group, it would
    /// be drawn underneath, and the ring would vanish behind the text.
    #[test]
    fn a_shadow_that_reaches_a_label_only_with_its_outer_ring_still_splits() {
        let mut layered = FlattenScratch::default();
        layered.push(command_at(Text, Rect::new(0.0, 0.0, 100.0, 40.0)));
        // 12 below the label: past one blur of 8, inside two.
        let shadow = crate::renderer::Shadow::new((0.0, 0.0), 8.0, 0.0, Color::BLACK);
        layered.push(FlattenedCommand {
            command: Rc::new(DrawCommand::RoundedRect {
                rect: Rect::new(0.0, 52.0, 100.0, 40.0),
                color: Color::WHITE,
                radius: 0.0.into(),
                curvature: 1.0,
                border: None,
                shadow: Some(shadow),
                gradient: None,
                background: false,
                shadow_only: false,
            }),
            ..command(Shapes)
        });

        let mut commands = Vec::new();
        let mut layers = Vec::new();
        layered.drain_into(&mut commands, &mut layers);
        assert_eq!(layers.len(), 2, "the ring lands on the label");
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
    fn an_unbounded_command_already_drawn_covers_what_comes_after() {
        // The other half of the one above: the turned command is drawn first,
        // and what regresses under it later is bounded. An unbounded command
        // covers everything, so the frost has to wait for the bars it frosts
        // — held in the same group, it filtered the target before they were
        // drawn and blurred nothing (#487). Twice: with nothing else under
        // the frost, which only the turned command can split, and with a bar
        // it overlaps, which the turned command's bounds must not hide.
        for bars in [vec![], vec![Rect::new(0.0, 0.0, 100.0, 100.0)]] {
            let mut layered = FlattenScratch::default();
            let mut turned = command_at(Shapes, Rect::new(900.0, 900.0, 10.0, 10.0));
            turned.world_transform = Transform::rotate(12.0);
            layered.push(turned);
            for bar in bars {
                layered.push(command_at(Shapes, bar));
            }
            layered.push(command_at(Backdrop, Rect::new(10.0, 10.0, 50.0, 20.0)));

            let mut commands = Vec::new();
            let mut layers = Vec::new();
            layered.drain_into(&mut commands, &mut layers);
            assert_eq!(layers.len(), 2);
            assert!(layers[0].backdrop.is_empty());
            assert_eq!(layers[1].backdrop.len(), 1);
        }
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
            font_style: Default::default(),
            font_features: Default::default(),
            line_height: Default::default(),
            letter_spacing: 0.0,
            align: Default::default(),
            fit: None,
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
        let carried = flatten_root_into(node, 1.0, &mut commands, &mut layers, scratch);
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
    /// answered a constant, or added its six buckets up with the wrong
    /// operator, would satisfy as happily as a correct one.
    ///
    /// So the buffers are given room by hand, in distinct amounts across two
    /// groups, and the answer is the one arithmetic anybody can check.
    #[test]
    fn the_capacity_reported_is_the_room_the_buffers_hold() {
        let mut scratch = FlattenScratch::default();
        scratch.groups.push(LayerBuckets::default());

        // Distinct primes, so that a sum taken with the wrong operator lands
        // nowhere near this one: 2*3*5*7*11*13 is 30030, and going down from
        // 2 underflows rather than arriving at 41.
        let rooms = [[2, 3, 5, 7, 11, 13], [17, 19, 23, 29, 31, 37]];
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
            scratch.clips.place(clip, None);
        }

        assert_eq!(
            commands, 197,
            "the primes above add up to this and to nothing else, which is \
             what the sum below is being held to"
        );

        let reported = scratch.capacity();
        assert_eq!(
            reported.commands, commands,
            "the commands reported are not the six buckets of both groups \
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
    use crate::renderer::CornerRadii;
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
            1.0,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );

        let overlay = commands
            .iter()
            .find(|c| c.layer == RenderLayer::Overlay)
            .expect("the ripple is in the frame");
        let clip = overlay.clip().expect("and it is clipped").first;

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

    /// An overlay drawn before the overlay clip was set is not cut by it: a
    /// container's outline stands outside the box its ripples are cut to, and
    /// is drawn first so a press does not erase it (#767).
    #[test]
    fn an_overlay_drawn_before_the_overlay_clip_is_not_cut_by_it() {
        let mut node = RenderNode::new(1);
        node.bounds = Rect::new(0.0, 0.0, 80.0, 80.0);
        let ring = Rect::new(-4.0, -4.0, 88.0, 88.0);
        node.overlay_commands
            .push(Rc::new(DrawCommand::rounded_rect(ring, Color::WHITE, 0.0)));
        node.overlay_clip = Some(ClipRegion {
            rect: Rect::new(0.0, 0.0, 80.0, 80.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        });
        node.overlay_clip_from = 1;
        node.overlay_commands
            .push(Rc::new(DrawCommand::rounded_rect(
                Rect::new(10.0, 10.0, 20.0, 20.0),
                Color::RED,
                0.0,
            )));

        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        flatten_root_into(
            &node,
            1.0,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );
        let clip_of = |color: Color| {
            commands
                .iter()
                .find(|c| matches!(&*c.command, DrawCommand::RoundedRect { color: k, .. } if *k == color))
                .unwrap_or_else(|| panic!("{color:?} is drawn"))
                .clip()
                .map(|clip| clip.first.rect)
        };
        assert_eq!(clip_of(Color::WHITE), None, "the outline is left whole");
        assert_eq!(
            clip_of(Color::RED),
            Some(Rect::new(0.0, 0.0, 80.0, 80.0)),
            "the ripple is cut to the box"
        );
    }

    /// A children clip cuts the children and their merged backgrounds, under
    /// the node's own clip, and leaves what the node draws itself to the own
    /// clip alone (#764).
    ///
    /// The two clips are rects that overlap only in part, so a children clip
    /// placed under the own one is their intersection and one placed under
    /// anything else is not.
    #[test]
    fn a_children_clip_cuts_the_children_under_the_nodes_own_clip() {
        let region = |x: f32, y: f32, w: f32, h: f32| ClipRegion {
            rect: Rect::new(x, y, w, h),
            corner_radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
        };
        let fill = |color: Color, background: bool| {
            let mut cmd = DrawCommand::rounded_rect(Rect::new(0.0, 0.0, 100.0, 100.0), color, 0.0);
            if let DrawCommand::RoundedRect { background: b, .. } = &mut cmd {
                *b = background;
            }
            Rc::new(cmd)
        };

        let mut child = RenderNode::new(2);
        child.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        child.commands.push(fill(Color::WHITE, true));
        child.commands.push(fill(Color::BLUE, false));

        let mut root = RenderNode::new(1);
        root.bounds = Rect::new(0.0, 0.0, 100.0, 100.0);
        root.clip = Some(region(0.0, 0.0, 50.0, 100.0));
        root.children_clip = Some(region(10.0, 10.0, 80.0, 80.0));
        root.merge = Some(Box::new(super::super::merge::MergeChildren {
            distance: 0.0,
            border: None,
            shadows: crate::renderer::Shadows::none(),
        }));
        root.commands.push(fill(Color::RED, false));
        root.children.push(Rc::new(child));

        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        flatten_root_into(
            &root,
            1.0,
            &mut commands,
            &mut layers,
            &mut FlattenScratch::default(),
        );
        let cut_to = |color: Color| {
            let cmd = commands
                .iter()
                .find(|c| matches!(&*c.command, DrawCommand::RoundedRect { color: k, .. } if *k == color))
                .unwrap_or_else(|| panic!("{color:?} is drawn"));
            cmd.clip()
                .expect("everything here is clipped")
                .first
                .world_aabb()
        };
        let merged = commands
            .iter()
            .find(|c| !matches!(&*c.command, DrawCommand::RoundedRect { .. }))
            .expect("the merged shape is drawn");

        assert_eq!(cut_to(Color::RED), Rect::new(0.0, 0.0, 50.0, 100.0), "own");
        assert_eq!(
            cut_to(Color::BLUE),
            Rect::new(10.0, 10.0, 40.0, 80.0),
            "child"
        );
        assert_eq!(
            merged
                .clip()
                .expect("the merged shape is clipped")
                .first
                .world_aabb(),
            Rect::new(10.0, 10.0, 40.0, 80.0),
            "merged background"
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
                1.0,
                &mut commands,
                &mut layers,
                &mut FlattenScratch::default(),
            );
            let cmd = commands.first().expect("the child draws");
            let clip = cmd.clip().expect("the child clips").first;
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
                1.0,
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
                .first
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

    /// An overlay is cut by the clip above it as well as by its own, rounded
    /// corners included — flattened in full, and replayed (#708).
    ///
    /// A ripple used to be cut to the shape it belongs to and to nothing
    /// else, so a scrollbar handle's press ripple in a rounded scroller drew
    /// past the scroller's arc. Here the clip above is a rounded rect half the
    /// overlay's height whose four corners are the intersection's, so the
    /// overlay comes out as that rect — merged exactly, with its corners —
    /// whichever path placed it. The replayed half is the one that asks
    /// whether the cache remembers the parent rather than the shape.
    #[test]
    fn an_overlay_is_cut_by_the_clip_above_it() {
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
            corner_radius: CornerRadii::uniform(10.0),
            curvature: 1.0,
        });
        root.children.push(Rc::new(middle));

        let overlay_clip = |node: &RenderNode| {
            let (mut commands, mut layers) = (Vec::new(), Vec::new());
            flatten_root_into(
                node,
                1.0,
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
        };
        let cut_by_both = |clip: ResolvedClip| {
            clip.rest == crate::renderer::NO_CHAIN
                && clip.first.rect == Rect::new(0.0, 0.0, 40.0, 20.0)
                && clip.first.radii == CornerRadii::uniform(10.0)
        };

        let flattened = overlay_clip(&root);
        assert!(
            cut_by_both(flattened),
            "flattened in full, the overlay is cut by the rounded clip above \
             it as well as its own: {flattened:?}"
        );

        // The subtree below the clip is clean and the clip's own node is not,
        // so the middle node is replayed while the clip above it is placed
        // afresh — which is the arrangement that asks the question.
        for c in &root.children {
            c.repainted.set(false);
        }

        let replayed = overlay_clip(&root);
        assert!(
            cut_by_both(replayed),
            "replayed, the overlay is cut by the clip above it just the same: \
             {replayed:?}"
        );
    }

    /// A translation moves the box and changes nothing else.
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
            1.0,
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
    /// Drawn draw by draw — [`OpacityMode::Modulate`] — which is the
    /// arithmetic this is about: the boxes overlap, and under the default the
    /// outer one would be composited as a layer instead.
    #[test]
    fn nested_opacities_multiply() {
        let mut inner = with_box(3, 0.5);
        inner.opacity_mode = OpacityMode::Modulate;
        inner
            .overlay_commands
            .push(Rc::new(DrawCommand::circle((5.0, 5.0), 5.0, Color::WHITE)));
        let mut outer = with_box(2, 0.5);
        outer.opacity_mode = OpacityMode::Modulate;
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

#[cfg(test)]
mod layer_tests {
    use super::*;
    use crate::backdrop::BackdropSources;
    use crate::renderer::CornerRadii;
    use crate::renderer::tree::ClipRegion;
    use crate::widgets::Color;

    const CARD: Color = Color::rgb(0.8, 0.2, 0.2);
    const TILE: Color = Color::rgb(0.2, 0.2, 0.8);

    struct Flat {
        commands: Vec<FlattenedCommand>,
        scratch: FlattenScratch,
    }

    fn flatten(root: &RenderNode) -> Flat {
        let (mut commands, mut layers) = (Vec::new(), Vec::new());
        let mut scratch = FlattenScratch::default();
        flatten_root_into(root, 1.0, &mut commands, &mut layers, &mut scratch);
        Flat { commands, scratch }
    }

    fn rect_node(id: NodeId, rect: Rect, color: Color) -> RenderNode {
        let mut node = RenderNode::with_bounds(id, Rect::new(0.0, 0.0, rect.width, rect.height));
        node.local_transform = Transform::translate(rect.x, rect.y);
        node.commands.push(Rc::new(DrawCommand::rounded_rect(
            Rect::new(0.0, 0.0, rect.width, rect.height),
            color,
            0.0,
        )));
        node
    }

    /// A surface clipped to a rounded box, holding a card at `opacity` and
    /// `mode` whose tile overlaps the card's background — and, when `speaks`,
    /// tells the compositor where input goes and what to blur, under a clip of
    /// the tile's own.
    fn surface(opacity: f32, mode: OpacityMode, tile_at: (f32, f32), speaks: bool) -> RenderNode {
        let mut tile = rect_node(3, Rect::new(tile_at.0, tile_at.1, 40.0, 40.0), TILE);
        if speaks {
            tile.clip = Some(ClipRegion {
                rect: Rect::new(0.0, 0.0, 30.0, 30.0),
                corner_radius: CornerRadii::uniform(6.0),
                curvature: 1.0,
            });
            tile.commands.push(Rc::new(DrawCommand::InputRegion {
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                corner_radii: CornerRadii::uniform(4.0),
                curvature: 1.0,
                takes: false,
            }));
            tile.commands.push(Rc::new(DrawCommand::BackdropBlur {
                rect: Rect::new(0.0, 0.0, 40.0, 40.0),
                sources: BackdropSources::SURFACE | BackdropSources::COMPOSITOR,
                radius: 6.0,
                corner_radii: CornerRadii::uniform(4.0),
                curvature: 1.0,
            }));
        }
        let mut card = rect_node(2, Rect::new(30.0, 20.0, 100.0, 60.0), CARD);
        card.opacity = opacity;
        card.opacity_mode = mode;
        card.children.push(Rc::new(tile));
        let mut root = RenderNode::with_bounds(1, Rect::new(0.0, 0.0, 200.0, 120.0));
        root.clip = Some(ClipRegion {
            rect: Rect::new(10.0, 10.0, 120.0, 80.0),
            corner_radius: CornerRadii::uniform(16.0),
            curvature: 1.0,
        });
        root.children.push(Rc::new(card));
        root
    }

    /// The content of the frame's first layer, and its composite.
    fn layer(flat: &Flat) -> (&[FlattenedCommand], &FlattenedCommand) {
        let pass = &flat.scratch.passes()[0];
        let composite = flat
            .commands
            .iter()
            .find(|cmd| matches!(&*cmd.command, DrawCommand::Layer { .. }))
            .expect("composited");
        (&flat.commands[pass.commands.clone()], composite)
    }

    /// A fade over a tile that overlaps its card is a layer: the content is
    /// drawn at full strength in the texture's space, and the fade and the
    /// clip around the card are the composite's.
    #[test]
    fn an_overlap_under_a_fade_is_composited_with_the_clips_around_it() {
        let flat = flatten(&surface(0.5, OpacityMode::Auto, (10.0, 10.0), false));
        assert_eq!(flat.scratch.passes().len(), 1);
        let (content, composite) = layer(&flat);

        assert_eq!(content.len(), 2, "the card and the tile");
        assert!(content.iter().all(|cmd| cmd.opacity == 1.0));
        assert!(
            content.iter().all(|cmd| cmd.clip.is_none()),
            "nothing above the layer cuts what is drawn into it"
        );
        assert_eq!(composite.opacity, 0.5);
        assert_eq!(
            composite
                .clip()
                .expect("cut by the surface's clip")
                .first
                .rect,
            Rect::new(10.0, 10.0, 120.0, 80.0)
        );
        let DrawCommand::Layer { frame, .. } = &*composite.command else {
            unreachable!()
        };
        // The texture covers the card and the texel its antialiased edge is
        // drawn in past it, a texel of empty edge round both, its origin on an
        // even texel.
        assert_eq!(*frame, Rect::new(-2.0, -2.0, 104.0, 64.0));
        assert_eq!(flat.scratch.passes()[0].size, (104, 64));
        assert!(
            content.iter().all(|cmd| cmd.world_transform.tx() >= 2.0),
            "drawn in the texture's space, the card two texels in"
        );
    }

    /// A fade over draws that never touch is put back as modulation: no pass,
    /// and the very commands — transforms, opacities and clips — that
    /// [`OpacityMode::Modulate`] gives.
    #[test]
    fn a_fade_whose_draws_do_not_overlap_is_modulation() {
        let mut auto = surface(0.5, OpacityMode::Auto, (10.0, 10.0), false);
        let mut each = surface(0.5, OpacityMode::Modulate, (10.0, 10.0), false);
        // No background on the card: the tile is all it draws.
        for root in [&mut auto, &mut each] {
            let card = Rc::get_mut(&mut root.children[0]).unwrap();
            card.commands.clear();
        }
        let (auto, each) = (flatten(&auto), flatten(&each));

        assert!(auto.scratch.passes().is_empty(), "no layer");
        assert_eq!(auto.commands.len(), each.commands.len());
        for (a, e) in auto.commands.iter().zip(&each.commands) {
            assert_eq!(a.command, e.command);
            assert_eq!(a.world_transform, e.world_transform);
            assert_eq!(a.opacity, e.opacity);
            assert_eq!(a.clip(), e.clip());
        }
    }

    /// What the content says to the compositor is said where the layer
    /// stands, cut by the clips inside the layer and around it, exactly as
    /// without one — the texture has no input region and nothing behind it.
    /// The texture keeps the surface's half of a blur that asks for both.
    #[test]
    fn a_layer_tells_the_compositor_what_its_content_does() {
        // A frost inside a fade is faded draw by draw rather than made a
        // layer; a declared blur makes one regardless, which is the case where
        // what the content says to the compositor has to leave the texture.
        let mut layered = surface(0.5, OpacityMode::Auto, (10.0, 10.0), true);
        Rc::get_mut(&mut layered.children[0]).unwrap().blur = Some(0.0);
        let auto = flatten(&layered);
        let each = flatten(&surface(0.5, OpacityMode::Modulate, (10.0, 10.0), true));
        assert_eq!(auto.scratch.passes().len(), 1);

        let (content, _) = layer(&auto);
        assert!(
            content.iter().all(|cmd| match &*cmd.command {
                DrawCommand::InputRegion { .. } => false,
                DrawCommand::BackdropBlur { sources, .. } => *sources == BackdropSources::SURFACE,
                _ => true,
            }),
            "the texture draws, and says nothing"
        );
        assert!(
            content
                .iter()
                .any(|cmd| matches!(&*cmd.command, DrawCommand::BackdropBlur { .. })),
            "and still frosts what is beneath it in the layer"
        );

        // Only the surface's commands: the layer's are in its texture's space.
        let surface_of = |flat: &Flat| {
            let start = flat
                .scratch
                .passes()
                .last()
                .map_or(0, |pass| pass.commands.end);
            flat.commands[start..].to_vec()
        };
        let regions = |flat: &Flat| {
            let commands = surface_of(flat);
            (
                crate::region::input_ops_from_commands(&commands, flat.scratch.clip_chains()),
                crate::blur::regions_from_commands(&commands, flat.scratch.clip_chains()),
            )
        };
        let (auto_input, auto_blur) = regions(&auto);
        let (each_input, each_blur) = regions(&each);
        assert!(!each_input.is_empty() && !each_blur.is_empty());
        assert_eq!(auto_input, each_input, "the input region");
        assert_eq!(auto_blur, each_blur, "the compositor's blur");
    }

    /// A path that is both filled and stroked overlaps itself: the stroke is
    /// drawn over the fill, so fading them one after the other shows the fill
    /// through the stroke. A fade over it alone is a layer.
    #[test]
    fn a_path_filled_and_stroked_overlaps_itself() {
        use crate::renderer::{Fill, Path, Stroke};
        let path = Path::new()
            .move_to((0.0, 0.0))
            .line_to((40.0, 0.0))
            .line_to((20.0, 30.0))
            .close();
        let mut node = RenderNode::with_bounds(2, Rect::new(0.0, 0.0, 40.0, 30.0));
        node.opacity = 0.5;
        node.commands.push(Rc::new(DrawCommand::Path {
            path,
            fill: Some(Fill::color(CARD)),
            stroke: Some(Stroke::new(4.0, TILE)),
        }));
        let mut root = RenderNode::with_bounds(1, Rect::new(0.0, 0.0, 100.0, 100.0));
        root.children.push(Rc::new(node));
        assert_eq!(flatten(&root).scratch.passes().len(), 1);
    }

    /// A layer inside a layer is drawn first, and the outer one's content
    /// holds its composite.
    #[test]
    fn a_layer_inside_a_layer_is_drawn_first() {
        let mut root = surface(0.5, OpacityMode::Auto, (10.0, 10.0), false);
        let card = Rc::get_mut(&mut root.children[0]).unwrap();
        let tile = Rc::get_mut(&mut card.children[0]).unwrap();
        tile.blur = Some(3.0);
        let flat = flatten(&root);

        let passes = flat.scratch.passes();
        assert_eq!(
            passes.iter().map(|pass| pass.id).collect::<Vec<_>>(),
            [3, 2],
            "the tile, then the card"
        );
        assert!(
            flat.commands[passes[1].commands.clone()]
                .iter()
                .any(|cmd| matches!(&*cmd.command, DrawCommand::Layer { id: 3, blur, .. } if *blur == 3.0)),
            "the card's texture lays the tile's down"
        );
        assert_eq!(
            passes[0].groups.end, passes[1].groups.start,
            "and their groups in that order"
        );
    }
}
