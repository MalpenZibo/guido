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
        Self {
            position,
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            _padding: 0.0,
            border_width: [0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
        }
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
        Self {
            position,
            color,
            shape_rect,
            shape_radius,
            shape_curvature,
            _padding: 0.0,
            border_width,
            border_color,
        }
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

        // Simple quad - SDF rendering handles the shape in fragment shader
        let vertices = vec![
            Vertex::with_border(
                [x1, y1],
                self.color_at(x1, y1, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
            ),
            Vertex::with_border(
                [x2, y1],
                self.color_at(x2, y1, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
            ),
            Vertex::with_border(
                [x2, y2],
                self.color_at(x2, y2, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
            ),
            Vertex::with_border(
                [x1, y2],
                self.color_at(x1, y2, x1, y1, x2, y2),
                shape_rect,
                shape_radius,
                shape_curvature,
                border_width_ndc,
                border_color,
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
}

impl Circle {
    pub fn new(center_x: f32, center_y: f32, radius: f32, color: Color) -> Self {
        Self {
            center_x,
            center_y,
            radius,
            color,
            clip: None,
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

        // Center vertex - pass clip region as shape params for clipping
        vertices.push(Vertex::with_border(
            [cx, cy],
            color,
            clip_rect,
            clip_radius,
            clip_curvature,
            border_width,
            border_color,
        ));

        // Edge vertices
        for i in 0..=segments {
            let angle = (i as f32 / segments as f32) * std::f32::consts::PI * 2.0;
            let vx = cx + angle.cos() * r_ndc_x;
            let vy = cy - angle.sin() * r_ndc_y;
            vertices.push(Vertex::with_border(
                [vx, vy],
                color,
                clip_rect,
                clip_radius,
                clip_curvature,
                border_width,
                border_color,
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
