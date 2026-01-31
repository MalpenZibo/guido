// Guido V2 Image Quad Shader
//
// This shader renders textured quads for images with clipping support.
// Vertices include pre-computed NDC positions and clip data.

// === Vertex Input ===

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) screen_pos: vec2<f32>,
    @location(3) clip_rect: vec4<f32>,
    @location(4) clip_params: vec4<f32>,
}

// === Vertex Output ===

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) screen_pos: vec2<f32>,
    @location(2) clip_rect: vec4<f32>,
    @location(3) clip_params: vec2<f32>,
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
    out.screen_pos = in.screen_pos;
    out.clip_rect = in.clip_rect;
    out.clip_params = in.clip_params.xy;
    return out;
}

// === SDF Functions ===

// SDF for rounded rectangle clipping
fn rounded_rect_sdf(pos: vec2<f32>, rect: vec4<f32>, radius: f32) -> f32 {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half_size = vec2<f32>(rect.z * 0.5, rect.w * 0.5);
    let r = min(radius, min(half_size.x, half_size.y));

    if (r <= 0.0) {
        let d = abs(pos - center) - half_size;
        return max(d.x, d.y);
    }

    let p = abs(pos - center);
    let q = p - half_size + r;
    let qm = max(q, vec2<f32>(0.0, 0.0));
    let inside = min(max(q.x, q.y), 0.0);
    return inside + length(qm) - r;
}

// === Fragment Shader ===

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(t_texture, s_sampler, in.uv);

    // Apply clipping if enabled (width and height > 0)
    if (in.clip_rect.z > 0.0 && in.clip_rect.w > 0.0) {
        let clip_dist = rounded_rect_sdf(
            in.screen_pos,
            in.clip_rect,
            in.clip_params.x  // corner_radius
        );

        // Anti-aliased clip edge
        let clip_aa = fwidth(clip_dist);
        let clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);

        color = vec4<f32>(color.rgb, color.a * clip_alpha);
    }

    return color;
}
