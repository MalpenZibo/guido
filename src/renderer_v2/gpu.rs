//! GPU data structures for instanced rendering.
//!
//! This module contains the vertex and instance data structures used by the
//! V2 renderer's instanced rendering pipeline. Instead of duplicating vertex
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
/// position, size, colors, border, clip region, and transform.
///
/// Total size: ~160 bytes per instance (much better than ~768 bytes in V1)
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShapeInstance {
    // === Shape geometry (logical pixels) ===
    /// Rectangle bounds: [x, y, width, height]
    pub rect: [f32; 4],

    /// Corner radius in logical pixels
    pub corner_radius: f32,
    /// Superellipse curvature (K-value: 1.0=circle, 2.0=squircle)
    pub shape_curvature: f32,
    /// Padding for alignment
    pub _pad0: [f32; 2],

    // === Colors ===
    /// Fill color RGBA
    pub fill_color: [f32; 4],
    /// Border color RGBA
    pub border_color: [f32; 4],

    // === Border ===
    /// Border width in logical pixels
    pub border_width: f32,
    /// Padding for alignment
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

    // === Clipping (logical pixels) ===
    /// Clip rectangle: [x, y, width, height] (0,0,0,0 = no clip)
    pub clip_rect: [f32; 4],
    /// Clip corner radius
    pub clip_radius: f32,
    /// Clip curvature
    pub clip_curvature: f32,
    /// Padding for alignment
    pub _pad2: [f32; 2],

    // === Transform (2x3 affine matrix) ===
    /// Transform matrix: [a, b, tx, c, d, ty] (row-major 2x3)
    /// Note: Transform origin is baked into the matrix via center_at() on CPU
    pub transform: [f32; 6],
    /// Padding for alignment (origin was baked into transform matrix)
    pub _pad3: [f32; 2],
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
            clip_rect: [0.0, 0.0, 0.0, 0.0],
            clip_radius: 0.0,
            clip_curvature: 1.0,
            _pad2: [0.0, 0.0],
            transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // identity
            _pad3: [0.0, 0.0],
        }
    }
}

impl ShapeInstance {
    /// Create a simple colored rectangle.
    #[allow(dead_code)]
    pub fn rect(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Self {
        Self {
            rect: [x, y, width, height],
            fill_color: color,
            ..Default::default()
        }
    }

    /// Create a rounded rectangle.
    #[allow(dead_code)]
    pub fn rounded_rect(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: [f32; 4],
        radius: f32,
    ) -> Self {
        Self {
            rect: [x, y, width, height],
            fill_color: color,
            corner_radius: radius,
            ..Default::default()
        }
    }

    /// Set the border.
    #[allow(dead_code)]
    pub fn with_border(mut self, width: f32, color: [f32; 4]) -> Self {
        self.border_width = width;
        self.border_color = color;
        self
    }

    /// Set the curvature (superellipse K-value).
    #[allow(dead_code)]
    pub fn with_curvature(mut self, curvature: f32) -> Self {
        self.shape_curvature = curvature;
        self
    }

    /// Set the shadow.
    #[allow(dead_code)]
    pub fn with_shadow(
        mut self,
        offset: [f32; 2],
        blur: f32,
        spread: f32,
        color: [f32; 4],
    ) -> Self {
        self.shadow_offset = offset;
        self.shadow_blur = blur;
        self.shadow_spread = spread;
        self.shadow_color = color;
        self
    }

    /// Set the clip region.
    #[allow(dead_code)]
    pub fn with_clip(mut self, x: f32, y: f32, width: f32, height: f32, radius: f32) -> Self {
        self.clip_rect = [x, y, width, height];
        self.clip_radius = radius;
        self
    }

    /// Set the transform from a 2x3 affine matrix.
    /// Note: The origin should be baked into the matrix via center_at() on CPU.
    #[allow(dead_code)]
    pub fn with_transform(mut self, transform: [f32; 6]) -> Self {
        self.transform = transform;
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
                // clip_rect
                VertexAttribute {
                    offset: 112,
                    shader_location: 8,
                    format: VertexFormat::Float32x4,
                },
                // clip_radius, clip_curvature, _pad2[0], _pad2[1]
                VertexAttribute {
                    offset: 128,
                    shader_location: 9,
                    format: VertexFormat::Float32x4,
                },
                // transform[0..4] (a, b, tx, c)
                VertexAttribute {
                    offset: 144,
                    shader_location: 10,
                    format: VertexFormat::Float32x4,
                },
                // transform[4..6], _pad3 (d, ty, _pad, _pad)
                VertexAttribute {
                    offset: 160,
                    shader_location: 11,
                    format: VertexFormat::Float32x4,
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
        // Verify the size is reasonable (should be around 176 bytes)
        let size = std::mem::size_of::<ShapeInstance>();
        println!("ShapeInstance size: {} bytes", size);
        assert!(size <= 256, "ShapeInstance is too large: {} bytes", size);
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
