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
    // The shape the mask cuts, [x, y, width, height], in its own space.
    shape_rect: vec4<f32>,
    // Where this viewport sits on the target, in physical pixels.
    viewport_origin: vec2<f32>,
    // Size of the coverage mask in texels.
    mask_size: vec2<f32>,
    // Target physical pixels to shape space: [a, b, tx, c] and [d, ty].
    to_shape_0: vec4<f32>,
    to_shape_1: vec2<f32>,
    _pad5: vec2<f32>,
    // Colour of the outline drawn by `fs_outline`, and how far it reaches out
    // from the glyph edge in mask texels.
    stroke_color: vec4<f32>,
    stroke_width: f32,
    _pad1: f32,
    _pad2: f32,
    _pad3: f32,
}

@group(0) @binding(0) var t_source: texture_2d<f32>;
@group(0) @binding(1) var s_source: sampler;
@group(0) @binding(2) var<uniform> params: Params;
// Coverage mask for `fs_composite_mask`, covering `shape_rect` in the shape's
// own space. Declared for every entry point, bound only by the pipeline that
// reads it.
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

// The fragment on the target, then back in the shape's own space — where the
// shape is an ordinary rect whatever it has been turned or stretched by. The
// viewport is the box around that shape, so this is also what cuts the box
// back down to it, and where a coverage mask is read.
fn in_shape_space(uv: vec2<f32>) -> vec2<f32> {
    let on_target = params.viewport_origin + uv * params.dst_size;
    return vec2<f32>(
        params.to_shape_0.x * on_target.x + params.to_shape_0.y * on_target.y + params.to_shape_0.z,
        params.to_shape_0.w * on_target.x + params.to_shape_1.x * on_target.y + params.to_shape_1.y
    );
}

// Where in a coverage mask this fragment falls: the mask covers `shape_rect`,
// so its uv is the fragment's place within that rect.
fn mask_uv(uv: vec2<f32>) -> vec2<f32> {
    return (in_shape_space(uv) - params.shape_rect.xy) / params.shape_rect.zw;
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

// === SHARED SDF — three copies, kept identical by
// `tests/shader_sdf_is_one_definition.rs` ===
//
// WGSL has no include, so the corner geometry lives once per pipeline that
// cuts a shape: the shape shader, the textured quad (images and transformed
// text), and the backdrop composite. A clip is one shape and a corner is one
// curve; three spellings of them is three answers to the same question, which
// is what #403 was — the backdrop read `k` as `max(k, 0.05) * 2` where these
// read `pow(2, k)`, so a bevel blurred a concave notch inside its own
// octagonal border.
//
// A fourth copy is in Rust: `Rect::shape_distance` in `src/widgets/widget.rs`,
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

// Write the blurred region back over the target, shaped by the container's
// own corners so the effect stops where the container does.
@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = textureSample(t_source, s_source, source_uv(in.uv));

    let p = in_shape_space(in.uv);
    let distance = rounded_rect_sdf(p, params.shape_rect, params.radii, params.curvature);
    // One pixel of feathering, however the shape is placed. The distance is in
    // shape space now, so the fixed half-pixel threshold this used to carry
    // would widen under a scale and shear under a rotation; the derivative is
    // that pixel measured on the *target*, whatever sits between.
    //
    // Halved so an untransformed shape lands on the old threshold. It lands
    // there exactly along the straight edges, where `fwidth` is 1 — and about
    // 1.41 across a corner, because `fwidth` sums the two partials rather than
    // taking their length. So an upright card's corners feather a third of a
    // pixel softer than they did: measured at 104 pixels of a 110x70 card's
    // perimeter, none of them off by more than 8 of 255.
    let aa = max(fwidth(distance) * 0.5, 0.0001);
    let mask = 1.0 - smoothstep(-aa, aa, distance);

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
    let centre = mask_uv(in.uv);
    let own = textureSample(t_mask, s_source, centre).a;

    // One mask texel, in mask uv — which is what the stroke width is measured
    // in, so the contour is dilated in the letters' own space and arrives on
    // the target stretched exactly as they are.
    let px = 1.0 / max(params.mask_size, vec2<f32>(1.0));
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
            dilated = max(dilated, textureSample(t_mask, s_source, centre + offset).a);
        }
    }

    let contour = clamp(dilated - own, 0.0, 1.0);
    return vec4<f32>(params.stroke_color.rgb, params.stroke_color.a * contour);
}

// The same composite, shaped by a coverage mask instead of a rectangle: what
// the glyphs cover shows the blurred backdrop, everything else is left alone.
//
// The mask is rasterized in the text's own space, not laid over the target, so
// the fragment is carried back there to find its texel — the same journey the
// corners take in `fs_composite`. That is what lets a turned or stretched text
// keep its frost: the coverage goes where the letters go.
@fragment
fn fs_composite_mask(in: VertexOutput) -> @location(0) vec4<f32> {
    let blurred = textureSample(t_source, s_source, source_uv(in.uv));
    let coverage = textureSample(t_mask, s_source, mask_uv(in.uv)).a;

    return vec4<f32>(blurred.rgb, blurred.a * coverage);
}
