// Guido Path Shader
//
// Draws the triangles a path was tessellated into (`path_mesh.rs`). Each
// vertex is in physical pixels of the path's own space and carries its
// coverage — one inside, falling to zero across the one-pixel fringe the mesh
// was given — and the record of the draw it belongs to: the colour, the
// transform that places it, and the clip that cuts it.

// === Uniforms ===

struct Uniforms {
    screen_size: vec2<f32>,  // Physical pixels
    scale_factor: f32,       // HiDPI scale
    _pad: f32,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

// One per fill or stroke drawn, read by the index every vertex carries.
//
// **This struct and `PathRecord` in `path_pipeline.rs` are one layout written
// twice**, and the unit test beside that struct computes this one's offsets
// from WGSL's layout rules and checks them against the Rust ones. The clip is
// the same `ClipRecord` a chain is made of, declared in the shared block
// below: a lone clip is the first link of a chain, and its `next` is where the
// rest of that chain continues.
struct PathRecord {
    color: vec4<f32>,
    // a, b, tx, c, d, ty — row-major 2x3, physical pixels
    transform: array<f32, 6>,
    clip: ClipRecord,
}

@group(0) @binding(1) var<storage, read> records: array<PathRecord>;

// The frame's clip chains — see the shared clip-chain block below.
@group(1) @binding(0) var<storage, read> clip_records: array<ClipRecord>;

// === Vertex ===

struct VertexInput {
    // physical pixels, in the path's own space
    @location(0) position: vec2<f32>,
    // one inside the path, zero at the outer edge of its fringe
    @location(1) coverage: f32,
    @location(2) record: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // This fragment in the clip's own space. The map is affine, so it is
    // applied to the corners and interpolated, as the shape shader does.
    @location(1) clip_pos: vec2<f32>,
    // The rest of the clip, handed on rather than read again per fragment.
    @location(2) @interpolate(flat) clip_rect: vec4<f32>,
    @location(3) @interpolate(flat) clip_radii: vec4<f32>,
    @location(4) @interpolate(flat) clip_curvature: f32,
    @location(5) @interpolate(flat) clip_next: u32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let r = records[in.record];
    let t = r.transform;
    let world = vec2<f32>(
        t[0] * in.position.x + t[1] * in.position.y + t[2],
        t[3] * in.position.x + t[4] * in.position.y + t[5]
    );
    let i = r.clip.inverse;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(
        (world.x / uniforms.screen_size.x) * 2.0 - 1.0,
        1.0 - (world.y / uniforms.screen_size.y) * 2.0,
        0.0,
        1.0
    );
    out.clip_pos = vec2<f32>(
        i[0] * world.x + i[1] * world.y + i[2],
        i[3] * world.x + i[4] * world.y + i[5]
    );
    out.color = vec4<f32>(r.color.rgb, r.color.a * in.coverage);
    out.clip_rect = r.clip.rect;
    out.clip_radii = r.clip.radii;
    out.clip_curvature = r.clip.curvature;
    out.clip_next = r.clip.next;
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

// === Fragment ===

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = in.color;

    // Negative width or height is the sentinel for no clip at all.
    if (in.clip_rect.z >= 0.0 && in.clip_rect.w >= 0.0) {
        let clip_dist = rounded_rect_sdf(in.clip_pos, in.clip_rect, in.clip_radii, in.clip_curvature);

        var clip_alpha: f32;
        if (in.clip_next == NO_CHAIN) {
            let clip_aa = fwidth(clip_dist);
            clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);
        } else {
            let d = chain_distance(in.clip_position.xy, pixel_distance(clip_dist), in.clip_next);
            clip_alpha = 1.0 - smoothstep(-1.0, 1.0, d);
        }

        color = vec4<f32>(color.rgb, color.a * clip_alpha);
    }

    return color;
}
