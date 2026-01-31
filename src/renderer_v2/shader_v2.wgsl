// Guido V2 Instanced Shader - SDF-based shape rendering
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

struct InstanceInput {
    // rect: [x, y, width, height] in logical pixels
    @location(1) rect: vec4<f32>,
    // corner_radius, shape_curvature, _pad, _pad
    @location(2) shape_params: vec4<f32>,
    // fill_color RGBA
    @location(3) fill_color: vec4<f32>,
    // border_color RGBA
    @location(4) border_color: vec4<f32>,
    // border_width, _pad, _pad, _pad
    @location(5) border_params: vec4<f32>,
    // shadow_offset.xy, shadow_blur, shadow_spread
    @location(6) shadow_params: vec4<f32>,
    // shadow_color RGBA
    @location(7) shadow_color: vec4<f32>,
    // transform: a, b, tx, c
    @location(8) transform_0: vec4<f32>,
    // transform: d, ty, _pad, _pad
    @location(9) transform_1: vec4<f32>,
    // clip_rect: [x, y, width, height] in logical pixels (scaled to physical in render.rs)
    @location(10) clip_rect: vec4<f32>,
    // clip_corner_radius, clip_curvature, clip_is_local, _pad
    @location(11) clip_params: vec4<f32>,
    // gradient_start RGBA
    @location(12) gradient_start: vec4<f32>,
    // gradient_end RGBA
    @location(13) gradient_end: vec4<f32>,
    // gradient_type (0=none, 1=horizontal, 2=vertical, 3=diagonal, 4=diagonal_reverse), _pad, _pad, _pad
    @location(14) gradient_params: vec4<u32>,
}

// === Vertex Output ===

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) fill_color: vec4<f32>,
    @location(1) border_color: vec4<f32>,
    // Fragment position in logical pixels (for SDF computation)
    @location(2) frag_pos: vec2<f32>,
    // Shape rect in logical pixels [x, y, width, height]
    @location(3) shape_rect: vec4<f32>,
    // corner_radius, shape_curvature
    @location(4) shape_params: vec2<f32>,
    // border_width
    @location(5) border_width: f32,
    // shadow_offset.xy, shadow_blur, shadow_spread
    @location(6) shadow_params: vec4<f32>,
    // shadow_color
    @location(7) shadow_color: vec4<f32>,
    // World position in logical pixels (for clip computation)
    @location(8) world_pos: vec2<f32>,
    // Clip rect in logical pixels (scaled to physical in render.rs)
    @location(9) clip_rect: vec4<f32>,
    // Clip corner_radius, curvature, is_local
    @location(10) clip_params: vec3<f32>,
    // Gradient start color
    @location(11) gradient_start: vec4<f32>,
    // Gradient end color
    @location(12) gradient_end: vec4<f32>,
    // Gradient type (0=none, 1=horizontal, 2=vertical, 3=diagonal, 4=diagonal_reverse)
    @location(13) @interpolate(flat) gradient_type: u32,
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

// === Vertex Shader ===

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    // Extract transform components
    // Note: transform already includes center_at from CPU, so no origin handling needed here
    let a = instance.transform_0.x;
    let b = instance.transform_0.y;
    let tx = instance.transform_0.z;
    let c = instance.transform_0.w;
    let d = instance.transform_1.x;
    let ty = instance.transform_1.y;

    // Expand quad for shadow if needed
    let shadow_blur = instance.shadow_params.z;
    let shadow_spread = instance.shadow_params.w;
    let shadow_offset = instance.shadow_params.xy;
    let has_shadow = instance.shadow_color.a > 0.0;

    // Calculate shadow expansion (3x blur for smooth fadeout)
    let fadeout = 3.0;
    var expand = vec4<f32>(0.0, 0.0, 0.0, 0.0);  // left, top, right, bottom
    if (has_shadow) {
        expand.x = max(shadow_blur * fadeout - shadow_offset.x, 0.0) + shadow_spread;
        expand.y = max(shadow_blur * fadeout - shadow_offset.y, 0.0) + shadow_spread;
        expand.z = max(shadow_blur * fadeout + shadow_offset.x, 0.0) + shadow_spread;
        expand.w = max(shadow_blur * fadeout + shadow_offset.y, 0.0) + shadow_spread;
    }

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

    // Pass world position (in logical pixels) for clip computation
    out.world_pos = world_pos;

    // Pass instance data to fragment shader
    out.fill_color = instance.fill_color;
    out.border_color = instance.border_color;
    out.shape_rect = instance.rect;
    out.shape_params = instance.shape_params.xy;  // corner_radius, curvature
    out.border_width = instance.border_params.x;
    out.shadow_params = instance.shadow_params;
    out.shadow_color = instance.shadow_color;

    // Pass clip data to fragment shader
    out.clip_rect = instance.clip_rect;
    out.clip_params = instance.clip_params.xyz;  // corner_radius, curvature, is_local

    // Pass gradient data to fragment shader
    out.gradient_start = instance.gradient_start;
    out.gradient_end = instance.gradient_end;
    out.gradient_type = instance.gradient_params.x;

    return out;
}

// === SDF Functions ===

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
// rect: [x, y, width, height], radius: corner radius, k: curvature
fn rounded_rect_sdf(pos: vec2<f32>, rect: vec4<f32>, radius: f32, k: f32) -> f32 {
    let center = vec2<f32>(rect.x + rect.z * 0.5, rect.y + rect.w * 0.5);
    let half_size = vec2<f32>(rect.z * 0.5, rect.w * 0.5);

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
    return mix(start_color, end_color, clamp(t, 0.0, 1.0));
}

// === Fragment Shader ===

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Early discard for zero-area shapes
    if (in.shape_rect.z <= 0.0 || in.shape_rect.w <= 0.0) {
        discard;
    }

    let pos = in.frag_pos;
    let radius = in.shape_params.x;
    let curvature = in.shape_params.y;

    // Compute SDF for the shape
    let dist = rounded_rect_sdf(pos, in.shape_rect, radius, curvature);

    // Anti-aliasing using fwidth
    let aa = fwidth(dist) * 1.0;

    // Compute local UV coordinates (0..1 within the shape rect)
    let local_uv = (pos - in.shape_rect.xy) / in.shape_rect.zw;

    // Determine fill color (gradient or solid)
    var fill_color: vec4<f32>;
    if (in.gradient_type > 0u) {
        fill_color = compute_gradient_color(local_uv, in.gradient_start, in.gradient_end, in.gradient_type);
    } else {
        fill_color = in.fill_color;
    }

    // === Shadow ===
    var shadow_contribution = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.shadow_color.a > 0.0) {
        let shadow_offset = in.shadow_params.xy;
        let shadow_blur = in.shadow_params.z;
        let shadow_spread = in.shadow_params.w;

        // Compute shadow position (offset from shape)
        let shadow_pos = pos - shadow_offset;

        // Compute shadow SDF (expanded by spread)
        let shadow_dist = rounded_rect_sdf(shadow_pos, in.shape_rect, radius, curvature) - shadow_spread;

        // Convert to alpha with blur falloff
        let shadow_alpha = in.shadow_color.a * (1.0 - smoothstep(-shadow_blur, shadow_blur * 2.0, shadow_dist));
        shadow_contribution = vec4<f32>(in.shadow_color.rgb, shadow_alpha);
    }

    // === Main Shape ===
    var shape_result: vec4<f32>;

    if (in.border_width <= 0.0) {
        // No border - simple filled shape
        let alpha = 1.0 - smoothstep(-aa, aa, dist);
        shape_result = vec4<f32>(fill_color.rgb, fill_color.a * alpha);
    } else {
        // With border
        let outer_edge = dist;
        let inner_edge = dist + in.border_width;

        let shape_alpha = 1.0 - smoothstep(-aa, aa, outer_edge);
        let fill_alpha = 1.0 - smoothstep(-aa, aa, inner_edge);
        let border_alpha = max(shape_alpha - fill_alpha, 0.0);

        if (fill_color.a <= 0.0) {
            // Border only (transparent fill)
            shape_result = vec4<f32>(in.border_color.rgb, in.border_color.a * border_alpha);
        } else {
            // Fill + border composite
            let fill_contribution = vec4<f32>(fill_color.rgb, fill_color.a * fill_alpha);
            let border_contribution = vec4<f32>(in.border_color.rgb, in.border_color.a * border_alpha);

            let result_rgb = border_contribution.rgb * border_contribution.a +
                             fill_contribution.rgb * fill_contribution.a * (1.0 - border_contribution.a);
            let result_a = border_contribution.a + fill_contribution.a * (1.0 - border_contribution.a);

            if (result_a <= 0.0) {
                shape_result = vec4<f32>(0.0, 0.0, 0.0, 0.0);
            } else {
                shape_result = vec4<f32>(result_rgb / result_a, result_a);
            }
        }
    }

    // === Composite shadow behind shape ===
    var final_result: vec4<f32>;
    if (shadow_contribution.a > 0.0) {
        let final_rgb = shape_result.rgb * shape_result.a +
                        shadow_contribution.rgb * shadow_contribution.a * (1.0 - shape_result.a);
        let final_a = shape_result.a + shadow_contribution.a * (1.0 - shape_result.a);
        final_result = vec4<f32>(final_rgb, final_a);
    } else {
        final_result = shape_result;
    }

    // === Apply clipping ===
    // Check if clipping is enabled (width and height > 0)
    if (in.clip_rect.z > 0.0 && in.clip_rect.w > 0.0) {
        // Use frag_pos for local clips (overlay clips on transformed containers),
        // world_pos for world clips (regular clipping)
        let clip_pos = select(in.world_pos, in.frag_pos, in.clip_params.z > 0.5);

        // Compute clip SDF
        let clip_dist = rounded_rect_sdf(
            clip_pos,
            in.clip_rect,
            in.clip_params.x,  // corner_radius
            in.clip_params.y   // curvature
        );

        // Smooth clip edge (anti-aliased)
        let clip_aa = fwidth(clip_dist);
        let clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);

        // Apply clip to final result
        final_result = vec4<f32>(final_result.rgb, final_result.a * clip_alpha);
    }

    return final_result;
}
