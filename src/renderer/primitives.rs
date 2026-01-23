use crate::transform::Transform;
use crate::widgets::{Color, Rect};

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
    /// Padding for alignment
    pub _padding: f32,
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
    /// Transform matrix row 0
    pub transform_row0: [f32; 4],
    /// Transform matrix row 1
    pub transform_row1: [f32; 4],
    /// Transform matrix row 2
    pub transform_row2: [f32; 4],
    /// Transform matrix row 3
    pub transform_row3: [f32; 4],
    /// Local position (untransformed) for SDF evaluation in fragment shader
    pub local_pos: [f32; 2],
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
                // transform_row0
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 28]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row1
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 32]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row2
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 36]>() as wgpu::BufferAddress,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // transform_row3
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
        let rows = transform.rows();
        Self {
            position,
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            _padding: 0.0,
            border_width,
            border_color,
            shadow_offset,
            shadow_blur,
            shadow_spread,
            shadow_color,
            transform_row0: rows[0],
            transform_row1: rows[1],
            transform_row2: rows[2],
            transform_row3: rows[3],
            local_pos,
        }
    }

    /// Set the transform on an existing vertex
    pub fn set_transform(&mut self, transform: Transform) {
        let rows = transform.rows();
        self.transform_row0 = rows[0];
        self.transform_row1 = rows[1];
        self.transform_row2 = rows[2];
        self.transform_row3 = rows[3];
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
#[derive(Debug, Clone)]
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
    /// If true, the transform is already centered and should not be auto-centered
    pub transform_is_centered: bool,
}

#[derive(Debug, Clone)]
pub struct ClipRegion {
    pub rect: Rect,
    pub radius: f32,
    /// Superellipse curvature K-value (default 1.0 = circle)
    pub curvature: f32,
}

impl RoundedRect {
    pub fn new(rect: Rect, color: Color, radius: f32) -> Self {
        Self {
            rect,
            color,
            radius,
            clip: None,
            gradient: None,
            curvature: 1.0, // Default K=1 (circular)
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    pub fn with_clip(rect: Rect, color: Color, radius: f32, clip: ClipRegion) -> Self {
        Self {
            rect,
            color,
            radius,
            clip: Some(clip),
            gradient: None,
            curvature: 1.0, // Default K=1 (circular)
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    pub fn with_gradient(rect: Rect, gradient: Gradient, radius: f32) -> Self {
        Self {
            rect,
            color: gradient.start_color, // fallback
            radius,
            clip: None,
            gradient: Some(gradient),
            curvature: 1.0, // Default K=1 (circular)
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    pub fn with_curvature(rect: Rect, color: Color, radius: f32, curvature: f32) -> Self {
        Self {
            rect,
            color,
            radius,
            clip: None,
            gradient: None,
            curvature,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    /// Create a rounded rect with a border
    pub fn with_border(
        rect: Rect,
        fill_color: Color,
        radius: f32,
        border_width: f32,
        border_color: Color,
    ) -> Self {
        Self {
            rect,
            color: fill_color,
            radius,
            clip: None,
            gradient: None,
            curvature: 1.0,
            border_width,
            border_color,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    /// Create a border-only rounded rect (transparent fill)
    pub fn border_only(rect: Rect, radius: f32, border_width: f32, border_color: Color) -> Self {
        Self {
            rect,
            color: Color::TRANSPARENT,
            radius,
            clip: None,
            gradient: None,
            curvature: 1.0,
            border_width,
            border_color,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    /// Create a border-only rounded rect with custom curvature
    pub fn border_only_with_curvature(
        rect: Rect,
        radius: f32,
        border_width: f32,
        border_color: Color,
        curvature: f32,
    ) -> Self {
        Self {
            rect,
            color: Color::TRANSPARENT,
            radius,
            clip: None,
            gradient: None,
            curvature,
            border_width,
            border_color,
            shadow: Shadow::none(),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    /// Calculate color at a position based on gradient
    fn color_at(&self, x: f32, y: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> [f32; 4] {
        if let Some(ref grad) = self.gradient {
            let t = match grad.direction {
                GradientDir::Horizontal => {
                    if (x2 - x1).abs() < 0.0001 {
                        0.5
                    } else {
                        (x - x1) / (x2 - x1)
                    }
                }
                GradientDir::Vertical => {
                    // Note: y1 > y2 in NDC (top is positive)
                    if (y1 - y2).abs() < 0.0001 {
                        0.5
                    } else {
                        (y1 - y) / (y1 - y2)
                    }
                }
                GradientDir::Diagonal => {
                    let tx = if (x2 - x1).abs() < 0.0001 {
                        0.5
                    } else {
                        (x - x1) / (x2 - x1)
                    };
                    let ty = if (y1 - y2).abs() < 0.0001 {
                        0.5
                    } else {
                        (y1 - y) / (y1 - y2)
                    };
                    (tx + ty) / 2.0
                }
                GradientDir::DiagonalReverse => {
                    let tx = if (x2 - x1).abs() < 0.0001 {
                        0.5
                    } else {
                        (x - x1) / (x2 - x1)
                    };
                    let ty = if (y1 - y2).abs() < 0.0001 {
                        0.5
                    } else {
                        (y1 - y) / (y1 - y2)
                    };
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
        // Convert to normalized device coordinates
        let to_ndc_x = |x: f32| (x / screen_width) * 2.0 - 1.0;
        let to_ndc_y = |y: f32| 1.0 - (y / screen_height) * 2.0;

        let x1 = to_ndc_x(self.rect.x);
        let y1 = to_ndc_y(self.rect.y);
        let x2 = to_ndc_x(self.rect.x + self.rect.width);
        let y2 = to_ndc_y(self.rect.y + self.rect.height);

        // Convert transform to work correctly in NDC space
        // NDC is non-isotropic: x and y have different pixel scales due to aspect ratio
        // For rotation θ in pixel space, the equivalent NDC rotation matrix is:
        //   [[cos θ, -sin θ / aspect], [aspect * sin θ, cos θ]]
        // This preserves visual angles when the shape is displayed on screen
        let aspect = screen_width / screen_height;

        let centered_transform = if !self.transform.is_identity() {
            // Extract rotation and scale from the transform matrix
            // The transform is in row-major: [a, b, 0, tx; c, d, 0, ty; ...]
            // For a rotation: a = cos θ, b = -sin θ, c = sin θ, d = cos θ
            let a = self.transform.data[0]; // cos θ or scale_x * cos θ
            let b = self.transform.data[1]; // -sin θ or scale_x * (-sin θ)
            let c = self.transform.data[4]; // sin θ or scale_y * sin θ
            let d = self.transform.data[5]; // cos θ or scale_y * cos θ
            let tx = self.transform.data[3];
            let ty = self.transform.data[7];

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

            // If transform is already centered (from Container with custom transform_origin),
            // use it directly. Otherwise, center around the shape's center in NDC.
            if self.transform_is_centered {
                ndc_transform
            } else {
                let center_x = (x1 + x2) / 2.0;
                let center_y = (y1 + y2) / 2.0;
                ndc_transform.center_at(center_x, center_y)
            }
        } else {
            Transform::IDENTITY
        };

        // Compute radius in NDC
        let radius = self
            .radius
            .min(self.rect.width / 2.0)
            .min(self.rect.height / 2.0);
        let r_ndc_x = (radius / screen_width) * 2.0;
        let r_ndc_y = (radius / screen_height) * 2.0;

        // Shape bounds for SDF in shader
        let shape_rect = [x1, y2, x2, y1]; // min_x, min_y, max_x, max_y
        let shape_radius = [r_ndc_x, r_ndc_y];
        let shape_curvature = self.curvature;

        // Convert border width to NDC separately for x and y (aspect-ratio correct)
        let border_width_ndc = [
            (self.border_width / screen_width) * 2.0,
            (self.border_width / screen_height) * 2.0,
        ];
        let border_color = [
            self.border_color.r,
            self.border_color.g,
            self.border_color.b,
            self.border_color.a,
        ];

        // Convert shadow parameters to NDC
        let shadow_offset_ndc = [
            (self.shadow.offset.0 / screen_width) * 2.0,
            -(self.shadow.offset.1 / screen_height) * 2.0, // Negative because NDC y is flipped
        ];
        let shadow_blur_ndc = (self.shadow.blur / screen_height) * 2.0; // Use height for uniform blur
        let shadow_spread_ndc = (self.shadow.spread / screen_height) * 2.0;
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

            // Calculate how far shadow extends beyond the shape in each direction
            // Account for offset direction and full fadeout distance
            let left_extend = (self.shadow.blur * fadeout - self.shadow.offset.0).max(0.0);
            let right_extend = (self.shadow.blur * fadeout + self.shadow.offset.0).max(0.0);
            let top_extend = (self.shadow.blur * fadeout - self.shadow.offset.1).max(0.0);
            let bottom_extend = (self.shadow.blur * fadeout + self.shadow.offset.1).max(0.0);

            (
                to_ndc_x(self.rect.x - left_extend),
                to_ndc_y(self.rect.y - top_extend),
                to_ndc_x(self.rect.x + self.rect.width + right_extend),
                to_ndc_y(self.rect.y + self.rect.height + bottom_extend),
            )
        } else {
            // No shadow - use exact bounds
            (x1, y1, x2, y2)
        };

        // Simple quad - SDF rendering handles the shape in fragment shader
        // Use expanded quad bounds for vertices, but keep original shape_rect for SDF
        // local_pos = untransformed position for SDF evaluation (same as position)
        let vertices = vec![
            Vertex::with_transform(
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
            ),
            Vertex::with_transform(
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
            ),
            Vertex::with_transform(
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
            ),
            Vertex::with_transform(
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
            ),
        ];

        let indices = vec![0, 1, 2, 0, 2, 3];
        (vertices, indices)
    }
}

/// A circle primitive (uses dedicated vertex generation for clean rendering)
#[derive(Debug, Clone)]
pub struct Circle {
    pub center_x: f32,
    pub center_y: f32,
    pub radius: f32,
    pub color: Color,
    pub clip: Option<ClipRegion>,
    /// Transform matrix for this shape
    pub transform: Transform,
    /// If true, the transform is already centered and should not be auto-centered
    pub transform_is_centered: bool,
}

/// Trait for shapes that can have transforms applied.
///
/// This allows PaintContext to apply transforms uniformly to all shape types.
pub trait Transformable {
    /// Set the transform matrix and whether it's already centered
    fn set_transform(&mut self, transform: Transform, is_centered: bool);
}

impl Transformable for RoundedRect {
    fn set_transform(&mut self, transform: Transform, is_centered: bool) {
        self.transform = transform;
        self.transform_is_centered = is_centered;
    }
}

impl Transformable for Circle {
    fn set_transform(&mut self, transform: Transform, is_centered: bool) {
        self.transform = transform;
        self.transform_is_centered = is_centered;
    }
}

impl Circle {
    pub fn new(center_x: f32, center_y: f32, radius: f32, color: Color) -> Self {
        Self {
            center_x,
            center_y,
            radius,
            color,
            clip: None,
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    pub fn with_clip(
        center_x: f32,
        center_y: f32,
        radius: f32,
        color: Color,
        clip: ClipRegion,
    ) -> Self {
        Self {
            center_x,
            center_y,
            radius,
            color,
            clip: Some(clip),
            transform: Transform::IDENTITY,
            transform_is_centered: false,
        }
    }

    pub fn to_vertices(&self, screen_width: f32, screen_height: f32) -> (Vec<Vertex>, Vec<u16>) {
        let color = [self.color.r, self.color.g, self.color.b, self.color.a];
        let segments = 32; // More segments for smooth circle

        let to_ndc_x = |x: f32| (x / screen_width) * 2.0 - 1.0;
        let to_ndc_y = |y: f32| 1.0 - (y / screen_height) * 2.0;

        let cx = to_ndc_x(self.center_x);
        let cy = to_ndc_y(self.center_y);
        let r_ndc_x = (self.radius / screen_width) * 2.0;
        let r_ndc_y = (self.radius / screen_height) * 2.0;

        // Compute centered transform in NDC space (circle is already centered at cx, cy)
        // If transform is already centered (from Container with custom transform_origin),
        // use it directly.
        let centered_transform = if !self.transform.is_identity() {
            if self.transform_is_centered {
                self.transform
            } else {
                self.transform.center_at(cx, cy)
            }
        } else {
            Transform::IDENTITY
        };

        // Compute clip rect in NDC - used for clipping the circle to container bounds
        let (clip_rect, clip_radius, clip_curvature) = if let Some(ref clip) = self.clip {
            let cx1 = to_ndc_x(clip.rect.x);
            let cy1 = to_ndc_y(clip.rect.y);
            let cx2 = to_ndc_x(clip.rect.x + clip.rect.width);
            let cy2 = to_ndc_y(clip.rect.y + clip.rect.height);
            let cr_x = (clip.radius / screen_width) * 2.0;
            let cr_y = (clip.radius / screen_height) * 2.0;
            ([cx1, cy2, cx2, cy1], [cr_x, cr_y], clip.curvature)
        } else {
            ([0.0, 0.0, 0.0, 0.0], [0.0, 0.0], 1.0)
        };

        let mut vertices = Vec::with_capacity(segments + 2);
        let mut indices = Vec::with_capacity(segments * 3);

        // For clipped circles, use a marker to indicate clip-only mode
        // border_color.r = -1.0 signals the shader to use SDF for clipping only
        let (border_width, border_color) = if self.clip.is_some() {
            ([0.0, 0.0], [-1.0, 0.0, 0.0, 0.0]) // Marker for clip-only mode
        } else {
            ([0.0, 0.0], [0.0, 0.0, 0.0, 0.0])
        };

        // No shadow for circles (use default/zero values)
        let shadow_offset = [0.0, 0.0];
        let shadow_blur = 0.0;
        let shadow_spread = 0.0;
        let shadow_color = [0.0, 0.0, 0.0, 0.0];

        // Center vertex - pass clip region as shape params for clipping
        // local_pos = untransformed position for SDF evaluation
        vertices.push(Vertex::with_transform(
            [cx, cy],
            [cx, cy], // local_pos
            color,
            clip_rect,
            clip_radius,
            clip_curvature,
            border_width,
            border_color,
            shadow_offset,
            shadow_blur,
            shadow_spread,
            shadow_color,
            centered_transform,
        ));

        // Edge vertices
        for i in 0..=segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::PI * 2.0;
            let vx = cx + angle.cos() * r_ndc_x;
            let vy = cy - angle.sin() * r_ndc_y;
            vertices.push(Vertex::with_transform(
                [vx, vy],
                [vx, vy], // local_pos
                color,
                clip_rect,
                clip_radius,
                clip_curvature,
                border_width,
                border_color,
                shadow_offset,
                shadow_blur,
                shadow_spread,
                shadow_color,
                centered_transform,
            ));
        }

        // Triangle fan indices
        for i in 1..=segments {
            indices.push(0);
            indices.push(i as u16);
            indices.push((i + 1) as u16);
        }

        (vertices, indices)
    }
}
