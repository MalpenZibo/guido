// Guido Backdrop Shader
//
// Filters what is already on the render target. Three entry points share one
// fullscreen-triangle vertex stage; the render pass viewport decides where the
// output lands, so no entry point needs to know about NDC.

struct Params {
    // Sub-rectangle of the source texture to read, in normalised UV.
    src_rect: vec4<f32>,
    // Texel step for the blur, in source UV. Zero on the downsample step.
    direction: vec2<f32>,
    // Gaussian sigma in source texels, and the tap count derived from it.
    sigma: f32,
    taps: f32,
    // Destination rect size in physical pixels, for the mask SDF.
    dst_size: vec2<f32>,
    curvature: f32,
    _pad: f32,
    // Corner radii in physical pixels: top-left, top-right, bottom-right,
    // bottom-left.
    radii: vec4<f32>,
    // Sub-rectangle of the coverage mask this viewport covers, normalised.
    mask_rect: vec4<f32>,
    // Colour of the outline drawn by `fs_outline`, and how far it reaches out
    // from the glyph edge in physical pixels.
    stroke_color: vec4<f32>,
    stroke_width: f32,
    _pad1: f32,
    _pad2: f32,
    _pad3: f32,
}

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var s_source: sampler;
@group(0) @binding(2) var<uniform> params: Params;
// Coverage mask for `fs_composite_mask`, one texel per destination pixel.
// Declared for every entry point, bound only by the pipeline that reads it.
@group(0) @binding(3) var t_mask: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // 0..1 across the destination viewport.
    @location(0) uv: vec2<f32>,
}

// A single oversized triangle covering the viewport: cheaper than a quad and
// avoids the diagonal seam two triangles can show under some drivers.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    out.uv = uv;
    out.clip_position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    // Framebuffer Y grows downwards, clip space upwards.
    out.clip_position.y = -out.clip_position.y;
    return out;
}

fn source_uv(uv: vec2<f32>) -> vec2<f32> {
    return params.src_rect.xy + uv * params.src_rect.zw;
}

// Copy the region into the working texture. Rendering into a smaller target
// with a linear sampler is itself a box filter, which is most of the blur for
// free — the gaussian afterwards only has to smooth what is left.
@fragment
fn fs_downsample(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(t_source, s_source, source_uv(in.uv));
}

// One axis of a separable gaussian. Two of these compose into a 2D blur at a
// fraction of the cost of sampling the full kernel.
@fragment
fn fs_blur(in: VertexOutput) -> @location(0) vec4<f32> {
    let sigma = max(params.sigma, 0.0001);
    let taps = i32(params.taps);

    var accum = textureSample(t_source, s_source, source_uv(in.uv));
    var weight_sum = 1.0;

    for (var i = 1; i <= taps; i = i + 1) {
        let offset = f32(i);
        let weight = exp(-(offset * offset) / (2.0 * sigma * sigma));
        let step = params.direction * offset;
        // Clamped inside the region by the sampler's clamp-to-edge address
        // mode, so the border smears rather than pulling in neighbours.
        accum = accum + textureSample(t_source, s_source, source_uv(in.uv) + step) * weight;
        accum = accum + textureSample(t_source, s_source, source_uv(in.uv) - step) * weight;
        weight_sum = weight_sum + 2.0 * weight;
    }

    return accum / weight_sum;
}

// Signed distance to a superellipse-cornered rectangle centred on the origin.
fn rounded_box_sdf(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>, k: f32) -> f32 {
    // Pick the radius of the corner this fragment is nearest.
    var r = select(radii.w, radii.x, p.x < 0.0 && p.y < 0.0);
    r = select(r, radii.y, p.x >= 0.0 && p.y < 0.0);
    r = select(r, radii.z, p.x >= 0.0 && p.y >= 0.0);
    r = min(r, min(half_size.x, half_size.y));

    let q = abs(p) - half_size + vec2<f32>(r, r);
    if (q.x <= 0.0 || q.y <= 0.0 || r <= 0.0) {
        return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
    }
    // Superellipse corner: |x|^k + |y|^k = r^k, k = 1 is a circle.
    let n = max(k, 0.05) * 2.0;
    let d = pow(pow(q.x, n) + pow(q.y, n), 1.0 / n);
    return d - r;
}

// Write the blurred region back over the target, shaped by the container's
// own corners so the effect stops where the container does.
@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = textureSample(t_source, s_source, source_uv(in.uv));

    let half_size = params.dst_size * 0.5;
    let p = in.uv * params.dst_size - half_size;
    let distance = rounded_box_sdf(p, half_size, params.radii, params.curvature);
    // One pixel of feathering: the mask is the only anti-aliasing the edge gets.
    let mask = 1.0 - smoothstep(-0.5, 0.5, distance);

    return vec4<f32>(blurred.rgb, blurred.a * mask);
}

// A contour around the coverage, drawn outside it.
//
// The dilate is the largest coverage within `stroke_width` of the pixel;
// subtracting the pixel's own coverage leaves a band that starts at the glyph
// edge and reaches outwards, which is what a stroke is. Drawing copies of the
// glyphs instead — the cheap approximation, and the right one under an opaque
// fill — would fill the letter as well as ring it, and over frost the letter is
// exactly what must stay clear.
@fragment
fn fs_outline(in: VertexOutput) -> @location(0) vec4<f32> {
    let mask_uv = params.mask_rect.xy + in.uv * params.mask_rect.zw;
    let own = textureSample(t_mask, s_source, mask_uv).a;

    // One physical pixel, in mask uv.
    let px = params.mask_rect.zw / max(params.dst_size, vec2<f32>(1.0));
    let taps = 16;
    let tau = 6.2831855;

    var dilated = own;
    // Two rings: the outer one decides how far the contour reaches, the inner
    // one keeps a thick contour solid rather than hollow at the corners.
    for (var ring = 1; ring <= 2; ring = ring + 1) {
        let radius = params.stroke_width * f32(ring) * 0.5;
        for (var i = 0; i < taps; i = i + 1) {
            let angle = tau * f32(i) / f32(taps);
            let offset = vec2<f32>(cos(angle), sin(angle)) * radius * px;
            dilated = max(dilated, textureSample(t_mask, s_source, mask_uv + offset).a);
        }
    }

    let contour = clamp(dilated - own, 0.0, 1.0);
    return vec4<f32>(params.stroke_color.rgb, params.stroke_color.a * contour);
}

// The same composite, shaped by a coverage mask instead of a rectangle: what
// the glyphs cover shows the blurred backdrop, everything else is left alone.
// The mask lines up one texel per destination pixel, so it is read with the
// destination uv rather than the source rect.
@fragment
fn fs_composite_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = textureSample(t_source, s_source, source_uv(in.uv));
    let mask_uv = params.mask_rect.xy + in.uv * params.mask_rect.zw;
    let coverage = textureSample(t_mask, s_source, mask_uv).a;

    return vec4<f32>(blurred.rgb, blurred.a * coverage);
}
