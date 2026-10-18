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
    pub opacity: f32,                        // This subtree's fade
    pub opacity_mode: OpacityMode,           // As one layer (Auto) or draw by draw
    pub blur: Option<f32>,                   // Blurs the subtree, as a layer
    pub commands: SmallVec<[Rc<DrawCommand>; 2]>,        // Draw commands (shapes, text, images)
    pub children: Vec<Rc<RenderNode>>,       // Child nodes, Rc-shared with the paint cache
    pub overlay_commands: SmallVec<[Rc<DrawCommand>; 1]>, // Commands drawn after children
    pub clip: Option<ClipRegion>,            // Clips this node and children
    pub overlay_clip: Option<ClipRegion>,    // Clips only overlay commands
    pub overlay_clip_from: usize,            // ... from this one on
    pub children_clip: Option<ClipRegion>,   // Clips only the children, under `clip`
    pub merge: Option<Box<MergeChildren>>,   // Children's backgrounds drawn as one shape
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

// Set clip only for overlay commands (doesn't clip children), and only for
// those drawn after it is set
ctx.set_overlay_clip(rect, corner_radius, curvature);

// Set clip only for the children (doesn't clip this node's own commands)
ctx.set_children_clip(rect, corner_radius, curvature);
```

A container's overflow clip is the last of these. CSS clips overflow at the
padding box and leaves the box's own background, border and shadow alone, and
so does a scroller or an `Overflow::Hidden` container here: it cuts its
children to its box inset by its border width, each corner radius less that
width (floored at zero), the curvature kept — and its own fill, border and
shadow are not cut at all (#764). `set_clip`, which cuts the node's own
commands too, is what `TextInput` uses to keep its text inside its box.

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

// Arc: `sweep` degrees from `start`, clockwise from twelve o'clock, drawn as
// one SDF shape with the stroke's width, colour and cap
ctx.draw_arc(center, radius, start, sweep, stroke);

// Path: filled, stroked or both, tessellated once and kept — see "Paths"
ctx.draw_path(&path, Fill::color(color), Stroke::new(1.6, color));

// Text
ctx.draw_text(text, rect, color, font_size);
ctx.draw_text_styled(text, rect, color, font_size, font_family, font_weight, font_style, font_features, line_height, letter_spacing, fit);
// `fit` is the width the text was measured in, and the cut; without one, the
// text wraps at `rect`'s width.

// Image — a raster Path or Bytes source draws nothing until its decode lands
ctx.draw_image(source, rect, content_fit, tint);
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

Overlay commands are drawn after all children, useful for effects like ripples,
and for a container's outline — a ring above everything the box holds:

```rust
ctx.draw_overlay_circle(cx, cy, radius, color);
ctx.draw_overlay_rounded_rect(rect, color, radius);
ctx.draw_overlay_border_frame(rect, color, radius, width, curvature);
```

The overlay clip cuts only the overlay commands drawn after it was set. A
container draws its outline first and sets the clip its ripples are cut to
after, so the ring, which stands outside the box, is not erased by a press.

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
overlap the overlap shows through. It is what `OpacityMode::Modulate` asks
for, and what the default, `OpacityMode::Auto`, draws whenever none of the
subtree's draws overlap — the same picture, for nothing. Where they do
overlap, the subtree is a layer: see "Layers" below.

A transformed text and an image take the opacity on the vertex rather than into
their texture, because both textures are cached by content — a fade folded into
the colour would rasterise the text again on every frame of it.

### Layers

A container whose `opacity` is below one under `OpacityMode::Auto`, or which
declares a `blur`, asks to be composited as one picture — CSS's `opacity` and
`filter`, Compose's `CompositingStrategy.Auto`. `flatten_as_layer` in
`src/renderer/flatten.rs` answers in three steps.

**Its content is flattened in the node's own space**, after its own transform
and before every one above it, under none of the clips around it and at full
opacity. The scratch opens a group of its own for it, and the clip tree numbers
the clips it places (`ClipTree::enter_segment`), because a layer inside it
interleaves its own clips with them.

**Then the content says what it is.** `scan_layer` takes the box every draw
lands in, and whether two of those boxes meet — the same rect test the draw
groups split on, against the box each command covers. A path both filled and
stroked counts as meeting itself: its stroke is drawn over its fill.

- A fade whose draws never meet needs no texture, and nor does a fade over a
  backdrop blur: in a layer the frost would see only the layer, and pop back
  to frosting the surface when the fade ended. The content is put back where
  the node stands by `elide_layer`: carried out of the node's space by
  its world transform, faded by the node's opacity and every ancestor's, and
  cut by the clips around it — the replay a cached subtree takes, with a
  transform where the offset was (`ClipTree::replay` takes one now). Its clips
  are dropped first and placed again; nothing outside the layer was cut against
  them, since the first was placed under none. A node elided this way is cached
  like any other.
- Anything else is a layer: a declared blur, an overlap, or a layer inside
  it.
  `finish_layer` moves the content into its texture's space — the box it
  covers, a texel of transparent edge round it, its origin on an even texel —
  and out of the surface's groups into a `LayerPass`, and pushes one
  `DrawCommand::Layer` where the node stands: in the image bucket, at the
  node's opacity and every ancestor's, by its world transform, cut by the clips
  around it.

**The content is drawn on the device's pixel grid**, Chromium's way: a node
that is only moved has its content nudged by the fraction of a pixel it stands
off the grid, so the texture's texels land on the surface's pixels and the
layer is as sharp as the content drawn without one. A move by whole device
pixels changes nothing in the content and only lays the texture down again; a
move to a new fraction draws it again. A turned or stretched node is
resampled whatever is done, so its content is drawn unnudged and its own
transform only re-composites.

**The composite is cached like any command.** A subtree holding a layer keeps
its flatten entry, composite and all (`CachedFlatten::holds_layers`), and is
replayed while it moves by whole device pixels. A replay emits no pass — the
layer's content is not among the commands an entry keeps — so the frame lists
the layer in `FlatFrame::kept` and the renderer lays down the texture it already
has. Should that texture not be there to lay down — released, drawn at
another scale, or drawn from pixels still on their way — the loop asks before
drawing anything (`Renderer::lacks_kept_layers`) and paints and flattens the
frame again in full, which hands over the layer's content: no frame is
presented without it. A layer the surface's edge cut, or left out for being
wholly off the surface, holds only where it stands, so its entry replays only
there (`CachedFlatten::cut_by_surface`): moved, it is flattened afresh. A
layer's own clips are left out of the entries of the subtrees above it: they
are in its texture's space, and drawn from it.

What the content says to the compositor is not drawn: an input region, or the
compositor's half of a backdrop blur. Those are left out of the texture and
said again where the layer stands on the surface, under twins of the clips
that cut them (`ClipTree::twin_segment`), so the regions read off the frame are
the ones the same tree publishes without a layer.

The texture is cut to what of the node's space can reach the surface, and to
`MAX_LAYER_TEXELS` a side. A blurred layer reaches the surface from further
off: its cut is widened by the blur's margin (`blur_margin`, the same reach its
composite spreads by), so near the edge the blur reads the content that goes on
past it rather than the texture's last texel smeared outwards. Where the
content itself ends inside that margin the cut ends with it, and the blur fades
it there. The cut is the surface's only — the content was flattened under none
of the clips around it, so a blur inside a clipping parent already reads past
the clip, and only its composite is cut by it.

**The frame carries its layers ahead of the surface.** `FlatFrame::passes`
lists them, a layer inside another first, each with its commands, its groups
and its texture's size in texels; `FlatFrame::surface_layers` is what is left.
The renderer draws every pass into its texture before the surface, so each
composite samples a texture already finished, and draws the composite as a
textured quad through the image pipeline — premultiplied, which is what the
shape and text pipelines leave in a texture cleared transparent.

**A texture is kept while it holds the content.** `LayerTextures`
(`src/renderer/layer_pass.rs`) keeps one per node, and at the start of a frame
compares the pass's commands with the ones it drew — the same command (or an
equal one: a container that repaints for its own opacity draws an equal
background, a new value), the same transform, opacity and clip, at the same
scale and size. A layer inside it drawn again changes it too, and so does an
image that was drawn unsettled — its pixels still on their way — which the
frame they land in may not show in any command. A kept texture costs its
content nothing: its groups are not even prepared.

**A blur runs over the texture.** The backdrop's passes — shrink into a working
texture, then a gaussian each way (`BackdropRenderer::blur_layer`) — read the
content's texture and write a blurred one the composite reads instead, larger
by the blur's reach. They go through the backdrop's own bindings — a slot of
its one uniform buffer per pass, and the bind group kept for each texture — so
a blur that animates makes no buffer and no bind group after its first frame,
and a frame whose only blur is a layer's is not an idle one for them. The shrink follows the radius (`BlurPlan` in
`src/renderer/layer_blur.rs`): none for a small blur, so a two-pixel softening
is not drawn blocky, up to the backdrop's quarter and beyond only where the
kernel would not fit its taps. A blurred texture is kept with the content and
the radius it was blurred by, so a radius that animates blurs again without
drawing again. The container's damage is sized by `blur_reach_bound`, which no
blur at any scale of one or more exceeds, added past everything the texture
holds: the container's own shadow and outline, and whatever its children draw
outside it. The tree keeps that last half apart
(`Tree::set_own_paint_reach_and_spread`), because the children's overhang is
gathered into it on a schedule of its own.

**A backdrop inside a layer filters the layer**, CSS's Backdrop Root: the
effect's scene is the layer's texture rather than the surface's. Only a
declared blur — or a layer holding a layer — puts a frost inside one; a fade
over a frost is drawn draw by draw. A backdrop
replaces what is beneath it; over the opaque surface one straight-alpha blend
does that, but a layer's texture is transparent wherever its content is not, so
there the composite is an erase by its coverage and then an add of the
premultiplied blur (`fs_erase`, `fs_replace`).

**Textures come from a pool** and go back to it: allocated in steps of 64
texels, so a layer whose size animates keeps its texture; a layer its own
surface has not named for `IDLE_FRAMES_BEFORE_RELEASE` of that surface's frames
— the backdrop target's 120 — gives its textures to the pool, and the pool
drops what nobody took after as many again. Its own surface's frames, because
the renderer is shared: a still panel beside a surface animating at full rate
would otherwise lose its texture in two seconds. One no surface has named for
thirty times as long is let go regardless, which is what releases the texture
of a surface that has gone. A blur's two working textures are taken as a pair
of one size (`take_pair`): the blur reads both through the first one's size,
and a pool holding a larger layer's texture once handed it two of different
sizes, which drew the layer stretched (#808).

`group_opacity_fades_an_overlap_as_one_picture`,
`an_opacity_over_nothing_that_overlaps_is_drawn_without_a_layer`,
`a_layer_blurred_by_nothing_is_the_scene_without_one`, `layers_nest_inner_first`,
`a_backdrop_inside_a_layer_frosts_only_the_layer`,
`a_fading_frost_keeps_frosting_what_is_beneath_it`,
`a_layer_off_the_pixel_grid_draws_its_content_on_it`,
`clips_inside_a_layer_cut_where_they_cut_without_one` and
`paths_inside_a_layer_are_drawn_where_they_are_without_one` are the goldens;
`tests/draw_calls_are_counted.rs` counts the passes,
`tests/paint_cache_across_frames.rs` says a fade hands over the content it drew,
and `tests/a_blur_reads_past_the_surface.rs` reads a blur at the surface's edge
back in pixels.
`benches/fade` says what a fade costs a frame, against modulation.

### RenderLayer Ordering

Commands are bucketed by kind so that commands of a kind share a draw call:

```rust
pub enum RenderLayer {
    Backdrop = 0, // Backdrop effects, which open a group
    Shapes = 1,   // Background shapes (rectangles, borders, arcs)
    Paths = 2,    // Filled and stroked paths, as triangles
    Images = 3,   // Image content
    Text = 4,     // Text content
    Overlay = 5,  // Overlay effects (ripples, highlights)
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
    pub backdrop: Range<usize>,
    pub shapes: Range<usize>,
    pub paths: Range<usize>,
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
effects run, and the pass resumes with `LoadOp::Load`. The target is blitted to
the swapchain once at the end and released after enough idle frames.

Per region: downsample into a quarter-size working texture (rendering into a
smaller target with a linear sampler is already a box filter), two separable
gaussian passes ping-ponging between two working textures, then a composite
back over the scene masked by the container's rounded-rect SDF. See
`src/renderer/backdrop_pass.rs`.

**The regions of a group share those passes** (#766). Each is shrunk into its
own slot of the working texture — the place its pixels shrink to, so regions
apart on the scene are apart there — and one downsample, one gaussian each way
serve them all, a draw per region. The composites are drawn in the scene's
pass as it resumes, so a group costs three passes of the backdrop's own
however many frosts it holds. A render pass costs wgpu-core about fifteen
allocations whatever it draws, which is what the frosts were paying for: over
a list of frosted cards a frame went from 843 allocations to 453, against 338
for the same cards unfrosted. Every read of a slot is clamped to its rect in
`src/renderer/backdrop_shader.wgsl`, since its neighbours share the texture.
Only regions whose viewports do not overlap can share — a later one reads what
an earlier one lays back — and two that land on one working texel cannot
either, so `BackdropRenderer::batch_end` closes a batch at the first that
would, and the batches run in paint order. Inside a layer it is the same,
against the layer's texture. `render_stats` counts the passes in
`backdrop_passes`, and `tests/draw_calls_are_counted.rs` holds a group at
three; `frosts_side_by_side_keep_to_their_own_cells` and
`a_frost_over_a_frost_blurs_it_again` are the goldens.

What those passes read is kept across frames, as the shape pipeline's
instances are. Each draw takes the next slot of one uniform buffer, read
through a dynamic offset; the slots are gathered on the CPU and written in a
single `write_buffer` when the frame is finished, and the buffer is replaced by
one twice its size only when a frame needs more slots than it holds — the
frame goes on at the slot it reached, so the next frame like it fits. A bind
group says only which textures a pass reads — the scene or a working texture,
and the coverage mask for a frosted text — so one is made the first time a
texture is bound and kept while frames go on binding it, and let go after a
frame that blurred without it. A frame that blurs what the last one blurred
creates neither: `backdrop_allocations` in `render_stats` counts both, and
`a_frame_that_blurs_its_backdrop_again_makes_no_buffer_and_no_bind_group` in
`tests/draw_calls_are_counted.rs` holds it at zero.

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

One corner, five copies. WGSL has no include, so the signed distance function
for a superellipse corner is written out in `shader.wgsl`, in
`textured_quad_shader.wgsl`, in `backdrop_shader.wgsl` and in
`path_shader.wgsl`; the fifth is
`Rect::shape_distance` in `src/widgets/widget.rs`, in Rust, and it is what
hit-testing runs — so it decides where a *click* lands the way the other four
decide where a pixel lands.

The four shaders are kept character-for-character identical between the
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

**The clips are shapes here too** — every one a command is cut by, its first
and each down its chain. At any scanline the overlap is the overlap of all
their spans — exact, turned clips or not, and it gets the corners right for
free: a card filling a rounded scroller is cornered by the scroller, where a box
intersection would publish the scroller's square corners.

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
`FlattenedCommand::clip()` is what resolves it to the `ResolvedClip` every
pipeline takes: a first `PlacedShape`, and the index of the rest of its chain
in the frame's `ClipChains` — see "Clips inside clips" below. The reads in
`render.rs` and `region.rs` are the only places that resolution happens.

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
| transformed text (`text_quad.rs`) | the same map again, on the same quad — it is the image pipeline with a glyph texture in it, and it takes the same `QuadClip::resolved` |
| upright text (glyphon) | the box every clip of the chain leaves open — `TextBounds` is four integers and glyphon has nowhere to put a shape. Exact all the same: a text that any one of those boxes would cut differently from its shape is routed to the quad above instead, which is what `PlacedShape::box_cuts_like_the_shape` decides (#405) |
| the backdrop pass | both: the box every clip leaves open narrows the viewport, because a viewport is four integers, and then each fragment is carried into each clip's own space and tested there, so a rounded scroller keeps its corners |
| compositor blur and input regions | the *shape*, tessellated by `region::placed_shape_to_rects` — and so is every clip, intersected scanline by scanline rather than as a box |

What one rect and one matrix cannot express is **the intersection of most
pairs of clips** — which is why a clip under another is kept as two shapes
unless one is exactly enough. That is the next section.

### Clips inside clips

A clip placed under another meets it in `ClipTree::cut`, and the two are
**merged only when one rounded rect is exactly their intersection** —
`merge_exact` in `src/renderer/clip.rs`, which is Skia's
`SkRRectPriv::ConservativeIntersect`. The inner clip has to be rewritten in the
outer's space by a map that sends a rect to a rect and a circle to a circle: a
shared placement, a translation, a uniform scale, a quarter turn or a mirror,
with `Transform::corner_permutation` moving the radii the way Skia's
`SkRRect::transform` does (#397). Then the merge is the two boxes intersected,
and each corner of that box is read off its corner point. A clip whose shape
contains the point does not cut there; a clip that does not must own the
corner — its own corner sits on the point — or the cut is a curve no corner can
draw. Where both own it, one has to nest in the other, a radius no smaller and
a curve no rounder, and that one is the corner; a scoop nests only in a larger
scoop. The corners that end up rounded have to share one curvature, and none
can be larger than half the intersection's shorter side, where the shader
would clamp it.

**Everything else is chained.** The clip keeps its own shape as `first`, and
the parent's resolved first shape is pushed onto the front of the parent's
chain as a `ClipLink`; the clip's `rest` names that link. The links of a frame
are one persistent cons-list in `ClipChains`: a parent is pushed once however
many of its children chain to it, and they all share its tail. It is rebuilt
every frame on both flatten paths, as the clips are: a chain index means
something in the frame that made it and in no other. The box every shape of a
chain leaves open is worked out there too, as `ResolvedClip::open`, for the
consumers that can take only a box.

| consumer | how it walks a chain |
| --- | --- |
| the shape shader | `ShapeInstance::clip_chain`, in the four bytes that padded `gradient_start`, names the first link. The renderer uploads the frame's links as `ClipRecord`s — rect, radii, the physical-to-clip map, curvature and the next index, 64 bytes — into one storage buffer bound at group 1, and the fragment walks it from `@builtin(position)` only when the chain is not empty |
| images and transformed text | the same, through a `Uint32` vertex attribute in what used to pad the curvature and opacity to a `Float32x4` — the vertex stays 88 bytes — and the same records bound at group 1 |
| upright text (glyphon) | the box all the shapes leave open, and the quad as soon as any one of them would cut the text differently from its box |
| the backdrop pass | the viewport is the box all the shapes leave open; the chain head rides in what was padding in `Params`, and the composites walk it |
| compositor regions | every shape, a scanline at a time |

The shaders combine a chain by the **largest distance, each in pixels of its
own gradient**, and smooth once: `max(d / fwidth(d))`. A product of coverages
would darken every edge two clips share, which is every edge a scroller shares
with the card it fills. An instance with no chain takes the code path it always
did, so a frame without one costs a branch. The walk is a block every shader
carries word for word, held there by `tests/shader_sdf_is_one_definition.rs`,
and capped at 64 links so a broken chain cannot hang the GPU.

The alternatives, and why not: a stencil clip (Flutter's Impeller) needs a
depth-stencil attachment and breaks batching across clips; a fixed number of
clips inline in the instance is exact only up to that number and widens every
instance; a rasterised clip mask (GTK's GSK) is a pass and a texture per
clipped subtree. `nested_clips_cut_exactly` (at 1x and 2x),
`nested_clips_cut_images_exactly`, `nested_clips_cut_text_exactly` and
`backdrop_blur_is_cut_by_a_chain_of_clips` are the goldens; the unit tests
beside `merge_exact` hold the merge, both ways round.

What nesting costs a frame is `benches/nested_clip`: rounded cards whose child
clips again, both where the two clips merge exactly and where they do not.

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

Resolution is `FlattenedCommand::clip()`, called at the places `render.rs` and
`region.rs` read a clip, and what it returns is the `ResolvedClip` the section
above describes.

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

The surface root never makes one either. Its node is `clear()`ed before every
paint, which empties its entry and marks it repainted, so no frame is ever
offered what was kept for it — and its subtree is the whole frame, so the entry
was a copy of every command drawn, on every painted frame of a surface that
culls nothing. `flatten_root_into` flattens the root with `replayable` off;
everything below it is flattened as before.

A clip costs a replay two cases instead, in `ClipTree::replay`. A clip the
subtree placed itself travels with it: translated by the same offset as the
content, then cut afresh by its parent, so a viewport resized above cuts the
new intersection rather than the old one shifted. A clip it inherited is not
its to carry and is looked up where the subtree now finds itself — that is how
a row scrolls while its scroller does not. An overlay's clip is the first kind:
a ripple is cut by its own node's shape and by every clip above it, so it
stops at a rounded scroller's arc (#708). The references are the ones the cached commands
already hold, so a replay writes the shapes through them and touches no
command.

That is also why a node that sets a clip of its own is cached like any other
now: its clip is one of the ones it placed.

The cache lives in `RefCell<Option<Rc<CachedFlatten>>>` on the node, so flatten
only needs `&RenderNode` and shallow node clones share the cached output.

### Merged shapes

`Container::merge_children` draws the backgrounds of a container's direct
children as one shape. The decision and its prior art are in #733; this is how
it is built.

**What a member is.** A container paints its own box — the fill, and the border
frame it draws as a second command — inside `PaintContext::background`, which
marks those `RoundedRect`s with `background: true`. The mark is in the command,
as a `ClipRef` is, so it survives every cache a command passes through. A
container that merges sets `RenderNode::merge` to a `MergeChildren`: the
distance, and its own border and shadow, which the merged shape takes.

**Where it is built: flatten.** `merge::gather` reads each direct child's node
for the commands it marked, places them by the child's own `placement` — the
same transform flatten composes onto the parent's world transform — and emits
one `DrawCommand::MergedShape` after the container's own commands and before its
children's. Each child is then flattened as usual, and the commands the merged
shape stands in for are taken back out of what it emitted, by address
(`FlattenScratch::take_out_since`). Taken out afterwards rather than left out:
the child's own flatten entry is made inside that call, and it has to hold the
background, because a later frame may replay the child under a parent that no
longer merges it.

That is also all the paint cache needs. A member that moves repaints, and
`mark_needs_paint` marks its ancestors, so the merging container repaints and
flatten gathers again — from a member that moved and from members that came
back out of the paint and flatten caches alike, since a replay hands back the
same `Rc`s. A group that moves as a whole replays its cached merged command with
the offset, because the members are placed relative to the command's own world
transform. `a_member_that_slides_moves_the_merged_shape` and
`a_group_that_moves_replays_its_merged_shape` in
`tests/paint_cache_across_frames.rs` hold both.

**How it is drawn.** One instance, over the members' box grown by a quarter of
the distance — a smooth minimum of reach `k` takes at most `k / 4` off the
nearer distance, so the neck never reaches further. `ShapeInstance::merged`
builds it: no transform, `shape_kind` `SHAPE_MERGED`, and — since a merged shape
has no gradient — `gradient_type` naming the first of its members'
`MergeRecord`s, written per frame into a storage buffer bound at group 0,
binding 2 and chained by `next` as a clip chain is. Each record carries what a
clip record does — the rect, the radii, the curvature and the
physical-to-member map — plus `to_pixels`, which brings each member's distance
to physical pixels so they can be compared, the premultiplied fill and border
colour, and the border width.

**Its own fragment entry point.** Merged instances are drawn by a second
pipeline: the same `vs_main`, and `fs_merged` in place of `fs_main`. A shape
drawn alone runs the fragment shader it ran before merging existed, with no
loop and no record in it to size its registers by. The two share what follows
the distance — the functions that cast the shadow, fill and border the shape,
leave the shadow out from under it and cut it by its clip — so both are
composited by one definition. A group's shapes are one draw as before; only a
group holding a merged shape is drawn in runs, a draw per change of pipeline,
so a frame that merges nothing issues the draws it always did.

`fs_merged` folds the members with the quadratic polynomial smooth minimum,
which leaves two distances further apart than its reach exactly as `min` leaves
them — a member that moves away splits off by itself — and gives, beside the
distance, the share of the fragment each side keeps. The fill, the border
colour and the border width blend by that share, so the border follows the
merged outline and fades along a neck towards a member without one. The shadow
is evaluated over the merged distance: the same SDF through the same
`smoothstep` a lone shape's shadow uses, and it is left out from under the
merged shape as a lone shape's is from under that shape. The merging
container's outer shadows are all the merged shape's, each taking an instance
as a box's do: the first rides the merged instance, and every further one is
the merged shape drawn again beneath it with `SHADOW_ONLY` set, for that shadow
alone. Its inset shadows stay on its own box. A member's own outer shadows are moved
beneath the merged shape; its inset ones stay with the member, over the merged
fill. The shape and its shadow take the same
clip any instance takes.

**Damage.** A member that moves changes pixels outside its own box: the neck,
its neighbour's outline leaning towards it, and the merged shadow. So a
container that merges publishes `Tree::set_merge_reach` — the furthest a merged
shape reaches past what its children paint, five quarters of the distance plus
the shadow's extent — and `mark_needs_paint` on a child of such a container
damages the container's whole area as well: its box, grown by its paint reach
and that merge reach. Coarse, and always enough, which is the bargain Chromium's
damage tracker makes for damage under an ancestor's effect. A merge reach that
shrinks damages the area it vacates before it goes, as a paint reach does.

**Limits.** At most `MAX_MEMBERS` (16) members. A member with a gradient, and any
child past the limit, keeps its own background and is reported once per
container and reason in debug builds (`merge_member_left_out`). So does a child
drawn as a layer, which only its own flatten knows: `flatten_as_layer` builds a
texture or elides it once it has scanned what the child drew. A composited
child's background has moved into its layer by the time `take_out_since`
searches for it, so a merged shape that kept it would draw it twice, sharp
under a blur and faded twice under a fade. So once each member is flattened,
`FlattenScratch::composited_since` asks whether its `DrawCommand::Layer` came
out — drawn this frame or replayed — and the members that were composited are
let go: the merged command already in the frame is put back as
`Gathered::without` them, and their shadows beneath it taken out. A fade that
elides its layer, with nothing over the background, stays a member, faded
there, and keeps its neck. `a_member_that_fades_over_nothing_keeps_its_neck`
and `a_member_composited_leaves_the_merged_shape_and_comes_back` in
`tests/paint_cache_across_frames.rs` hold both across frames. The neck takes no
input: hit testing still asks each child's own box. The goldens are
`merged_pills_at_three_gaps`, `merged_pills_apart_are_the_pills_alone`,
`merged_borders_blend_along_the_neck` (and at 2x),
`a_gradient_member_is_drawn_unmerged`, `a_layer_member_draws_as_it_does_unmerged`,
`a_merged_shape_casts_one_shadow`,
`a_merge_at_distance_zero_is_a_plain_union` and
`a_merged_shape_is_cut_by_its_clip`. A child at no opacity is not a member: it
draws nothing, and would otherwise pull its neighbours' outline towards where it
would be.

The reach is the distance, and the join is at half of it: at the midpoint
between two members both distances are half the gap, and the smooth minimum
takes at most a quarter of its reach off, so the outline meets itself there once
the gap is half the distance. Between the two the outlines lean towards each
other without touching.

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
    pub clip_chain: u32,          // the rest of the clip's chain, or NO_CHAIN
    pub gradient_start: [f32; 4],
    pub gradient_end: [f32; 4],
    pub border_width: f32,
    pub shape_curvature: f32,     // superellipse K-value
    pub gradient_type: u32,       // 0=none, 1=horizontal, 2=vertical, 3/4=diagonal
    pub shape_kind: u32,          // a rounded rect, an arc and its cap, or merged; flag bits above
}
```

**One shadow per instance.** A container casting several takes an instance for
each shadow past the first: the first outer shadow rides the fill's own
instance, every other outer one is that instance again with `SHADOW_ONLY` set,
drawn before it for its shadow alone, and every inset one an instance drawn
after it and before the border frame — so a box with one shadow costs what it
always did, and only a box that asks for more pays for more. Every comparable
renderer that draws a list of box shadows draws one primitive per shadow; a
fixed array in the instance would have widened every shape drawn, shadow or
none, by the three it rarely uses.

An outer shadow is drawn outside its shape alone, as CSS draws a `box-shadow`
outside the border box, so a translucent fill shows what is behind it and not
the shadow. Drawn with its shape, the shadow is weighted by `1 - coverage`, the
shape's edge antialiased as its fill is. Drawn apart, beneath the shape, it has
to come out the same once the shape is drawn over it, which is why a
`SHADOW_ONLY` instance carries the fill rather than a transparent one: it draws
`(1 - coverage) / (1 - alpha)` of the shadow, `alpha` the fill's at that
fragment. Under an opaque fill that is the whole shadow along the edge, as it
was before, and a cut that ignored the fill would show there as a light seam.

An inset instance carries the container's border width with a transparent
border colour: the shader casts the shadow inside `dist + border_width`, the
edge the border frame's inner side is drawn on, which is where CSS casts one.
Whether a shadow is inset is the `SHADOW_INSET` bit of `shape_kind`, above the
byte the kinds use: the arcs had taken the tail padding, and every inter-stage
location was already spoken for.

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

### Textured Quads

Images and transformed text are textured quads, drawn by
`TexturedQuadPipeline` (`src/renderer/textured_quad.rs`). A quad owns no GPU
object of its own:

- **Its corners go into the frame's vertex buffer.** Every quad a frame
  prepares is pushed into one `Vec` of vertices, four each, uploaded once
  before the pass opens into a buffer that grows only when a frame holds more
  quads than any frame before it. A shared index buffer draws quad `k` from
  vertices `4k..4k + 4`.
- **Its bind group is its texture's.** It is made when the texture is cached
  and lives with it, so a frame that draws a cached texture again makes
  nothing.
- **A run is one draw call.** Consecutive quads that sample through the same
  bind group and sit next to each other in the vertex buffer are one
  `draw_indexed`. A quad that samples something else ends the run, even if a
  later quad could have joined it, because drawing that one early would draw
  it underneath the quad between.

**Small images share an atlas page** (`src/renderer/image_atlas.rs`), which is
what makes runs long. An image whose texture is at most `MAX_ENTRY` (256) texels
a side goes onto a `PAGE` (1024) square page, allocated with `etagere`; a
larger one keeps a texture of its own and ends the run it lands in. A full page
opens another, and one with nothing left on it is dropped at the next trim.
Nothing is ever copied between textures, and the shader samples a plain 2D
texture either way. Each entry is surrounded by one texel repeating its own
edge, which is what the clamped bilinear sampler read outside a texture of its
own, so a packed image's edges look as they did.

`tests/draw_calls_are_counted.rs` holds the counts — seven small images are one
call, small, large, small is three, and a second frame of cached textures makes
no buffer and no bind group — and `atlas_neighbours_do_not_bleed` the edges.
`benches/icon_grid` says what a scrolling screen of them costs.

### Arcs

An arc is a shape, not a path: `DrawCommand::Arc` becomes one `ShapeInstance`
with `shape_kind` naming it and its cap, and the shape shader draws it with a
distance function of its own in place of `rounded_rect_sdf`. Its numbers ride in the corner radii —
the radius of the line's centre, half its width, the angle halfway along it and
half the angle it spans — since a rect is never an arc and the instance stays
240 bytes; `shape_kind` was the padding that rounded it out. The fragment is
turned so the arc's middle points one way and folded about it, and every arc
is then the one symmetric case Inigo Quilez's sdArc (round caps) and sdRing
(butt) solve, with a box added past each end for a square cap.

That is what makes a progress ring cheap: a sweep that eases every frame is a
new number in an instance, never a new tessellation, and the edge is exact at
any scale because the distance is measured per fragment. Impeller draws its
simple shapes the same way.

A dashed arc is the exception: `draw_arc` builds it as a path — one cubic per
quarter turn, `Path::arc` — and hands it to `draw_path`, so it is cut and
tessellated as every dashed path is. An SDF has no arc length to dash by
without a shader route of its own, which #755 left for a measurement to ask
for.

### Paths

A path is filled and stroked into triangles by lyon
(`src/renderer/path_mesh.rs`) and drawn by a pipeline of its own
(`src/renderer/path_pipeline.rs`, `path_shader.wgsl`). The choices, and why:

- **No MSAA.** Every pipeline here draws one sample, and an MSAA target would
  change all of them and every offscreen texture besides. GPUI's history is the
  warning: VRAM and crashes from an MSAA path atlas, and slowness on Intel from
  full-frame MSAA.
- **A fringe instead.** lyon does no anti-aliasing, so guido draws the edge:
  the boundary of lyon's mesh — the edges one triangle uses, which is the
  outline with holes, crossings and joins already resolved, and the inside of
  each is wherever its own triangle is — is moved half a physical pixel in
  along each vertex's miter normal, and a twin of each boundary vertex goes
  half a pixel out at coverage zero. Coverage is interpolated across the quad
  between them, so it ramps from one to zero over the pixel the edge crosses.
  egui feathers its shapes this way, and Skia's non-MSAA path renderers draw
  the same alpha ramp. A stroke thinner than a pixel is tessellated a pixel
  wide at a coverage that keeps its weight.
- **Cached by content and scale.** A mesh is kept, keyed by a hash of the
  path's segments, its fill rule or stroke geometry and dashes, and the
  surface scale —
  the fringe is a physical pixel, so a new scale is a new mesh. The colour and
  the transform are not in the key: they are the draw's `PathRecord`, which
  the vertex shader reads, so a path that moves, turns, fades or changes colour
  draws the mesh it has. A transform's own scale scales the fringe with it.
  A mesh no frame has drawn for 120 frames is dropped.
- **Dashes cut before the stroke.** A dashed `Stroke` is cut on the CPU,
  after the curves are flattened and before lyon strokes it: the walk along
  the flattened path (`dash` in `path_mesh.rs`) emits each dash as an open
  subpath that keeps every vertex it passes, so each is capped at its two ends
  and joined at its corners as a solid line would be. On a closed subpath the
  last dash runs into the first, as Skia and cairo draw it. iced, Vello and
  GTK dash on the CPU the same way; Skia's shader route is for one straight
  segment only. lyon's own walk along a path reports only where the pattern
  changes, which would turn a dash's corners into chords — iced's dashes do.
  The pattern and the offset are part of the mesh key, the offset kept modulo
  the pattern's period, so an offset that marches is a tessellation a frame
  for that one path until it comes back round. A pattern that would cut a
  path into more than 200 000 intervals draws it solid, as Skia gives up past
  a million.
- **Its own layer.** `RenderLayer::Paths` sits between the shapes and the
  images, so a group's paths are one indexed draw however many shapes lie
  between them, and a shape painted over a path opens a group as one painted
  over an image does.
- **The clip as every pipeline takes it.** A record carries its clip as the
  first `ClipRecord` of the chain, and the fragment stage cuts by it and walks
  the rest, from the same shared block every shader carries.

What the fringe cannot undo is lyon's own overlap: a stroke whose segments are
shorter than it is wide folds the inside of its sharpest joins back over
itself, and a translucent line is drawn twice there — a handful of pixels in
`a_dense_translucent_sparkline`, against none at ordinary densities.

Each frame copies the meshes it draws into one vertex and one index buffer,
tagged with their record's index. The copy is linear in vertices and allocates
nothing once the buffers have grown; keeping meshes on the GPU instead would
save it at the cost of a buffer per mesh.

`a_sparkline_has_one_tone_at_every_join`, `closed_paths_are_filled`,
`a_stroke_follows_its_transform`, `a_path_is_cut_by_a_rounded_clip` and
`dashed_strokes_keep_their_caps_and_joins` are the goldens;
`benches/sparkline_ring` is the workload, and its `path.tessellated` and
`path.reused` rows say how often a frame made a mesh rather than drawing one
again. Its marching outline is one tessellation every painted frame, which is
what an animated dash offset costs.

### Render Order

1. **Shapes** - Background rectangles, borders, shadows, arcs
2. **Paths** - Path triangles via `PathRenderer`
3. **Images** - Image quads via `ImageQuadRenderer`
4. **Text** - Regular text via glyphon, transformed text via `TextQuadRenderer`
5. **Overlay** - Ripple effects and highlights

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
| `src/renderer/merge.rs` | MergeChildren, gathering a container's members, MergeRecord |
| `src/renderer/render.rs` | Main Renderer, GPU pipeline |
| `src/renderer/shader.wgsl` | WGSL shaders for SDF rendering |
| `src/renderer/text.rs` | Text rendering via glyphon |
| `src/renderer/text_quad.rs` | Transformed text as textured quads |
| `src/renderer/image_quad.rs` | Image rendering |
| `src/renderer/image_atlas.rs` | The pages small images are packed onto |
| `src/renderer/path.rs` | `Path`, `Fill`, `Stroke`: what a widget draws |
| `src/renderer/path_mesh.rs` | lyon's tessellation, and the fringe that anti-aliases it |
| `src/renderer/path_pipeline.rs` | The mesh cache, the frame's triangles, the path pipeline |
| `src/renderer/textured_quad.rs` | The textured-quad pipeline, the frame's quad vertices, runs |
| `src/renderer/layer_pass.rs` | A layer's texture across frames, and the pool |
| `src/renderer/layer_blur.rs` | How a layer's blur is run, and how far it reaches |
