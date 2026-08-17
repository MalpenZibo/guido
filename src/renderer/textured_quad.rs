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

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer as WgpuBuffer, Device, RenderPass, RenderPipeline, Sampler,
    TextureFormat, TextureView,
};

use super::textured_vertex::{TexturedVertex, to_ndc};

/// Pipeline, bind group layout, sampler and index buffer for textured quads.
pub(super) struct TexturedQuadPipeline {
    pub(super) pipeline: RenderPipeline,
    pub(super) bind_group_layout: BindGroupLayout,
    pub(super) sampler: Sampler,
    /// Two triangles, shared by every quad — only the vertices are per-quad.
    pub(super) index_buffer: WgpuBuffer,

    /// The surface size the vertices are projected against. Every quad's
    /// geometry is computed in screen pixels and converted here, so this is
    /// part of drawing a quad rather than of producing its texture.
    screen_width: f32,
    screen_height: f32,
}

/// A quad ready to draw: the texture to sample, and the four corners its
/// geometry resolved to.
///
/// The two renderers reach the bind group differently — one owns it per quad,
/// the other shares it through a cached texture — which was the only reason
/// they each wrote out the same render loop.
pub(super) trait QuadDraw {
    fn bind_group(&self) -> &BindGroup;
    fn vertex_buffer(&self) -> &WgpuBuffer;
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
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&format!("{label} Pipeline")),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TexturedVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // Premultiplied alpha: the colour is already scaled, so it
                    // is added whole and only the destination is attenuated.
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
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

        let indices: [u16; 6] = [0, 1, 2, 1, 3, 2];
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label} Index Buffer")),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            index_buffer,
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

    /// Bind a texture for sampling, with this pipeline's layout and sampler.
    pub(super) fn bind_texture(
        &self,
        device: &Device,
        view: &TextureView,
        label: &str,
    ) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        })
    }

    /// Draw prepared quads: one pipeline and index buffer for all of them,
    /// then a bind group and vertex buffer per quad.
    pub(super) fn draw<'a, Q: QuadDraw>(
        &'a self,
        render_pass: &mut RenderPass<'a>,
        quads: &'a [Q],
    ) {
        if quads.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for quad in quads {
            render_pass.set_bind_group(0, quad.bind_group(), &[]);
            render_pass.set_vertex_buffer(0, quad.vertex_buffer().slice(..));
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }
}
