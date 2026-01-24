// Guido Texture Shader - Renders textured quads with transforms and clipping
//
// Used for displaying text textures and images that have been rendered to
// textures, allowing them to follow parent container transforms and clip regions.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) transform_row0: vec4<f32>,
    @location(3) transform_row1: vec4<f32>,
    @location(4) transform_row2: vec4<f32>,
    @location(5) transform_row3: vec4<f32>,
    @location(6) clip_rect: vec4<f32>,  // clip region: min_x, min_y, max_x, max_y in NDC
    @location(7) clip_data: vec4<f32>,  // x = clip_radius, yzw = padding
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) clip_rect: vec4<f32>,
    @location(2) clip_radius: f32,
    @location(3) frag_pos: vec2<f32>,  // position in NDC for clip evaluation
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Build transform matrix from row vectors
    let transform = mat4x4<f32>(
        vec4<f32>(in.transform_row0.x, in.transform_row1.x, in.transform_row2.x, in.transform_row3.x),
        vec4<f32>(in.transform_row0.y, in.transform_row1.y, in.transform_row2.y, in.transform_row3.y),
        vec4<f32>(in.transform_row0.z, in.transform_row1.z, in.transform_row2.z, in.transform_row3.z),
        vec4<f32>(in.transform_row0.w, in.transform_row1.w, in.transform_row2.w, in.transform_row3.w)
    );

    // Transform position
    let transformed = transform * vec4<f32>(in.position, 0.0, 1.0);
    out.clip_position = vec4<f32>(transformed.xy, 0.0, 1.0);

    // Pass through texture coordinates
    out.tex_coords = in.tex_coords;

    // Pass through clip region
    out.clip_rect = in.clip_rect;
    out.clip_radius = in.clip_data.x;

    // Pass local position for clip evaluation (untransformed for correct clipping)
    out.frag_pos = in.position;

    return out;
}

@group(0) @binding(0)
var t_texture: texture_2d<f32>;
@group(0) @binding(1)
var s_sampler: sampler;

// Standard rounded box SDF (for circular corners)
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

// Compute SDF for a rounded rectangle clip region
fn clip_sdf(pos: vec2<f32>, rect: vec4<f32>, radius: f32) -> f32 {
    let min_corner = vec2<f32>(rect.x, rect.y);
    let max_corner = vec2<f32>(rect.z, rect.w);
    let center = (min_corner + max_corner) * 0.5;
    let half_size = (max_corner - min_corner) * 0.5;

    // Clamp radius to half the smaller dimension
    let r = min(radius, min(half_size.x, half_size.y));

    // Position relative to center
    let p = pos - center;

    // Use standard rounded box SDF
    return sd_rounded_box(p, half_size, r);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture with the interpolated UV coordinates
    var color = textureSample(t_texture, s_sampler, in.tex_coords);

    // Apply clip region if defined (clip_rect width > 0)
    let clip_width = in.clip_rect.z - in.clip_rect.x;
    let clip_height = in.clip_rect.w - in.clip_rect.y;
    if (clip_width > 0.0 && clip_height > 0.0) {
        // Compute aspect ratio for correct circular corners
        // The clip_radius is provided in height-based NDC, so we need to scale x
        let aspect = clip_height / clip_width;

        // Scale x coordinates to make clip region appear with correct aspect ratio
        let scaled_pos = vec2<f32>(in.frag_pos.x * aspect, in.frag_pos.y);
        let scaled_clip_rect = vec4<f32>(
            in.clip_rect.x * aspect,
            in.clip_rect.y,
            in.clip_rect.z * aspect,
            in.clip_rect.w
        );

        // Compute clip SDF (using circular corners)
        let clip_dist = clip_sdf(scaled_pos, scaled_clip_rect, in.clip_radius);

        // Anti-aliasing using fwidth
        let clip_aa = fwidth(clip_dist);
        let clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);

        color = vec4<f32>(color.rgb, color.a * clip_alpha);
    }

    return color;
}
