//! GPU rendering for the V2 render tree using instanced rendering.
//!
//! This module uses a single draw call per layer to render all shapes,
//! significantly reducing CPU-GPU communication overhead.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline, ShaderModule,
};

use crate::renderer::TextEntry;
use crate::renderer::context::SurfaceState;
use crate::renderer::text::TextRenderState;
use crate::widgets::Color;

use super::commands::{Border, DrawCommand};
use super::flatten::{FlattenedCommand, RenderLayer};
use super::gpu::{QUAD_INDICES, QUAD_VERTICES, QuadVertex, ShaderUniforms, ShapeInstance};

/// The V2 renderer using instanced rendering.
///
/// This renderer converts [`FlattenedCommand`]s into GPU instance data
/// and renders all shapes with a single draw call per layer.
pub struct RendererV2 {
    device: Arc<Device>,
    queue: Arc<Queue>,
    pipeline: RenderPipeline,
    #[allow(dead_code)]
    bind_group_layout: BindGroupLayout,

    // Shared vertex buffer (unit quad)
    vertex_buffer: Buffer,
    index_buffer: Buffer,

    // Uniform buffer
    uniform_buffer: Buffer,
    uniform_bind_group: BindGroup,

    // Instance buffer (resized as needed)
    instance_buffer: Buffer,
    instance_buffer_capacity: usize,

    // Text rendering (reuses V1's glyphon-based renderer)
    text_state: TextRenderState,

    // Screen dimensions
    screen_width: f32,
    screen_height: f32,
    scale_factor: f32,
}

impl RendererV2 {
    /// Create a new V2 renderer with instanced rendering.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, format: wgpu::TextureFormat) -> Self {
        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("RendererV2 Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader_v2.wgsl").into()),
        });

        // Create bind group layout for uniforms
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("RendererV2 Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // Create pipeline
        let pipeline = Self::create_pipeline(&device, &shader, &bind_group_layout, format);

        // Create vertex buffer (unit quad)
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("RendererV2 Vertex Buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: BufferUsages::VERTEX,
        });

        // Create index buffer
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("RendererV2 Index Buffer"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: BufferUsages::INDEX,
        });

        // Create uniform buffer
        let uniforms = ShaderUniforms::new(800.0, 600.0, 1.0);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("RendererV2 Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        // Create uniform bind group
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("RendererV2 Uniform Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create initial instance buffer (will be resized as needed)
        let initial_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("RendererV2 Instance Buffer"),
            size: (initial_capacity * std::mem::size_of::<ShapeInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Initialize text renderer (reuses V1's glyphon-based renderer)
        let text_state = TextRenderState::new(&device, &queue, format);

        Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            uniform_bind_group,
            instance_buffer,
            instance_buffer_capacity: initial_capacity,
            text_state,
            screen_width: 800.0,
            screen_height: 600.0,
            scale_factor: 1.0,
        }
    }

    /// Create the render pipeline.
    fn create_pipeline(
        device: &Device,
        shader: &ShaderModule,
        bind_group_layout: &BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> RenderPipeline {
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("RendererV2 Pipeline Layout"),
            bind_group_layouts: &[bind_group_layout],
            immediate_size: 0,
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("RendererV2 Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[QuadVertex::desc(), ShapeInstance::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
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
                cull_mode: None, // No culling for 2D
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Set the screen size in logical pixels.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Set the HiDPI scale factor.
    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = scale;
    }

    /// Ensure instance buffer has enough capacity.
    fn ensure_instance_capacity(&mut self, count: usize) {
        if count > self.instance_buffer_capacity {
            // Double capacity or use count, whichever is larger
            let new_capacity = (self.instance_buffer_capacity * 2).max(count);
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("RendererV2 Instance Buffer"),
                size: (new_capacity * std::mem::size_of::<ShapeInstance>()) as u64,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_buffer_capacity = new_capacity;
        }
    }

    /// Render flattened commands to a surface.
    pub fn render(
        &mut self,
        surface: &mut SurfaceState,
        commands: &[FlattenedCommand],
        clear_color: Color,
    ) {
        let output = match surface.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost) => {
                surface.resize(surface.width(), surface.height());
                return;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Out of GPU memory");
                return;
            }
            Err(e) => {
                log::error!("Surface error: {:?}", e);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Update uniform buffer with current screen size
        // Note: screen_width/height are already in physical pixels (set by set_screen_size)
        let uniforms =
            ShaderUniforms::new(self.screen_width, self.screen_height, self.scale_factor);
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Separate commands by layer
        let shape_commands: Vec<_> = commands
            .iter()
            .filter(|c| c.layer == RenderLayer::Shapes)
            .collect();
        let text_commands: Vec<_> = commands
            .iter()
            .filter(|c| c.layer == RenderLayer::Text)
            .collect();
        let overlay_commands: Vec<_> = commands
            .iter()
            .filter(|c| c.layer == RenderLayer::Overlay)
            .collect();

        // Convert shape commands to instances
        let shape_instances = self.commands_to_instances(&shape_commands);
        let overlay_instances = self.commands_to_instances(&overlay_commands);

        // Convert text commands to TextEntry for the V1 text renderer
        let text_entries: Vec<TextEntry> = text_commands
            .iter()
            .filter_map(|cmd| self.command_to_text_entry(cmd))
            .collect();

        // Prepare text for rendering (this returns indices of transformed texts,
        // but for simplicity we render all text directly for now)
        if !text_entries.is_empty() {
            let _transformed_indices = self.text_state.prepare_text(
                &self.device,
                &self.queue,
                &text_entries,
                self.screen_width as u32,
                self.screen_height as u32,
                self.scale_factor,
            );
        }

        // Ensure we have enough capacity
        let total_instances = shape_instances.len() + overlay_instances.len();
        self.ensure_instance_capacity(total_instances);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("RendererV2 Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("RendererV2 Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: clear_color.r as f64,
                            g: clear_color.g as f64,
                            b: clear_color.b as f64,
                            a: clear_color.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

            // Draw shapes (background layer)
            if !shape_instances.is_empty() {
                self.queue.write_buffer(
                    &self.instance_buffer,
                    0,
                    bytemuck::cast_slice(&shape_instances),
                );
                render_pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                render_pass.draw_indexed(0..6, 0, 0..shape_instances.len() as u32);
            }

            // Draw text layer (between shapes and overlay)
            if !text_entries.is_empty() {
                self.text_state.render(&mut render_pass, &self.device);
            }

            // Draw overlay shapes (after text, for effects like ripples)
            if !overlay_instances.is_empty() {
                // Write overlay instances after shape instances
                let offset = (shape_instances.len() * std::mem::size_of::<ShapeInstance>()) as u64;
                self.queue.write_buffer(
                    &self.instance_buffer,
                    offset,
                    bytemuck::cast_slice(&overlay_instances),
                );
                render_pass.set_vertex_buffer(
                    1,
                    self.instance_buffer.slice(
                        offset
                            ..offset
                                + (overlay_instances.len() * std::mem::size_of::<ShapeInstance>())
                                    as u64,
                    ),
                );
                render_pass.draw_indexed(0..6, 0, 0..overlay_instances.len() as u32);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    /// Convert flattened commands to shape instances.
    fn commands_to_instances(&self, commands: &[&FlattenedCommand]) -> Vec<ShapeInstance> {
        commands
            .iter()
            .filter_map(|cmd| self.command_to_instance(cmd))
            .collect()
    }

    /// Convert a single command to a shape instance.
    fn command_to_instance(&self, cmd: &FlattenedCommand) -> Option<ShapeInstance> {
        match &cmd.command {
            DrawCommand::RoundedRect {
                rect,
                color,
                radius,
                curvature,
                border,
                shadow,
                gradient: _, // TODO: gradients not yet supported in V2
            } => {
                // Scale coordinates for HiDPI
                let scale = self.scale_factor;

                let mut instance = ShapeInstance {
                    rect: [
                        rect.x * scale,
                        rect.y * scale,
                        rect.width * scale,
                        rect.height * scale,
                    ],
                    corner_radius: radius * scale,
                    shape_curvature: *curvature,
                    _pad0: [0.0, 0.0],
                    fill_color: [color.r, color.g, color.b, color.a],
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
                    transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    _pad3: [0.0, 0.0],
                };

                // Border
                if let Some(Border { width, color }) = border {
                    instance.border_width = width * scale;
                    instance.border_color = [color.r, color.g, color.b, color.a];
                }

                // Shadow
                if let Some(s) = shadow {
                    instance.shadow_offset = [s.offset.0 * scale, s.offset.1 * scale];
                    instance.shadow_blur = s.blur * scale;
                    instance.shadow_spread = s.spread * scale;
                    instance.shadow_color = [s.color.r, s.color.g, s.color.b, s.color.a];
                }

                // Clip - transform from world space to shape's local space
                if let Some(clip) = &cmd.clip {
                    // Transform clip corners to local space using inverse transform
                    let inv = cmd.world_transform.inverse();
                    let (x1, y1) = inv.transform_point(clip.rect.x, clip.rect.y);
                    let (x2, y2) = inv.transform_point(
                        clip.rect.x + clip.rect.width,
                        clip.rect.y + clip.rect.height,
                    );

                    // Reconstruct axis-aligned bounds (min/max handles rotation)
                    let local_x = x1.min(x2);
                    let local_y = y1.min(y2);
                    let local_w = (x2 - x1).abs();
                    let local_h = (y2 - y1).abs();

                    instance.clip_rect = [
                        local_x * scale,
                        local_y * scale,
                        local_w * scale,
                        local_h * scale,
                    ];

                    // Scale radius by inverse transform's scale factor
                    let (inv_scale_x, inv_scale_y) = inv.extract_scale_components();
                    let radius_scale = inv_scale_x.min(inv_scale_y);
                    instance.clip_radius = clip.radius * radius_scale * scale;
                    instance.clip_curvature = clip.curvature;
                }

                // Transform
                if !cmd.world_transform.is_identity() {
                    let t = &cmd.world_transform;
                    // Extract 2x3 affine matrix components [a, b, tx, c, d, ty]
                    // Note: The matrix already includes center_at from CPU, so no origin
                    // handling needed in the shader.
                    instance.transform = [
                        t.data[0],         // a
                        t.data[1],         // b
                        t.data[3] * scale, // tx (scaled)
                        t.data[4],         // c
                        t.data[5],         // d
                        t.data[7] * scale, // ty (scaled)
                    ];
                }

                Some(instance)
            }
            DrawCommand::Circle {
                center,
                radius,
                color,
            } => {
                // Convert circle to a rounded rect with radius = half size
                let scale = self.scale_factor;
                let rect_x = (center.0 - radius) * scale;
                let rect_y = (center.1 - radius) * scale;
                let size = radius * 2.0 * scale;

                let mut instance = ShapeInstance {
                    rect: [rect_x, rect_y, size, size],
                    corner_radius: radius * scale, // Full radius = circle
                    shape_curvature: 1.0,          // Circular corners
                    _pad0: [0.0, 0.0],
                    fill_color: [color.r, color.g, color.b, color.a],
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
                    transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                    _pad3: [0.0, 0.0],
                };

                // Clip - transform from world space to shape's local space
                if let Some(clip) = &cmd.clip {
                    // Transform clip corners to local space using inverse transform
                    let inv = cmd.world_transform.inverse();
                    let (x1, y1) = inv.transform_point(clip.rect.x, clip.rect.y);
                    let (x2, y2) = inv.transform_point(
                        clip.rect.x + clip.rect.width,
                        clip.rect.y + clip.rect.height,
                    );

                    // Reconstruct axis-aligned bounds (min/max handles rotation)
                    let local_x = x1.min(x2);
                    let local_y = y1.min(y2);
                    let local_w = (x2 - x1).abs();
                    let local_h = (y2 - y1).abs();

                    instance.clip_rect = [
                        local_x * scale,
                        local_y * scale,
                        local_w * scale,
                        local_h * scale,
                    ];

                    // Scale radius by inverse transform's scale factor
                    let (inv_scale_x, inv_scale_y) = inv.extract_scale_components();
                    let radius_scale = inv_scale_x.min(inv_scale_y);
                    instance.clip_radius = clip.radius * radius_scale * scale;
                    instance.clip_curvature = clip.curvature;
                }

                // Transform (origin already baked into matrix via center_at)
                if !cmd.world_transform.is_identity() {
                    let t = &cmd.world_transform;
                    instance.transform = [
                        t.data[0],
                        t.data[1],
                        t.data[3] * scale,
                        t.data[4],
                        t.data[5],
                        t.data[7] * scale,
                    ];
                }

                Some(instance)
            }
            // Text commands are handled separately via command_to_text_entry
            DrawCommand::Text { .. } => None,
        }
    }

    /// Convert a text command to a TextEntry for the V1 text renderer.
    fn command_to_text_entry(&self, cmd: &FlattenedCommand) -> Option<TextEntry> {
        match &cmd.command {
            DrawCommand::Text {
                text,
                rect,
                color,
                font_size,
                font_family,
                font_weight,
            } => {
                // Convert clip region to a simple Rect for the text renderer
                let clip_rect = cmd.clip.as_ref().map(|c| c.rect);

                Some(TextEntry {
                    text: text.clone(),
                    rect: *rect,
                    color: *color,
                    font_size: *font_size,
                    font_family: font_family.clone(),
                    font_weight: *font_weight,
                    clip_rect,
                    transform: cmd.world_transform,
                    transform_origin: cmd.world_transform_origin,
                })
            }
            _ => None,
        }
    }
}
