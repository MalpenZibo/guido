//! Shared textured vertex types for quad rendering.
//!
//! This module provides the common vertex format used by both text quad and image quad
//! rendering pipelines.

use crate::transform::Transform;
use wgpu::{VertexAttribute, VertexBufferLayout, VertexFormat, VertexStepMode};

/// Vertex with pre-computed NDC position, UV coordinates, and clip data.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedVertex {
    /// Position in NDC (pre-computed on CPU)
    pub position: [f32; 2],
    /// Texture coordinates
    pub uv: [f32; 2],
    /// This corner, in whatever space [`clip_rect`](Self::clip_rect) is
    /// written in.
    ///
    /// The clip test is an SDF against a rect, and the only thing it needs is
    /// for the point and the rect to be in one space. Which space that is, is
    /// the caller's business: an image is clipped in the clip's *own* space, so
    /// a turned clip cuts the turned shape; text is clipped in physical world
    /// pixels against the box around that shape, because glyphon needs a box
    /// anyway and #405 is where the rest of it lives.
    ///
    /// Mapping the four corners on the CPU is what keeps this free. The map is
    /// affine, and interpolating an affine function of position across a
    /// triangle gives the same answer as applying it to the interpolated
    /// position — so the shader needs no matrix, and the vertex no room for
    /// one.
    pub clip_pos: [f32; 2],
    /// Clip rect [x, y, width, height], in that same space.
    pub clip_rect: [f32; 4],
    /// Clip corner radii [top_left, top_right, bottom_right, bottom_left], in
    /// that same space. The shader reads all four — a rect clip passes zeros.
    pub clip_params: [f32; 4],
    /// `[curvature, _, _, _]`. Its own 16-byte slot because a `Float32x4` is
    /// the narrowest attribute that keeps the rest of the struct aligned, and
    /// there is no padding here to mine — unlike `ShapeInstance`, this is a
    /// per-*vertex* buffer with four attributes in it, nowhere near a limit.
    pub clip_curvature: [f32; 4],
}

/// The clip a textured quad is cut by, in whatever space the caller chose.
///
/// Four values that always travel together and always have to agree about
/// which space they are in, which is exactly the thing that is easy to get
/// wrong when they are four arguments in a row.
///
/// Which space it is, is the caller's business. [`shape`](Self::shape) cuts in
/// the clip's *own* coordinates, so a turned clip cuts the turned shape;
/// [`world_box`](Self::world_box) cuts in physical world pixels against the box
/// around that shape, which is all glyphon can be given — #405 is the rest of
/// that story.
#[derive(Clone, Copy, Debug)]
pub struct QuadClip {
    /// `[x, y, width, height]`. Negative width/height is the no-clip sentinel.
    pub rect: [f32; 4],
    /// `[top_left, top_right, bottom_right, bottom_left]`.
    pub radii: [f32; 4],
    /// Superellipse curvature (K-value), so a squircle clip cuts an image the
    /// way it cuts a rectangle. The shape shader has always taken this; this
    /// pipeline did not, and a squircle clip over an image cut with circular
    /// corners while the container beside it cut with squircular ones.
    pub curvature: f32,
    /// Physical world pixels into the space `rect` is written in. The identity
    /// where that space *is* physical world pixels, rather than an `Option` —
    /// "no map needed" and "no clip" are different facts, and `rect`'s sentinel
    /// is the one that answers the second.
    pub to_clip_space: Transform,
}

impl QuadClip {
    /// Nothing is cut.
    pub const NONE: Self = Self {
        rect: crate::renderer::gpu::NO_CLIP_RECT,
        radii: [0.0; 4],
        curvature: 1.0,
        to_clip_space: Transform::IDENTITY,
    };

    /// Everything is cut — a clip whose placement collapsed has no inside.
    pub(crate) const EMPTY: Self = Self {
        rect: crate::renderer::gpu::EMPTY_CLIP_RECT,
        radii: [0.0; 4],
        curvature: 1.0,
        to_clip_space: Transform::IDENTITY,
    };

    /// Cut to the clip's shape, in the coordinates the widget declared it in.
    pub fn shape(clip: &crate::shape::PlacedShape, scale: f32) -> Self {
        let Some(to_clip_space) = clip.to_local(scale) else {
            return Self::EMPTY;
        };
        let r = clip.rect;
        Self {
            rect: [r.x, r.y, r.width, r.height],
            radii: clip.radii.to_array(),
            curvature: clip.curvature,
            to_clip_space,
        }
    }

    /// Cut to a plain box in physical world pixels.
    pub fn world_box(rect: crate::widgets::Rect, scale: f32) -> Self {
        Self {
            rect: [
                rect.x * scale,
                rect.y * scale,
                rect.width * scale,
                rect.height * scale,
            ],
            radii: [0.0; 4],
            curvature: 1.0,
            to_clip_space: Transform::IDENTITY,
        }
    }

    /// This corner, in the clip's space.
    #[inline]
    pub(crate) fn place(&self, x: f32, y: f32) -> [f32; 2] {
        let (x, y) = self.to_clip_space.transform_point(x, y);
        [x, y]
    }
}

impl TexturedVertex {
    /// One corner of a quad: where it is on screen, where it is in the
    /// texture, and where it falls in its clip.
    ///
    /// `screen` is in physical pixels and `ndc` is that same point in clip
    /// space — both are wanted, so the caller passes both rather than having
    /// this undo one to get the other.
    #[inline]
    pub fn corner(ndc: [f32; 2], uv: [f32; 2], screen: (f32, f32), clip: &QuadClip) -> Self {
        Self {
            position: ndc,
            uv,
            clip_pos: clip.place(screen.0, screen.1),
            clip_rect: clip.rect,
            clip_params: clip.radii,
            clip_curvature: [clip.curvature, 0.0, 0.0, 0.0],
        }
    }

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
                // clip_pos
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
                // clip_curvature
                VertexAttribute {
                    offset: 56,
                    shader_location: 5,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::CornerRadii;
    use crate::shape::PlacedShape;
    use crate::widgets::Rect;

    /// The two constructors put their clip in two different spaces, and each
    /// has to scale — or not scale — accordingly.
    ///
    /// `world_box` is the text path: glyphon clips to a box in physical
    /// pixels, so the rect is scaled here and the map is the identity.
    /// `shape` is the image path: the rect stays in the logical units the
    /// widget declared, and the scale is folded into the map instead. Getting
    /// that backwards in either one scales twice or not at all, and the
    /// symptom is a HiDPI surface clipped to a quarter of its viewport.
    ///
    /// Written because the mutation job found every multiply in `world_box`
    /// could be a plus or a divide with the suite still green: nothing called
    /// it.
    #[test]
    fn a_clip_is_scaled_in_exactly_one_of_the_two_spaces() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);

        let boxed = QuadClip::world_box(rect, 2.0);
        assert_eq!(
            boxed.rect,
            [20.0, 40.0, 200.0, 100.0],
            "origin and extent both in physical pixels"
        );
        assert_eq!(
            boxed.place(20.0, 40.0),
            [20.0, 40.0],
            "and the corner is left where it is, because it is already there"
        );

        let shaped = QuadClip::shape(
            &PlacedShape {
                rect,
                radii: CornerRadii::uniform(4.0),
                curvature: 1.0,
                placement: crate::transform::Transform::IDENTITY,
            },
            2.0,
        );
        assert_eq!(
            shaped.rect,
            [10.0, 20.0, 100.0, 50.0],
            "the declaration, unscaled — the map is what carries the scale"
        );
        let [x, y] = shaped.place(20.0, 40.0);
        assert!(
            (x - 10.0).abs() < 0.01 && (y - 20.0).abs() < 0.01,
            "so the same physical corner arrives at the logical one, got ({x}, {y})"
        );
        assert_eq!(shaped.radii, [4.0; 4], "radii travel with the rect");
    }

    /// A clip whose placement collapses lets nothing through, rather than
    /// everything.
    #[test]
    fn a_collapsed_clip_cuts_it_all() {
        let collapsed = QuadClip::shape(
            &PlacedShape {
                rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                radii: CornerRadii::uniform(0.0),
                curvature: 1.0,
                placement: crate::transform::Transform::scale(0.0),
            },
            1.0,
        );
        assert_eq!(collapsed.rect, crate::renderer::gpu::EMPTY_CLIP_RECT);
        assert_ne!(
            collapsed.rect,
            crate::renderer::gpu::NO_CLIP_RECT,
            "the sentinel for `clip everything`, not the one for `clip nothing`"
        );
    }
}
