// Guido GPU Shader - SDF-based shape rendering with transforms
//
// This shader renders rounded rectangles, circles, and other shapes using
// signed distance fields (SDF) for crisp anti-aliased edges at any resolution.

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) shape_rect: vec4<f32>,  // min_x, min_y, max_x, max_y in NDC
    @location(3) shape_radius: vec2<f32>, // corner radius in NDC (x, y)
    @location(4) shape_curvature: vec2<f32>, // x = curvature, y = clip_radius (uniform)
    @location(5) border_width: vec2<f32>,  // border width in NDC (x, y)
    @location(6) border_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,  // shadow offset in NDC (x, y)
    @location(8) shadow_params: vec2<f32>,  // x = blur, y = spread
    @location(9) shadow_color: vec4<f32>,
    @location(10) transform_row0: vec4<f32>,  // 2D affine row 0: [a, b, 0, tx]
    @location(11) transform_row1: vec4<f32>,  // 2D affine row 1: [c, d, 0, ty]
    @location(12) clip_inv_row0: vec4<f32>,   // inverse transform for clip (row 0)
    @location(13) clip_inv_row1: vec4<f32>,   // inverse transform for clip (row 1)
    @location(14) local_pos: vec2<f32>,       // untransformed position for SDF
    @location(15) clip_rect: vec4<f32>,       // clip region: min_x, min_y, max_x, max_y in NDC
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) shape_rect: vec4<f32>,
    @location(2) shape_radius: vec2<f32>,
    @location(3) frag_pos: vec2<f32>,  // local position (untransformed) for SDF
    @location(4) shape_curvature: f32,  // superellipse K-value
    @location(5) border_width: vec2<f32>,  // border width (x, y) in NDC
    @location(6) border_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_params: vec2<f32>,  // x = blur, y = spread
    @location(9) shadow_color: vec4<f32>,
    @location(10) clip_rect: vec4<f32>,  // clip region in NDC (local space)
    @location(11) clip_radius: f32,  // clip corner radius (uniform, height-based NDC)
    @location(12) screen_pos: vec2<f32>,  // transformed position for clipping (screen space)
    @location(13) clip_inv_row0: vec4<f32>,  // inverse clip transform (row 0)
    @location(14) clip_inv_row1: vec4<f32>,  // inverse clip transform (row 1)
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Build transform matrix from 2 row vectors (2D affine transform)
    // Rows 2 and 3 are identity: [0, 0, 1, 0] and [0, 0, 0, 1]
    // WGSL mat4x4 constructor takes columns, so we transpose by extracting columns from rows
    let transform = mat4x4<f32>(
        vec4<f32>(in.transform_row0.x, in.transform_row1.x, 0.0, 0.0),
        vec4<f32>(in.transform_row0.y, in.transform_row1.y, 0.0, 0.0),
        vec4<f32>(in.transform_row0.z, in.transform_row1.z, 1.0, 0.0),
        vec4<f32>(in.transform_row0.w, in.transform_row1.w, 0.0, 1.0)
    );

    // Transform position for screen placement
    let transformed = transform * vec4<f32>(in.position, 0.0, 1.0);
    out.clip_position = vec4<f32>(transformed.xy, 0.0, 1.0);

    // Pass LOCAL position directly (untransformed) for SDF evaluation
    // Hardware interpolation gives us the correct local coordinate per-pixel
    out.frag_pos = in.local_pos;

    out.color = in.color;
    out.shape_rect = in.shape_rect;
    out.shape_radius = in.shape_radius;
    out.shape_curvature = in.shape_curvature.x;
    out.border_width = in.border_width;
    out.border_color = in.border_color;
    out.shadow_offset = in.shadow_offset;
    out.shadow_params = in.shadow_params;
    out.shadow_color = in.shadow_color;
    out.clip_rect = in.clip_rect;
    // clip_radius comes from shape_curvature.y (packed to save vertex attributes)
    out.clip_radius = in.shape_curvature.y;
    // Pass transformed position for screen-space clipping
    out.screen_pos = transformed.xy;
    // Pass clip inverse transform for rotated clipping
    out.clip_inv_row0 = in.clip_inv_row0;
    out.clip_inv_row1 = in.clip_inv_row1;

    return out;
}

// Convert CSS-style K value to superellipse exponent n
// K: -1=scoop, 0=bevel, 1=round, 2=squircle
// Returns n = 2^K
fn k_to_n(k: f32) -> f32 {
    return pow(2.0, k);
}

// Standard rounded box SDF (for K=1, circular corners)
// This is the reference implementation that's known to work correctly
fn sd_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + r;
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

// Superellipse "length" function - generalizes L2 norm
// n=1: L1 (diamond/bevel), n=2: L2 (circle), n>2: squircle
fn superellipse_length(p: vec2<f32>, n: f32) -> f32 {
    if (abs(n - 1.0) < 0.01) {
        return abs(p.x) + abs(p.y);  // L1
    } else if (abs(n - 2.0) < 0.01) {
        return length(p);  // L2
    } else {
        let ap = abs(p);
        return pow(pow(ap.x, n) + pow(ap.y, n), 1.0 / n);
    }
}

// Unified SDF for rounded rectangle with superellipse corners
fn rounded_rect_sdf(pos: vec2<f32>, rect: vec4<f32>, radius: vec2<f32>, k: f32) -> f32 {
    let min_corner = vec2<f32>(rect.x, rect.y);
    let max_corner = vec2<f32>(rect.z, rect.w);
    let center = (min_corner + max_corner) * 0.5;
    let half_size = (max_corner - min_corner) * 0.5;

    // Use average radius for simplicity
    let r = min((radius.x + radius.y) * 0.5, min(half_size.x, half_size.y));

    // For rectangles with no corners, use simple box SDF
    if (r <= 0.0) {
        let d = abs(pos - center) - half_size;
        return max(d.x, d.y);
    }

    // Position relative to center (work in first quadrant using abs)
    let p = abs(pos - center);

    // Handle scoop (concave corners) - K < 0
    if (k < 0.0) {
        // Scoop = rectangle MINUS corner circles
        // The shape is inside if: inside rectangle AND outside all corner circles

        // Box SDF (negative inside, positive outside)
        let d_box = p - half_size;
        let box_sdf = max(d_box.x, d_box.y);

        // Circle SDF for circle centered at corner with radius r
        // Circle is at position `half_size` in abs(p) space
        let circle_sdf = length(p - half_size) - r;

        // Boolean subtraction: max(box, -circle)
        // Inside shape when: box_sdf < 0 AND circle_sdf > 0
        // Which means: max(box_sdf, -circle_sdf) < 0
        return max(box_sdf, -circle_sdf);
    }

    // Convex corners (bevel, round, squircle) - K >= 0
    let n = k_to_n(k);

    // Use generalized rounded box formula with superellipse length
    let q = p - half_size + r;
    let qm = max(q, vec2<f32>(0.0, 0.0));
    let inside = min(max(q.x, q.y), 0.0);
    let corner_dist = superellipse_length(qm, n);

    return inside + corner_dist - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Check if this is an explicitly tessellated shape (no SDF needed for shape)
    // Shapes like Circle may pass a clip region as shape_rect for clipping only
    let rect_width = in.shape_rect.z - in.shape_rect.x;
    let rect_height = in.shape_rect.w - in.shape_rect.y;

    // If no valid shape_rect, pass through color directly (no clipping needed)
    if (rect_width <= 0.0 || rect_height <= 0.0) {
        return in.color;
    }

    // Check if this is a clipped explicit shape (like circles)
    // Marker: border_color.r == -1.0 indicates clip-only mode
    let is_clip_only = in.border_color.r < 0.0;

    // Use local position directly (passed from vertex shader via hardware interpolation)
    // No inverse transform needed - this is the browser-standard approach
    let local_pos = in.frag_pos;

    // Compute aspect ratio from shape_rect to make coordinate space isotropic
    // This is independent of rotation - uses the shape's own proportions
    // shape_rect: [min_x, min_y, max_x, max_y] in NDC
    let rect_width_ndc = in.shape_rect.z - in.shape_rect.x;
    let rect_height_ndc = in.shape_rect.w - in.shape_rect.y;

    // The aspect ratio is derived from how the shape was converted to NDC
    // In NDC: width_ndc = (pixel_width / screen_width) * 2
    //         height_ndc = (pixel_height / screen_height) * 2
    // So: aspect = height_ndc/width_ndc * (pixel_width/pixel_height) for a square shape
    // But we can use radius ratio which directly encodes the screen aspect ratio
    var aspect: f32;
    if (in.shape_radius.x > 0.0001) {
        // Use radius ratio - this directly encodes screen aspect ratio
        // radius_x = (r / screen_width) * 2, radius_y = (r / screen_height) * 2
        // So radius_y / radius_x = screen_width / screen_height = aspect
        aspect = in.shape_radius.y / in.shape_radius.x;
    } else if (in.border_width.x > 0.0001) {
        // Use border_width ratio as fallback
        aspect = in.border_width.y / in.border_width.x;
    } else {
        // Last resort: compute from screen-space derivatives
        // NOTE: This breaks under rotation, but we rarely hit this case
        let dx = abs(dpdx(local_pos.x));
        let dy = abs(dpdy(local_pos.y));
        aspect = dy / max(dx, 0.0001);
    }

    // Scale x coordinates to create uniform pixel density space
    let scaled_pos = vec2<f32>(local_pos.x * aspect, local_pos.y);
    let scaled_rect = vec4<f32>(
        in.shape_rect.x * aspect,
        in.shape_rect.y,
        in.shape_rect.z * aspect,
        in.shape_rect.w
    );
    let scaled_radius = vec2<f32>(in.shape_radius.x * aspect, in.shape_radius.y);

    // Compute signed distance in scaled (isotropic) space
    let dist = rounded_rect_sdf(scaled_pos, scaled_rect, scaled_radius, in.shape_curvature);

    // Anti-aliasing: use fwidth on the DISTANCE value (browser-standard approach)
    // This measures how fast the distance changes per screen pixel
    // and automatically handles rotation and aspect ratio
    let aa = fwidth(dist);

    // Use border_width.y since we scaled to match y's pixel density
    let border_w = in.border_width.y;

    // Clip-only case: explicitly tessellated shapes (like circles) using shape_rect for clipping
    if (is_clip_only) {
        // Use SDF to compute clip alpha - inside clip region = visible
        let clip_alpha = 1.0 - smoothstep(-aa, aa, dist);
        return vec4<f32>(in.color.rgb, in.color.a * clip_alpha);
    }

    // Compute shadow if enabled (shadow_color.a > 0)
    var shadow_contribution = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.shadow_color.a > 0.0) {
        let shadow_blur = in.shadow_params.x;
        let shadow_spread = in.shadow_params.y;

        // Scale shadow offset to match aspect ratio
        let scaled_shadow_offset = vec2<f32>(in.shadow_offset.x * aspect, in.shadow_offset.y);

        // Compute shadow position (offset from shape)
        let shadow_pos = scaled_pos - scaled_shadow_offset;

        // Scale shadow spread
        let scaled_spread = shadow_spread * aspect;

        // Compute shadow SDF (expanded by spread)
        let shadow_dist = rounded_rect_sdf(shadow_pos, scaled_rect, scaled_radius, in.shape_curvature) - scaled_spread;

        // Convert to alpha with blur falloff
        // Use wider range for smoother fadeout: from -blur to 2*blur
        // This creates a more gradual, CSS-like shadow that fades naturally
        let shadow_alpha = in.shadow_color.a * (1.0 - smoothstep(-shadow_blur, shadow_blur * 2.0, shadow_dist));
        shadow_contribution = vec4<f32>(in.shadow_color.rgb, shadow_alpha);
    }

    // Compute main shape color
    var shape_result: vec4<f32>;

    // No border case - simple filled shape (SDF defines the shape)
    if (in.border_width.x <= 0.0 && in.border_width.y <= 0.0) {
        let alpha = 1.0 - smoothstep(-aa, aa, dist);
        shape_result = vec4<f32>(in.color.rgb, in.color.a * alpha);
    } else {
        // Outer edge: shape boundary (dist = 0)
        // Inner edge: dist = -border_w (inside by border width)
        let outer_edge = dist;
        let inner_edge = dist + border_w;

        // Shape alpha (inside the outer boundary)
        let shape_alpha = 1.0 - smoothstep(-aa, aa, outer_edge);

        // Fill alpha (inside the inner boundary)
        let fill_alpha = 1.0 - smoothstep(-aa, aa, inner_edge);

        // Border alpha = shape minus fill
        let border_alpha = max(shape_alpha - fill_alpha, 0.0);

        // Handle border-only case (transparent fill)
        if (in.color.a <= 0.0) {
            shape_result = vec4<f32>(in.border_color.rgb, in.border_color.a * border_alpha);
        } else {
            // Composite fill and border
            let fill_contribution = vec4<f32>(in.color.rgb, in.color.a * fill_alpha);
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

    // Composite shadow behind shape
    var final_result: vec4<f32>;
    if (shadow_contribution.a > 0.0) {
        let final_rgb = shape_result.rgb * shape_result.a +
                        shadow_contribution.rgb * shadow_contribution.a * (1.0 - shape_result.a);
        let final_a = shape_result.a + shadow_contribution.a * (1.0 - shape_result.a);
        final_result = vec4<f32>(final_rgb, final_a);
    } else {
        final_result = shape_result;
    }

    // Apply clip region if defined (clip_rect width > 0)
    // clip_rect is in local (untransformed) space, so we need to transform
    // screen_pos back to local space using the inverse clip transform.
    let clip_width = in.clip_rect.z - in.clip_rect.x;
    let clip_height = in.clip_rect.w - in.clip_rect.y;
    if (clip_width > 0.0 && clip_height > 0.0) {
        // Transform screen_pos back to clip-local space using inverse transform.
        // This handles rotated/transformed containers correctly.
        // The inverse transform is a 2D affine transform stored in two rows:
        // | a  b  0  tx |  -> row0 = [a, b, 0, tx]
        // | c  d  0  ty |  -> row1 = [c, d, 0, ty]
        let inv_a = in.clip_inv_row0.x;
        let inv_b = in.clip_inv_row0.y;
        let inv_tx = in.clip_inv_row0.w;
        let inv_c = in.clip_inv_row1.x;
        let inv_d = in.clip_inv_row1.y;
        let inv_ty = in.clip_inv_row1.w;

        // Apply inverse transform to get local position for clip testing
        let local_clip_x = inv_a * in.screen_pos.x + inv_b * in.screen_pos.y + inv_tx;
        let local_clip_y = inv_c * in.screen_pos.x + inv_d * in.screen_pos.y + inv_ty;
        let local_clip_pos = vec2<f32>(local_clip_x, local_clip_y);

        // Scale local position and clip region for aspect ratio
        let scaled_local_clip_pos = vec2<f32>(local_clip_pos.x * aspect, local_clip_pos.y);
        let scaled_clip_rect = vec4<f32>(
            in.clip_rect.x * aspect,
            in.clip_rect.y,
            in.clip_rect.z * aspect,
            in.clip_rect.w
        );
        // clip_radius is height-based NDC, which is already in "uniform" units
        // (the same units that y coordinates use after aspect scaling).
        // No additional scaling needed - both components should be equal for circular corners.
        let scaled_clip_radius = vec2<f32>(in.clip_radius, in.clip_radius);

        // Compute clip SDF (use K=1.0 for circular corners)
        let clip_dist = rounded_rect_sdf(scaled_local_clip_pos, scaled_clip_rect, scaled_clip_radius, 1.0);
        let clip_aa = fwidth(clip_dist);
        let clip_alpha = 1.0 - smoothstep(-clip_aa, clip_aa, clip_dist);

        final_result = vec4<f32>(final_result.rgb, final_result.a * clip_alpha);
    }

    return final_result;
}
