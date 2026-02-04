//! GPU data structures for instanced rendering.
//!
//! This module contains the vertex and instance data structures used by the
//! Instanced rendering pipeline data structures. Instead of duplicating vertex
//! data for each shape, we use a single unit quad and per-instance data.

use wgpu::{VertexAttribute, VertexBufferLayout, VertexFormat, VertexStepMode};

/// Uniform buffer data passed to the shader.
///
/// Contains screen-wide information needed for coordinate conversion.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShaderUniforms {
    /// Screen size in logical pixels (width, height)
    pub screen_size: [f32; 2],
    /// HiDPI scale factor
    pub scale_factor: f32,
    /// Padding for 16-byte alignment
    pub _pad: f32,
}

impl ShaderUniforms {
    /// Create new shader uniforms.
    pub fn new(screen_width: f32, screen_height: f32, scale_factor: f32) -> Self {
        Self {
            screen_size: [screen_width, screen_height],
            scale_factor,
            _pad: 0.0,
        }
    }
}

/// A single vertex of the unit quad (shared across all instances).
///
/// The unit quad spans [0,0] to [1,1] and is transformed per-instance
/// to the actual shape position and size.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct QuadVertex {
    /// Position in 0..1 range
    pub position: [f32; 2],
}

impl QuadVertex {
    /// Vertex buffer layout for the unit quad.
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: VertexFormat::Float32x2,
            }],
        }
    }
}

/// The shared unit quad vertices (created once, used by all shapes).
pub const QUAD_VERTICES: &[QuadVertex] = &[
    QuadVertex {
        position: [0.0, 0.0],
    }, // top-left
    QuadVertex {
        position: [1.0, 0.0],
    }, // top-right
    QuadVertex {
        position: [0.0, 1.0],
    }, // bottom-left
    QuadVertex {
        position: [1.0, 1.0],
    }, // bottom-right
];

/// Index buffer for the unit quad (two triangles).
pub const QUAD_INDICES: &[u16] = &[
    0, 1, 2, // first triangle: top-left, top-right, bottom-left
    1, 3, 2, // second triangle: top-right, bottom-right, bottom-left
];

/// Per-instance data for a single shape.
///
/// Contains all the information needed to render one rounded rectangle:
/// position, size, colors, border, and transform.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShapeInstance {
    // === Shape geometry (physical pixels, scaled in render.rs) ===
    /// Rectangle bounds: [x, y, width, height]
    pub rect: [f32; 4],

    /// Corner radius in logical pixels
    pub corner_radius: f32,
    /// Superellipse curvature (K-value: 1.0=circle, 2.0=squircle)
    pub shape_curvature: f32,
    /// Padding for 16-byte alignment (wgpu uniform buffer requirement)
    pub _pad0: [f32; 2],

    // === Colors ===
    /// Fill color RGBA
    pub fill_color: [f32; 4],
    /// Border color RGBA
    pub border_color: [f32; 4],

    // === Border ===
    /// Border width in logical pixels
    pub border_width: f32,
    /// Padding for 16-byte alignment
    pub _pad1: [f32; 3],

    // === Shadow ===
    /// Shadow offset in logical pixels (x, y)
    pub shadow_offset: [f32; 2],
    /// Shadow blur radius in logical pixels
    pub shadow_blur: f32,
    /// Shadow spread in logical pixels
    pub shadow_spread: f32,
    /// Shadow color RGBA
    pub shadow_color: [f32; 4],

    // === Transform (2x3 affine matrix) ===
    /// Transform matrix: [a, b, tx, c, d, ty] (row-major 2x3)
    /// Note: Transform origin is baked into the matrix via center_at() on CPU
    pub transform: [f32; 6],
    /// Padding for 16-byte alignment
    pub _pad2: [f32; 2],

    // === Clip Region ===
    /// Clip rect in physical pixels [x, y, width, height]
    /// If width or height <= 0, no clipping is applied
    pub clip_rect: [f32; 4],
    /// Clip corner radius in physical pixels
    pub clip_corner_radius: f32,
    /// Clip curvature (K-value)
    pub clip_curvature: f32,
    /// Whether to use local coordinates (frag_pos) for clipping instead of world_pos.
    /// 1.0 = local clip, 0.0 = world clip
    pub clip_is_local: f32,
    /// Padding for 16-byte alignment
    pub _pad3: f32,

    // === Gradient ===
    /// Gradient start color [r, g, b, a]
    pub gradient_start: [f32; 4],
    /// Gradient end color [r, g, b, a]
    pub gradient_end: [f32; 4],
    /// Gradient type: 0=none, 1=horizontal, 2=vertical, 3=diagonal, 4=diagonal_reverse
    pub gradient_type: u32,
    /// Padding for 16-byte alignment
    pub _pad4: [u32; 3],
}

impl Default for ShapeInstance {
    fn default() -> Self {
        Self {
            rect: [0.0, 0.0, 0.0, 0.0],
            corner_radius: 0.0,
            shape_curvature: 1.0,
            _pad0: [0.0, 0.0],
            fill_color: [0.0, 0.0, 0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
            border_width: 0.0,
            _pad1: [0.0, 0.0, 0.0],
            shadow_offset: [0.0, 0.0],
            shadow_blur: 0.0,
            shadow_spread: 0.0,
            shadow_color: [0.0, 0.0, 0.0, 0.0],
            transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // identity
            _pad2: [0.0, 0.0],
            clip_rect: [0.0, 0.0, 0.0, 0.0], // No clipping (width/height = 0)
            clip_corner_radius: 0.0,
            clip_curvature: 1.0,
            clip_is_local: 0.0,
            _pad3: 0.0,
            gradient_start: [0.0, 0.0, 0.0, 0.0],
            gradient_end: [0.0, 0.0, 0.0, 0.0],
            gradient_type: 0, // No gradient
            _pad4: [0, 0, 0],
        }
    }
}

impl ShapeInstance {
    /// Create a shape instance from a rectangle with basic properties.
    pub fn from_rect(
        rect: [f32; 4],
        fill_color: [f32; 4],
        corner_radius: f32,
        curvature: f32,
    ) -> Self {
        Self {
            rect,
            corner_radius,
            shape_curvature: curvature,
            fill_color,
            ..Default::default()
        }
    }

    /// Set transform from a Transform struct, scaling translation by scale_factor.
    pub fn with_transform(mut self, transform: &crate::transform::Transform, scale: f32) -> Self {
        if !transform.is_identity() {
            self.transform = [
                transform.data[0],         // a
                transform.data[1],         // b
                transform.data[3] * scale, // tx (scaled)
                transform.data[4],         // c
                transform.data[5],         // d
                transform.data[7] * scale, // ty (scaled)
            ];
        }
        self
    }

    /// Set clip region from WorldClip, scaling by scale_factor.
    pub fn with_clip(
        mut self,
        clip: &super::flatten::WorldClip,
        scale: f32,
        is_local: bool,
    ) -> Self {
        self.clip_rect = [
            clip.rect.x * scale,
            clip.rect.y * scale,
            clip.rect.width * scale,
            clip.rect.height * scale,
        ];
        self.clip_corner_radius = clip.corner_radius * scale;
        self.clip_curvature = clip.curvature;
        self.clip_is_local = if is_local { 1.0 } else { 0.0 };
        self
    }

    /// Set border properties.
    pub fn with_border(mut self, border: &super::commands::Border, scale: f32) -> Self {
        self.border_width = border.width * scale;
        self.border_color = [
            border.color.r,
            border.color.g,
            border.color.b,
            border.color.a,
        ];
        self
    }

    /// Set shadow properties.
    pub fn with_shadow(mut self, shadow: &super::types::Shadow, scale: f32) -> Self {
        self.shadow_offset = [shadow.offset.0 * scale, shadow.offset.1 * scale];
        self.shadow_blur = shadow.blur * scale;
        self.shadow_spread = shadow.spread * scale;
        self.shadow_color = [
            shadow.color.r,
            shadow.color.g,
            shadow.color.b,
            shadow.color.a,
        ];
        self
    }

    /// Set gradient properties.
    pub fn with_gradient(mut self, gradient: &super::types::Gradient) -> Self {
        self.gradient_start = [
            gradient.start_color.r,
            gradient.start_color.g,
            gradient.start_color.b,
            gradient.start_color.a,
        ];
        self.gradient_end = [
            gradient.end_color.r,
            gradient.end_color.g,
            gradient.end_color.b,
            gradient.end_color.a,
        ];
        self.gradient_type = match gradient.direction {
            super::types::GradientDir::Horizontal => 1,
            super::types::GradientDir::Vertical => 2,
            super::types::GradientDir::Diagonal => 3,
            super::types::GradientDir::DiagonalReverse => 4,
        };
        self
    }

    /// Vertex buffer layout for instance data.
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<ShapeInstance>() as u64,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                // rect: [x, y, width, height]
                VertexAttribute {
                    offset: 0,
                    shader_location: 1,
                    format: VertexFormat::Float32x4,
                },
                // corner_radius, shape_curvature, _pad0[0], _pad0[1]
                VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: VertexFormat::Float32x4,
                },
                // fill_color
                VertexAttribute {
                    offset: 32,
                    shader_location: 3,
                    format: VertexFormat::Float32x4,
                },
                // border_color
                VertexAttribute {
                    offset: 48,
                    shader_location: 4,
                    format: VertexFormat::Float32x4,
                },
                // border_width, _pad1[0], _pad1[1], _pad1[2]
                VertexAttribute {
                    offset: 64,
                    shader_location: 5,
                    format: VertexFormat::Float32x4,
                },
                // shadow_offset, shadow_blur, shadow_spread
                VertexAttribute {
                    offset: 80,
                    shader_location: 6,
                    format: VertexFormat::Float32x4,
                },
                // shadow_color
                VertexAttribute {
                    offset: 96,
                    shader_location: 7,
                    format: VertexFormat::Float32x4,
                },
                // transform[0..4] (a, b, tx, c)
                VertexAttribute {
                    offset: 112,
                    shader_location: 8,
                    format: VertexFormat::Float32x4,
                },
                // transform[4..6], _pad2 (d, ty, _pad, _pad)
                VertexAttribute {
                    offset: 128,
                    shader_location: 9,
                    format: VertexFormat::Float32x4,
                },
                // clip_rect: [x, y, width, height]
                VertexAttribute {
                    offset: 144,
                    shader_location: 10,
                    format: VertexFormat::Float32x4,
                },
                // clip_corner_radius, clip_curvature, clip_is_local, _pad3
                VertexAttribute {
                    offset: 160,
                    shader_location: 11,
                    format: VertexFormat::Float32x4,
                },
                // gradient_start
                VertexAttribute {
                    offset: 176,
                    shader_location: 12,
                    format: VertexFormat::Float32x4,
                },
                // gradient_end
                VertexAttribute {
                    offset: 192,
                    shader_location: 13,
                    format: VertexFormat::Float32x4,
                },
                // gradient_type, _pad4[0], _pad4[1], _pad4[2]
                VertexAttribute {
                    offset: 208,
                    shader_location: 14,
                    format: VertexFormat::Uint32x4,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shape_instance_size() {
        // Verify the size is reasonable (should be around 224 bytes with clip + gradient)
        let size = std::mem::size_of::<ShapeInstance>();
        println!("ShapeInstance size: {} bytes", size);
        assert!(size <= 256, "ShapeInstance is too large: {} bytes", size);
        // Verify expected size: 176 (base + clip) + 48 (gradient) = 224
        assert_eq!(size, 224, "ShapeInstance size changed unexpectedly");
    }

    #[test]
    fn test_quad_vertices() {
        assert_eq!(QUAD_VERTICES.len(), 4);
        assert_eq!(QUAD_INDICES.len(), 6);
    }

    #[test]
    fn test_default_instance() {
        let instance = ShapeInstance::default();
        assert_eq!(instance.transform, [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        assert_eq!(instance.shape_curvature, 1.0);
    }
}
