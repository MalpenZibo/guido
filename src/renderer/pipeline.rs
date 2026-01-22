use wgpu::{Device, RenderPipeline, TextureFormat};

use super::primitives::Vertex;

const SHADER_SOURCE: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) shape_rect: vec4<f32>,  // min_x, min_y, max_x, max_y in NDC
    @location(3) shape_radius: vec2<f32>, // corner radius in NDC (x, y)
    @location(4) shape_curvature: vec2<f32>, // x = curvature, y = padding
    @location(5) border_width: vec2<f32>,  // border width in NDC (x, y)
    @location(6) border_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,  // shadow offset in NDC (x, y)
    @location(8) shadow_params: vec2<f32>,  // x = blur, y = spread
    @location(9) shadow_color: vec4<f32>,
    @location(10) transform_row0: vec4<f32>,
    @location(11) transform_row1: vec4<f32>,
    @location(12) transform_row2: vec4<f32>,
    @location(13) transform_row3: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) shape_rect: vec4<f32>,
    @location(2) shape_radius: vec2<f32>,
    @location(3) frag_pos: vec2<f32>,  // position in NDC (transformed)
    @location(4) shape_curvature: f32,  // superellipse K-value
    @location(5) border_width: vec2<f32>,  // border width (x, y) in NDC
    @location(6) border_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_params: vec2<f32>,  // x = blur, y = spread
    @location(9) shadow_color: vec4<f32>,
    @location(10) transform_row0: vec4<f32>,
    @location(11) transform_row1: vec4<f32>,
    @location(12) transform_row2: vec4<f32>,
    @location(13) transform_row3: vec4<f32>,
}

// Compute 2D affine inverse for a row-major 4x4 matrix
// Used in fragment shader to transform screen coordinates back to local space
fn inverse_2d_affine(r0: vec4<f32>, r1: vec4<f32>) -> mat3x2<f32> {
    // Extract 2D affine components:
    // | a  b  tx |   from   | r0.x r0.y r0.z r0.w |
    // | c  d  ty |          | r1.x r1.y r1.z r1.w |
    let a = r0.x;
    let b = r0.y;
    let tx = r0.w;
    let c = r1.x;
    let d = r1.y;
    let ty = r1.w;

    let det = a * d - b * c;
    let inv_det = select(1.0 / det, 0.0, abs(det) < 1e-10);

    // Return as mat3x2 where:
    // column 0 = (d*inv, -c*inv)
    // column 1 = (-b*inv, a*inv)
    // column 2 = ((-d*tx + b*ty)*inv, (c*tx - a*ty)*inv)
    return mat3x2<f32>(
        vec2<f32>(d * inv_det, -c * inv_det),
        vec2<f32>(-b * inv_det, a * inv_det),
        vec2<f32>((-d * tx + b * ty) * inv_det, (c * tx - a * ty) * inv_det)
    );
}

// Apply inverse transform to a 2D point
fn apply_inverse_transform(p: vec2<f32>, inv: mat3x2<f32>) -> vec2<f32> {
    return vec2<f32>(
        inv[0].x * p.x + inv[1].x * p.y + inv[2].x,
        inv[0].y * p.x + inv[1].y * p.y + inv[2].y
    );
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;

    // Build transform matrix from row vectors
    // WGSL mat4x4 constructor takes columns, so we transpose by extracting columns from rows
    let transform = mat4x4<f32>(
        vec4<f32>(in.transform_row0.x, in.transform_row1.x, in.transform_row2.x, in.transform_row3.x),
        vec4<f32>(in.transform_row0.y, in.transform_row1.y, in.transform_row2.y, in.transform_row3.y),
        vec4<f32>(in.transform_row0.z, in.transform_row1.z, in.transform_row2.z, in.transform_row3.z),
        vec4<f32>(in.transform_row0.w, in.transform_row1.w, in.transform_row2.w, in.transform_row3.w)
    );

    // Transform position
    let transformed = transform * vec4<f32>(in.position, 0.0, 1.0);
    out.clip_position = vec4<f32>(transformed.xy, 0.0, 1.0);
    out.frag_pos = transformed.xy;

    out.color = in.color;
    out.shape_rect = in.shape_rect;
    out.shape_radius = in.shape_radius;
    out.shape_curvature = in.shape_curvature.x;
    out.border_width = in.border_width;
    out.border_color = in.border_color;
    out.shadow_offset = in.shadow_offset;
    out.shadow_params = in.shadow_params;
    out.shadow_color = in.shadow_color;

    // Pass transform rows to fragment shader for inverse calculation
    out.transform_row0 = in.transform_row0;
    out.transform_row1 = in.transform_row1;
    out.transform_row2 = in.transform_row2;
    out.transform_row3 = in.transform_row3;

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

    // Check if this is a clipped explicit shape (like ripple circles)
    // Marker: border_color.r == -1.0 indicates clip-only mode
    let is_clip_only = in.border_color.r < 0.0;

    // Compute inverse transform to map fragment position back to local space
    let inv_transform = inverse_2d_affine(in.transform_row0, in.transform_row1);

    // Transform fragment position back to local (untransformed) space for SDF evaluation
    let local_pos = apply_inverse_transform(in.frag_pos, inv_transform);

    // Compute aspect ratio to make coordinate space isotropic
    // Use screen-space derivatives to get aspect ratio even when border_width is 0
    var aspect: f32;
    if (in.border_width.x > 0.0001) {
        // Use border_width ratio when available
        aspect = in.border_width.y / in.border_width.x;
    } else {
        // Compute from screen-space derivatives
        let dx = abs(dpdx(in.frag_pos.x));  // NDC change per screen pixel in x
        let dy = abs(dpdy(in.frag_pos.y));  // NDC change per screen pixel in y
        aspect = dy / max(dx, 0.0001);
    }

    // Scale x coordinates to create uniform pixel density space
    // Use local_pos for SDF evaluation (untransformed space)
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

    // Anti-aliasing factor based on screen-space derivatives (in scaled space)
    let aa = length(fwidth(scaled_pos)) * 0.5;

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
    if (shadow_contribution.a > 0.0) {
        let final_rgb = shape_result.rgb * shape_result.a +
                        shadow_contribution.rgb * shadow_contribution.a * (1.0 - shape_result.a);
        let final_a = shape_result.a + shadow_contribution.a * (1.0 - shape_result.a);
        return vec4<f32>(final_rgb, final_a);
    }

    return shape_result;
}
"#;

pub fn create_render_pipeline(device: &Device, format: TextureFormat) -> RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Guido Shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Guido Pipeline Layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Guido Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::desc()],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}
