//! Clips as things a command refers to, rather than shapes written into it.
//!
//! **The render tree already holds a clip in the right place.** It sits on the
//! node that declared it and stays there while its descendants move. Flatten
//! used to destroy that on the way down: it intersected the chain into one
//! world-space [`PlacedShape`] and copied the result into every command below,
//! after which the only thing a cached subtree could do with its clip was
//! shift it along with the content — which is a viewport that follows the rows
//! it is supposed to be cutting.
//!
//! So a command names its clip and the renderer asks where it is. The names
//! are [`ClipRef`]s, the frame's clips live in a [`ClipTree`] that records what
//! each one cuts and whose child it is, and resolution happens at the half
//! dozen places `render.rs` and `region.rs` read `cmd.clip()`.
//!
//! **What a clip resolves to is exact.** Two clips are merged into one rounded
//! rect only where that rect is their intersection ([`merge_exact`]); a clip
//! that cannot be merged keeps its own shape and the clips above it hang from
//! it as a chain ([`ClipChains`]), which every pipeline walks — the shape and
//! quad shaders and the backdrop composite per fragment, glyphon through the
//! box all of them leave open, and the region tessellator a scanline at a time
//! (#698).
//!
//! Chromium's clip tree is this, grown up: four property trees, clips among
//! them, and draw commands referencing nodes in them rather than carrying
//! copies. Flutter reaches the same place from the other side, with the clip in
//! a layer of its own from the content that scrolls under it.

use std::cell::Cell;
use std::rc::Rc;

use crate::shape::PlacedShape;
use crate::transform::Transform;
use crate::widgets::Rect;

/// The end of a chain: a clip with nothing above it left to test.
pub const NO_CHAIN: u32 = u32::MAX;

/// The most links a shader walks down one chain — `MAX_CHAIN` in the shared
/// clip-chain block, which stops there so a broken chain cannot hang the GPU.
/// A chain deeper than this is cut by its first `MAX_CHAIN` links only, and
/// the tree says so when it builds one.
pub(crate) const MAX_CHAIN: u32 = 64;

/// What a clip cuts, once every clip above it has had its say.
///
/// **Exactly, not one shape that approximates the lot.** Two clips merge into
/// one rounded rect only where that rect *is* their intersection — see
/// `merge_exact`. Everywhere else the clip keeps its own shape and the ones
/// above it are kept as a chain: `rest` names the first of them in the frame's
/// [`ClipChains`], and a pixel is inside when it is inside `first` and every
/// shape down the chain. Skia's `ClipStack` makes the same split, merging two
/// rrects only when `SkRRectPriv::ConservativeIntersect` says the result is
/// exact and keeping separate elements otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedClip {
    /// The innermost shape, or the exact merge of it with what it was under.
    pub first: PlacedShape,
    /// The rest of the chain, as an index into this frame's [`ClipChains`]:
    /// [`NO_CHAIN`] where `first` is the whole of it, which is most clips.
    pub rest: u32,
    /// The world box every shape of the chain leaves open: their boxes,
    /// intersected, and empty where they miss. What a consumer that can take
    /// only a box is given — glyphon's bounds, the backdrop's viewport —
    /// worked out once, where the chain is built, rather than by walking it.
    pub open: Rect,
}

impl ResolvedClip {
    /// A clip that answers to nothing above it.
    pub(crate) fn alone(first: PlacedShape) -> Self {
        Self {
            first,
            rest: NO_CHAIN,
            open: first.world_aabb(),
        }
    }
}

/// One link of a chain: a shape, and the link after it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClipLink {
    pub(crate) shape: PlacedShape,
    /// The next link, or [`NO_CHAIN`].
    pub(crate) next: u32,
    /// How many links a walk from this one takes, this one included.
    depth: u32,
}

/// The chains one frame's clips hang from, as one persistent cons-list.
///
/// A link is pushed when a clip cannot be merged with the one above it and is
/// never changed afterwards, so two clips under one parent share the parent's
/// tail rather than copying it. Rebuilt every frame, as the clips are: the
/// index a [`ResolvedClip`] carries means something in the frame that placed
/// it and in no other.
#[derive(Debug, Default)]
pub struct ClipChains {
    links: Vec<ClipLink>,
}

impl ClipChains {
    /// Every link this frame pushed, in index order — what the renderer
    /// uploads for its shaders to walk.
    pub(crate) fn links(&self) -> &[ClipLink] {
        &self.links
    }

    /// Every shape `clip` is cut by: its first, then down its chain.
    pub(crate) fn shapes<'a>(
        &'a self,
        clip: &'a ResolvedClip,
    ) -> impl Iterator<Item = &'a PlacedShape> + 'a {
        std::iter::once(&clip.first).chain(self.chain(clip.rest))
    }

    /// The shapes down the chain starting at `rest`, nearest first.
    fn chain(&self, rest: u32) -> impl Iterator<Item = &PlacedShape> + '_ {
        let mut at = rest;
        std::iter::from_fn(move || {
            let link = self.links.get(at as usize)?;
            at = link.next;
            Some(&link.shape)
        })
    }

    fn push(&mut self, shape: PlacedShape, next: u32) -> u32 {
        let index = self.links.len() as u32;
        let depth = 1 + self.depth(next);
        if depth == MAX_CHAIN + 1 {
            log::warn!(
                "clips nested {depth} deep without merging: the shaders walk {MAX_CHAIN} \
                 of them, and the outermost are not cut by"
            );
        }
        self.links.push(ClipLink { shape, next, depth });
        index
    }

    /// How many links a walk from `rest` takes.
    pub(crate) fn depth(&self, rest: u32) -> u32 {
        self.links.get(rest as usize).map_or(0, |link| link.depth)
    }
}

/// The clip a draw command is cut to.
///
/// A name rather than a copy. Several commands share one, and a frame that
/// replays a cached subtree under a clip that did not move hands those commands
/// back unchanged — the clip is not theirs to carry.
///
/// The shape behind the name is written once per frame, by whichever of the two
/// flatten paths placed the clip, and read afterwards by everything that draws.
/// Both happen inside one frame and in that order, which is what makes the
/// interior mutability safe to look at: nothing reads a clip while flatten is
/// still deciding where it goes.
///
/// **"Once" is an invariant, not an observation.** One reference is shared by
/// the live command list, by the entry that placed the clip and by every
/// ancestor entry that copied it, so a second placement in one frame would
/// silently decide the clip for all of them. It holds because flatten meets
/// each node once: a replay short-circuits everything below it, and entries
/// nest rather than overlap. A render tree that reached the same `Rc<RenderNode>`
/// from two places would break it, and nothing here would say so.
///
/// It is also what keeps a chain index honest: every reference a frame's
/// commands name was placed by that frame, so the `rest` it holds points into
/// that frame's [`ClipChains`] and not the last one's.
#[derive(Debug, Clone)]
pub struct ClipRef(Rc<Cell<ResolvedClip>>);

impl ClipRef {
    /// What this clip cuts, this frame: its own shape, or its exact merge with
    /// the ones above it, and the chain of those it could not be merged with.
    pub fn resolved(&self) -> ResolvedClip {
        self.0.get()
    }

    /// A clip standing where it was put, named by nobody else yet.
    #[cfg(test)]
    pub(crate) fn placed(shape: PlacedShape) -> Self {
        Self::holding(ResolvedClip::alone(shape))
    }

    fn holding(resolved: ResolvedClip) -> Self {
        Self(Rc::new(Cell::new(resolved)))
    }

    fn place(&self, resolved: ResolvedClip) {
        self.0.set(resolved);
    }

    /// Whether two references name the same clip, rather than two clips that
    /// happen to cut alike.
    pub(crate) fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Where a clip sits in the frame's [`ClipTree`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipIndex(usize);

/// One clip of a frame: what it cuts on its own, and what it is under.
struct Placed {
    reference: ClipRef,
    /// The clip's own shape, before its ancestors cut it — which is what a
    /// replay translates, and what it is re-intersected from.
    own: PlacedShape,
    /// The clip in effect where this one was placed, if any. A replay asks
    /// again rather than trusting it: a clip that inherits nothing *today*
    /// still inherits once its subtree is put back under an
    /// `Overflow::Hidden` that has since been switched on.
    under: Option<ClipIndex>,
    /// The link this clip's resolved shape was pushed as for the children that
    /// chain to it, or [`NO_CHAIN`] until one does.
    link: u32,
}

/// The clips one frame placed, in the order flatten met them.
///
/// A tree by the parent links, a vector by storage: a clip's parent is always
/// placed before it, so a subtree's clips are a contiguous run and the entry
/// kept for a cached subtree is a slice of one.
#[derive(Default)]
pub(crate) struct ClipTree {
    placed: Vec<Placed>,
    /// The chains the placed clips hang from.
    chains: ClipChains,
}

impl ClipTree {
    /// How many clips have been placed, which is where the next subtree's own
    /// clips will start.
    pub(crate) fn mark(&self) -> usize {
        self.placed.len()
    }

    /// Forget the clips of the frame that has been drawn, keeping the room
    /// they took for the frame that follows it.
    ///
    /// The references go with them: a clip is a name for one frame only, and
    /// the entries a cached subtree keeps hold the `Rc`s they still need.
    pub(crate) fn clear(&mut self) {
        // Destructured, as the flattener's own resets are: a field added to
        // this tree has to be answered for here.
        let Self { placed, chains } = self;
        placed.clear();
        chains.links.clear();
    }

    /// How many clips this tree can hold before it allocates again.
    pub(crate) fn capacity(&self) -> usize {
        self.placed.capacity()
    }

    /// How many chain links it can hold before it allocates again.
    pub(crate) fn link_capacity(&self) -> usize {
        self.chains.links.capacity()
    }

    /// The chains this frame's clips hang from.
    pub(crate) fn chains(&self) -> &ClipChains {
        &self.chains
    }

    /// The name of `index`, for a command to carry.
    pub(crate) fn reference(&self, index: ClipIndex) -> ClipRef {
        self.placed[index.0].reference.clone()
    }

    /// What `own` cuts once the clip above it has had its say.
    ///
    /// The one place a clip meets its parent, which is why both ways of
    /// placing one go through it: the tree's own invariant is that every
    /// reference in it holds the shape this returns.
    ///
    /// Merged into the parent's first shape where [`merge_exact`] can, and
    /// otherwise kept as it is, with the parent's first shape pushed onto the
    /// front of the parent's chain — which is the whole of what a link is for.
    fn cut(&mut self, own: PlacedShape, under: Option<ClipIndex>) -> ResolvedClip {
        let Some(parent) = under else {
            return ResolvedClip::alone(own);
        };
        let above = &mut self.placed[parent.0];
        let parent = above.reference.resolved();
        let open = parent
            .open
            .intersection(&own.world_aabb())
            .unwrap_or_default();
        match merge_exact(&parent.first, &own) {
            Some(first) => ResolvedClip {
                first,
                rest: parent.rest,
                open,
            },
            None => {
                // One link per parent, however many of its children chain to
                // it: the first pushes it and the rest find it here.
                if above.link == NO_CHAIN {
                    above.link = self.chains.push(parent.first, parent.rest);
                }
                ResolvedClip {
                    first: own,
                    rest: above.link,
                    open,
                }
            }
        }
    }

    fn push(
        &mut self,
        reference: ClipRef,
        own: PlacedShape,
        under: Option<ClipIndex>,
    ) -> ClipIndex {
        let index = ClipIndex(self.placed.len());
        self.placed.push(Placed {
            reference,
            own,
            under,
            link: NO_CHAIN,
        });
        index
    }

    /// Place a clip this frame met for the first time.
    pub(crate) fn place(&mut self, own: PlacedShape, under: Option<ClipIndex>) -> ClipIndex {
        let held = self.cut(own, under);
        self.push(ClipRef::holding(held), own, under)
    }

    /// Place the clips a cached subtree carries, where the subtree now is.
    ///
    /// Two kinds, and the difference between them is the whole of what a clip
    /// costs a replay. One the subtree placed for itself travels with it,
    /// so it is translated by the same offset as the content and cut afresh by
    /// its parent — a resized viewport above cuts the new intersection, not
    /// the old one shifted. One it inherited is not its to carry: it is looked
    /// up where the subtree now finds itself, which is how a row scrolls while
    /// its scroller does not. An overlay's clip is one of the first kind, cut
    /// by whatever its node is under like any other (#708).
    ///
    /// Writing through the kept reference is what moves the clip for every
    /// command that names it at once. None of them is touched.
    ///
    /// The inverse of [`since`](Self::since), and beside it for that reason:
    /// one of them encodes the parent links and the other reads them back.
    ///
    /// `transform` is a translation for a cached subtree. It is anything at
    /// all for a layer's content that is drawn on the surface after all,
    /// which was flattened in the layer's own space.
    pub(crate) fn replay(
        &mut self,
        clips: &[CachedClip],
        transform: &Transform,
        inherited: Option<ClipIndex>,
    ) {
        let start = self.placed.len();
        for clip in clips {
            let own = clip.own.mapped(transform);
            let under = match clip.parent {
                CachedParent::Within(offset) => Some(ClipIndex(start + offset)),
                CachedParent::Inherited => inherited,
            };
            let held = self.cut(own, under);
            clip.reference.place(held);
            self.push(clip.reference.clone(), own, under);
        }
    }

    /// The clips placed since `mark`, written down so the subtree that placed
    /// them can be replayed somewhere else.
    ///
    /// Parent links are rebased on the way out: one pointing inside the run
    /// becomes an offset within it, and one pointing outside — or at nothing —
    /// becomes "whatever the subtree inherits when it is put back", since every
    /// clip in the run that inherits at all inherits through the deepest clip
    /// above its root. Which is *not* where it was pointing when the entry was
    /// made: a subtree cached under no clip at all can be replayed under one
    /// that has since appeared, and it has to be cut by it.
    pub(crate) fn since(&self, mark: usize) -> Vec<CachedClip> {
        self.placed[mark..]
            .iter()
            .map(|placed| CachedClip {
                reference: placed.reference.clone(),
                own: placed.own,
                parent: match placed.under {
                    Some(ClipIndex(index)) if index >= mark => CachedParent::Within(index - mark),
                    _ => CachedParent::Inherited,
                },
            })
            .collect()
    }
}

/// A clip a cached subtree placed, kept so a replay can place it again.
#[derive(Debug, Clone)]
pub struct CachedClip {
    /// The very reference the cached commands carry, so a replay moves the
    /// clip by writing through it.
    pub(crate) reference: ClipRef,
    /// What it cut by itself when the entry was made; a replay translates this
    /// by the same offset as the content and intersects it afresh.
    pub(crate) own: PlacedShape,
    pub(crate) parent: CachedParent,
}

/// What a cached subtree's clip sits under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachedParent {
    /// Another clip of the same subtree, at this offset into its run.
    Within(usize),
    /// Whatever clip the subtree finds itself under when it is replayed —
    /// which need not be the one it was cached under, and need not have
    /// existed then at all. That is the whole point.
    ///
    /// The same sentence is spelled a second way one type over: a cached
    /// *command* says it by naming no clip at all (`commands_since` in
    /// `flatten.rs` writes `None` for the clip its subtree inherited). The two
    /// are read by the two halves of one replay and have to mean the same
    /// thing.
    Inherited,
}

/// How near two points have to be to be one corner, and how far outside a
/// clip a point may sit and still be on its edge — in the units of the space
/// the two clips are compared in.
const SAME_POINT: f32 = 1e-3;

/// Two clips as one rounded rect, where one rounded rect *is* their
/// intersection — and `None` everywhere else.
///
/// The answer is `parent`'s and `own`'s boxes intersected, with each corner of
/// that box rounded by the clip that cuts it. Which clip cuts a corner is read
/// off the corner point: a convex corner, and a scoop's disc too, cuts
/// everything between its curve and its box corner, so if the point at a
/// corner of the intersection is inside a clip, nothing of that clip cuts the
/// intersection anywhere near it. If the point is outside, the clip's own
/// corner has to sit exactly there, or the cut is a curve no corner of the
/// result can draw. Where both clips cut one corner, one has to nest inside
/// the other — a larger radius and a curve no rounder — and that one is the
/// corner; two corners that cross each other are not one corner.
///
/// Refused besides: a turn or a skew between the two spaces, where the
/// intersection is not an upright rect in either; a scale that is not the same
/// on both axes, where a circle in one space is an ellipse in the other; two
/// curvatures among the corners, because a rounded rect has one; and a corner
/// larger than half the intersection's shorter side, which the shader would
/// clamp smaller than the clip that supplied it.
///
/// The test is Skia's, `SkRRectPriv::ConservativeIntersect`, which merges two
/// rrects only where the result is exact and leaves the rest to its clip stack.
pub(crate) fn merge_exact(parent: &PlacedShape, own: &PlacedShape) -> Option<PlacedShape> {
    let own = rebase_exactly(own, parent)?;
    // Two clips that miss leave nothing, and nothing is exact.
    let rect = parent.rect.intersection(&own.rect).unwrap_or_default();
    let merged = |radii: [f32; 4], curvature: f32| PlacedShape {
        rect,
        radii: radii.into(),
        curvature,
        placement: parent.placement,
    };
    let clips = [parent, &own].map(|clip| {
        let radii = clip.clamped_radii().to_array();
        (
            clip,
            corners(clip.rect),
            radii,
            radii.iter().any(|&r| r > 0.0),
        )
    });
    // A clip with no rounded corner cuts nothing its box does not, and the
    // intersection is inside its box. Two of them — the commonest pair there
    // is — are the box.
    if rect.width <= 0.0 || rect.height <= 0.0 || clips.iter().all(|&(.., rounded)| !rounded) {
        return Some(merged([0.0; 4], parent.curvature));
    }

    let half = rect.width.min(rect.height) * 0.5;
    let mut radii = [0.0; 4];
    let mut curvature = None;
    for (corner, (px, py)) in corners(rect).into_iter().enumerate() {
        let mut cut: Option<(f32, f32)> = None;
        for &(clip, clip_corners, clip_radii, rounded) in &clips {
            if !rounded || clip.distance(px, py) <= SAME_POINT {
                continue;
            }
            let (cx, cy) = clip_corners[corner];
            if (cx - px).abs() > SAME_POINT || (cy - py).abs() > SAME_POINT {
                return None;
            }
            let radius = clip_radii[corner];
            if radius > half {
                return None;
            }
            let this = (radius, clip.curvature);
            cut = Some(match cut {
                None => this,
                Some(other) => deeper(other, this)?,
            });
        }
        if let Some((radius, k)) = cut {
            match curvature {
                None => curvature = Some(k),
                Some(seen) if same_curve(seen, k) => {}
                Some(_) => return None,
            }
            radii[corner] = radius;
        }
    }
    Some(merged(radii, curvature.unwrap_or(parent.curvature)))
}

/// A rect's four corners, in [`CornerRadii`](super::CornerRadii)'s order.
fn corners(rect: Rect) -> [(f32, f32); 4] {
    let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
    [(rect.x, rect.y), (x1, rect.y), (x1, y1), (rect.x, y1)]
}

/// Of two corners on one box corner, the one that cuts the other's region
/// entirely — `None` when neither does.
///
/// A convex corner cuts more as its radius grows and as its curve flattens
/// toward the bevel, so it nests inside another exactly when it is no smaller
/// and no rounder. A scoop is a disc taken out of the corner and nests only
/// inside a larger disc; against a convex corner the two curves cross.
fn deeper(a: (f32, f32), b: (f32, f32)) -> Option<(f32, f32)> {
    let ((ra, ka), (rb, kb)) = (a, b);
    match (ka < 0.0, kb < 0.0) {
        (true, true) => Some(if ra >= rb { a } else { b }),
        (false, false) if ra >= rb && ka <= kb => Some(a),
        (false, false) if rb >= ra && kb <= ka => Some(b),
        _ => None,
    }
}

/// Whether two curvatures draw the same corner: equal, or both a scoop, which
/// the shaders draw alike whatever its value.
fn same_curve(a: f32, b: f32) -> bool {
    a == b || (a < 0.0 && b < 0.0)
}

/// `clip` written in `onto`'s coordinates, where that loses nothing: the map
/// between the two sends a rect to a rect, and a circle to a circle.
///
/// A quarter turn and a mirror pass, with the corners permuted to follow them
/// (#397). A turn of any other angle and a skew are refused, since the image is
/// not an upright rect; so is a scale that differs between the axes, since a
/// rounded corner is then an ellipse and a radius cannot say so.
fn rebase_exactly(clip: &PlacedShape, onto: &PlacedShape) -> Option<PlacedShape> {
    if clip.placement == onto.placement {
        return Some(*clip);
    }
    let between = onto.placement.inverse()?.then(&clip.placement);
    if !between.rect_stays_rect() {
        return None;
    }
    let (sx, sy) = between.extract_scale_components();
    if (sx - sy).abs() > sx.max(sy) * 1e-5 {
        return None;
    }
    Some(PlacedShape {
        rect: between.map_rect(clip.rect),
        radii: clip.radii.scaled(sx).permuted(between.corner_permutation()),
        curvature: clip.curvature,
        placement: onto.placement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::CornerRadii;
    use crate::transform::Transform;

    fn clip(rect: Rect, radii: f32, curvature: f32) -> PlacedShape {
        PlacedShape::placed(
            rect,
            CornerRadii::uniform(radii),
            curvature,
            Transform::IDENTITY,
        )
    }

    fn square(x: f32, y: f32, side: f32) -> Rect {
        Rect::new(x, y, side, side)
    }

    fn one_corner(top_left: f32, bottom_right: f32) -> CornerRadii {
        CornerRadii {
            top_left,
            top_right: 0.0,
            bottom_right,
            bottom_left: 0.0,
        }
    }

    /// The table #698 measured, both ways round: a rounded clip and a square
    /// one over the same box cut what the rounded one cuts.
    #[test]
    fn a_square_clip_over_a_rounded_one_keeps_its_corners() {
        let rounded = clip(square(0.0, 0.0, 100.0), 20.0, 1.0);
        let squared = clip(square(0.0, 0.0, 100.0), 0.0, 1.0);
        for (parent, own) in [(rounded, squared), (squared, rounded)] {
            let merged = merge_exact(&parent, &own).expect("one rounded rect says it exactly");
            assert_eq!(merged.rect, square(0.0, 0.0, 100.0));
            assert_eq!(
                merged.radii,
                CornerRadii::uniform(20.0),
                "the square clip cuts none of the corners the rounded one does"
            );
        }
    }

    #[test]
    fn a_larger_square_clip_leaves_the_rounded_one_as_it_is() {
        let rounded = clip(square(0.0, 0.0, 100.0), 20.0, 1.0);
        let larger = clip(square(-50.0, -50.0, 200.0), 0.0, 1.0);
        for (parent, own) in [(rounded, larger), (larger, rounded)] {
            let merged = merge_exact(&parent, &own).expect("exact");
            assert_eq!(merged.rect, square(0.0, 0.0, 100.0));
            assert_eq!(merged.radii, CornerRadii::uniform(20.0));
        }
    }

    /// The inner clip's corners are inside the rounded one, so they are the
    /// intersection's, and square.
    #[test]
    fn a_square_clip_inside_the_rounded_one_stays_square() {
        let rounded = clip(square(0.0, 0.0, 100.0), 20.0, 1.0);
        let inside = clip(square(10.0, 10.0, 80.0), 0.0, 1.0);
        let merged = merge_exact(&rounded, &inside).expect("exact");
        assert_eq!(merged.rect, square(10.0, 10.0, 80.0));
        assert_eq!(merged.radii, CornerRadii::uniform(0.0));
    }

    /// A circle over a squircle on the same box cuts deeper at every corner —
    /// a radius no smaller and a flatter curve — so the circle is the answer.
    #[test]
    fn a_circle_over_a_squircle_on_the_same_box_is_the_circle() {
        let circle = clip(square(0.0, 0.0, 100.0), 50.0, 1.0);
        for radius in [12.0, 40.0] {
            let squircle = clip(square(0.0, 0.0, 100.0), radius, 2.0);
            for (parent, own) in [(circle, squircle), (squircle, circle)] {
                let merged = merge_exact(&parent, &own).expect("the circle nests the squircle");
                assert_eq!(merged.radii, CornerRadii::uniform(50.0), "radius {radius}");
                assert_eq!(merged.curvature, 1.0);
            }
        }
    }

    /// Moved in from the circle's corners, the squircle's corner points sit
    /// where the circle has already cut — a curve meeting a curve, which no
    /// one corner draws.
    #[test]
    fn a_squircle_inset_in_a_circle_is_chained() {
        let circle = clip(square(0.0, 0.0, 100.0), 50.0, 1.0);
        let inset = clip(square(10.0, 10.0, 80.0), 12.0, 2.0);
        assert_eq!(merge_exact(&circle, &inset), None);
        assert_eq!(merge_exact(&inset, &circle), None);
    }

    /// Two corners on one box corner that cross: a larger squircle and a
    /// smaller circle each cut somewhere the other does not.
    #[test]
    fn crossing_corners_are_chained() {
        let squircle = clip(square(0.0, 0.0, 100.0), 30.0, 2.0);
        let circle = clip(square(0.0, 0.0, 100.0), 20.0, 1.0);
        assert_eq!(merge_exact(&squircle, &circle), None);
        assert_eq!(merge_exact(&circle, &squircle), None);
    }

    /// Each clip rounds a corner of its own, and with a curve of its own: the
    /// corners are exact one by one, and no one curvature draws both.
    #[test]
    fn two_curvatures_among_the_corners_are_chained() {
        let top_left = PlacedShape {
            radii: one_corner(20.0, 0.0),
            ..clip(square(0.0, 0.0, 100.0), 0.0, 1.0)
        };
        let bottom_right = PlacedShape {
            radii: one_corner(0.0, 20.0),
            ..clip(square(0.0, 0.0, 100.0), 0.0, 2.0)
        };
        assert_eq!(merge_exact(&top_left, &bottom_right), None);

        let same_curve = PlacedShape {
            curvature: 1.0,
            ..bottom_right
        };
        let merged = merge_exact(&top_left, &same_curve).expect("one curve, two corners");
        assert_eq!(merged.radii, one_corner(20.0, 20.0));
    }

    /// A corner the intersection is too small to hold would be clamped by the
    /// shader to less than the clip that supplied it.
    #[test]
    fn a_corner_larger_than_the_intersection_holds_is_chained() {
        let rounded = clip(square(0.0, 0.0, 100.0), 40.0, 1.0);
        let strip = clip(Rect::new(0.0, 0.0, 100.0, 50.0), 0.0, 1.0);
        assert_eq!(merge_exact(&rounded, &strip), None);
    }

    #[test]
    fn a_turn_a_skew_or_an_uneven_scale_between_the_two_is_chained() {
        let outer = clip(square(0.0, 0.0, 100.0), 20.0, 1.0);
        for placement in [
            Transform::rotate_degrees(25.0),
            Transform::rotate_degrees(25.0).then(&Transform::scale_xy(3.0, 1.0)),
            Transform::scale_xy(0.5, 1.0),
        ] {
            let inner = PlacedShape {
                placement,
                ..clip(square(10.0, 10.0, 40.0), 0.0, 1.0)
            };
            assert_eq!(merge_exact(&outer, &inner), None, "{placement:?}");
        }
    }

    /// A quarter turn and a uniform scale lose nothing, and the corners follow
    /// the turn.
    #[test]
    fn a_quarter_turn_and_a_uniform_scale_are_merged() {
        let outer = clip(square(0.0, 0.0, 100.0), 0.0, 1.0);
        let inner = PlacedShape {
            radii: one_corner(10.0, 0.0),
            placement: Transform::translate(100.0, 0.0)
                .then(&Transform::rotate_degrees(90.0))
                .then(&Transform::scale(2.0)),
            ..clip(square(0.0, 0.0, 50.0), 0.0, 1.0)
        };
        let merged = merge_exact(&outer, &inner).expect("a rect, and circles, either way");
        assert!(
            (merged.rect.width - 100.0).abs() < 1e-3,
            "{:?}",
            merged.rect
        );
        assert!(
            (merged.radii.top_right - 20.0).abs() < 1e-3,
            "turned a quarter clockwise and doubled: {:?}",
            merged.radii
        );
    }

    /// Two clips under one transform merge in that transform's space and come
    /// out still turned — and each corner is rounded by the clip that makes
    /// it, which here is a different clip at each end of the same edge.
    ///
    /// The rule this replaced took the smaller radius everywhere, so the inner
    /// clip's 16 came out as the outer's 8.
    #[test]
    fn two_clips_in_one_turned_space_merge_in_it() {
        let placement = Transform::rotate_degrees(30.0);
        let outer = PlacedShape {
            placement,
            ..clip(square(0.0, 0.0, 100.0), 8.0, 1.0)
        };
        let inner = PlacedShape {
            placement,
            ..clip(Rect::new(20.0, 0.0, 100.0, 60.0), 16.0, 1.0)
        };

        let both = merge_exact(&outer, &inner).expect("one space, one rounded rect");
        assert_eq!(both.placement, placement, "still turned by the same thing");
        assert_eq!(both.rect, Rect::new(20.0, 0.0, 80.0, 60.0));
        assert_eq!(
            both.radii,
            CornerRadii {
                top_left: 16.0,
                top_right: 8.0,
                bottom_right: 0.0,
                bottom_left: 16.0,
            },
            "the inner clip's own corners on the left, the outer's on the right, \
             and a square corner where an edge of each meets"
        );
    }

    /// A clip in a scroller in a scroller: the two spaces differ by a
    /// translation, and the inner is rewritten in the outer's coordinates.
    #[test]
    fn a_clip_is_merged_through_a_translation() {
        let outer = clip(square(0.0, 0.0, 100.0), 0.0, 1.0);
        let inner = PlacedShape {
            placement: Transform::translate(40.0, 0.0),
            ..outer
        };
        let both = merge_exact(&outer, &inner).expect("a translation loses nothing");
        assert_eq!(
            both.placement,
            Transform::IDENTITY,
            "rebased onto the first"
        );
        assert_eq!((both.rect.x, both.rect.width), (40.0, 60.0));
    }

    /// A mirror sends a rect to a rect too, and `Container::scale` takes a
    /// negative factor, so this is reachable rather than hypothetical. The
    /// left corners land on the right.
    #[test]
    fn a_mirrored_clip_is_merged_with_its_corners_moved() {
        let outer = PlacedShape {
            placement: Transform::rotate_degrees(20.0),
            ..clip(square(0.0, 0.0, 100.0), 20.0, 1.0)
        };
        let inner = PlacedShape {
            radii: CornerRadii {
                top_left: 9.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 5.0,
            },
            placement: outer
                .placement
                .then(&Transform::translate(100.0, 0.0).then(&Transform::scale_xy(-1.0, 1.0))),
            ..clip(square(0.0, 0.0, 80.0), 0.0, 1.0)
        };

        let both = merge_exact(&outer, &inner).expect("a mirror loses nothing");
        assert_eq!(both.placement, outer.placement);
        assert!(
            (both.radii.bottom_right - 5.0).abs() < 1e-3,
            "the inner's bottom-left, mirrored: {:?}",
            both.radii
        );
        assert!(
            (both.radii.top_right - 20.0).abs() < 1e-3,
            "and where the inner's mirrored 9 shares a corner with the outer's \
             20, the 20 cuts deeper: {:?}",
            both.radii
        );
    }

    /// What the compositor is told a surface covers is tessellated from the
    /// merged shape, so a corner that follows a quarter turn to the wrong side
    /// is a wedge of the surface that stops being ours: the click goes to
    /// whatever is behind.
    #[test]
    fn a_region_under_two_clips_a_quarter_turn_apart_rounds_the_right_corner() {
        let covers = |rects: &[crate::region::RegionRect], x: i32, y: i32| {
            rects
                .iter()
                .any(|r| x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height)
        };
        let outer = clip(square(0.0, 0.0, 100.0), 0.0, 1.0);
        // The same square, a quarter turn round and moved back over the outer,
        // rounded at its top-left only.
        let inner = PlacedShape {
            radii: CornerRadii {
                top_left: 40.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            placement: Transform::translate(100.0, 0.0).then(&Transform::rotate_degrees(90.0)),
            ..outer
        };

        let both = merge_exact(&outer, &inner).expect("a quarter turn loses nothing");
        let rects = crate::region::placed_shape_to_rects(both, &[]);
        assert!(
            covers(&rects, 3, 3),
            "the rounding turned a quarter clockwise off the top-left, so this \
             corner is square and the surface still claims it"
        );
        assert!(
            !covers(&rects, 96, 3),
            "and the top-right is where it landed, so the region gives that \
             wedge up"
        );
    }

    /// Two clips that miss each other leave nothing, which is exact.
    #[test]
    fn two_clips_that_miss_merge_into_nothing() {
        let here = clip(square(0.0, 0.0, 50.0), 10.0, 1.0);
        let far = clip(square(500.0, 500.0, 50.0), 0.0, 1.0);
        let merged = merge_exact(&here, &far).expect("nothing is exact");
        assert_eq!((merged.rect.width, merged.rect.height), (0.0, 0.0));
    }

    // -- the tree -------------------------------------------------------

    /// A clip that cannot merge keeps its own shape and hangs its parent's
    /// from its chain; a clip under it that can merge shares that chain.
    #[test]
    fn a_clip_that_cannot_merge_is_chained_to_its_parent() {
        let mut tree = ClipTree::default();
        let card = clip(square(0.0, 0.0, 100.0), 50.0, 1.0);
        let inset = clip(square(10.0, 10.0, 80.0), 12.0, 2.0);
        let row = clip(Rect::new(10.0, 40.0, 80.0, 20.0), 0.0, 1.0);

        let outer = tree.place(card, None);
        let middle = tree.place(inset, Some(outer));
        let inner = tree.place(row, Some(middle));

        assert_eq!(tree.reference(outer).resolved(), ResolvedClip::alone(card));
        let chained = tree.reference(middle).resolved();
        assert_eq!(chained.first, inset);
        assert_eq!(
            tree.chains()
                .chain(chained.rest)
                .copied()
                .collect::<Vec<_>>(),
            vec![card]
        );
        let merged = tree.reference(inner).resolved();
        assert_eq!(
            merged.first.rect, row.rect,
            "inside the squircle, so merged"
        );
        assert_eq!(merged.rest, chained.rest, "and under the same chain");
        assert_eq!(tree.chains().links().len(), 1, "one link, shared");
    }

    /// A clip under nothing is its own shape, and hangs from no chain.
    #[test]
    fn a_clip_under_nothing_has_no_chain() {
        let mut tree = ClipTree::default();
        let ripple = clip(square(10.0, 10.0, 80.0), 12.0, 2.0);
        let alone = tree.place(ripple, None);
        assert_eq!(
            tree.reference(alone).resolved(),
            ResolvedClip::alone(ripple)
        );
        assert!(tree.chains().links().is_empty());
    }

    /// A replay places a cached subtree's clips again, chains and all, in a
    /// frame whose chains started empty — the index the last frame handed out
    /// means nothing now.
    #[test]
    fn a_replay_chains_its_clips_again() {
        let card = clip(square(0.0, 0.0, 100.0), 50.0, 1.0);
        let inset = clip(square(10.0, 10.0, 80.0), 12.0, 2.0);

        let mut tree = ClipTree::default();
        let outer = tree.place(card, None);
        let mark = tree.mark();
        let inner = tree.place(inset, Some(outer));
        let reference = tree.reference(inner);
        let cached = tree.since(mark);

        tree.clear();
        assert!(tree.chains().links().is_empty());
        // Something else chains first this frame, so last frame's index is
        // somebody else's link now.
        let other = tree.place(card, None);
        tree.place(inset, Some(other));
        let moved = card.mapped(&Transform::translate(5.0, 0.0));
        let outer = tree.place(moved, None);
        tree.replay(&cached, &Transform::translate(5.0, 0.0), Some(outer));

        let resolved = reference.resolved();
        assert_eq!(
            resolved.first,
            inset.mapped(&Transform::translate(5.0, 0.0))
        );
        assert_eq!(
            tree.chains()
                .chain(resolved.rest)
                .copied()
                .collect::<Vec<_>>(),
            vec![moved],
            "under the card where it is now, through this frame's link"
        );
    }

    /// Siblings that chain to one clip share the one link it was pushed as,
    /// rather than each pushing a copy of it for the GPU to read again.
    #[test]
    fn siblings_chained_to_one_clip_share_its_link() {
        let mut tree = ClipTree::default();
        let card = tree.place(clip(square(0.0, 0.0, 100.0), 50.0, 1.0), None);
        let rests: Vec<u32> = (0..3)
            .map(|_| {
                let inset = tree.place(clip(square(10.0, 10.0, 80.0), 12.0, 2.0), Some(card));
                tree.reference(inset).resolved().rest
            })
            .collect();
        assert_eq!(rests, vec![0, 0, 0]);
        assert_eq!(tree.chains().links().len(), 1);
    }

    /// What a consumer that takes only a box is given is the box every shape
    /// down the chain leaves open, not the first shape's alone.
    #[test]
    fn a_chain_leaves_open_the_box_all_its_shapes_do() {
        let mut tree = ClipTree::default();
        let card = clip(square(0.0, 0.0, 100.0), 50.0, 1.0);
        let wide = clip(Rect::new(10.0, 10.0, 200.0, 80.0), 12.0, 2.0);
        let outer = tree.place(card, None);
        let inner = tree.place(wide, Some(outer));
        let resolved = tree.reference(inner).resolved();
        assert_ne!(resolved.rest, NO_CHAIN, "chained, so the box is a question");
        assert_eq!(resolved.open, Rect::new(10.0, 10.0, 90.0, 80.0));
        assert_eq!(tree.chains().shapes(&resolved).count(), 2);
    }

    /// A chain knows how deep it is, so one deeper than the shaders walk is
    /// said when it is built rather than cut short in silence.
    #[test]
    fn a_chain_counts_its_links() {
        let mut tree = ClipTree::default();
        // Each clip inset from the last by less than its corners reach, so
        // none merges and every one adds a link.
        let mut parent = None;
        let mut last = None;
        for step in 0..=MAX_CHAIN + 1 {
            let inset = step as f32;
            let shape = clip(square(inset, inset, 1000.0 - 2.0 * inset), 400.0, 1.0);
            let index = tree.place(shape, parent);
            parent = Some(index);
            last = Some(index);
        }
        let rest = tree.reference(last.unwrap()).resolved().rest;
        assert_eq!(tree.chains().depth(rest), MAX_CHAIN + 1);
        assert_eq!(tree.chains().depth(NO_CHAIN), 0);
    }

    /// The cap the tree warns past is the one the shaders stop at.
    #[test]
    fn the_cap_is_the_shaders() {
        let shader = include_str!("shader.wgsl");
        assert!(
            shader.contains(&format!("const MAX_CHAIN: u32 = {MAX_CHAIN}u;")),
            "shader.wgsl walks a different number of links than MAX_CHAIN says"
        );
    }

    /// Clearing keeps the room the links took, as it keeps the clips'.
    #[test]
    fn clearing_keeps_the_links_room() {
        let mut tree = ClipTree::default();
        for _ in 0..8 {
            let outer = tree.place(clip(square(0.0, 0.0, 100.0), 50.0, 1.0), None);
            tree.place(clip(square(10.0, 10.0, 80.0), 12.0, 2.0), Some(outer));
        }
        let room = tree.link_capacity();
        assert!(room >= 8);
        tree.clear();
        assert_eq!(tree.link_capacity(), room);
    }
}
