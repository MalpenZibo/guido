// Guido Texture Shader - Renders textured quads with transforms
//
// Used for displaying text textures that have been rendered to offscreen
// textures, allowing text to follow parent container transforms.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) transform_row0: vec4<f32>,
    @location(3) transform_row1: vec4<f32>,
    @location(4) transform_row2: vec4<f32>,
    @location(5) transform_row3: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
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

    return out;
}

@group(0) @binding(0)
var t_texture: texture_2d<f32>;
@group(0) @binding(1)
var s_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Sample the texture with the interpolated UV coordinates
    let color = textureSample(t_texture, s_sampler, in.tex_coords);
    return color;
}
