use crate::transform::Transform;
use crate::widgets::{Color, Rect};

/// Convert a transform from logical screen space to NDC (Normalized Device Coordinates).
///
/// This handles aspect ratio correction for rotation and proper translation scaling.
/// The transform is centered at the specified point (or the default center if None).
///
/// # Arguments
/// * `transform` - The transform in logical screen coordinates
/// * `transform_origin` - Custom transform origin in logical screen coords, if any
/// * `default_center` - Default center point in NDC (used if transform_origin is None)
/// * `screen_width` - Screen width in logical pixels
/// * `screen_height` - Screen height in logical pixels
fn transform_to_ndc(
    transform: &Transform,
    transform_origin: Option<(f32, f32)>,
    default_center: (f32, f32),
    screen_width: f32,
    screen_height: f32,
) -> Transform {
    if transform.is_identity() {
        return Transform::IDENTITY;
    }

    let aspect = screen_width / screen_height;

    // Extract rotation and scale from the transform matrix
    // The transform is in row-major: [a, b, 0, tx; c, d, 0, ty; ...]
    // For a rotation: a = cos θ, b = -sin θ, c = sin θ, d = cos θ
    let a = transform.data[0];
    let b = transform.data[1];
    let c = transform.data[4];
    let d = transform.data[5];
    let tx = transform.data[3];
    let ty = transform.data[7];

    // Build aspect-corrected transform matrix for NDC
    // For pure rotation: new_a = a, new_b = b/aspect, new_c = c*aspect, new_d = d
    let ndc_transform = Transform {
        data: [
            a,
            b / aspect,
            0.0,
            tx * 2.0 / screen_width,
            c * aspect,
            d,
            0.0,
            -ty * 2.0 / screen_height,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ],
    };

    // Center the transform at the appropriate point in NDC space
    let (center_x, center_y) = if let Some((ox, oy)) = transform_origin {
        // Custom origin: convert from logical screen coords to NDC
        let ndc_ox = (ox / screen_width) * 2.0 - 1.0;
        let ndc_oy = 1.0 - (oy / screen_height) * 2.0;
        (ndc_ox, ndc_oy)
    } else {
        default_center
    };

    ndc_transform.center_at(center_x, center_y)
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
    /// Shape rectangle in NDC: [min_x, min_y, max_x, max_y] for SDF rendering
    pub shape_rect: [f32; 4],
    /// Shape corner radius in NDC (x and y may differ due to aspect ratio)
    pub shape_radius: [f32; 2],
    /// Superellipse curvature (CSS K-value: K=-1 scoop, K=0 bevel, K=1 round, K=2 squircle)
    pub shape_curvature: f32,
    /// Clip corner radius (uniform value, used when clip_rect is set)
    pub clip_radius: f32,
    /// Border width in NDC (x and y separately for aspect ratio correction)
    pub border_width: [f32; 2],
    /// Border color RGBA
    pub border_color: [f32; 4],
    /// Shadow offset in NDC (x, y)
    pub shadow_offset: [f32; 2],
    /// Shadow blur radius in NDC
    pub shadow_blur: f32,
    /// Shadow spread amount (expands shadow)
    pub shadow_spread: f32,
    /// Shadow color RGBA
    pub shadow_color: [f32; 4],
    /// Transform matrix row 0 (2D affine: [a, b, 0, tx])
    pub transform_row0: [f32; 4],
    /// Transform matrix row 1 (2D affine: [c, d, 0, ty])
    pub transform_row1: [f32; 4],
    /// Inverse transform for clip region (row 0): [a, b, 0, tx]
    /// Used to transform screen_pos back to clip-local space for proper rotated clipping
    pub clip_inv_transform_row0: [f32; 4],
    /// Inverse transform for clip region (row 1): [c, d, 0, ty]
    pub clip_inv_transform_row1: [f32; 4],
    /// Local position (untransformed) for SDF evaluation in fragment shader
    pub local_pos: [f32; 2],
    /// Clip rectangle in NDC: [min_x, min_y, max_x, max_y] - (0,0,0,0) means no clip
    /// When clip is enabled, uses shape_radius and shape_curvature for clip corners
    pub clip_rect: [f32; 4],
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // shape_rect
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 6]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // shape_radius
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 10]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // shape_curvature + padding
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // border_width (x and y)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 14]>() as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // border_color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // shadow_offset
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 20]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // shadow_blur + shadow_spread
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 22]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // shadow_color
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 24]>() as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row0 (2D affine)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 28]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row1 (2D affine)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 32]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // clip_inv_transform_row0
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 36]>() as wgpu::BufferAddress,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // clip_inv_transform_row1
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 40]>() as wgpu::BufferAddress,
                    shader_location: 13,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // local_pos (untransformed position for SDF)
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 44]>() as wgpu::BufferAddress,
                    shader_location: 14,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // clip_rect
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 46]>() as wgpu::BufferAddress,
                    shader_location: 15,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }

    /// Create a vertex for a shape with SDF rendering (no border)
    pub fn new(
        position: [f32; 2],
        color: [f32; 4],
        shape_rect: [f32; 4],
        shape_radius: [f32; 2],
        shape_curvature: f32,
    ) -> Self {
        Self::with_transform(
            position,
            position, // local_pos = position (no transform)
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            [0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0],
            0.0,
            0.0,
            [0.0, 0.0, 0.0, 0.0],
            Transform::IDENTITY,
        )
    }

    /// Create a vertex for a shape with SDF rendering and border
    /// border_width is [x, y] in NDC for aspect-ratio correct borders
    pub fn with_border(
        position: [f32; 2],
        color: [f32; 4],
        shape_rect: [f32; 4],
        shape_radius: [f32; 2],
        shape_curvature: f32,
        border_width: [f32; 2],
        border_color: [f32; 4],
    ) -> Self {
        Self::with_transform(
            position,
            position, // local_pos = position (no transform)
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            border_width,
            border_color,
            [0.0, 0.0],
            0.0,
            0.0,
            [0.0, 0.0, 0.0, 0.0],
            Transform::IDENTITY,
        )
    }

    /// Create a vertex for a shape with SDF rendering, border, and shadow
    #[allow(clippy::too_many_arguments)]
    pub fn with_shadow(
        position: [f32; 2],
        color: [f32; 4],
        shape_rect: [f32; 4],
        shape_radius: [f32; 2],
        shape_curvature: f32,
        border_width: [f32; 2],
        border_color: [f32; 4],
        shadow_offset: [f32; 2],
        shadow_blur: f32,
        shadow_spread: f32,
        shadow_color: [f32; 4],
    ) -> Self {
        Self::with_transform(
            position,
            position, // local_pos = position (no transform)
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            border_width,
            border_color,
            shadow_offset,
            shadow_blur,
            shadow_spread,
            shadow_color,
            Transform::IDENTITY,
        )
    }

    /// Create a vertex with full transform support
    #[allow(clippy::too_many_arguments)]
    pub fn with_transform(
        position: [f32; 2],
        local_pos: [f32; 2],
        color: [f32; 4],
        shape_rect: [f32; 4],
        shape_radius: [f32; 2],
        shape_curvature: f32,
        border_width: [f32; 2],
        border_color: [f32; 4],
        shadow_offset: [f32; 2],
        shadow_blur: f32,
        shadow_spread: f32,
        shadow_color: [f32; 4],
        transform: Transform,
    ) -> Self {
        Self::with_transform_and_clip(
            position,
            local_pos,
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            border_width,
            border_color,
            shadow_offset,
            shadow_blur,
            shadow_spread,
            shadow_color,
            transform,
            [0.0, 0.0, 0.0, 0.0], // No clip
            0.0,                  // No clip radius
            None,                 // No clip inverse transform
        )
    }

    /// Create a vertex with full transform and clip support
    #[allow(clippy::too_many_arguments)]
    pub fn with_transform_and_clip(
        position: [f32; 2],
        local_pos: [f32; 2],
        color: [f32; 4],
        shape_rect: [f32; 4],
        shape_radius: [f32; 2],
        shape_curvature: f32,
        border_width: [f32; 2],
        border_color: [f32; 4],
        shadow_offset: [f32; 2],
        shadow_blur: f32,
        shadow_spread: f32,
        shadow_color: [f32; 4],
        transform: Transform,
        clip_rect: [f32; 4],
        clip_radius: f32,
        clip_inv_transform: Option<Transform>,
    ) -> Self {
        let rows = transform.rows();
        // Extract the 2D affine inverse transform rows (only need first 2 rows for 2D)
        let (clip_inv_row0, clip_inv_row1) = if let Some(inv) = clip_inv_transform {
            let inv_rows = inv.rows();
            (inv_rows[0], inv_rows[1])
        } else {
            // Identity: no inverse transform needed
            ([1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0])
        };
        Self {
            position,
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            clip_radius,
            border_width,
            border_color,
            shadow_offset,
            shadow_blur,
            shadow_spread,
            shadow_color,
            // Only store first 2 rows of transform (2D affine)
            transform_row0: rows[0],
            transform_row1: rows[1],
            clip_inv_transform_row0: clip_inv_row0,
            clip_inv_transform_row1: clip_inv_row1,
            local_pos,
            clip_rect,
        }
    }

    /// Set the transform on an existing vertex (only 2D affine components)
    pub fn set_transform(&mut self, transform: Transform) {
        let rows = transform.rows();
        self.transform_row0 = rows[0];
        self.transform_row1 = rows[1];
    }
}

/// Gradient direction for linear gradients
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientDir {
    Horizontal,
    Vertical,
    Diagonal,
    DiagonalReverse,
}

/// Optional gradient for shapes
#[derive(Debug, Clone, Copy)]
pub struct Gradient {
    pub start_color: Color,
    pub end_color: Color,
    pub direction: GradientDir,
}

/// Shadow configuration for shapes
#[derive(Debug, Clone, Copy)]
pub struct Shadow {
    /// Shadow offset in logical pixels (x, y)
    pub offset: (f32, f32),
    /// Blur radius in logical pixels
    pub blur: f32,
    /// Spread amount in logical pixels (expands shadow)
    pub spread: f32,
    /// Shadow color
    pub color: Color,
}

impl Shadow {
    /// Create a shadow with the given parameters
    pub fn new(offset: (f32, f32), blur: f32, spread: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread,
            color,
        }
    }

    /// Create a shadow with no spread
    pub fn simple(offset: (f32, f32), blur: f32, color: Color) -> Self {
        Self {
            offset,
            blur,
            spread: 0.0,
            color,
        }
    }

    /// Create a default shadow (no shadow)
    pub fn none() -> Self {
        Self {
            offset: (0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color::TRANSPARENT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoundedRect {
    pub rect: Rect,
    pub color: Color,
    pub radius: f32,
    /// Optional clip region for this rect
    pub clip: Option<ClipRegion>,
    /// Optional gradient (overrides solid color)
    pub gradient: Option<Gradient>,
    /// Superellipse curvature K-value (default 1.0 = circle)
    pub curvature: f32,
    /// Border width in logical pixels (0 = no border)
    pub border_width: f32,
    /// Border color
    pub border_color: Color,
    /// Shadow configuration
    pub shadow: Shadow,
    /// Transform matrix for this shape
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    /// If None, transform is centered at the shape's center
    pub transform_origin: Option<(f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct ClipRegion {
    pub rect: Rect,
    pub radius: f32,
    /// Superellipse curvature K-value (default 1.0 = circle)
    pub curvature: f32,
    /// Optional transform for the clip region (for rotated/scaled clips)
    /// This allows clipping to transformed container bounds without transforming the shape itself
    pub transform: Option<Transform>,
    /// Transform origin for the clip, if transform is set
    pub transform_origin: Option<(f32, f32)>,
}

impl Default for RoundedRect {
    fn default() -> Self {
        Self {
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            color: Color::TRANSPARENT,
            radius: 0.0,
            clip: None,
            gradient: None,
            curvature: 1.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_origin: None,
        }
    }
}

impl RoundedRect {
    pub fn new(rect: Rect, color: Color, radius: f32) -> Self {
        Self {
            rect,
            color,
            radius,
            ..Default::default()
        }
    }

    pub fn with_clip(rect: Rect, color: Color, radius: f32, clip: ClipRegion) -> Self {
        Self::new(rect, color, radius).clip_region(clip)
    }

    pub fn with_gradient(rect: Rect, gradient: Gradient, radius: f32) -> Self {
        Self::new(rect, gradient.start_color, radius).gradient(gradient)
    }

    pub fn with_curvature(rect: Rect, color: Color, radius: f32, curvature: f32) -> Self {
        Self::new(rect, color, radius).curvature(curvature)
    }

    /// Create a rounded rect with a border
    pub fn with_border(
        rect: Rect,
        fill_color: Color,
        radius: f32,
        border_width: f32,
        border_color: Color,
    ) -> Self {
        Self::new(rect, fill_color, radius).border(border_width, border_color)
    }

    /// Create a border-only rounded rect (transparent fill)
    pub fn border_only(rect: Rect, radius: f32, border_width: f32, border_color: Color) -> Self {
        Self::new(rect, Color::TRANSPARENT, radius).border(border_width, border_color)
    }

    /// Create a border-only rounded rect with custom curvature
    pub fn border_only_with_curvature(
        rect: Rect,
        radius: f32,
        border_width: f32,
        border_color: Color,
        curvature: f32,
    ) -> Self {
        Self::border_only(rect, radius, border_width, border_color).curvature(curvature)
    }

    // Builder methods for chainable configuration

    /// Set the superellipse curvature (K-value).
    /// K=1 is circular (default), K=2 is squircle, K=0 is bevel, K=-1 is scoop.
    pub fn curvature(mut self, curvature: f32) -> Self {
        self.curvature = curvature;
        self
    }

    /// Set a linear gradient (overrides solid color).
    pub fn gradient(mut self, gradient: Gradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Set a border with width and color.
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border_width = width;
        self.border_color = color;
        self
    }

    /// Set a clip region for this shape.
    pub fn clip_region(mut self, clip: ClipRegion) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Set the shadow configuration.
    pub fn shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = shadow;
        self
    }

    /// Calculate safe progress value, avoiding division by zero
    #[inline]
    fn safe_progress(value: f32, start: f32, end: f32) -> f32 {
        let range = end - start;
        if range.abs() < 0.0001 {
            0.5
        } else {
            (value - start) / range
        }
    }

    /// Calculate color at a position based on gradient
    fn color_at(&self, x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> [f32; 4] {
        if let Some(ref grad) = self.gradient {
            let t = match grad.direction {
                GradientDir::Horizontal => Self::safe_progress(x, x1, x2),
                GradientDir::Vertical => {
                    // Note: y1 > y2 in NDC (top is positive)
                    Self::safe_progress(y, y1, y2)
                }
                GradientDir::Diagonal => {
                    let tx = Self::safe_progress(x, x1, x2);
                    let ty = Self::safe_progress(y, y1, y2);
                    (tx + ty) / 2.0
                }
                GradientDir::DiagonalReverse => {
                    let tx = Self::safe_progress(x, x1, x2);
                    let ty = Self::safe_progress(y, y1, y2);
                    (tx + (1.0 - ty)) / 2.0
                }
            };
            let t = t.clamp(0.0, 1.0);
            let s = &grad.start_color;
            let e = &grad.end_color;
            [
                s.r + (e.r - s.r) * t,
                s.g + (e.g - s.g) * t,
                s.b + (e.b - s.b) * t,
                s.a + (e.a - s.a) * t,
            ]
        } else {
            [self.color.r, self.color.g, self.color.b, self.color.a]
        }
    }

    pub fn to_vertices(&self, screen_width: f32, screen_height: f32) -> (Vec<Vertex>, Vec<u16>) {
        // Extract non-uniform scale factors from transform - scale is applied to geometry, not via GPU transform
        // This is necessary because SDF rendering doesn't work correctly with GPU-based scale transforms
        let (scale_x, scale_y) = self.transform.extract_scale_components();

        // Get the rotation-only transform (scale removed) for GPU transform
        // This preserves rotation behavior while allowing geometry-based scaling
        let rotation_only_transform = self.transform.without_scale();

        // Convert to normalized device coordinates
        let to_ndc_x = |x: f32| (x / screen_width) * 2.0 - 1.0;
        let to_ndc_y = |y: f32| 1.0 - (y / screen_height) * 2.0;

        // Determine scale origin: use transform_origin if specified, otherwise shape's center
        let shape_center_x = self.rect.x + self.rect.width / 2.0;
        let shape_center_y = self.rect.y + self.rect.height / 2.0;
        let (scale_origin_x, scale_origin_y) = self
            .transform_origin
            .unwrap_or((shape_center_x, shape_center_y));

        // Pre-scale the rect geometry around the scale origin (non-uniform scaling)
        let scaled_rect = Rect::new(
            scale_origin_x + (self.rect.x - scale_origin_x) * scale_x,
            scale_origin_y + (self.rect.y - scale_origin_y) * scale_y,
            self.rect.width * scale_x,
            self.rect.height * scale_y,
        );

        // Convert scaled rect to NDC
        let x1 = to_ndc_x(scaled_rect.x);
        let y1 = to_ndc_y(scaled_rect.y);
        let x2 = to_ndc_x(scaled_rect.x + scaled_rect.width);
        let y2 = to_ndc_y(scaled_rect.y + scaled_rect.height);

        // Convert rotation-only transform to work correctly in NDC space
        let centered_transform = transform_to_ndc(
            &rotation_only_transform,
            self.transform_origin,
            ((x1 + x2) / 2.0, (y1 + y2) / 2.0), // Default: shape's center in NDC
            screen_width,
            screen_height,
        );

        // Compute radius in NDC
        // Clamp radius to half the smaller dimension of the SCALED rect
        // This ensures corner radius scales properly with non-uniform transforms
        // (e.g., scrollbar expanding horizontally maintains pill shape)
        let radius = self
            .radius
            .min(scaled_rect.width / 2.0)
            .min(scaled_rect.height / 2.0);
        let r_ndc_x = (radius / screen_width) * 2.0;
        let r_ndc_y = (radius / screen_height) * 2.0;

        // For border and shadow, use minimum scale to keep them consistent
        let min_scale = scale_x.min(scale_y);

        // Shape bounds for SDF in shader
        let shape_rect = [x1, y2, x2, y1]; // min_x, min_y, max_x, max_y
        let shape_radius = [r_ndc_x, r_ndc_y];
        let shape_curvature = self.curvature;

        // Convert border width to NDC separately for x and y (aspect-ratio correct, scaled)
        // For non-uniform scaling, use minimum scale to keep border consistent
        let scaled_border = self.border_width * min_scale;
        let border_width_ndc = [
            (scaled_border / screen_width) * 2.0,
            (scaled_border / screen_height) * 2.0,
        ];
        let border_color = [
            self.border_color.r,
            self.border_color.g,
            self.border_color.b,
            self.border_color.a,
        ];

        // Convert shadow parameters to NDC (scaled)
        // For shadows, use min_scale to keep them uniform even with non-uniform scaling
        let shadow_offset_ndc = [
            (self.shadow.offset.0 * min_scale / screen_width) * 2.0,
            -(self.shadow.offset.1 * min_scale / screen_height) * 2.0, // Negative because NDC y is flipped
        ];
        let shadow_blur_ndc = (self.shadow.blur * min_scale / screen_height) * 2.0;
        let shadow_spread_ndc = (self.shadow.spread * min_scale / screen_height) * 2.0;
        let shadow_color = [
            self.shadow.color.r,
            self.shadow.color.g,
            self.shadow.color.b,
            self.shadow.color.a,
        ];

        // Expand quad bounds to include shadow if there is one
        // The shadow needs extra space to fade smoothly to zero
        // Blur defines the falloff distance, but we need ~3x blur for complete fadeout
        let (quad_x1, quad_y1, quad_x2, quad_y2) = if self.shadow.color.a > 0.0 {
            // Shadow fadeout multiplier: 3x ensures smooth gradient to transparent
            let fadeout = 3.0;

            // Calculate how far shadow extends beyond the scaled shape in each direction
            // Scale shadow extends by the same factor (use min_scale for uniform shadow)
            let left_extend = (self.shadow.blur * min_scale * fadeout
                - self.shadow.offset.0 * min_scale)
                .max(0.0);
            let right_extend = (self.shadow.blur * min_scale * fadeout
                + self.shadow.offset.0 * min_scale)
                .max(0.0);
            let top_extend = (self.shadow.blur * min_scale * fadeout
                - self.shadow.offset.1 * min_scale)
                .max(0.0);
            let bottom_extend = (self.shadow.blur * min_scale * fadeout
                + self.shadow.offset.1 * min_scale)
                .max(0.0);

            (
                to_ndc_x(scaled_rect.x - left_extend),
                to_ndc_y(scaled_rect.y - top_extend),
                to_ndc_x(scaled_rect.x + scaled_rect.width + right_extend),
                to_ndc_y(scaled_rect.y + scaled_rect.height + bottom_extend),
            )
        } else {
            // No shadow - use exact bounds
            (x1, y1, x2, y2)
        };

        // Compute clip region in NDC if present
        let (clip_rect_ndc, clip_radius_ndc, clip_inv_transform) = if let Some(ref clip) = self.clip
        {
            // Use clip's transform origin if set, otherwise use shape's origin or clip center
            let clip_center_x = clip.rect.x + clip.rect.width / 2.0;
            let clip_center_y = clip.rect.y + clip.rect.height / 2.0;
            let (clip_origin_x, clip_origin_y) = clip
                .transform_origin
                .or(self.transform_origin)
                .unwrap_or((clip_center_x, clip_center_y));

            // Determine which transform affects the clip region:
            // - If clip has its own transform, use that (for ripples on transformed containers)
            // - Otherwise, use the shape's transform (for clipped shapes that are transformed)
            let clip_transform = clip.transform.unwrap_or(self.transform);

            // Extract scale and translation from clip transform
            let (clip_scale_x, clip_scale_y) = clip_transform.extract_scale_components();
            let clip_rotation_only = clip_transform.without_scale();

            // Get translation from the transform
            let clip_tx = clip_transform.tx();
            let clip_ty = clip_transform.ty();

            // Scale the clip rect around the transform origin, then apply translation
            // Translation is applied directly to the clip rect position so the clip
            // follows the transformed shape without needing shader inverse transform magic.
            let scaled_clip = Rect::new(
                clip_origin_x + (clip.rect.x - clip_origin_x) * clip_scale_x + clip_tx,
                clip_origin_y + (clip.rect.y - clip_origin_y) * clip_scale_y + clip_ty,
                clip.rect.width * clip_scale_x,
                clip.rect.height * clip_scale_y,
            );

            let clip_x1 = to_ndc_x(scaled_clip.x);
            let clip_y1 = to_ndc_y(scaled_clip.y);
            let clip_x2 = to_ndc_x(scaled_clip.x + scaled_clip.width);
            let clip_y2 = to_ndc_y(scaled_clip.y + scaled_clip.height);

            // Scale clip radius to match scaled clip dimensions
            let clip_radius = clip
                .radius
                .min(scaled_clip.width / 2.0)
                .min(scaled_clip.height / 2.0);
            // Use height-based NDC for uniform clip radius (aspect-corrected in shader)
            let clip_r_ndc = (clip_radius / screen_height) * 2.0;

            // Compute inverse transform for clip region if there's rotation.
            // Translation is already applied to clip rect above, so we only need
            // inverse for rotation (to map screen positions back to rotated clip space).
            let clip_inv = if clip_rotation_only.has_rotation() {
                // Build the NDC transform for the clip's rotation only (no translation)
                // Translation was already applied to the clip rect geometry above.
                let rotation_for_clip =
                    Transform::rotate(clip_rotation_only.extract_rotation_angle());
                let clip_ndc_transform = transform_to_ndc(
                    &rotation_for_clip,
                    Some((clip_origin_x + clip_tx, clip_origin_y + clip_ty)),
                    ((clip_x1 + clip_x2) / 2.0, (clip_y1 + clip_y2) / 2.0),
                    screen_width,
                    screen_height,
                );
                Some(clip_ndc_transform.inverse())
            } else {
                None
            };

            (
                [clip_x1, clip_y2, clip_x2, clip_y1], // min_x, min_y, max_x, max_y
                clip_r_ndc,
                clip_inv,
            )
        } else {
            ([0.0, 0.0, 0.0, 0.0], 0.0, None)
        };

        // Simple quad - SDF rendering handles the shape in fragment shader
        // local_pos = same as position since geometry is already pre-scaled
        let vertices = vec![
            Vertex::with_transform_and_clip(
                [quad_x1, quad_y1],
                [quad_x1, quad_y1], // local_pos
                self.color_at(x1, y1, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
                shadow_offset_ndc,
                shadow_blur_ndc,
                shadow_spread_ndc,
                shadow_color,
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
                clip_inv_transform,
            ),
            Vertex::with_transform_and_clip(
                [quad_x2, quad_y1],
                [quad_x2, quad_y1], // local_pos
                self.color_at(x2, y1, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
                shadow_offset_ndc,
                shadow_blur_ndc,
                shadow_spread_ndc,
                shadow_color,
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
                clip_inv_transform,
            ),
            Vertex::with_transform_and_clip(
                [quad_x2, quad_y2],
                [quad_x2, quad_y2], // local_pos
                self.color_at(x2, y2, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
                shadow_offset_ndc,
                shadow_blur_ndc,
                shadow_spread_ndc,
                shadow_color,
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
                clip_inv_transform,
            ),
            Vertex::with_transform_and_clip(
                [quad_x1, quad_y2],
                [quad_x1, quad_y2], // local_pos
                self.color_at(x1, y2, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
                shadow_offset_ndc,
                shadow_blur_ndc,
                shadow_spread_ndc,
                shadow_color,
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
                clip_inv_transform,
            ),
        ];

        let indices = vec![0, 1, 2, 0, 2, 3];
        (vertices, indices)
    }
}

/// Trait for shapes that can have transforms applied.
///
/// This allows PaintContext to apply transforms uniformly to all shape types.
pub trait Transformable {
    /// Set the transform matrix and optional custom origin point
    fn set_transform(&mut self, transform: Transform, origin: Option<(f32, f32)>);
}

impl Transformable for RoundedRect {
    fn set_transform(&mut self, transform: Transform, origin: Option<(f32, f32)>) {
        self.transform = transform;
        self.transform_origin = origin;
    }
}

/// Vertex for textured quads (used for transformed text and image rendering).
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedVertex {
    /// Position in NDC
    pub position: [f32; 2],
    /// Texture coordinates (UV)
    pub tex_coords: [f32; 2],
    /// Transform matrix row 0
    pub transform_row0: [f32; 4],
    /// Transform matrix row 1
    pub transform_row1: [f32; 4],
    /// Transform matrix row 2
    pub transform_row2: [f32; 4],
    /// Transform matrix row 3
    pub transform_row3: [f32; 4],
    /// Clip rectangle in NDC: [min_x, min_y, max_x, max_y] - (0,0,0,0) means no clip
    pub clip_rect: [f32; 4],
    /// Clip corner radius in NDC (height-based for aspect-correct rendering)
    pub clip_radius: f32,
    /// Padding for alignment
    pub _padding: [f32; 3],
}

impl TexturedVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // tex_coords
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // transform_row0
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row1
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row2
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row3
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // clip_rect
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 20]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // clip_radius + padding
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 24]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }

    pub fn new(position: [f32; 2], tex_coords: [f32; 2], transform: Transform) -> Self {
        Self::with_clip(position, tex_coords, transform, [0.0, 0.0, 0.0, 0.0], 0.0)
    }

    pub fn with_clip(
        position: [f32; 2],
        tex_coords: [f32; 2],
        transform: Transform,
        clip_rect: [f32; 4],
        clip_radius: f32,
    ) -> Self {
        let rows = transform.rows();
        Self {
            position,
            tex_coords,
            transform_row0: rows[0],
            transform_row1: rows[1],
            transform_row2: rows[2],
            transform_row3: rows[3],
            clip_rect,
            clip_radius,
            _padding: [0.0, 0.0, 0.0],
        }
    }
}

/// A textured quad for rendering text textures and images with transforms.
pub struct TexturedQuad {
    /// The rect in logical pixels
    pub rect: Rect,
    /// Transform to apply
    pub transform: Transform,
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
    /// UV coordinates: (u_min, v_min, u_max, v_max), defaults to (0, 0, 1, 1)
    pub uv: (f32, f32, f32, f32),
    /// Optional clip region for this quad
    pub clip: Option<ClipRegion>,
}

impl TexturedQuad {
    pub fn new(rect: Rect, transform: Transform, transform_origin: Option<(f32, f32)>) -> Self {
        Self {
            rect,
            transform,
            transform_origin,
            uv: (0.0, 0.0, 1.0, 1.0),
            clip: None,
        }
    }

    /// Create a textured quad with custom UV coordinates for content fit modes.
    pub fn with_uv(
        rect: Rect,
        transform: Transform,
        transform_origin: Option<(f32, f32)>,
        uv: (f32, f32, f32, f32),
    ) -> Self {
        Self {
            rect,
            transform,
            transform_origin,
            uv,
            clip: None,
        }
    }

    /// Create a textured quad with custom UV coordinates and clip region.
    pub fn with_uv_and_clip(
        rect: Rect,
        transform: Transform,
        transform_origin: Option<(f32, f32)>,
        uv: (f32, f32, f32, f32),
        clip: Option<ClipRegion>,
    ) -> Self {
        Self {
            rect,
            transform,
            transform_origin,
            uv,
            clip,
        }
    }

    /// Generate vertices for this textured quad.
    ///
    /// The vertices are positioned in NDC with UV coordinates for texture sampling.
    pub fn to_vertices(
        &self,
        screen_width: f32,
        screen_height: f32,
    ) -> (Vec<TexturedVertex>, Vec<u16>) {
        // Extract non-uniform scale factors from transform - scale is applied to geometry, not via GPU transform
        // This matches how RoundedRect handles scale transforms
        let (scale_x, scale_y) = self.transform.extract_scale_components();

        // Get the rotation-only transform (scale removed) for GPU transform
        let rotation_only_transform = self.transform.without_scale();

        // Convert to normalized device coordinates
        let to_ndc_x = |x: f32| (x / screen_width) * 2.0 - 1.0;
        let to_ndc_y = |y: f32| 1.0 - (y / screen_height) * 2.0;

        // Determine scale origin: use transform_origin if specified, otherwise quad's center
        let quad_center_x = self.rect.x + self.rect.width / 2.0;
        let quad_center_y = self.rect.y + self.rect.height / 2.0;
        let (scale_origin_x, scale_origin_y) = self
            .transform_origin
            .unwrap_or((quad_center_x, quad_center_y));

        // Pre-scale the rect geometry around the scale origin (non-uniform scaling)
        let scaled_rect = Rect::new(
            scale_origin_x + (self.rect.x - scale_origin_x) * scale_x,
            scale_origin_y + (self.rect.y - scale_origin_y) * scale_y,
            self.rect.width * scale_x,
            self.rect.height * scale_y,
        );

        let x1 = to_ndc_x(scaled_rect.x);
        let y1 = to_ndc_y(scaled_rect.y);
        let x2 = to_ndc_x(scaled_rect.x + scaled_rect.width);
        let y2 = to_ndc_y(scaled_rect.y + scaled_rect.height);

        // Convert rotation-only transform to work correctly in NDC space
        let centered_transform = transform_to_ndc(
            &rotation_only_transform,
            self.transform_origin,
            ((x1 + x2) / 2.0, (y1 + y2) / 2.0), // Default: quad's center in NDC
            screen_width,
            screen_height,
        );

        // Compute clip region in NDC if present
        let (clip_rect_ndc, clip_radius_ndc) = if let Some(ref clip) = self.clip {
            let clip_x1 = to_ndc_x(clip.rect.x);
            let clip_y1 = to_ndc_y(clip.rect.y);
            let clip_x2 = to_ndc_x(clip.rect.x + clip.rect.width);
            let clip_y2 = to_ndc_y(clip.rect.y + clip.rect.height);

            let clip_radius = clip
                .radius
                .min(clip.rect.width / 2.0)
                .min(clip.rect.height / 2.0);
            // Use height-based NDC for uniform clip radius (aspect-corrected in shader)
            let clip_r_ndc = (clip_radius / screen_height) * 2.0;

            (
                [clip_x1, clip_y2, clip_x2, clip_y1], // min_x, min_y, max_x, max_y
                clip_r_ndc,
            )
        } else {
            ([0.0, 0.0, 0.0, 0.0], 0.0)
        };

        // UV coordinates from the uv field
        let (u_min, v_min, u_max, v_max) = self.uv;
        let vertices = vec![
            TexturedVertex::with_clip(
                [x1, y1],
                [u_min, v_min],
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
            ), // top-left
            TexturedVertex::with_clip(
                [x2, y1],
                [u_max, v_min],
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
            ), // top-right
            TexturedVertex::with_clip(
                [x2, y2],
                [u_max, v_max],
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
            ), // bottom-right
            TexturedVertex::with_clip(
                [x1, y2],
                [u_min, v_max],
                centered_transform,
                clip_rect_ndc,
                clip_radius_ndc,
            ), // bottom-left
        ];

        let indices = vec![0, 1, 2, 0, 2, 3];
        (vertices, indices)
    }
}
