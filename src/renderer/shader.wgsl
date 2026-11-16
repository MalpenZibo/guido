// Guido Instanced Shader - SDF-based shape rendering
//
// This shader uses instanced rendering with a shared unit quad.
// All shape parameters are passed per-instance instead of per-vertex,
// significantly reducing memory usage and draw calls.

// === Uniforms ===

struct Uniforms {
    screen_size: vec2<f32>,  // Logical pixels
    scale_factor: f32,       // HiDPI scale
    _pad: f32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// === Vertex Input (unit quad) ===

struct VertexInput {
    @location(0) position: vec2<f32>,  // 0..1 range
}

// === Instance Input ===

// One per shape, read by `@builtin(instance_index)` from a storage buffer
// rather than fetched as vertex attributes.
//
// **This struct and `ShapeInstance` in `gpu.rs` are one layout written
// twice**, field for field and in the same order. The unit test beside that
// struct computes this one's offsets from WGSL's layout rules and checks them
// against the Rust ones, and fails if they stop agreeing. The Rust side
// carries explicit `_pad` members where this one is padded implicitly; they
// are the only thing the two spell differently.
//
// Vertex attributes could not be checked that way. There were sixteen of
// them — the whole allowance — and the map from struct field to attribute was
// a hand-written offset table in `desc()` that agreed with the struct by
// arithmetic and with this file by comment (#398).
struct InstanceInput {
    // rect: [x, y, width, height] in logical pixels
    rect: vec4<f32>,
    // corner radii: [top_left, top_right, bottom_right, bottom_left]
    corner_radii: vec4<f32>,
    fill_color: vec4<f32>,
    border_color: vec4<f32>,
    shadow_color: vec4<f32>,
    shadow_offset: vec2<f32>,
    shadow_blur: f32,
    shadow_spread: f32,
    // a, b, tx, c, d, ty — row-major 2x3, origin already baked in
    transform: array<f32, 6>,
    // clip_rect: [x, y, width, height] in the clip's own space, logical pixels
    clip_rect: vec4<f32>,
    // clip corner radii, in the clip's own space and logical units
    clip_radii: vec4<f32>,
    // world (physical pixels) to clip space, the same row-major 2x3
    clip_inverse: array<f32, 6>,
    clip_curvature: f32,
    // where the clip continues in `clip_records`, or NO_CHAIN
    clip_chain: u32,
    gradient_start: vec4<f32>,
    gradient_end: vec4<f32>,
    border_width: f32,
    shape_curvature: f32,
    // 0=none, 1=horizontal, 2=vertical, 3=diagonal, 4=diagonal_reverse
    gradient_type: u32,
    // SHAPE_ROUNDED_RECT, or an arc and its cap — see `arc_sdf` — in the low
    // byte, and SHADOW_INSET above it where the shadow is cast inside
    shape_kind: u32,
}

@group(0) @binding(1) var<storage, read> instances: array<InstanceInput>;

// The frame's clip chains — see the shared clip-chain block below.
@group(1) @binding(0) var<storage, read> clip_records: array<ClipRecord>;

// === Vertex Output ===

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) fill_color: vec4<f32>,
    @location(1) border_color: vec4<f32>,
    // Fragment position in physical pixels (for SDF computation)
    @location(2) frag_pos: vec2<f32>,
    // Shape rect in logical pixels [x, y, width, height]
    @location(3) shape_rect: vec4<f32>,
    // corner radii: [top_left, top_right, bottom_right, bottom_left]
    @location(4) shape_radii: vec4<f32>,
    // border_width, shape_curvature, and the fade across a vertical side and
    // across a horizontal one — `antialiasing_reach`, which is half of
    // `fwidth(frag_pos)` exactly, since the instance's transform is affine
    @location(5) @interpolate(flat) border_params: vec4<f32>,
    // shadow_offset.xy, shadow_blur, shadow_spread
    @location(6) shadow_params: vec4<f32>,
    // shadow_color
    @location(7) shadow_color: vec4<f32>,
    // This fragment in the clip's own space — the vertex shader has already
    // undone the clip's placement. The map is affine, and interpolating an
    // affine function of position across a triangle gives the same answer as
    // applying it to the interpolated position, so the matrix need not reach
    // the fragment stage at all: four varyings fewer, and the multiply happens
    // three times per triangle instead of once per covered pixel. Exact, not
    // an approximation — which is the same argument the image quad makes for
    // mapping its four corners on the CPU.
    @location(8) clip_pos: vec2<f32>,
    // Clip rect in the clip's own space, logical pixels
    @location(9) clip_rect: vec4<f32>,
    // clip curvature
    @location(10) @interpolate(flat) clip_curvature: f32,
    // Gradient start color
    @location(11) gradient_start: vec4<f32>,
    // Gradient end color
    @location(12) gradient_end: vec4<f32>,
    // Gradient type (0=none, 1=horizontal, 2=vertical, 3=diagonal,
    // 4=diagonal_reverse), and the shape kind with its SHADOW_INSET bit. One
    // location for both: the
    // sixteen this stage may hand on are all spoken for.
    @location(13) @interpolate(flat) kinds: vec2<u32>,
    // clip corner radii, in the clip's own space
    @location(14) clip_radii: vec4<f32>,
    // where the clip continues in `clip_records`
    @location(15) @interpolate(flat) clip_chain: u32,
}

// === Helper Functions ===

// Apply 2x3 affine transform directly
// Note: The transform matrix already includes center_at from CPU side,
// so we just apply it directly without additional origin handling.
fn apply_transform(
    pos: vec2<f32>,
    a: f32, b: f32, tx: f32,
    c: f32, d: f32, ty: f32
) -> vec2<f32> {
    return vec2<f32>(
        a * pos.x + b * pos.y + tx,
        c * pos.x + d * pos.y + ty
    );
}

// Convert logical pixels to NDC
fn to_ndc(pos: vec2<f32>) -> vec2<f32> {
    let ndc_x = (pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y / uniforms.screen_size.y) * 2.0;  // Y flipped
    return vec2<f32>(ndc_x, ndc_y);
}

// How far the fragment stage's fade can reach past the shape's box, in the
// instance's own space, across its left and right sides and across its top and
// bottom, where the linear part of its transform is `a b / c d`. The fade runs
// `edge_fade` either side of an edge, half its distance's `fwidth`: half a
// device pixel's extent along the edge's normal — measured here, from the
// inverse of the transform, along each axis. Half a unit each way upright and
// unscaled, more under a turn or a shrink. A square corner fades as its two
// sides do, so it reaches no further; a rounded one bends away from the box's
// corner, which leaves it the room. A transform that collapses draws nothing,
// and reaches nothing. `ANTIALIASING_REACH` in `gpu.rs` bounds this in device
// pixels under a turn or a uniform scale; a shear's acute corners reach past it.
fn antialiasing_reach(a: f32, b: f32, c: f32, d: f32) -> vec2<f32> {
    let det = abs(a * d - b * c);
    if (det <= 0.0) {
        return vec2<f32>(0.0);
    }
    return 0.5 * vec2<f32>(abs(d) + abs(b), abs(c) + abs(a)) / det;
}

// === Vertex Shader ===

@vertex
fn vs_main(vertex: VertexInput, @builtin(instance_index) index: u32) -> VertexOutput {
    let instance = instances[index];
    var out: VertexOutput;

    // Extract transform components
    // Note: transform already includes center_at from CPU, so no origin handling needed here
    let a = instance.transform[0];
    let b = instance.transform[1];
    let tx = instance.transform[2];
    let c = instance.transform[3];
    let d = instance.transform[4];
    let ty = instance.transform[5];

    // Expand quad for shadow if needed
    let shadow_blur = instance.shadow_blur;
    let shadow_spread = instance.shadow_spread;
    let shadow_offset = instance.shadow_offset;
    // An inset shadow is drawn inside the shape, so its quad stays the shape's.
    let has_shadow = instance.shadow_color.a > 0.0
        && (instance.shape_kind & SHADOW_INSET) == 0u;

    // Calculate shadow expansion (3x blur for smooth fadeout)
    let fadeout = 3.0;
    var expand = vec4<f32>(0.0, 0.0, 0.0, 0.0);  // left, top, right, bottom
    if (has_shadow) {
        expand.x = max(shadow_blur * fadeout - shadow_offset.x, 0.0) + shadow_spread;
        expand.y = max(shadow_blur * fadeout - shadow_offset.y, 0.0) + shadow_spread;
        expand.z = max(shadow_blur * fadeout + shadow_offset.x, 0.0) + shadow_spread;
        expand.w = max(shadow_blur * fadeout + shadow_offset.y, 0.0) + shadow_spread;
    }
    // The fragment stage fades an edge over `edge_fade` either side of it, so
    // the quad reaches as far past the box as that fade can, or its outer
    // half is never drawn (#836). A shadow reaching less far on a side, or a
    // negative spread, leaves this.
    // A hair short of the reach: the rasterizer draws a pixel whose centre
    // lies on the quad's left or top edge, and on the pixel grid that pixel is
    // half a pixel out, where the fade has just reached nothing. Short of it, a
    // box on the grid is drawn over its own pixels and no more.
    let reach = antialiasing_reach(a, b, c, d);
    expand = max(expand, reach.xyxy * 0.99);

    // Compute expanded quad bounds
    let quad_min = vec2<f32>(
        instance.rect.x - expand.x,
        instance.rect.y - expand.y
    );
    let quad_max = vec2<f32>(
        instance.rect.x + instance.rect.z + expand.z,
        instance.rect.y + instance.rect.w + expand.w
    );
    let quad_size = quad_max - quad_min;

    // Transform unit quad [0,1] to shape position
    let local_pos = quad_min + vertex.position * quad_size;

    // Apply instance transform (matrix already includes center_at from CPU)
    let world_pos = apply_transform(local_pos, a, b, tx, c, d, ty);

    // Convert to NDC for clip position
    let ndc = to_ndc(world_pos);
    out.clip_position = vec4<f32>(ndc, 0.0, 1.0);

    // Pass fragment position (local, untransformed) for SDF
    // We interpolate the LOCAL position, not world position
    out.frag_pos = local_pos;

    // Into the clip's own space, where the shape it was declared as is an
    // ordinary rounded rect again — turned, scaled or neither. Identity for a
    // clip already in world coordinates, which is most of them.
    out.clip_pos = apply_transform(
        world_pos,
        instance.clip_inverse[0], instance.clip_inverse[1], instance.clip_inverse[2],
        instance.clip_inverse[3], instance.clip_inverse[4], instance.clip_inverse[5]
    );

    // Pass instance data to fragment shader
    out.fill_color = instance.fill_color;
    out.border_color = instance.border_color;
    out.shape_rect = instance.rect;
    out.shape_radii = instance.corner_radii;
    out.border_params = vec4<f32>(instance.border_width, instance.shape_curvature, reach);
    out.shadow_params = vec4<f32>(instance.shadow_offset, instance.shadow_blur, instance.shadow_spread);
    out.shadow_color = instance.shadow_color;

    // Pass clip data to fragment shader
    out.clip_rect = instance.clip_rect;
    out.clip_curvature = instance.clip_curvature;
    out.clip_radii = instance.clip_radii;
    out.clip_chain = instance.clip_chain;

    // Pass gradient data to fragment shader
    out.gradient_start = instance.gradient_start;
    out.gradient_end = instance.gradient_end;
    out.kinds = vec2<u32>(instance.gradient_type, instance.shape_kind);

    return out;
}

// === SDF Functions ===

// === SHARED SDF — four copies, kept identical by
// `tests/shader_sdf_is_one_definition.rs` ===
//
// WGSL has no include, so the corner geometry lives once per pipeline that
// cuts a shape: the shape shader, the textured quad (images and transformed
// text), the backdrop composite, and the path triangles. A clip is one shape
// and a corner is one curve; four spellings of them is four answers to the
// same question, which is what #403 was — the backdrop read `k` as
// `max(k, 0.05) * 2` where these read `pow(2, k)`, so a bevel blurred a
// concave notch inside its own octagonal border.
//
// A fifth copy is in Rust: `Rect::shape_distance` in `src/widgets/widget.rs`,
// which is what hit-testing runs. Changing the curve here means changing it
// there, or a click stops landing where the pixel is. The test pins that one
// to the geometry rather than to this text.
//
// Edit one and the test tells you to edit the others. Do not reword the
// comments inside this block; the test compares it character for character.

// Convert CSS-style K value to superellipse exponent n
fn k_to_n(k: f32) -> f32 {
    return pow(2.0, k);
}

// Superellipse "length" function - generalizes L2 norm
//
// Divide through by the larger component before raising it. Taken straight,
// |x|^n leaves f32 a little above k = 4 at an ordinary corner radius: 28^32
// is about 1e46. The sum is then inf whatever the other component holds, so
// the distance is inf for every fragment the corner box reaches - along the
// sides as much as around the corners - and what a pipeline makes of inf is
// undefined. On lavapipe the antialiasing collapses and the shape is drawn
// as a hard-edged box a pixel wider all round. Rect::shape_distance, the
// fifth copy of this in Rust, reads inf as outside instead, and the shape
// stops taking clicks over all of it but the inner square.
//
// Scaled, the larger term is exactly 1 and the smaller at most 1, so nothing
// overflows at any n, and the root is multiplied back at the end. As n grows
// the smaller term underflows to 0 and this tends to max(|x|, |y|), which is
// the square corner a superellipse approaches.
fn superellipse_length(p: vec2<f32>, n: f32) -> f32 {
    if (abs(n - 1.0) < 0.01) {
        return abs(p.x) + abs(p.y);  // L1 (diamond)
    } else if (abs(n - 2.0) < 0.01) {
        return length(p);  // L2 (circle)
    } else {
        let ap = abs(p);
        let m = max(ap.x, ap.y);
        if (m <= 0.0) {
            return 0.0;
        }
        let t = min(ap.x, ap.y) / m;
        return m * pow(1.0 + pow(t, n), 1.0 / n);
    }
}

// Unified SDF for rounded rectangle with superellipse corners
// rect: [x, y, width, height], k: curvature
// radii: [top_left, top_right, bottom_right, bottom_left] — the quadrant
// the fragment falls in selects its radius, then the math is identical to
// the uniform case (the abs() below folds everything into one quadrant).
fn rounded_rect_sdf(pos: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>, k: f32) -> f32 {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half_size = vec2<f32>(rect.z * 0.5, rect.w * 0.5);

    // Select this quadrant's radius
    let rel = pos - center;
    let top = select(vec2<f32>(radii.y, radii.z), vec2<f32>(radii.x, radii.w), rel.x < 0.0);
    let radius = select(top.y, top.x, rel.y < 0.0);

    // Clamp radius to half the smaller dimension
    let r = min(radius, min(half_size.x, half_size.y));

    // For rectangles with no corners
    if (r <= 0.0) {
        let d = abs(pos - center) - half_size;
        return max(d.x, d.y);
    }

    // Position relative to center (work in first quadrant)
    let p = abs(pos - center);

    // Handle scoop (concave corners) - K < 0
    if (k < 0.0) {
        let d_box = p - half_size;
        let box_sdf = max(d_box.x, d_box.y);
        let circle_sdf = length(p - half_size) - r;
        return max(box_sdf, -circle_sdf);
    }

    // Convex corners (bevel, round, squircle)
    let n = k_to_n(k);
    let q = p - half_size + r;
    let qm = max(q, vec2<f32>(0.0, 0.0));
    let inside = min(max(q.x, q.y), 0.0);
    let corner_dist = superellipse_length(qm, n);

    return inside + corner_dist - r;
}
// === END SHARED SDF ===

// === SHARED CLIP CHAIN — four copies, kept identical by
// `tests/shader_sdf_is_one_definition.rs` ===
//
// A clip that cannot be merged with the clips above it into one rounded rect
// keeps them as a chain: `ClipChains` in `src/renderer/clip.rs` builds it,
// and the renderer uploads it once a frame as `clip_records`, which each
// pipeline that walks a chain declares for itself. The first shape of a chain
// is tested as a lone clip always was; the rest are walked here.
//
// The shapes are combined by the largest distance, each in pixels of its own
// gradient, and smoothed once by the caller. A product of coverages would
// darken an edge two clips share, which is every edge a scroller shares with
// the card it fills.

struct ClipRecord {
    rect: vec4<f32>,
    radii: vec4<f32>,
    // physical world pixels to the clip's own space, row-major 2x3
    inverse: array<f32, 6>,
    curvature: f32,
    next: u32,
}

const NO_CHAIN: u32 = 0xffffffffu;

// A broken chain cannot hang the GPU: no frame nests clips this deep.
const MAX_CHAIN: u32 = 64u;

// A distance, in pixels: the distance over how fast it changes across one.
fn pixel_distance(d: f32) -> f32 {
    return d / max(fwidth(d), 1e-6);
}

// The largest pixel distance down the chain from `link`, starting at
// `nearest`, the first shape's own. `at` is the fragment in physical pixels.
fn chain_distance(at: vec2<f32>, nearest: f32, link: u32) -> f32 {
    var d = nearest;
    var i = link;
    for (var n = 0u; n < MAX_CHAIN && i != NO_CHAIN; n = n + 1u) {
        let r = clip_records[i];
        let p = vec2<f32>(
            r.inverse[0] * at.x + r.inverse[1] * at.y + r.inverse[2],
            r.inverse[3] * at.x + r.inverse[4] * at.y + r.inverse[5]
        );
        d = max(d, pixel_distance(rounded_rect_sdf(p, r.rect, r.radii, r.curvature)));
        i = r.next;
    }
    return d;
}
// === END SHARED CLIP CHAIN ===

// === Arcs ===

const SHAPE_ROUNDED_RECT: u32 = 0u;
const SHAPE_ARC_BUTT: u32 = 1u;
const SHAPE_ARC_ROUND: u32 = 2u;
const SHAPE_ARC_SQUARE: u32 = 3u;
// Above the kinds, in the same word: the shadow is cast inside the shape.
const SHADOW_INSET: u32 = 256u;
// The members of a merged shape, drawn by `fs_merged` alone.
const SHAPE_MERGED: u32 = 4u;
// Above the kinds too: a shape drawn for one of its shadows alone.
const SHADOW_ONLY: u32 = 512u;

// The distance to a stroked circular arc, centred in `rect`.
//
// `params` is how `ShapeInstance::arc` packs it into the corner radii: the
// radius of the line's centre, half its width, the angle halfway along it and
// half the angle it spans — radians, clockwise from twelve o'clock, in y-down
// pixels. The fragment is turned so that the arc's middle points along +y and
// folded about that axis, after which every arc is the one symmetric case
// Inigo Quilez's sdArc and sdRing solve: the end the fragment is nearer is at
// `half` from +y, and the other end is its mirror.
fn arc_sdf(pos: vec2<f32>, rect: vec4<f32>, params: vec4<f32>, kind: u32) -> f32 {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let radius = params.x;
    let half_width = params.y;
    let middle = params.z;
    let half = params.w;
    let p = pos - center;

    // The middle's direction is (sin m, -cos m) in y-down pixels; across it is
    // (cos m, sin m). Folded, so the near end is the one on +x.
    let q = vec2<f32>(
        abs(p.x * cos(middle) + p.y * sin(middle)),
        p.x * sin(middle) - p.y * cos(middle)
    );
    let ring = abs(length(q) - radius) - half_width;
    // Half a turn either way is a whole one, and no end is left to cap.
    if (half >= 3.14159) {
        return ring;
    }
    // The near end of the line's centre, and the direction it runs on in.
    let end = vec2<f32>(sin(half), cos(half));
    let onward = vec2<f32>(end.y, -end.x);

    if (kind == SHAPE_ARC_ROUND) {
        // Past the end, the nearest point is the end itself.
        if (end.y * q.x > end.x * q.y) {
            return length(q - end * radius) - half_width;
        }
        return ring;
    }

    // Square ends: the ring cut by the end's radial line, as sdRing does —
    // turn the end onto +y, and past it the distance is to that segment.
    let t = vec2<f32>(
        end.y * q.x - end.x * q.y,
        end.x * q.x + end.y * q.y
    );
    let butt = max(ring, length(vec2<f32>(t.x, max(0.0, abs(radius - t.y) - half_width))) * sign(t.x));
    if (kind == SHAPE_ARC_BUTT) {
        return butt;
    }

    // A square cap: the butt end, and a box half a width long past it. The
    // box reaches two pixels back into the arc. Two shapes that only touch
    // along the end's line are both at distance zero there, and the union
    // draws that line half covered through the solid stroke; overlapping by
    // less than the pixel the edge is smoothed over still shows it faintly.
    let back = 2.0;
    let local = q - (end * radius + onward * (half_width - back) * 0.5);
    let along = vec2<f32>(dot(local, onward), dot(local, end));
    let b = abs(along) - vec2<f32>((half_width + back) * 0.5, half_width);
    let cap = length(max(b, vec2<f32>(0.0))) + min(max(b.x, b.y), 0.0);
    return min(butt, cap);
}

// Compute gradient color based on local UV coordinates
fn compute_gradient_color(
    local_uv: vec2<f32>,
    start_color: vec4<f32>,
    end_color: vec4<f32>,
    gradient_type: u32,
) -> vec4<f32> {
    var t: f32;
    switch gradient_type {
        case 1u: { t = local_uv.x; }                              // Horizontal (left to right)
        case 2u: { t = local_uv.y; }                              // Vertical (top to bottom)
        case 3u: { t = (local_uv.x + local_uv.y) / 2.0; }        // Diagonal (top-left to bottom-right)
        case 4u: { t = (local_uv.x + (1.0 - local_uv.y)) / 2.0; } // DiagonalReverse (top-right to bottom-left)
        default: { return start_color; }                          // No gradient (0 or invalid)
    }
    // Mixed premultiplied, as CSS mixes a gradient's stops and `Color::mix`
    // mixes two colours, so a stop at `TRANSPARENT` fades the other one out
    // in its own colour rather than through black.
    return unpremultiplied(mix(premultiplied(start_color), premultiplied(end_color), clamp(t, 0.0, 1.0)));
}

// === Fragment Shader ===

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Early discard for zero-area shapes
    if (in.shape_rect.z <= 0.0 || in.shape_rect.w <= 0.0) {
        discard;
    }

    let pos = in.frag_pos;
    let radii = in.shape_radii;
    let curvature = in.border_params.y;
    let border_width = in.border_params.x;

    // Compute SDF for the shape
    let gradient_type = in.kinds.x;
    let shape_kind = in.kinds.y & 0xffu;
    let inset = (in.kinds.y & SHADOW_INSET) != 0u;
    var dist: f32;
    if (shape_kind == SHAPE_ROUNDED_RECT) {
        dist = rounded_rect_sdf(pos, in.shape_rect, radii, curvature);
    } else {
        dist = arc_sdf(pos, in.shape_rect, radii, shape_kind);
    }

    let covered = shape_coverage(
        pos,
        in.shape_rect,
        radii,
        shape_kind,
        dist,
        border_width,
        max(in.border_params.zw, vec2<f32>(1e-4)),
    );

    // Compute local UV coordinates (0..1 within the shape rect)
    let local_uv = (pos - in.shape_rect.xy) / in.shape_rect.zw;

    // Determine fill color (gradient or solid)
    var fill_color: vec4<f32>;
    if (gradient_type > 0u) {
        fill_color = compute_gradient_color(local_uv, in.gradient_start, in.gradient_end, gradient_type);
    } else {
        fill_color = in.fill_color;
    }

    // === Shadow ===
    var shadow_contribution = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.shadow_color.a > 0.0 && !inset) {
        let shadow_offset = in.shadow_params.xy;
        let shadow_spread = in.shadow_params.w;

        // Compute shadow position (offset from shape)
        let shadow_pos = pos - shadow_offset;

        // Compute shadow SDF (expanded by spread)
        let shadow_dist = rounded_rect_sdf(shadow_pos, in.shape_rect, radii, curvature) - shadow_spread;
        shadow_contribution = cast_shadow(in, shadow_dist);
    }

    // === Main Shape ===
    var shape_result = filled_and_bordered(covered, fill_color, in.border_color, border_width);

    // === Inset shadow, over the fill ===
    // The outer shadow mirrored: the box inside the border — its edge
    // `border_width` in from the shape's, where the border's own inner edge
    // is drawn — moved by the offset and shrunk by the spread, is the hole
    // the shadow leaves clear, and the shadow fades across the hole's edge
    // over the same three blurs the outer one fades across the shape's.
    //
    // Measured from a hole shrunk by the two blurs the fade reaches into it,
    // so that every point of the fade is *outside* the shape it is measured
    // from. Outside a rounded rect the distance is round at the corners;
    // inside it, it is the nearer of two sides, and a fade measured there
    // meets itself along the diagonals like the mitre of a picture frame.
    if (in.shadow_color.a > 0.0 && inset) {
        let shadow_blur = in.shadow_params.z;
        let shrink = border_width + in.shadow_params.w + shadow_blur * 2.0;
        // No further than the hole's middle: past it the shrunk hole is a
        // line or a point, and the rest of the shrink is taken off the
        // distance instead.
        let s = min(shrink, 0.5 * min(in.shape_rect.z, in.shape_rect.w));
        let hole_rect = vec4<f32>(in.shape_rect.xy + s, in.shape_rect.zw - 2.0 * s);
        // A rounded corner stays rounded however far it is shrunk: a radius
        // of zero is measured as a box is, square outside as well as in,
        // which is what a square box's own shadow is and nothing else's.
        let hole_radii = select(
            vec4<f32>(0.0),
            max(radii - vec4<f32>(s), vec4<f32>(1e-3)),
            radii > vec4<f32>(0.0)
        );
        let hole = rounded_rect_sdf(pos - in.shadow_params.xy, hole_rect, hole_radii, curvature)
            - (shrink - s);
        let inside = covered.y;
        let a = in.shadow_color.a * smoothstep(0.0, shadow_blur * 3.0, hole) * inside;
        let out_a = a + shape_result.a * (1.0 - a);
        if (out_a > 0.0) {
            let out_rgb = (in.shadow_color.rgb * a + shape_result.rgb * shape_result.a * (1.0 - a)) / out_a;
            shape_result = vec4<f32>(out_rgb, out_a);
        }
    }

    return with_outer_shadow(in, shape_result, shadow_contribution, covered.x);
}

// How far either side of an edge at distance `dist` it fades, in the units the
// distance is measured in: half a device pixel, so that `smoothstep(-aa, aa,
// dist)` ramps across the one pixel the edge falls in, and covers about the
// share of it the shape does — as the backdrop composite feathers its shape.
// A pixel either side, `fwidth` whole, mixed a sixth of a pixel-aligned square
// into the pixel beside it. Floored, because at zero `smoothstep`'s two edges
// meet and WGSL leaves it undefined.
fn edge_fade(dist: f32) -> f32 {
    return max(fwidth(dist) * 0.5, 1e-4);
}

// How much of this fragment the edge at distance `dist` leaves inside.
fn edge_coverage(dist: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(-aa, aa, dist);
}

// How much of this fragment the shape covers, and how much of it lies inside
// the border `border_width` in from the shape's edge.
//
// A square corner is the two sides that meet there, each faded on its own and
// the two multiplied, which is what a pixel's box covers of a corner: a
// quarter, for a corner on the pixel's centre. Faded as one distance it would
// not be: the distance there is the larger of the two sides', so it bends
// along the corner's diagonal, and `fwidth` across the bend sums both sides'
// slopes — the pixel off the corner of a square on the pixel grid came out a
// sixth covered rather than not at all. A side's distance moves as the
// position does, so its fade is `side_fade`, worked out once per instance by
// the vertex stage, and only an instance with a square corner looks for one.
fn shape_coverage(
    pos: vec2<f32>,
    rect: vec4<f32>,
    radii: vec4<f32>,
    shape_kind: u32,
    dist: f32,
    border_width: f32,
    side_fade: vec2<f32>,
) -> vec2<f32> {
    let aa = edge_fade(dist);
    if (shape_kind == SHAPE_ROUNDED_RECT && any(radii <= vec4<f32>(0.0))) {
        let half_size = rect.zw * 0.5;
        let rel = pos - (rect.xy + half_size);
        let top = select(vec2<f32>(radii.y, radii.z), vec2<f32>(radii.x, radii.w), rel.x < 0.0);
        if (select(top.y, top.x, rel.y < 0.0) <= 0.0) {
            let sides = abs(rel) - half_size;
            let outer = edge_coverage(sides.x, side_fade.x) * edge_coverage(sides.y, side_fade.y);
            if (border_width <= 0.0) {
                return vec2<f32>(outer);
            }
            return vec2<f32>(
                outer,
                edge_coverage(sides.x + border_width, side_fade.x)
                    * edge_coverage(sides.y + border_width, side_fade.y),
            );
        }
    }
    let outer = edge_coverage(dist, aa);
    if (border_width <= 0.0) {
        return vec2<f32>(outer);
    }
    return vec2<f32>(outer, edge_coverage(dist + border_width, aa));
}

// The shadow at a fragment whose shadow distance is `shadow_dist`: the colour,
// faded out with the blur.
fn cast_shadow(in: VertexOutput, shadow_dist: f32) -> vec4<f32> {
    let shadow_blur = in.shadow_params.z;
    // Convert to alpha with blur falloff
    let shadow_alpha = in.shadow_color.a * (1.0 - smoothstep(-shadow_blur, shadow_blur * 2.0, shadow_dist));
    return vec4<f32>(in.shadow_color.rgb, shadow_alpha);
}

// A shape covering `covered.x` of the fragment, and the inside of its border
// `covered.y`: its fill, and its border `border_width` deep inside the edge.
fn filled_and_bordered(
    covered: vec2<f32>,
    fill_color: vec4<f32>,
    border_color: vec4<f32>,
    border_width: f32,
) -> vec4<f32> {
    if (border_width <= 0.0) {
        // No border - simple filled shape
        return vec4<f32>(fill_color.rgb, fill_color.a * covered.x);
    }
    // With border
    let shape_alpha = covered.x;
    let fill_alpha = covered.y;
    let border_alpha = max(shape_alpha - fill_alpha, 0.0);

    if (fill_color.a <= 0.0) {
        // Border only (transparent fill)
        return vec4<f32>(border_color.rgb, border_color.a * border_alpha);
    }
    // Fill + border composite
    let fill_contribution = vec4<f32>(fill_color.rgb, fill_color.a * fill_alpha);
    let border_contribution = vec4<f32>(border_color.rgb, border_color.a * border_alpha);

    let result_rgb = border_contribution.rgb * border_contribution.a +
                     fill_contribution.rgb * fill_contribution.a * (1.0 - border_contribution.a);
    let result_a = border_contribution.a + fill_contribution.a * (1.0 - border_contribution.a);

    if (result_a <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return vec4<f32>(result_rgb / result_a, result_a);
}

// How much of an outer shadow is drawn at a fragment its shape covers by
// `coverage`, the shape's edge antialiased as its fill is, where what is drawn
// over the shadow there has the alpha `above`: the shape itself, or for a
// shadow drawn alone, the same shape drawn next.
//
// A shadow is cast outside the shape alone, as CSS casts a `box-shadow`
// outside the border box, so once the shape is over it `1 - coverage` of it
// shows: none under a translucent fill. Under an opaque one `above` is the
// coverage, the share is 1, and the edge is drawn as it always was.
fn outside(coverage: f32, above: f32) -> f32 {
    if (above >= 1.0) {
        return 0.0;
    }
    return min((1.0 - coverage) / (1.0 - above), 1.0);
}

// `shape` over its outer `shadow`, left out from under it, and cut by the
// clip: what both fragment shaders end in. A shape drawn for its shadow alone
// reads its fill to know how much of the shadow to leave, and draws none of it.
fn with_outer_shadow(
    in: VertexOutput,
    shape: vec4<f32>,
    shadow: vec4<f32>,
    coverage: f32,
) -> vec4<f32> {
    var cut = shadow;
    if (cut.a > 0.0) {
        cut.a *= outside(coverage, shape.a);
    }
    var drawn = shape;
    if ((in.kinds.y & SHADOW_ONLY) != 0u) {
        drawn = vec4<f32>(0.0);
    }
    return clipped(in, over_shadow(drawn, cut));
}

// `shape` composited over its `shadow`, both straight, and straight again:
// the blend multiplies the colour by its alpha, so a premultiplied one here
// would be multiplied twice.
fn over_shadow(shape: vec4<f32>, shadow: vec4<f32>) -> vec4<f32> {
    if (shadow.a <= 0.0) {
        return shape;
    }
    let final_a = shape.a + shadow.a * (1.0 - shape.a);
    let final_rgb = shape.rgb * shape.a + shadow.rgb * shadow.a * (1.0 - shape.a);
    return vec4<f32>(final_rgb / final_a, final_a);
}

// `color` cut by the instance's clip and the chain it continues in.
fn clipped(in: VertexOutput, color: vec4<f32>) -> vec4<f32> {
    // Check if clipping is enabled (negative width/height = no clip sentinel)
    if (in.clip_rect.z < 0.0 || in.clip_rect.w < 0.0) {
        return color;
    }
    let clip_dist = rounded_rect_sdf(
        in.clip_pos,
        in.clip_rect,
        in.clip_radii,
        in.clip_curvature
    );

    var clip_alpha: f32;
    if (in.clip_chain == NO_CHAIN) {
        // Smooth clip edge (anti-aliased)
        let clip_aa = fwidth(clip_dist);
        clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);
    } else {
        let d = chain_distance(in.clip_position.xy, pixel_distance(clip_dist), in.clip_chain);
        clip_alpha = 1.0 - smoothstep(-1.0, 1.0, d);
    }
    return vec4<f32>(color.rgb, color.a * clip_alpha);
}

// === Merged shapes ===
//
// A container that merges its children draws their backgrounds as one shape:
// one record per member, written by `src/renderer/merge.rs` and chained by
// `next` as a clip chain is. Each member is measured in its own space, as a
// clip is, and brought to physical pixels so the members can be compared.
//
// They are folded with a polynomial smooth minimum, which leaves two distances
// further apart than its reach exactly as `min` would — a member that moves
// away splits off by itself — and gives back, beside the distance, how much of
// the fragment belongs to each. The fill, the border's colour and its width
// blend by that share, so the border runs along the merged outline and fades
// along a neck towards a member without one.
//
// Its own entry point, drawn by a pipeline of its own over the merged
// instances alone, so a shape drawn alone runs `fs_main` and nothing of this.
// The vertex stage is `vs_main`, unchanged: a merged instance has no gradient,
// so its `gradient_type` names its first member.

// The members of the frame's merged shapes — see the shared block below.
@group(0) @binding(2) var<storage, read> merge_records: array<MergeRecord>;

// === SHARED MERGED SHAPE — two copies, kept identical by
// `tests/shader_sdf_is_one_definition.rs` ===
//
// The members of a merged shape, and the fold that makes one outline of
// them. The shape shader fills that outline; the backdrop composite cuts a
// merged blur to it, so the frost and the fill over it meet at one edge. Each
// declares `merge_records` for itself, at its own binding.
//
// Mirrors `MergeRecord` in `src/renderer/merge.rs`, checked field for field by
// the test beside it.
struct MergeRecord {
    rect: vec4<f32>,
    radii: vec4<f32>,
    // physical world pixels to the member's own space, row-major 2x3
    inverse: array<f32, 6>,
    curvature: f32,
    // one unit of the member's own space, in physical pixels
    to_pixels: f32,
    // premultiplied
    fill: vec4<f32>,
    // premultiplied
    border_color: vec4<f32>,
    // physical pixels
    border_width: f32,
    // the smooth minimum's reach, physical pixels; 0 is a plain union
    smoothing: f32,
    next: u32,
}

const NO_MERGE: u32 = 0xffffffffu;

// No container merges more; a broken chain cannot hang the GPU either.
const MAX_MERGED: u32 = 16u;

struct Merged {
    dist: f32,
    // the merged distance's gradient: the members' unit normals, mixed as the
    // fold mixes their distances, so its length is how fast the distance
    // changes — one along a member's own edge, less across a neck
    normal: vec2<f32>,
    fill: vec4<f32>,
    border_color: vec4<f32>,
    border_width: f32,
    // whether the fold smoothed anywhere here: where it did, the distance is
    // not one, and changes more slowly than the position does
    smoothed: bool,
    // the member whose distance is the smallest so far, whose sides with the
    // next one's hold what the fold takes off
    nearest: u32,
}

// The way `rounded_rect_sdf` grows at `pos`, from the same pieces: a corner's
// own direction out past a square one, the scoop's circle where it bites, the
// superellipse's gradient `(x^(n-1), y^(n-1))` around a convex corner, and
// elsewhere the side whose distance wins. Not normalised.
fn rounded_rect_normal(pos: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>, k: f32) -> vec2<f32> {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half_size = vec2<f32>(rect.z * 0.5, rect.w * 0.5);
    let rel = pos - center;
    let top = select(vec2<f32>(radii.y, radii.z), vec2<f32>(radii.x, radii.w), rel.x < 0.0);
    let radius = select(top.y, top.x, rel.y < 0.0);
    let r = min(radius, min(half_size.x, half_size.y));
    let side = select(vec2<f32>(1.0), vec2<f32>(-1.0), rel < vec2<f32>(0.0));
    let d_box = abs(rel) - half_size;
    if (r <= 0.0) {
        // Past a square corner the distance is the larger side's, but the
        // way out is the corner's own, and it leans towards both sides.
        if (d_box.x > 0.0 && d_box.y > 0.0) {
            return side * d_box;
        }
    } else if (k < 0.0) {
        if (r - length(d_box) > max(d_box.x, d_box.y)) {
            return -side * d_box;
        }
    } else {
        let q = d_box + r;
        if (q.x > 0.0 && q.y > 0.0) {
            // Scaled so the larger part is 1, which no power overflows.
            return side * pow(q / max(q.x, q.y), vec2<f32>(k_to_n(k) - 1.0));
        }
    }
    return side * select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), d_box.x > d_box.y);
}

// `at`, in physical world pixels, in the member's own space.
fn member_space(r: MergeRecord, at: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(
        r.inverse[0] * at.x + r.inverse[1] * at.y + r.inverse[2],
        r.inverse[3] * at.x + r.inverse[4] * at.y + r.inverse[5]
    );
}

// The member's normal at `q`, a point of its own space, as a unit vector in
// world space: its own, through the transpose of the map into its space.
fn member_normal(r: MergeRecord, q: vec2<f32>) -> vec2<f32> {
    let g = rounded_rect_normal(q, r.rect, r.radii, r.curvature);
    let world = vec2<f32>(
        r.inverse[0] * g.x + r.inverse[3] * g.y,
        r.inverse[1] * g.x + r.inverse[4] * g.y
    );
    return world / max(length(world), 1e-6);
}

// How far the member reaches in the world direction `u`: its support
// function, the most `dot(u, x)` takes over the member. A rounded rect's is
// its box's, less each corner's radius along both axes and plus the corner's
// own reach, the dual norm of its superellipse: `max` for a bevel, the
// Euclidean length for a circle, the sum for a square. A scoop's is its box's,
// which is the hull of it.
fn member_support(r: MergeRecord, u: vec2<f32>) -> f32 {
    let det = r.inverse[0] * r.inverse[4] - r.inverse[1] * r.inverse[3];
    // `u` in the member's own space: the transpose of the map out of it.
    let w = vec2<f32>(
        r.inverse[4] * u.x - r.inverse[3] * u.y,
        r.inverse[0] * u.y - r.inverse[1] * u.x
    ) / det;
    let center = vec2<f32>(r.rect.x + r.rect.z * 0.5, r.rect.y + r.rect.w * 0.5);
    let half_size = vec2<f32>(r.rect.z * 0.5, r.rect.w * 0.5);
    let top = select(vec2<f32>(r.radii.y, r.radii.z), vec2<f32>(r.radii.x, r.radii.w), w.x < 0.0);
    let radius = select(top.y, top.x, w.y < 0.0);
    var rad = max(min(radius, min(half_size.x, half_size.y)), 0.0);
    let a = abs(w);
    var reach = 0.0;
    if (r.curvature < 0.0) {
        rad = 0.0;
    } else if (rad > 0.0) {
        let n = k_to_n(r.curvature);
        let m = max(a.x, a.y);
        if (abs(n - 1.0) < 0.01 || m <= 0.0) {
            reach = m;
        } else {
            let dual = n / (n - 1.0);
            let t = min(a.x, a.y) / m;
            reach = m * pow(1.0 + pow(t, dual), 1.0 / dual);
        }
    }
    let local = dot(center, w) + dot(a, half_size - rad) + rad * reach;
    return local - dot(w, vec2<f32>(r.inverse[2], r.inverse[5]));
}

// How far `at` lies past the reach of `a` and `b` along the four sides of
// `r`, and which way that is.
fn past_sides_of(r: MergeRecord, a: MergeRecord, b: MergeRecord, at: vec2<f32>) -> vec3<f32> {
    var most = vec3<f32>(-1e30, 0.0, 0.0);
    for (var side = 0u; side < 4u; side = side + 1u) {
        let toward = select(1.0, -1.0, side >= 2u);
        let axis = select(vec2<f32>(0.0, toward), vec2<f32>(toward, 0.0), side % 2u == 0u);
        let world = vec2<f32>(
            r.inverse[0] * axis.x + r.inverse[3] * axis.y,
            r.inverse[1] * axis.x + r.inverse[4] * axis.y
        );
        let u = world / max(length(world), 1e-6);
        let past = dot(at, u) - max(member_support(a, u), member_support(b, u));
        if (past > most.x) {
            most = vec3<f32>(past, u);
        }
    }
    return most;
}

// How far `at` lies beyond every side of `a` and `b` — the box they make
// together, for two that stand upright — and the way out. What a smooth
// minimum adds past it was a bulge: a member inside another pushing the
// other's edge out, or two side by side lifting the line their tops share
// (#918). Taking all four sides of each keeps it continuous.
fn beyond_sides(a: MergeRecord, b: MergeRecord, at: vec2<f32>) -> vec3<f32> {
    let from_a = past_sides_of(a, a, b, at);
    let from_b = past_sides_of(b, a, b, at);
    return select(from_b, from_a, from_a.x >= from_b.x);
}

// How far inside a smooth minimum's `reach` two distances `gap` apart are.
fn within_reach(gap: f32, reach: f32) -> f32 {
    if (reach <= 0.0) {
        return 0.0;
    }
    return max(reach - gap, 0.0) / reach;
}

// The merged distance at `at`, in physical pixels, and what the members give
// the fragment there — colours still premultiplied.
fn merged_shape(at: vec2<f32>, first: u32) -> Merged {
    var m = Merged(1e30, vec2<f32>(0.0), vec4<f32>(0.0), vec4<f32>(0.0), 0.0, false, NO_MERGE);
    var i = first;
    for (var n = 0u; n < MAX_MERGED && i != NO_MERGE; n = n + 1u) {
        let r = merge_records[i];
        let p = member_space(r, at);
        let d = rounded_rect_sdf(p, r.rect, r.radii, r.curvature) * r.to_pixels;
        let further_is_new = m.dist < d;
        // The quadratic smooth minimum (Inigo Quilez, "smooth minimum"): `h`
        // is how far inside each other's reach the two distances are, and
        // `q` the share the further one keeps — over the whole reach, so
        // fills blend across an overlap as they always did. The distance is
        // smoothed over the reach scaled by how far the two normals turn
        // apart, as liquid_glass_renderer does after Gourmel et al. (2013),
        // so edges that face the same way are not pushed out; and what it
        // takes off is held inside the sides of this member and the nearest
        // so far, which the angle alone does not do where a corner of one
        // meets the other's edge (#918; docs/RENDERER.md).
        let h = within_reach(abs(m.dist - d), r.smoothing);
        let q = h * h * 0.5;
        let share = select(1.0 - q, q, further_is_new);
        let nearer = min(m.dist, d);
        var dist = nearer;
        var h_dist = 0.0;
        if (r.smoothing > 0.0) {
            let normal = member_normal(r, p);
            let reach = r.smoothing * 0.5 * length(m.normal - normal);
            h_dist = within_reach(abs(m.dist - d), reach);
            // The further side's part of the smoothed distance's gradient.
            let pull = select(1.0 - h_dist * 0.5, h_dist * 0.5, further_is_new);
            m.normal = mix(m.normal, normal, pull);
            if (h_dist > 0.0) {
                dist = nearer - h_dist * h_dist * reach * 0.25;
                let sides = beyond_sides(merge_records[m.nearest], r, at);
                if (sides.x > dist) {
                    // Held, the distance changes as fast as a member's own,
                    // one pixel a pixel, along the side that holds it.
                    dist = min(sides.x, nearer);
                    m.normal = sides.yz;
                }
            }
        }
        if (!further_is_new) {
            m.nearest = i;
        }
        m.dist = dist;
        m.fill = mix(m.fill, r.fill, share);
        m.border_color = mix(m.border_color, r.border_color, share);
        m.border_width = mix(m.border_width, r.border_width, share);
        m.smoothed = m.smoothed || h_dist > 0.0;
        i = r.next;
    }
    return m;
}
// === END SHARED MERGED SHAPE ===

fn premultiplied(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn unpremultiplied(c: vec4<f32>) -> vec4<f32> {
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(c.rgb / c.a, c.a);
}

@fragment
fn fs_merged(in: VertexOutput) -> @location(0) vec4<f32> {
    // A merged shape draws in world space, so `pos` is where the members are
    // placed from.
    let pos = in.frag_pos;
    let first = in.kinds.x;
    let merged = merged_shape(pos, first);
    let aa = edge_fade(merged.dist);
    // The border is `border_width` pixels deep, and where the fold smoothed
    // the distance is not in pixels: it changes more slowly than one — by √2
    // where two edges meet square, by more where they meet wider, as two
    // pills' do — and a band of it as deep as the border swelled across the
    // neck (#885). So there the depth is scaled by how fast the distance
    // changes, which is the length of the gradient the fold carries. Not by
    // `dpdx`, a whole pixel wide, which notched the border where the outline
    // turns within a few; nor by the fold read either side, which across the
    // diagonal inside a square corner, where a member's normal flips, read
    // the reach's jump as a slope and drew the border along it. Only where
    // the fold smoothed: a member's own distance is already in pixels.
    var depth = merged.border_width;
    if (merged.smoothed) {
        depth = depth * length(merged.normal);
    }
    let covered = vec2<f32>(
        edge_coverage(merged.dist, aa),
        edge_coverage(merged.dist + depth, aa),
    );

    var shadow = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.shadow_color.a > 0.0) {
        let shadow_dist = merged_shape(pos - in.shadow_params.xy, first).dist - in.shadow_params.w;
        shadow = cast_shadow(in, shadow_dist);
    }

    // A further shadow is the merged shape drawn again beneath it for that
    // shadow alone, as a box draws its fill again for each.
    let shape = filled_and_bordered(
        covered,
        unpremultiplied(merged.fill),
        unpremultiplied(merged.border_color),
        merged.border_width,
    );
    return with_outer_shadow(in, shape, shadow, covered.x);
}
