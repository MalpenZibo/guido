# Renderer

This document provides a developer reference for Guido's GPU rendering system.

## Overview

Guido uses a hierarchical render tree architecture where widgets paint to their own nodes using local coordinates. Transforms are automatically inherited from parent to child during tree flattening, eliminating confusion from manual push/pop operations.

**Key Benefits:**
- Widgets always paint at (0,0) in local coordinates
- Transform inheritance happens automatically
- Clean separation between painting and coordinate transformation
- Proper clipping with rounded corners

## Render Tree Architecture

### RenderNode

Each widget creates a `RenderNode` containing its visual output:

```rust
pub struct RenderNode {
    pub id: NodeId,                          // Unique identifier (matches widget ID)
    pub bounds: Rect,                        // Local bounds for transform origin
    pub local_transform: Transform,          // Transform relative to parent
    pub parent_position: Transform,          // Position set by parent (for cache reuse)
    pub pivot: Pivot,                        // What rotation and scale act about
    pub opacity: f32,                        // Multiplied into this subtree's every draw
    pub commands: SmallVec<[Rc<DrawCommand>; 2]>,        // Draw commands (shapes, text, images)
    pub children: Vec<Rc<RenderNode>>,       // Child nodes, Rc-shared with the paint cache
    pub overlay_commands: SmallVec<[Rc<DrawCommand>; 1]>, // Commands drawn after children
    pub clip: Option<ClipRegion>,            // Clips this node and children
    pub overlay_clip: Option<ClipRegion>,    // Clips only overlay commands
    pub repainted: Cell<bool>,               // true = freshly painted this frame
    pub partial: bool,                       // Some children were not painted (do not cache)
    pub cached_flatten: RefCell<Option<Rc<CachedFlatten>>>, // Cached flatten output
}
```

### Sharing Model

Children are `Rc`-shared. The paint cache (`Tree::cache_paint`) stores an `Rc`
to the same node that sits in the frame's render tree:

- **Caching** a painted subtree is a refcount bump, not a deep clone.
- **Reusing** a clean child whose position didn't change is `Rc::clone` —
  zero copies. If it moved, only the node header is cloned (children and
  commands stay shared).
- The per-frame flags use interior mutability (`Cell` / `RefCell`) because
  cached nodes are shared: `repainted` is cleared once the node's output has
  been cached, and flatten writes `cached_flatten` through a `&RenderNode`.
- `partial` propagates to ancestors during the cache walk: a subtree that
  embeds a partially-painted node — children narrowed away by the visible
  window, or culled one at a time beneath it — is never cached, so a
  later cache reuse cannot resurrect an incomplete paint. It also invalidates:
  `Tree::clear_cached_paint` drops whatever the last complete paint left, which
  is a picture of the widget as it no longer is — a list that scrolls culls
  from its first scrolled frame onward, so the newest complete entry is the
  list at rest.

Each surface owns a single root `RenderNode` (`ManagedSurface::root_node`),
cleared and rebuilt from dirty widgets every rendered frame.

### Local Coordinate System

Widgets paint in local coordinates where (0,0) is the widget's top-left corner. `PaintContext::paint_child` places the child, from its bounds in the tree — a widget does not position what it paints, and does not call another widget's `paint`.

### ClipRegion

Defines a clip area for a node and its children:

```rust
pub struct ClipRegion {
    pub rect: Rect,         // Clip rectangle in local coordinates
    pub corner_radius: f32, // Corner radius for rounded clipping
    pub curvature: f32,     // Superellipse curvature (K-value)
}
```

## PaintContext API

`PaintContext` is the interface widgets use to build their render nodes.

### Node Properties

```rust
// Set bounds (for transform origin resolution)
ctx.set_bounds(Rect::new(0.0, 0.0, width, height));

// Apply transform (composes with existing): result = existing.then(transform)
ctx.apply_transform(Transform::rotate_degrees(45.0));

// Apply transform with origin
ctx.apply_transform_with_pivot(transform, Pivot::CENTER);

// Set transform origin only
ctx.set_pivot(Pivot::TOP_LEFT);
```

### Clipping

```rust
// Set clip for this node and children
ctx.set_clip(rect, corner_radius, curvature);

// Set rectangular clip (no rounded corners)
ctx.set_clip_rect(rect);

// Set clip only for overlay commands (doesn't clip children)
ctx.set_overlay_clip(rect, corner_radius, curvature);
```

### Draw Commands

```rust
// Rounded rectangle (basic)
ctx.draw_rounded_rect(rect, color, radius);

// Rounded rectangle with curvature
ctx.draw_rounded_rect_with_curvature(rect, color, radius, curvature);

// Gradient rectangle
ctx.draw_rounded_rect_full(rect, color, radius, curvature, border, shadow, gradient);

// Border frame (no fill)
ctx.draw_border_frame_with_curvature(rect, border_color, radius, border_width, curvature);

// With shadow
ctx.draw_rounded_rect_with_shadow(rect, color, radius, curvature, shadow);

// Full configuration
ctx.draw_rounded_rect_full(rect, color, radius, curvature, border, shadow, gradient);

// Circle
ctx.draw_circle(cx, cy, radius, color);

// Text
ctx.draw_text(text, rect, color, font_size);
ctx.draw_text_styled(text, rect, color, font_size, font_family, font_weight, fit);

// Image
ctx.draw_image(source, rect, content_fit);
```

### Children

```rust
// One call per child, and it is the framework's: it culls a child nobody can
// see, serves a clean one from its cache, places it from its bounds in the
// tree, opens its own Paint scope and counts it.
ctx.paint_child(child_id, &ChildPaintOptions::default());

// For an ordered row, the loop that narrows to the visible window first.
paint_children(ctx, &children, &opts);
```

### Overlay Commands

Overlay commands are drawn after all children, useful for effects like ripples:

```rust
ctx.draw_overlay_circle(cx, cy, radius, color);
ctx.draw_overlay_rounded_rect(rect, color, radius);
```

## Tree Flattening

The `flatten_root_into()` function converts the hierarchical render tree into a flat list of `FlattenedCommand`s ready for GPU submission, reusing the output buffer's capacity across frames. The intermediate it groups them in is reused too: a `FlattenScratch` lives on the surface beside those buffers and is emptied on the way in rather than built, so a steady-state frame allocates nothing for the groups, their buckets, their bounds or the frame's clips.

Nothing is ever handed back, so the price is the peak: a surface holds what its widest frame needed for as long as it lives. Per draw group that is five command buffers — the widest frame's commands, held a second time alongside the ones in `flattened_commands` — plus five `LayerBounds`, sixteen rects each inline and up to `MAX_TRACKED_RECTS` on the heap once one spills, on the order of a kilobyte and a half of inline group and five kilobytes of spilled rects. One pathological frame pins that for the surface's life, which is the same bargain the command buffer already made and the reason a `FlattenScratch` belongs to a surface rather than to an application.

### World Transform Computation

```rust
// For each node:
let local_centered = node.local_transform.center_at(origin_x, origin_y);
let world_transform = parent_world_transform.then(&local_centered);
```

The transform origin is resolved from the node's bounds and used to center the transform operation.

### Opacity

A node's `opacity` travels down beside the world transform: each node multiplies
its own into what it inherits, and every command it emits carries the product
as `FlattenedCommand::opacity`. The renderer multiplies it into each colour of
that one draw — the five colours of a `ShapeInstance` on the CPU
(`ShapeInstance::faded`), glyphon's vertex colour for upright text, a vertex
attribute of the textured quad for images and transformed text, and a uniform of
the backdrop composite and contour.

That is *modulation*: each draw faded on its own, so where two children
overlap the overlap shows through. The alternative — drawing the subtree into
an offscreen target and compositing it once, as CSS and Flutter's `Opacity` do
— is correct for overlaps and a render pass of its own; it is not built, and
the `opacity` a container declares is where it would go.

A transformed text and an image take the opacity on the vertex rather than into
their texture, because both textures are cached by content — a fade folded into
the colour would rasterise the text again on every frame of it.

### RenderLayer Ordering

Commands are bucketed by kind so that commands of a kind share a draw call:

```rust
pub enum RenderLayer {
    Shapes = 0,   // Background shapes (rectangles, borders)
    Images = 1,   // Image content
    Text = 2,     // Text content
    Overlay = 3,  // Overlay effects (ripples, highlights)
}
```

### Draw groups

Bucketing on its own reorders drawing: with one set of buckets for the whole
frame, *every* shape is drawn before *every* image, so a container background
painted over an image ends up underneath it however the tree is arranged.

The flattener therefore emits a sequence of **draw groups**. Each group has its
own four buckets, and a new group is opened whenever a command's layer would go
backwards:

```rust
pub struct CommandLayer {
    pub shapes: Range<usize>,
    pub images: Range<usize>,
    pub text: Range<usize>,
    pub overlay: Range<usize>,
}
```

The renderer walks the groups in order and draws each one bucket by bucket.
Batching survives, ordering survives with it, and a tree that never paints a
lower layer over a higher one produces exactly one group — the same single set
of draw calls as before. The split is minimal by construction, so no merge pass
is needed afterwards.

A layer going backwards is only split on when the incoming command's world
bounds actually intersect something already drawn above it in that group.
Without that test the rule fires between every pair of sibling widgets — each
paints a background then a label, so the next sibling's background regresses —
and `examples/showcase.rs` alone went to 16 groups, every one of them carrying
its own glyphon renderer. Siblings do not overlap, so those splits bought
nothing. With the test, `status_bar` is one group and `showcase` is four.

Each layer of a group tracks its commands' rects individually, not just their
union: labels scattered across a panel union into a box spanning the gaps
between them, and the next sibling's background lands in a gap and splits for
nothing. Past `MAX_TRACKED_RECTS` the union decides alone, which can only
over-split. With the exact test, `status_bar` and `showcase` are one group each
— the same draw calls as before groups existed.

Bounds are conservative: shapes grow by their shadow and border, text by half a
font size for glyph overshoot, and anything under a rotation or scale counts as
covering everything. Over-splitting costs a draw call; under-splitting would
draw in the wrong order.

glyphon draws everything a `TextRenderer` prepared in one call, so a group with
directly-rendered text gets a renderer of its own from a pool that grows on
demand. `examples/draw_order_example.rs` exercises the regressions.

### Backdrop effects

`RenderLayer::Backdrop` sits below `Shapes` so a backdrop command always opens
a group: everything it reads must already have been drawn, which means it must
land after the groups holding that content.

A backdrop effect samples the render target, which a pass cannot do to its own
attachment. When a frame contains one, it is drawn into an offscreen colour
target instead of the swapchain; at each backdrop group the pass ends, the
effect runs, and the pass resumes with `LoadOp::Load`. The target is blitted to
the swapchain once at the end and released after enough idle frames.

Per region: downsample into a quarter-size working texture (rendering into a
smaller target with a linear sampler is already a box filter), two separable
gaussian passes ping-ponging between two working textures, then a composite
back over the scene masked by the container's rounded-rect SDF. See
`src/renderer/backdrop_pass.rs`.

The viewport is the shape's world box, because a viewport is four integers and
has no other shape to be. The **mask** is the shape: `BackdropRegion` carries a
`PlacedShape` — the same type a clip is — plus the surface scale, and derives
the rest. The composite carries each fragment into the shape's own space before
testing it, so a turned container frosts what it drew instead of an upright
rounded rect the size of its box. That map is `Transform::physical_inverse`, the
same one a clip uses, and the viewport is the shape's own `world_aabb` rather
than a second field that has to agree with it.

The mask is the one part a caller replaces. `Text::backdrop_blur` emits
`DrawCommand::TextBackdropBlur`, which resolves to the same blur with a
coverage texture in place of the SDF: the glyphs are rasterized into it by
`src/renderer/text_mask.rs`, shaped exactly as `TextRenderState` shapes the
text drawn afterwards.

It takes the same journey as the corners, and that is the whole of how a
transform is handled. `shape` is the text's layout box plus the slack a
descender or a contour needs, in the text's **own** space; the mask covers that
frame; the composite carries each fragment back there and reads the texel it
lands on. Both regions are built by one `backdrop_region`, so the viewport is
always the box around the shape it travels with.

Where the mask is read is all the transform decides; how finely it is made
turns on one bit of the transform and not on its magnitude. A text only moved
is rasterized at the surface scale, one texel per physical pixel. One a
transform stretches takes `TEXT_SUPERSAMPLE` times that, the density its glyph
texture already has, because it will be read back across more pixels than it
has texels. A boolean rather than the stretch itself, so a scale animation
rasterizes twice over its length instead of once a frame. Both flat rules were
tried and each is worse in half the cases; the measurement is in the comment on
`command_to_text_backdrop` and is not repeated here, so there is one copy of it
to keep true.

The frame the mask is read over is a whole number of texels, not the box the
ink needs: the shader divides by that rect to find its texel, so a frame wider
than the texture it stands for would display the coverage short — about a per
cent, which is half a pixel of frost beside its letters at the edges.

A `text_stroke` over frost is dilated from the same mask, so its width is in
mask texels too.

One corner, four copies. WGSL has no include, so the signed distance function
for a superellipse corner is written out in `shader.wgsl`, in
`textured_quad_shader.wgsl` and in `backdrop_shader.wgsl`; the fourth is
`Rect::shape_distance` in `src/widgets/widget.rs`, in Rust, and it is what
hit-testing runs — so it decides where a *click* lands the way the other three
decide where a pixel lands.

The three shaders are kept character-for-character identical between the
`=== SHARED SDF` markers, and `tests/shader_sdf_is_one_definition.rs` fails if
any two drift — which they had: the backdrop read the curvature as
`max(k, 0.05) * 2` against the others' `pow(2, k)`, and those agree at exactly
k = 1 and k = 2, which is every curvature any golden drew *with a backdrop
blur* (#403). The Rust copy cannot be compared with WGSL as text, so the same
test pins it to the geometry: the point sitting on the superellipse is
bracketed a twentieth of a pixel either side, and the boundary must fall
between. It is bracketed rather than measured at the point itself because the
computed distance there lands within a few millionths of a pixel of zero, and
which side of zero it lands on is the last bit of two `pow` calls.

All four copies divide by the larger component before raising it. Taken
straight, `|x|^n` with `n = 2^k` leaves `f32` a little above k = 4 at an
ordinary radius — 28^32 is about 1e46 — and the sum is `inf` whatever the
other component holds, so the distance was `inf` for every fragment the corner
box reaches: along the sides as much as around the corners (#411). What a
pipeline makes of `inf` is undefined, and the two kinds of consumer answered
differently. The Rust copy reads it as outside, which is provable and was
measured: a 120×120 with `Corners::superellipse(40.0, 6.0)` took clicks on
2304 of its 14,400 pixels — the inner square, where `inside` alone decides,
and nothing else. The shaders reach `fwidth` with it, and on lavapipe the
antialiasing collapses and the shape is drawn as a hard-edged box a pixel
wider all round. Another adapter may do something else; that is what
undefined means, and it is reachable at a curvature `Corners::superellipse`
accepts and a transition can sweep through.

Scaled, the larger term is exactly 1 and the smaller at most 1, and as `n`
grows the smaller underflows so the norm tends to `max(|x|, |y|)` — the square
corner a superellipse approaches. The tessellator was never affected: it
samples `cos(t)^(2/n)`, whose exponent *shrinks* as `n` grows.

### Regions

Two things travel from a frame to the compositor as a `wl_region`: the backdrop
blur's area and the surface's input region. Both are read off the flattened
commands, and both go through one tessellation in `src/region.rs`.

`wl_region` is a union of axis-aligned rectangles, so a rounded or turned shape
has to become one — but *which* rectangles is the question, and the answer used
to be the single box containing it. A 150×90 card turned 20° has a 172×136 box,
73% more area: the compositor blurred the desktop in four triangles where the
card is not, and a container that declared an input region took clicks over the
same ground.

`placed_shape_to_rects` takes the shape as declared plus the transform that
places it, and cuts bands out of the outline. The outline is four straight edges
and four corners, each transformed; where a horizontal line meets it is where
the shape starts and stops on that line. A corner is an arc only when it is a
circle, which is the one curvature the scanline solve has a closed form for — a
bevel is a chord, and every other convex curvature is a polyline sampled along
the superellipse, whose chords lie inside it. A rotation is not a case — it is
whatever the matrix holds — so the upright answer falls out of the same
arithmetic rather than beside it.

**A scoop is subtracted rather than traced.** It curves inward, so chords of it
fall *outside* the shape and sampling cannot be used; it used to be cut by the
chord tangent to the bite, which never over-claimed and gave up 17% of the
whole shape (#410). What the shader draws is the plain box minus a disc on each
outer corner, and that is now what is built: the sides run corner to corner,
nothing is traced around the curve, and `Outline::take_bites_from` removes
the four discs from a span. Exact, through the
same closed-form circle solve every other arc uses. The scoop's published area
went from 83% of the shape to 98.6–99.2%, which is where every other curvature
already sat.

Bands are one pixel — finer than that cannot produce a row the output can tell
apart — and adjacent bands that round to the same span merge back into one
rectangle. That is what keeps an upright panel at a handful of
rects instead of one per band.

**The clip is a shape here too**, and the only consumer where it is. At any
scanline the overlap is the overlap of the two shapes' spans — exact, turned
clip or not, and it gets the corners right for free: a card filling a rounded
scroller is cornered by the scroller, where a box intersection would publish
the scroller's square corners.

**A span is a list, and the scoop is why.** Every other curvature bounds a
convex region, which a horizontal line meets in one interval. A scoop does not:
once something turns it, a line can enter the shape, cross into a bite and come
out into the shape again. Claiming the gap between the two halves sends clicks
to a surface that did not draw there — the promise that is zero and not a
tolerance — and keeping only the wider half gives up 5.5% of the shape at 45
degrees, most of the budget the whole tessellation has. So `bounds_at` answers
the convex part and `take_bites_from` removes what each disc covers. A band is claimed only where the shape covers it at
*every* height in it, so each disc is taken at its widest across the band: the
widest part of a bite can sit strictly between two scanlines, which is not
something a convex boundary can do.

**One clip, though.** What arrives is one resolved shape, which
`intersect_clips` has already collapsed from the chain the clip tree holds.
That collapse is exact when the two spaces differ by a scale, a quarter turn or
a mirror, and is the box around both for any other angle. What this consumer escapes is the *shape against clip* box intersection,
not the clip chain's own.

**`clamp_radii` is the shader's clamp**, each corner cut to at most half the
smaller side. It used to be the proportional CSS `border-radius` rule instead,
which agrees for a uniform radius and diverges the moment the four differ — so
a container with unequal corners published a region for a shape nobody drew,
claiming 74 pixels outside it at `tl = 100, tr = 40` on a 120x120 box (#417).

Two clamped corners cannot cross, which is the whole of what the proportional
rule was there to prevent: each is at most half the smaller side, so two on one
edge sum to at most that edge and the straight run between them shrinks to
nothing rather than running backwards.

### FlattenedCommand

The output of tree flattening:

```rust
pub struct FlattenedCommand {
    pub command: Rc<DrawCommand>,   // Shared with the render node — no deep clone
    pub world_transform: Transform,
    pub layer: RenderLayer,
    pub opacity: f32,               // Every opacity from the root down, multiplied
    pub clip: Option<ClipRef>,      // The clip it is cut to, named rather than copied
}
```

`clip` is a reference into the frame's clip tree, and
`FlattenedCommand::clip()` is what resolves it to the `PlacedShape` every
pipeline takes. The half dozen reads in `render.rs` are the only places that
resolution happens, which is why nothing below them changed when the
representation did.

### PlacedShape

A rounded rect as its widget declared it, plus the transform that puts it on the
screen — in `src/shape.rs`:

```rust
pub struct PlacedShape {
    pub rect: Rect,          // in the declaring widget's own space
    pub radii: CornerRadii,  // in that same space
    pub curvature: f32,
    pub placement: Transform, // that space -> world
}
```

**One type, because a clip and a region are the same object.** A clip is a
rounded rect and a transform; so is the area a backdrop blur filters, and so is
the region a surface takes input in. They were two structs with the same four
fields under two names, and the seam showed at the one call site that held
both: a region tessellated its own shape exactly and then asked the clip beside
it for a bounding box, because only one of the two knew how to be a shape.

It used to be a world-space rect, mapped through the transform — which is an
enclosing *box*, equal to the shape only for a transform that keeps the axes.
Under rotation it is larger and square-cornered, and past about 40° it is large
enough that `Overflow::Hidden` stops hiding.

Keeping the declaration answers every consumer:

| consumer | how it reads the clip |
| --- | --- |
| the shape shader | `PlacedShape::to_local` inverts the placement, and the *vertex* stage carries each corner into the clip's own space — the map is affine, so the fragment gets an interpolated `clip_pos` and no matrix at all. There the shape is a rounded rect again and the radii are circles rather than ellipses |
| images (`image_quad.rs`) | the same map, applied on the CPU to the quad's four corners, for the same reason |
| transformed text (`text_quad.rs`) | the same map again, on the same quad — it is the image pipeline with a glyph texture in it, and it takes the same `QuadClip::shape` |
| upright text (glyphon) | `world_aabb()` — `TextBounds` is four integers and glyphon has nowhere to put a shape. Exact all the same: a text the box would cut differently from the shape is routed to the quad above instead, which is what `PlacedShape::box_cuts_like_the_shape` decides (#405) |
| the backdrop pass | both: `world_aabb()` narrows the viewport, because a viewport is four integers, and then each fragment is carried into the clip's own space and tested there, so a rounded scroller keeps its corners |
| compositor blur and input regions | the *shape*, tessellated by `region::placed_shape_to_rects` — and so is the clip, intersected scanline by scanline rather than as a box. The clip *chain* is still collapsed before it arrives (#397) |

What one rect and one matrix cannot express is **two clips in two different
rotated spaces**. `intersect_clips` rebases the second onto the first when
`Transform::rect_stays_rect` holds between them — a shared placement, or a
translation, an axis-aligned scale, a quarter turn or a mirror — so nested
scrollers stay exact, and so does a child turned square-on to its parent — and
falls back to the enclosing box in world space for any other angle.

A quarter turn and a mirror send a rect to a rect, and used to be refused with
the rest because each also moves the top-left corner somewhere else and
`CornerRadii` is four numbers in a fixed order. `Transform::corner_permutation`
reads off where each corner lands and `CornerRadii::permuted` follows it, which
is what Skia's `SkRRect::transform` does under `rectStaysRect` (#397). The
remaining fallback is what *every* case produced before the clip carried a
transform, so nothing is worse than it was. GTK's GSK meets the same wall with
the same single-rounded-rect clip and answers it by rasterising a clip mask;
that is the door out if the fallback is ever seen.

### Clips, and why a command only names one

A clip belongs to the node that declared it. Flatten used to resolve the chain
into one world-space `PlacedShape` and write it into every command below, and
that copy is what a cached subtree could not get rid of: replaying its commands
somewhere else shifted the viewport along with the rows, so a scrolling list
painted straight through its scroller from the first scrolled frame.

So the clips of a frame live in a `ClipTree` (`src/renderer/clip.rs`) and a
command carries a `ClipRef` naming one. Each entry keeps what its clip cuts on
its own, what it cuts under its parent, and which entry that parent is — the
shape of Chromium's clip tree, where draw commands reference clip nodes and
scrolling moves a transform node rather than every clip below it.

Resolution is `FlattenedCommand::clip()`, called at the half dozen places
`render.rs` and `region.rs` read a clip. The five pipelines and every shader are
handed the `PlacedShape` they always were.

### Incremental Flatten

The flattener caches results per node to avoid re-flattening clean subtrees:

```rust
pub struct CachedFlatten {
    pub commands: Vec<FlattenedCommand>,  // Flattened output from this subtree
    pub clips: Vec<CachedClip>,           // The clips this subtree placed
    pub world_transform: Transform,       // World transform at time of caching
    pub opacity: f32,                     // Opacity the subtree inherited then
}
```

When a `RenderNode` has `repainted == false` (reused from paint cache) and both
the cached and current world transforms are translation-only, the flattener
reuses cached commands with a (dx, dy) offset instead of recursing into
children. After a full flatten, results are cached back onto the node for next
frame.

`CachedFlatten::replay_offset` is the whole of the rule for *using* an entry
where the transform is concerned, and a translation is the whole of what it
asks. It used to ask a second question — whether the clip inherited from above
had made the same journey the content did — which is what excluded every
scrolling subtree, and what #441 removed by removing the copy the question was
about.

`CachedFlatten::replay_fade` is the same question about opacity: a fading
ancestor repaints and the subtree under it comes back clean, with the old
inherited opacity multiplied into its commands, so a replay multiplies each by
the ratio of the new to the old. An entry made under an opacity of zero holds
nothing that ratio could recover, and is flattened again instead.

Whether an entry is worth *making* is a second rule, and it is the paint
cache's: an entry is collected only for a subtree with nothing culled under it.
`cache_paint_results` drops a partial paint from the paint cache, so the widget
repaints next frame, so the replay path is never offered its entry — collecting
one is a copy of everything the subtree drew, made for a frame that cannot ask
for it. Flatten computes the same partial-ness the cache walk does, one phase
earlier, by having `flatten_node` return it. In a scrolling list that is the
list itself and every ancestor of it: on `benches/scroll_list` at 5000 rows it
is 45 commands copied per frame instead of 266, for the same 4108 replays
across the run — seventeen a frame.

A clip costs a replay three cases instead, in `ClipTree::replay`. A clip the
subtree placed itself travels with it: translated by the same offset as the
content, then cut afresh by its parent, so a viewport resized above cuts the
new intersection rather than the old one shifted. A clip it inherited is not
its to carry and is looked up where the subtree now finds itself — that is how
a row scrolls while its scroller does not. An overlay's clip answers to no
ancestor and is only carried. The references are the ones the cached commands
already hold, so a replay writes the shapes through them and touches no
command.

That is also why a node that sets a clip of its own is cached like any other
now: its clip is one of the ones it placed.

The cache lives in `RefCell<Option<Rc<CachedFlatten>>>` on the node, so flatten
only needs `&RenderNode` and shallow node clones share the cached output.

## GPU Rendering Pipeline

### Instanced Rendering

The renderer uses instanced rendering for efficiency: a single draw call per layer renders all shapes using one shared unit quad and per-instance data.

### ShapeInstance

Per-instance data for each shape (240 bytes):

```rust
pub struct ShapeInstance {
    pub rect: [f32; 4],           // [x, y, width, height] in physical pixels
    pub corner_radii: [f32; 4],   // [top_left, top_right, bottom_right, bottom_left]
    pub fill_color: [f32; 4],     // RGBA
    pub border_color: [f32; 4],   // RGBA
    pub shadow_color: [f32; 4],   // RGBA
    pub shadow_offset: [f32; 2],
    pub shadow_blur: f32,
    pub shadow_spread: f32,
    pub transform: [f32; 6],      // 2x3 affine matrix [a, b, tx, c, d, ty]
    pub _pad_transform: [f32; 2],
    pub clip_rect: [f32; 4],      // in the clip's own space, logical pixels
    pub clip_radii: [f32; 4],     // in the clip's own space, logical units
    pub clip_inverse: [f32; 6],   // world to clip space, the same 2x3 shape
    pub clip_curvature: f32,
    pub _pad_clip: f32,
    pub gradient_start: [f32; 4],
    pub gradient_end: [f32; 4],
    pub border_width: f32,
    pub shape_curvature: f32,     // superellipse K-value
    pub gradient_type: u32,       // 0=none, 1=horizontal, 2=vertical, 3/4=diagonal
    pub _pad_tail: u32,
}
```

**It arrives as a storage buffer, not as vertex attributes.** The vertex shader
reads `instances[instance_index]`, which is why the struct can hold whatever it
needs to: a vertex layout may declare sixteen attributes and this one had
declared sixteen, so a seventeenth was a pipeline validation error rather than a
cost. Under that ceiling the clip's six floats lived in two places and its
curvature sat in the border block, because padding was the only room left
(#398).

The read is per *vertex* — four per instance, since the quad is four vertices
and six indices — not per fragment: the fragment stage is untouched, because
the vertex output already carries everything it needs as varyings.

It is not slower. Measured end to end — encode, submit and complete a frame —
on an AMD Radeon 860M (RADV) over five scenes from 400 to 40,000 instances,
the storage build finished every one faster than the attribute build it
replaced, by between four and thirteen hundredths of a millisecond per frame.
That harness cannot say where the saving is: it is dominated by frame
submission on this hardware, and the deltas are near-constant rather than
scaling with vertex count, so they are *not* explained by the attribute
fetches that went away. Isolating the GPU's own share would need timestamp
queries, which nothing here has yet. What the numbers support is the direction
and nothing finer.

`ShapeInstance` and `InstanceInput` in `shader.wgsl` are one layout written
twice, field for field and in the same order. The unit test beside the struct
computes the shader's offsets from WGSL's layout rules and checks them against
`offset_of!` on the Rust side, so a field inserted, reordered or resized on
either side fails rather than being read as its neighbour. Nothing could check
the attribute layout that way: the map from field to attribute was a
hand-written offset table that agreed with the struct by arithmetic and with
the shader by comment.

### HiDPI Scaling

Coordinates are scaled to physical pixels during instance creation:

```rust
instance.rect = [rect.x * scale, rect.y * scale, rect.width * scale, rect.height * scale];
instance.corner_radius = radius * scale;
```

**The clip is the exception.** `clip_rect` and `clip_radii` stay in the logical
units the widget declared them in, because the fragment is carried back to meet
them: `PlacedShape::to_local` folds the surface scale into the inverse matrix.
Scaling them here as well would apply the scale twice and clip a HiDPI surface
to a quarter of its viewport. `the_surface_scale_is_undone_once` in
`src/renderer/flatten.rs` is what says so.

### Render Order

1. **Shapes** - Background rectangles, borders, shadows
2. **Images** - Image quads via `ImageQuadRenderer`
3. **Text** - Regular text via glyphon, transformed text via `TextQuadRenderer`
4. **Overlay** - Ripple effects and highlights

## Example: Implementing paint()

```rust
fn paint(&self, ctx: &mut PaintContext) {
    // Get bounds from Tree (single source of truth), through the context that
    // carries both it and the id being painted.
    let bounds = ctx.tree().get_bounds(ctx.id()).unwrap_or_default();

    // Set local bounds (0,0 origin with widget dimensions)
    let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
    ctx.set_bounds(local_bounds);

    // Apply user transform if set (`paint_child` already placed this node)
    if !self.transform.is_identity() {
        ctx.apply_transform_with_pivot(self.transform, self.pivot);
    }

    // Draw background in LOCAL coordinates
    ctx.draw_rounded_rect(local_bounds, self.background, self.corner_radius);

    // Paint children. One call each, and it belongs to the framework: it culls
    // a child nobody can see, serves a clean one from its cache, places it from
    // its bounds in the tree, and opens its own Paint scope. A widget never
    // calls another widget's `paint`.
    for &child_id in self.children.iter() {
        ctx.paint_child(child_id, &ChildPaintOptions::default());
    }

    // Draw overlay effects (after children) in LOCAL coords
    if let Some(ripple) = &self.ripple {
        ctx.set_overlay_clip(local_bounds, self.corner_radius, self.curvature);
        ctx.draw_overlay_circle(ripple.x, ripple.y, ripple.radius, ripple.color);
    }
}
```

## Key Files

| File | Purpose |
|------|---------|
| `src/renderer/tree.rs` | RenderNode, ClipRegion, CachedFlatten |
| `src/renderer/paint_context.rs` | PaintContext API |
| `src/renderer/commands.rs` | DrawCommand enum |
| `src/renderer/flatten.rs` | Tree flattening with transform inheritance |
| `src/renderer/clip.rs` | ClipRef, ClipTree, CachedClip — the clips a frame places |
| `src/renderer/gpu.rs` | ShapeInstance, GPU data structures |
| `src/renderer/render.rs` | Main Renderer, GPU pipeline |
| `src/renderer/shader.wgsl` | WGSL shaders for SDF rendering |
| `src/renderer/text.rs` | Text rendering via glyphon |
| `src/renderer/text_quad.rs` | Transformed text as textured quads |
| `src/renderer/image_quad.rs` | Image rendering |
