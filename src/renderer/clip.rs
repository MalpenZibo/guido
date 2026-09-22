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
//! dozen places `render.rs` reads `cmd.clip()`. Every pipeline and every shader
//! is handed the same `PlacedShape` it was handed before.
//!
//! Chromium's clip tree is this, grown up: four property trees, clips among
//! them, and draw commands referencing nodes in them rather than carrying
//! copies. Flutter reaches the same place from the other side, with the clip in
//! a layer of its own from the content that scrolls under it.

use std::cell::Cell;
use std::rc::Rc;

use super::flatten::intersect_clips;
use crate::shape::PlacedShape;

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
#[derive(Debug, Clone)]
pub struct ClipRef(Rc<Cell<PlacedShape>>);

impl ClipRef {
    /// Where this clip cuts, in the coordinates it was declared in plus the
    /// transform that places it — already cut down by its ancestors.
    pub fn shape(&self) -> PlacedShape {
        self.0.get()
    }

    /// A clip standing where it was put, named by nobody else yet.
    pub(crate) fn placed(shape: PlacedShape) -> Self {
        Self(Rc::new(Cell::new(shape)))
    }

    fn place(&self, shape: PlacedShape) {
        self.0.set(shape);
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
    under: Under,
}

/// What a clip answers to besides itself.
///
/// The two are not the same thing and the difference only shows a frame later.
/// A clip that inherits nothing *today* still inherits: put its subtree back
/// under an `Overflow::Hidden` that has since been switched on, and the clip is
/// cut by it. A clip that answers to nothing never is, whatever appears above
/// it — which is a ripple, cut to the shape it belongs to and to nothing else.
/// Reading the first as the second is a clip that stops cutting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Under {
    /// Whatever clip is in effect where this one is placed, if any.
    Inherited(Option<ClipIndex>),
    /// Nothing, on any frame.
    Nothing,
}

/// The clips one frame placed, in the order flatten met them.
///
/// A tree by the parent links, a vector by storage: a clip's parent is always
/// placed before it, so a subtree's clips are a contiguous run and the entry
/// kept for a cached subtree is a slice of one.
#[derive(Default)]
pub(crate) struct ClipTree {
    placed: Vec<Placed>,
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
        let Self { placed } = self;
        placed.clear();
    }

    /// How many clips this tree can hold before it allocates again.
    pub(crate) fn capacity(&self) -> usize {
        self.placed.capacity()
    }

    /// Where `index` cuts, with every ancestor already taken into account.
    fn shape(&self, index: ClipIndex) -> PlacedShape {
        self.placed[index.0].reference.shape()
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
    fn cut(&self, own: PlacedShape, under: Under) -> PlacedShape {
        match under {
            Under::Inherited(Some(parent)) => intersect_clips(&self.shape(parent), &own),
            Under::Inherited(None) | Under::Nothing => own,
        }
    }

    fn push(&mut self, reference: ClipRef, own: PlacedShape, under: Under) -> ClipIndex {
        let index = ClipIndex(self.placed.len());
        self.placed.push(Placed {
            reference,
            own,
            under,
        });
        index
    }

    /// Place a clip this frame met for the first time.
    pub(crate) fn place(&mut self, own: PlacedShape, under: Under) -> ClipIndex {
        let resolved = self.cut(own, under);
        self.push(ClipRef::placed(resolved), own, under)
    }

    /// Place the clips a cached subtree carries, where the subtree now is.
    ///
    /// Three kinds, and the difference between them is the whole of what a
    /// clip costs a replay. One the subtree placed for itself travels with it,
    /// so it is translated by the same offset as the content and cut afresh by
    /// its parent — a resized viewport above cuts the new intersection, not
    /// the old one shifted. One it inherited is not its to carry: it is looked
    /// up where the subtree now finds itself, which is how a row scrolls while
    /// its scroller does not. And an overlay's clip answers to nobody, so it
    /// is only carried.
    ///
    /// Writing through the kept reference is what moves the clip for every
    /// command that names it at once. None of them is touched.
    ///
    /// The inverse of [`since`](Self::since), and beside it for that reason:
    /// one of them encodes the parent links and the other reads them back.
    pub(crate) fn replay(
        &mut self,
        clips: &[CachedClip],
        dx: f32,
        dy: f32,
        inherited: Option<ClipIndex>,
    ) {
        let start = self.placed.len();
        for clip in clips {
            let own = clip.own.translated(dx, dy);
            let under = match clip.parent {
                CachedParent::Within(offset) => Under::Inherited(Some(ClipIndex(start + offset))),
                CachedParent::Inherited => Under::Inherited(inherited),
                CachedParent::Nothing => Under::Nothing,
            };
            clip.reference.place(self.cut(own, under));
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
                    Under::Inherited(Some(ClipIndex(index))) if index >= mark => {
                        CachedParent::Within(index - mark)
                    }
                    Under::Inherited(_) => CachedParent::Inherited,
                    Under::Nothing => CachedParent::Nothing,
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
    /// Nothing, on this frame or any other: an overlay clip replaces what it
    /// inherits rather than narrowing it, the way a ripple is cut to the shape
    /// it belongs to and to nothing above it.
    Nothing,
}
