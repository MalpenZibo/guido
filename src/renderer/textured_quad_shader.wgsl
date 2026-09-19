// Guido Textured Quad Shader
//
// This shader renders textured quads for images and transformed text with clipping support.
// Vertices include pre-computed NDC positions and clip data. The clip test
// runs in whatever space the CPU wrote `clip_pos` and `clip_rect` in — the
// clip's own for an image, physical world pixels for text.

// === Vertex Input ===

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) clip_pos: vec2<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) clip_params: vec4<f32>,
    // clip curvature, _pad, _pad, _pad
    @location(5) clip_curvature: vec4<f32>,
}

// === Vertex Output ===

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) clip_pos: vec2<f32>,
    @location(2) clip_rect: vec4<f32>,
    @location(3) clip_radii: vec4<f32>,
    @location(4) @interpolate(flat) clip_curvature: f32,
}

// === Texture Bindings ===

@group(0) @binding(0) var t_texture: texture_2d<f32>;
@group(0) @binding(1) var s_sampler: sampler;

// === Vertex Shader ===

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    out.clip_pos = in.clip_pos;
    out.clip_rect = in.clip_rect;
    out.clip_radii = in.clip_params;
    out.clip_curvature = in.clip_curvature.x;
    return out;
}

// === SDF Functions ===

// Curvature (K) to superellipse exponent. The same two lines as
// `shader.wgsl`, because the clip is one shape and must not cut one way for a
// rectangle and another for an image.
fn k_to_n(k: f32) -> f32 {
    return pow(2.0, k);
}

fn superellipse_length(p: vec2<f32>, n: f32) -> f32 {
    if (abs(n - 1.0) < 0.01) {
        return abs(p.x) + abs(p.y);  // L1 (diamond)
    } else if (abs(n - 2.0) < 0.01) {
        return length(p);  // L2 (circle)
    } else {
        let ap = abs(p);
        return pow(pow(ap.x, n) + pow(ap.y, n), 1.0 / n);
    }
}

// SDF for rounded rectangle clipping.
// radii: [top_left, top_right, bottom_right, bottom_left] — the corner the
// point falls in decides which one applies. k: curvature.
fn rounded_rect_sdf(pos: vec2<f32>, rect: vec4<f32>, radii: vec4<f32>, k: f32) -> f32 {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half_size = vec2<f32>(rect.z * 0.5, rect.w * 0.5);
    let rel = pos - center;
    let pair = select(vec2<f32>(radii.y, radii.z), vec2<f32>(radii.x, radii.w), rel.x < 0.0);
    let radius = select(pair.y, pair.x, rel.y < 0.0);
    let r = min(radius, min(half_size.x, half_size.y));

    if (r <= 0.0) {
        let d = abs(pos - center) - half_size;
        return max(d.x, d.y);
    }

    let p = abs(pos - center);

    // Scoop (concave corners), K < 0
    if (k < 0.0) {
        let d_box = p - half_size;
        let box_sdf = max(d_box.x, d_box.y);
        let circle_sdf = length(p - half_size) - r;
        return max(box_sdf, -circle_sdf);
    }

    let n = k_to_n(k);
    let q = p - half_size + r;
    let qm = max(q, vec2<f32>(0.0, 0.0));
    let inside = min(max(q.x, q.y), 0.0);
    return inside + superellipse_length(qm, n) - r;
}

// === Fragment Shader ===

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(t_texture, s_sampler, in.uv);

    // Apply clipping if enabled (negative width/height = no clip sentinel)
    if (in.clip_rect.z >= 0.0 && in.clip_rect.w >= 0.0) {
        let clip_dist = rounded_rect_sdf(
            in.clip_pos,
            in.clip_rect,
            in.clip_radii,
            in.clip_curvature
        );

        // Anti-aliased clip edge
        let clip_aa = fwidth(clip_dist);
        let clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);

        color = vec4<f32>(color.rgb, color.a * clip_alpha);
    }

    return color;
}
