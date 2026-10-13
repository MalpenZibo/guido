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
    /// for the point and the rect to be in one space. That space is the clip's
    /// own, for an image and for a transformed text alike, so a turned clip
    /// cuts the turned shape rather than the box around it.
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
    /// `[clip curvature, opacity]`.
    ///
    /// The opacity is multiplied into the alpha of every texel the quad
    /// samples: the fade an ancestor asks for, carried on the vertex rather
    /// than into the texture, so a text or an image that fades is not
    /// rasterised again on every frame of it.
    pub curvature_opacity: [f32; 2],
    /// Where the clip continues in the frame's clip records, or
    /// [`NO_CHAIN`](super::clip::NO_CHAIN) — see
    /// [`ResolvedClip`](super::clip::ResolvedClip). Eight bytes that used to
    /// pad the two scalars above out to a `Float32x4` hold this and the
    /// padding beside it, so the vertex stays 88 bytes.
    pub clip_chain: u32,
    pub _pad: u32,
    /// `[r, g, b, amount]`: the colour every texel takes, keeping its alpha,
    /// and how much of it — `1.0` for a tinted image, `0.0` for everything
    /// else, which leaves the texel as it was. Carried on the vertex for the
    /// same reason as the opacity: a new tint is not a new texture.
    pub tint: [f32; 4],
}

/// The clip a textured quad is cut by, in the clip's own coordinates.
///
/// Four values that always travel together and always have to agree about
/// which space they are in, which is exactly the thing that is easy to get
/// wrong when they are four arguments in a row.
///
/// There used to be a second spelling — a plain box in physical world pixels —
/// because text was cut by the box around its clip rather than the clip. Both
/// quad pipelines now take [`shape`](Self::shape), so a turned clip cuts the
/// turned shape whichever of them is drawing.
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
    /// Where the clip continues in the frame's clip records, or
    /// [`NO_CHAIN`](super::clip::NO_CHAIN).
    pub chain: u32,
}

impl QuadClip {
    /// Nothing is cut.
    pub const NONE: Self = Self {
        rect: crate::renderer::gpu::NO_CLIP_RECT,
        radii: [0.0; 4],
        curvature: 1.0,
        to_clip_space: Transform::IDENTITY,
        chain: super::clip::NO_CHAIN,
    };

    /// Everything is cut — a clip whose placement collapsed has no inside.
    pub(crate) const EMPTY: Self = Self {
        rect: crate::renderer::gpu::EMPTY_CLIP_RECT,
        radii: [0.0; 4],
        curvature: 1.0,
        to_clip_space: Transform::IDENTITY,
        chain: super::clip::NO_CHAIN,
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
            chain: super::clip::NO_CHAIN,
        }
    }

    /// Cut to the clip's first shape, and down its chain.
    pub fn resolved(clip: &super::clip::ResolvedClip, scale: f32) -> Self {
        Self {
            chain: clip.rest,
            ..Self::shape(&clip.first, scale)
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
    /// texture, where it falls in its clip, and how opaque it is drawn.
    ///
    /// `screen` is in physical pixels and `ndc` is that same point in clip
    /// space — both are wanted, so the caller passes both rather than having
    /// this undo one to get the other.
    #[inline]
    pub fn corner(
        ndc: [f32; 2],
        uv: [f32; 2],
        screen: (f32, f32),
        clip: &QuadClip,
        opacity: f32,
        tint: [f32; 4],
    ) -> Self {
        Self {
            position: ndc,
            uv,
            clip_pos: clip.place(screen.0, screen.1),
            clip_rect: clip.rect,
            clip_params: clip.radii,
            curvature_opacity: [clip.curvature, opacity],
            clip_chain: clip.chain,
            _pad: 0,
            tint,
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
                // curvature_opacity
                VertexAttribute {
                    offset: 56,
                    shader_location: 5,
                    format: VertexFormat::Float32x2,
                },
                // clip_chain
                VertexAttribute {
                    offset: 64,
                    shader_location: 7,
                    format: VertexFormat::Uint32,
                },
                // tint
                VertexAttribute {
                    offset: 72,
                    shader_location: 6,
                    format: VertexFormat::Float32x4,
                },
            ],
        }
    }
}

/// The tint of a quad that has none: its texels are drawn as they are.
pub const NO_TINT: [f32; 4] = [0.0; 4];

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

    /// The rect stays in the logical units the widget declared and the scale is
    /// folded into the map instead. Getting that backwards scales twice or not
    /// at all, and the symptom is a HiDPI surface clipped to a quarter of its
    /// viewport.
    #[test]
    fn a_clip_is_scaled_by_its_map_and_not_by_its_rect() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);

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

    /// Every attribute the layout declares reads the field it names, and the
    /// chain head fits in what used to be padding.
    #[test]
    fn the_layout_reads_every_field_where_it_is() {
        use std::mem::offset_of;
        let fields = [
            offset_of!(TexturedVertex, position),
            offset_of!(TexturedVertex, uv),
            offset_of!(TexturedVertex, clip_pos),
            offset_of!(TexturedVertex, clip_rect),
            offset_of!(TexturedVertex, clip_params),
            offset_of!(TexturedVertex, curvature_opacity),
            offset_of!(TexturedVertex, tint),
            offset_of!(TexturedVertex, clip_chain),
        ];
        let layout = TexturedVertex::desc();
        for attribute in layout.attributes {
            assert_eq!(
                attribute.offset as usize, fields[attribute.shader_location as usize],
                "location {}",
                attribute.shader_location
            );
        }
        assert_eq!(layout.attributes.len(), fields.len());
        assert_eq!(size_of::<TexturedVertex>(), 88);
    }

    /// A resolved clip hands its chain to every corner.
    #[test]
    fn a_chained_clip_reaches_the_vertex() {
        let resolved = crate::renderer::clip::ResolvedClip {
            rest: 5,
            ..crate::renderer::clip::ResolvedClip::alone(PlacedShape::placed(
                Rect::new(0.0, 0.0, 10.0, 10.0),
                CornerRadii::uniform(0.0),
                1.0,
                crate::transform::Transform::IDENTITY,
            ))
        };
        let clip = QuadClip::resolved(&resolved, 1.0);
        let vertex = TexturedVertex::corner([0.0; 2], [0.0; 2], (0.0, 0.0), &clip, 1.0, NO_TINT);
        assert_eq!(vertex.clip_chain, 5);
        assert_eq!(
            TexturedVertex::corner(
                [0.0; 2],
                [0.0; 2],
                (0.0, 0.0),
                &QuadClip::NONE,
                1.0,
                NO_TINT
            )
            .clip_chain,
            crate::renderer::clip::NO_CHAIN
        );
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
