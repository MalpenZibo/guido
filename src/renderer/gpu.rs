//! GPU data structures for instanced rendering.
//!
//! This module contains the vertex and instance data structures used by the
//! Instanced rendering pipeline data structures. Instead of duplicating vertex
//! data for each shape, we use a single unit quad and per-instance data.

use wgpu::{VertexAttribute, VertexBufferLayout, VertexFormat, VertexStepMode};

/// Clip rect sentinel: negative width/height disables clipping in the shader.
pub const NO_CLIP_RECT: [f32; 4] = [0.0, 0.0, -1.0, -1.0];

/// Zero width and height: the clip lets nothing through, which is what a shape
/// whose placement collapsed should do. Distinct from [`NO_CLIP_RECT`], whose
/// negative extents mean there is no clip at all.
pub const EMPTY_CLIP_RECT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

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
    /// Corner radii in physical pixels:
    /// [top_left, top_right, bottom_right, bottom_left]
    pub corner_radii: [f32; 4],

    // === Colors ===
    /// Fill color RGBA
    pub fill_color: [f32; 4],
    /// Border color RGBA
    pub border_color: [f32; 4],
    /// Shadow color RGBA
    pub shadow_color: [f32; 4],

    // === Shadow ===
    /// Shadow offset in logical pixels (x, y)
    pub shadow_offset: [f32; 2],
    /// Shadow blur radius in logical pixels
    pub shadow_blur: f32,
    /// Shadow spread in logical pixels
    pub shadow_spread: f32,

    // === Placement ===
    /// Transform matrix: [a, b, tx, c, d, ty] (row-major 2x3).
    /// Transform origin is baked into the matrix via `center_at()` on CPU.
    pub transform: [f32; 6],
    /// Where `clip_rect` begins, because a `vec4` may not start at 120.
    pub _pad_transform: [f32; 2],

    // === Clip ===
    /// Clip rect in the clip's own space, logical pixels [x, y, width, height].
    /// Negative width/height = no clipping. Zero width/height = clip everything.
    ///
    /// **Not scaled to physical pixels**, unlike every other length in this
    /// struct. Scaling it here and folding the scale into the inverse as well
    /// would apply the surface scale twice.
    pub clip_rect: [f32; 4],
    /// [top_left, top_right, bottom_right, bottom_left], in the clip's own
    /// space and the logical units it was declared in — like `clip_rect`, and
    /// unlike everything else here. The surface scale reaches them through
    /// `clip_inverse`, which carries the fragment back to meet them.
    pub clip_radii: [f32; 4],
    /// World (physical pixels) to clip space: `[a, b, tx, c, d, ty]`, a
    /// row-major 2×3 affine map, identity for a clip already in world
    /// coordinates — which is most of them.
    ///
    /// The fragment is carried into the clip's own space and tested there, so
    /// a clip that has been turned cuts the turned shape rather than the box
    /// around it.
    ///
    /// **One field, since #398.** Until the instance moved to a storage
    /// buffer these six floats lived in two homes — four here and two beside
    /// the shape's own `transform` — because a vertex layout may declare
    /// sixteen attributes and this one declared sixteen. There was no
    /// seventeenth to have, only padding to mine.
    pub clip_inverse: [f32; 6],
    /// The clip's superellipse curvature, which used to sit in the *border*
    /// block for the same reason: evicting it freed a whole 16-byte attribute
    /// for the matrix above.
    pub clip_curvature: f32,
    /// Where `gradient_start` begins.
    pub _pad_clip: f32,

    // === Gradient ===
    /// Gradient start color [r, g, b, a]
    pub gradient_start: [f32; 4],
    /// Gradient end color [r, g, b, a]
    pub gradient_end: [f32; 4],

    // === Scalars ===
    //
    // Gathered rather than scattered. Each of these used to sit beside a
    // block it had nothing to do with, padded out to the next sixteen bytes;
    // together they fill one tail block and the struct is the size it was.
    /// Border width in logical pixels
    pub border_width: f32,
    /// Superellipse curvature (K-value: 1.0=circle, 2.0=squircle)
    pub shape_curvature: f32,
    /// Gradient type: 0=none, 1=horizontal, 2=vertical, 3=diagonal, 4=diagonal_reverse
    pub gradient_type: u32,
    /// Rounds the struct out to its 16-byte alignment.
    pub _pad_tail: u32,
}

impl Default for ShapeInstance {
    fn default() -> Self {
        Self {
            rect: [0.0, 0.0, 0.0, 0.0],
            corner_radii: [0.0; 4],
            fill_color: [0.0, 0.0, 0.0, 0.0],
            border_color: [0.0, 0.0, 0.0, 0.0],
            shadow_color: [0.0, 0.0, 0.0, 0.0],
            shadow_offset: [0.0, 0.0],
            shadow_blur: 0.0,
            shadow_spread: 0.0,
            transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // identity
            _pad_transform: [0.0, 0.0],
            clip_rect: NO_CLIP_RECT,
            clip_radii: [0.0; 4],
            clip_inverse: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0], // identity
            clip_curvature: 1.0,
            _pad_clip: 0.0,
            gradient_start: [0.0, 0.0, 0.0, 0.0],
            gradient_end: [0.0, 0.0, 0.0, 0.0],
            border_width: 0.0,
            shape_curvature: 1.0,
            gradient_type: 0, // No gradient
            _pad_tail: 0,
        }
    }
}

impl ShapeInstance {
    /// Create a shape instance from a rectangle with basic properties.
    pub fn from_rect(
        rect: [f32; 4],
        fill_color: [f32; 4],
        corner_radii: [f32; 4],
        curvature: f32,
    ) -> Self {
        Self {
            rect,
            corner_radii,
            shape_curvature: curvature,
            fill_color,
            ..Default::default()
        }
    }

    /// Set transform from a Transform struct, scaling translation by scale_factor.
    pub fn with_transform(mut self, transform: &crate::transform::Transform, scale: f32) -> Self {
        if !transform.is_identity() {
            // Transform stores exactly the affine layout the shader expects
            // ([a, b, tx, c, d, ty]); only the translation needs scaling.
            self.transform = transform.data;
            self.transform[2] *= scale;
            self.transform[5] *= scale;
        }
        self
    }

    /// Set the clip, as the shape it was declared to be.
    ///
    /// The rect and its radii go across untouched, in the logical units the
    /// widget wrote them in, and `scale` is folded into
    /// [`to_local`](crate::shape::PlacedShape::to_local) instead —
    /// the fragment arrives in physical pixels and has to come back to a space
    /// where the clip is that rect.
    ///
    /// A placement that collapses — `scale(0.0)` — has no inverse and no
    /// inside; the sentinel clips everything, which is what a shape of zero
    /// area should do.
    pub fn with_clip(mut self, clip: &crate::shape::PlacedShape, scale: f32) -> Self {
        let Some(to_clip) = clip.to_local(scale) else {
            self.clip_rect = EMPTY_CLIP_RECT;
            return self;
        };
        self.clip_rect = [clip.rect.x, clip.rect.y, clip.rect.width, clip.rect.height];
        self.clip_radii = clip.radii.to_array();
        self.clip_curvature = clip.curvature;
        self.clip_inverse = to_clip.data;
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
        let scaled = shadow.scaled(scale);
        self.shadow_offset = [scaled.offset.0, scaled.offset.1];
        self.shadow_blur = scaled.blur;
        self.shadow_spread = scaled.spread;
        self.shadow_color = [
            scaled.color.r,
            scaled.color.g,
            scaled.color.b,
            scaled.color.a,
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

    /// Multiply `opacity` into every colour the shape draws with — the fill,
    /// both ends of a gradient, the border and the shadow — each on its own.
    ///
    /// On the CPU and into the colours, rather than as one more field the
    /// shader multiplies at the end: the fragment shader composites the
    /// border over the fill and both over the shadow, and a fade is defined as
    /// each of those colours at a fraction of its alpha.
    pub fn faded(mut self, opacity: f32) -> Self {
        for colour in [
            &mut self.fill_color,
            &mut self.border_color,
            &mut self.shadow_color,
            &mut self.gradient_start,
            &mut self.gradient_end,
        ] {
            colour[3] *= opacity;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three lengths carry the surface scale into the instance buffer and
    /// the colour does not.
    ///
    /// The whole method could return `Default::default()` with the suite green:
    /// what draws a shadow correctly is watched by the goldens, and those skip
    /// on any adapter but lavapipe — including the one the mutation job runs on.
    /// So this is the only thing standing between a HiDPI shadow and silence.
    #[test]
    fn a_shadow_reaches_the_instance_buffer_scaled_but_not_faded() {
        let shadow = super::super::types::Shadow::new(
            (3.0, -4.0),
            8.0,
            2.0,
            crate::widgets::Color::rgba(1.0, 0.0, 0.0, 0.5),
        );
        let i = ShapeInstance::default().with_shadow(&shadow, 2.0);

        assert_eq!(i.shadow_offset, [6.0, -8.0], "both axes, sign kept");
        assert_eq!(i.shadow_blur, 16.0);
        assert_eq!(i.shadow_spread, 4.0);
        assert_eq!(
            i.shadow_color,
            [1.0, 0.0, 0.0, 0.5],
            "the colour is not a length"
        );
    }

    /// Every colour an instance draws with is faded, and nothing else is.
    #[test]
    fn a_faded_instance_keeps_its_colours_and_scales_their_alpha() {
        let colours = [
            [1.0, 0.0, 0.0, 0.8],
            [0.0, 1.0, 0.0, 0.6],
            [0.0, 0.0, 1.0, 0.4],
            [1.0, 1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0, 0.2],
        ];
        let solid = ShapeInstance {
            fill_color: colours[0],
            border_color: colours[1],
            shadow_color: colours[2],
            gradient_start: colours[3],
            gradient_end: colours[4],
            shadow_blur: 7.0,
            ..ShapeInstance::default()
        };
        let faded = solid.faded(0.5);
        let drawn = [
            faded.fill_color,
            faded.border_color,
            faded.shadow_color,
            faded.gradient_start,
            faded.gradient_end,
        ];
        for (before, after) in colours.iter().zip(drawn) {
            assert_eq!(after[..3], before[..3], "the colour is left alone");
            assert_eq!(after[3], before[3] * 0.5, "its alpha is halved");
        }
        assert_eq!(faded.shadow_blur, 7.0, "and a fade is not a length");
    }

    #[test]
    fn test_shape_instance_size() {
        // Every instance is uploaded per draw, so growth is paid on every
        // shape on screen: 176 (base + clip) + 48 (gradient) + 16 (the clip's
        // four corner radii) = 240.
        let size = std::mem::size_of::<ShapeInstance>();
        println!("ShapeInstance size: {} bytes", size);
        assert!(size <= 256, "ShapeInstance is too large: {} bytes", size);
        assert_eq!(size, 240, "ShapeInstance size changed unexpectedly");
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

/// `ShapeInstance` is one layout written twice, and the two have to agree.
///
/// The shape pipeline's per-instance data is declared in Rust
/// (`ShapeInstance`, `src/renderer/gpu.rs`) and again in WGSL (`InstanceInput`,
/// `src/renderer/shader.wgsl`). Nothing in the compiler connects them: the
/// bytes are written by `bytemuck` from the Rust struct and read back by
/// offset from the WGSL one, so a field inserted, reordered or resized on one
/// side is read as its neighbour on the other. Every colour after it shifts,
/// and the picture is wrong in a way no type checks.
///
/// **This could not be written before #398.** The instance arrived as sixteen
/// vertex attributes — the whole allowance a vertex layout has — and the map
/// from struct field to attribute was a hand-written table of offsets in
/// `desc()` that agreed with the struct by arithmetic and with the shader by
/// comment. Two of the fields were not even in their own homes: the clip's
/// curvature sat in the border block and two of the six floats of its matrix
/// sat beside the shape's own transform, because those were the only sixteen
/// bytes left. There was nothing to compare a struct against.
///
/// Delivered as a storage buffer indexed by `@builtin(instance_index)`, the
/// WGSL side is a struct with named members and a layout the specification
/// fixes — so it can be computed here and checked against the Rust one, field
/// by field.
///
/// The Rust struct carries explicit `_pad` members where WGSL pads implicitly
/// by its alignment rules, so they have no counterpart to be checked against
/// and do not appear in the list below. Everything else must line up, and the
/// two must agree on the total size.
#[cfg(test)]
mod instance_layout {
    use super::*;
    use std::mem::offset_of;

    /// What WGSL says a type occupies, as (align, size).
    ///
    /// Only the types this struct uses. A type appearing in the shader that is
    /// not here fails the test rather than being guessed at.
    fn layout_of(ty: &str) -> Option<(usize, usize)> {
        match ty {
            "f32" | "u32" | "i32" => Some((4, 4)),
            "vec2<f32>" => Some((8, 8)),
            "vec3<f32>" => Some((16, 12)),
            "vec4<f32>" | "vec4<u32>" => Some((16, 16)),
            // Fixed-size arrays in the storage address space take their element's
            // alignment as their stride, so `array<f32, 6>` is 24 tightly packed
            // bytes — which is what `[f32; 6]` is in Rust.
            _ => ty
                .strip_prefix("array<f32,")
                .and_then(|rest| rest.strip_suffix('>'))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .map(|n| (4, 4 * n)),
        }
    }

    /// The shader's struct, as (field name, offset), in declaration order,
    /// and the size of the whole.
    fn wgsl_fields() -> (Vec<(String, usize)>, usize) {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/renderer/shader.wgsl"),
        )
        .expect("shader.wgsl");

        let start = source
            .find("struct InstanceInput {")
            .expect("shader.wgsl declares no `InstanceInput` — the shape pipeline reads one");
        let body = &source[start..];
        let body = &body[..body.find('}').expect("`InstanceInput` is never closed")];

        let mut offset = 0usize;
        let mut align = 1usize;
        let mut fields = Vec::new();
        for line in body.lines().skip(1) {
            let line = line.trim();
            let Some(line) = line.strip_suffix(',') else {
                continue; // a comment, or the blank line before one
            };
            if line.starts_with("//") {
                continue;
            }
            let (name, ty) = line
                .split_once(':')
                .unwrap_or_else(|| panic!("cannot read `{line}` as a WGSL struct member"));
            let (name, ty) = (name.trim(), ty.trim());
            let (a, size) =
                layout_of(ty).unwrap_or_else(|| panic!("no WGSL layout recorded for type `{ty}`"));
            offset = offset.next_multiple_of(a);
            fields.push((name.to_owned(), offset));
            offset += size;
            align = align.max(a);
        }
        (fields, offset.next_multiple_of(align))
    }

    #[test]
    fn the_shader_and_the_struct_agree_field_for_field() {
        let rust: Vec<(&str, usize)> = vec![
            ("rect", offset_of!(ShapeInstance, rect)),
            ("corner_radii", offset_of!(ShapeInstance, corner_radii)),
            ("fill_color", offset_of!(ShapeInstance, fill_color)),
            ("border_color", offset_of!(ShapeInstance, border_color)),
            ("shadow_color", offset_of!(ShapeInstance, shadow_color)),
            ("shadow_offset", offset_of!(ShapeInstance, shadow_offset)),
            ("shadow_blur", offset_of!(ShapeInstance, shadow_blur)),
            ("shadow_spread", offset_of!(ShapeInstance, shadow_spread)),
            ("transform", offset_of!(ShapeInstance, transform)),
            ("clip_rect", offset_of!(ShapeInstance, clip_rect)),
            ("clip_radii", offset_of!(ShapeInstance, clip_radii)),
            ("clip_inverse", offset_of!(ShapeInstance, clip_inverse)),
            ("clip_curvature", offset_of!(ShapeInstance, clip_curvature)),
            ("gradient_start", offset_of!(ShapeInstance, gradient_start)),
            ("gradient_end", offset_of!(ShapeInstance, gradient_end)),
            ("border_width", offset_of!(ShapeInstance, border_width)),
            (
                "shape_curvature",
                offset_of!(ShapeInstance, shape_curvature),
            ),
            ("gradient_type", offset_of!(ShapeInstance, gradient_type)),
        ];

        let (wgsl, wgsl_size) = wgsl_fields();

        assert_eq!(
            wgsl.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            rust.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            "`InstanceInput` and `ShapeInstance` do not declare the same fields in \
             the same order. Every field after the first difference is read as its \
             neighbour."
        );

        for ((name, shader), (_, host)) in wgsl.iter().zip(&rust) {
            assert_eq!(
                shader, host,
                "`{name}` is at {host} in `ShapeInstance` and {shader} in \
                 `InstanceInput` — the shader reads it from the wrong bytes"
            );
        }

        assert_eq!(
            wgsl_size,
            size_of::<ShapeInstance>(),
            "the two structs are different sizes, so every instance after the \
             first is read from the wrong offset"
        );
    }
}
