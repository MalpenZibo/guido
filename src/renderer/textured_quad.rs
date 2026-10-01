//! The wgpu side of drawing a textured quad, once.
//!
//! Images and text differ entirely in how they *produce* a texture — one
//! decodes or rasterises a source, the other lays out glyphs into an atlas —
//! but what they do with it afterwards is the same: the same shader, the same
//! vertex format, the same premultiplied blend state, the same clamped
//! bilinear sampler, the same two triangles.
//!
//! Both pipelines were written out in full, and the two copies were identical
//! down to the byte apart from their debug labels. Keeping them apart meant
//! the blend state could drift on one side and nothing would say so.

use std::rc::Rc;

use wgpu::{
    BindGroup, BindGroupLayout, Buffer as WgpuBuffer, Device, Queue, RenderPass, RenderPipeline,
    Sampler, TextureFormat, TextureView,
};

use super::textured_vertex::{TexturedVertex, to_ndc};
use crate::render_stats::{self, Pipeline, QuadObject};

/// Pipeline, bind group layout, sampler, and the frame's vertices for
/// textured quads.
pub(super) struct TexturedQuadPipeline {
    pub(super) pipeline: RenderPipeline,
    pub(super) bind_group_layout: BindGroupLayout,
    pub(super) sampler: Sampler,

    /// Every quad this frame draws, four corners each, in the order they were
    /// pushed. Uploaded once by [`upload`](Self::upload) into `vertex_buffer`,
    /// which a quad addresses by its index in here rather than owning a buffer
    /// of its own: a buffer per quad per frame was an allocation per image per
    /// frame, for sixty-four bytes that change only when the image moves.
    vertices: Vec<TexturedVertex>,
    /// Holds `capacity` quads. Grown, never shrunk, and only when a frame draws
    /// more quads than any frame before it.
    vertex_buffer: Option<WgpuBuffer>,
    /// Two triangles for each of `capacity` quads, quad `k` using vertices
    /// `4k..4k + 4`. A run of `n` quads is its first `6n` indices, offset to
    /// the run's first vertex by `base_vertex`.
    index_buffer: Option<WgpuBuffer>,
    capacity: usize,

    /// The surface size the vertices are projected against. Every quad's
    /// geometry is computed in screen pixels and converted here, so this is
    /// part of drawing a quad rather than of producing its texture.
    screen_width: f32,
    screen_height: f32,
}

/// A quad ready to draw: the texture to sample, and where in the frame's
/// vertices its four corners are.
///
/// The bind group is shared: it belongs to the cached texture the quad
/// samples, so two quads sampling the same texture hold the same one, and a
/// run of them is one draw call.
pub(super) trait QuadDraw {
    fn bind_group(&self) -> &Rc<BindGroup>;
    /// The quad's index among the frame's quads, as [`TexturedQuadPipeline::push`]
    /// returned it.
    fn quad(&self) -> u32;
}

impl TexturedQuadPipeline {
    /// `label` names the caller in wgpu's debug output ("ImageQuad",
    /// "TextQuad"), which is the only thing that ever differed between the
    /// two copies.
    pub(super) fn new(device: &Device, format: TextureFormat, label: &str) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&format!("{label} Shader")),
            source: wgpu::ShaderSource::Wgsl(include_str!("textured_quad_shader.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label} Bind Group Layout")),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&format!("{label} Pipeline Layout")),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label} Pipeline")),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(TexturedVertex::desc())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha, as Skia stores textures: a texel's
                    // colour is already scaled by its coverage, so it is added
                    // whole and only the destination is attenuated. Every
                    // texture arrives that way — glyphon leaves a text quad's
                    // premultiplied, tiny-skia an SVG's, and raster images are
                    // premultiplied when decoded or uploaded. Bilinear
                    // filtering then mixes colour weighted by coverage, so an
                    // empty texel's black never bleeds into a scaled edge.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(&format!("{label} Sampler")),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            vertices: Vec::new(),
            vertex_buffer: None,
            index_buffer: None,
            capacity: 0,
            screen_width: 800.0,
            screen_height: 600.0,
        }
    }

    /// Update the surface size the vertices are projected against.
    pub(super) fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Screen pixels to normalised device coordinates.
    pub(super) fn to_ndc(&self, x: f32, y: f32) -> [f32; 2] {
        to_ndc(x, y, self.screen_width, self.screen_height)
    }

    /// Forget the last frame's quads.
    pub(super) fn begin_frame(&mut self) {
        self.vertices.clear();
    }

    /// Add a quad to this frame, returning the index [`QuadDraw::quad`]
    /// reports for it.
    pub(super) fn push(&mut self, corners: [TexturedVertex; 4]) -> u32 {
        let quad = (self.vertices.len() / 4) as u32;
        self.vertices.extend(corners);
        quad
    }

    /// Write this frame's quads to the GPU, before the pass that draws them
    /// opens. Allocates only when the frame holds more quads than the buffers
    /// have ever held.
    pub(super) fn upload(&mut self, device: &Device, queue: &Queue) {
        let quads = self.vertices.len() / 4;
        if quads == 0 {
            return;
        }
        if quads > self.capacity {
            let capacity = quads.max(self.capacity * 2).max(16);
            render_stats::record_quad_allocation(QuadObject::Buffer);
            self.vertex_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TexturedQuad Vertex Buffer"),
                size: (capacity * 4 * std::mem::size_of::<TexturedVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            let indices: Vec<u32> = (0..capacity as u32)
                .flat_map(|k| {
                    let v = 4 * k;
                    [v, v + 1, v + 2, v + 1, v + 3, v + 2]
                })
                .collect();
            render_stats::record_quad_allocation(QuadObject::Buffer);
            let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("TexturedQuad Index Buffer"),
                size: (indices.len() * std::mem::size_of::<u32>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&index_buffer, 0, bytemuck::cast_slice(&indices));
            self.index_buffer = Some(index_buffer);
            self.capacity = capacity;
        }
        if let Some(buffer) = &self.vertex_buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&self.vertices));
        }
    }

    /// Bind a texture for sampling, with this pipeline's layout and sampler.
    ///
    /// Made once per cached texture and kept with it, not once per frame.
    pub(super) fn bind_texture(
        &self,
        device: &Device,
        view: &TextureView,
        label: &str,
    ) -> Rc<BindGroup> {
        render_stats::record_quad_allocation(QuadObject::BindGroup);
        Rc::new(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        }))
    }

    /// Draw prepared quads, in order, one draw call per run: consecutive quads
    /// that sample through the same bind group and sit next to each other in
    /// the frame's vertices. Counted under `pipeline`.
    ///
    /// A run is exactly as long as paint order allows. A quad that samples
    /// something else ends it even if a later quad would have joined, because
    /// drawing that later quad early would draw it under the one between.
    pub(super) fn draw<'a, Q: QuadDraw>(
        &'a self,
        render_pass: &mut RenderPass<'a>,
        quads: &'a [Q],
        pipeline: Pipeline,
    ) {
        let (Some(vertices), Some(indices)) = (&self.vertex_buffer, &self.index_buffer) else {
            return;
        };
        if quads.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, vertices.slice(..));
        render_pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);

        let mut calls = 0;
        let mut rest = quads;
        while let Some(first) = rest.first() {
            let run = 1 + rest[1..]
                .iter()
                .zip(rest)
                .take_while(|(next, prev)| {
                    Rc::ptr_eq(next.bind_group(), prev.bind_group())
                        && next.quad() == prev.quad() + 1
                })
                .count();
            render_pass.set_bind_group(0, &**first.bind_group(), &[]);
            render_pass.draw_indexed(0..6 * run as u32, 4 * first.quad() as i32, 0..1);
            calls += 1;
            rest = &rest[run..];
        }
        render_stats::record_draw_calls(pipeline, calls);
    }
}
