//! Shared textured vertex types for quad rendering.
//!
//! This module provides the common vertex format used by both text quad and image quad
//! rendering pipelines.

use wgpu::{VertexAttribute, VertexBufferLayout, VertexFormat, VertexStepMode};

/// Vertex with pre-computed NDC position, UV coordinates, and clip data.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedVertex {
    /// Position in NDC (pre-computed on CPU)
    pub position: [f32; 2],
    /// Texture coordinates
    pub uv: [f32; 2],
    /// Screen position in physical pixels (for clip calculation)
    pub screen_pos: [f32; 2],
    /// Clip rect in physical pixels [x, y, width, height]
    pub clip_rect: [f32; 4],
    /// Clip parameters [corner_radius, curvature, 0, 0]
    pub clip_params: [f32; 4],
}

impl TexturedVertex {
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedVertex>() as u64,
            step_mode: VertexStepMode::Vertex,
            attributes: &[
                // position (NDC)
                VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: VertexFormat::Float32x2,
                },
                // uv
                VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: VertexFormat::Float32x2,
                },
                // screen_pos
                VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: VertexFormat::Float32x2,
                },
                // clip_rect
                VertexAttribute {
                    offset: 24,
                    shader_location: 3,
                    format: VertexFormat::Float32x4,
                },
                // clip_params
                VertexAttribute {
                    offset: 40,
                    shader_location: 4,
                    format: VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// Convert screen coordinates to NDC (Normalized Device Coordinates).
#[inline]
pub fn to_ndc(x: f32, y: f32, screen_width: f32, screen_height: f32) -> [f32; 2] {
    [
        (x / screen_width) * 2.0 - 1.0,
        1.0 - (y / screen_height) * 2.0,
    ]
}
