//! GPU rendering for the render tree using instanced rendering.
//!
//! This module uses a single draw call per layer to render all shapes,
//! significantly reducing CPU-GPU communication overhead.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline, ShaderModule,
};

use super::commands::DrawCommand;
use super::flatten::{FlattenedCommand, RenderLayer};
use super::gpu::{QUAD_INDICES, QUAD_VERTICES, QuadVertex, ShaderUniforms, ShapeInstance};
use super::gpu_context::SurfaceState;
use super::image_quad::{ImageQuadRenderer, PreparedImageQuad};
use super::text::TextRenderState;
use super::text_quad::{PreparedTextQuad, TextQuadRenderer};
use super::types::TextEntry;
use crate::widgets::Color;

/// The renderer using instanced rendering.
///
/// This renderer converts [`FlattenedCommand`]s into GPU instance data
/// and renders all shapes with a single draw call per layer.
pub struct Renderer {
    device: Arc<Device>,
    queue: Arc<Queue>,
    pipeline: RenderPipeline,
    #[allow(dead_code)] // Kept alive - bind groups hold reference to layout
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

    // Text rendering via glyphon
    text_state: TextRenderState,

    // Transformed text rendering (renders text to textures for rotation/scale)
    text_quad_renderer: TextQuadRenderer,

    // Image rendering
    image_quad_renderer: ImageQuadRenderer,

    // Screen dimensions
    screen_width: f32,
    screen_height: f32,
    scale_factor: f32,
}

impl Renderer {
    /// Create a new renderer with instanced rendering.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, format: wgpu::TextureFormat) -> Self {
        // Load shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Renderer Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        // Create bind group layout for uniforms
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Renderer Bind Group Layout"),
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
            label: Some("Renderer Vertex Buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: BufferUsages::VERTEX,
        });

        // Create index buffer
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Renderer Index Buffer"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: BufferUsages::INDEX,
        });

        // Create uniform buffer
        let uniforms = ShaderUniforms::new(800.0, 600.0, 1.0);
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Renderer Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        // Create uniform bind group
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Renderer Uniform Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // Create initial instance buffer (will be resized as needed)
        let initial_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Renderer Instance Buffer"),
            size: (initial_capacity * std::mem::size_of::<ShapeInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Initialize text renderer
        let text_state = TextRenderState::new(&device, &queue, format);

        // Initialize transformed text renderer
        let text_quad_renderer = TextQuadRenderer::new(&device, &queue, format);

        // Initialize image renderer
        let image_quad_renderer = ImageQuadRenderer::new(&device, format);

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
            text_quad_renderer,
            image_quad_renderer,
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
            label: Some("Renderer Pipeline Layout"),
            bind_group_layouts: &[bind_group_layout],
            immediate_size: 0,
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Renderer Pipeline"),
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
                label: Some("Renderer Instance Buffer"),
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

        // Update uniform buffer with current screen size (in logical pixels)
        let uniforms =
            ShaderUniforms::new(self.screen_width, self.screen_height, self.scale_factor);
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        // Separate commands by layer
        let shape_commands: Vec<_> = commands
            .iter()
            .filter(|c| c.layer == RenderLayer::Shapes)
            .collect();
        let image_commands: Vec<_> = commands
            .iter()
            .filter(|c| c.layer == RenderLayer::Images)
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

        // Convert text commands to TextEntry for text rendering
        let text_entries: Vec<TextEntry> = text_commands
            .iter()
            .filter_map(|cmd| self.command_to_text_entry(cmd))
            .collect();

        // Prepare regular text and get indices of texts that need texture-based rendering
        let transformed_indices = if !text_entries.is_empty() {
            self.text_state.prepare_text(
                &self.device,
                &self.queue,
                &text_entries,
                self.screen_width as u32,
                self.screen_height as u32,
                self.scale_factor,
            )
        } else {
            Vec::new()
        };

        // Prepare transformed text as textured quads
        let text_quads: Vec<PreparedTextQuad> = if !transformed_indices.is_empty() {
            log::debug!(
                "Renderer: {} transformed texts to render as quads",
                transformed_indices.len()
            );
            // Update text quad renderer screen size
            self.text_quad_renderer
                .set_screen_size(self.screen_width, self.screen_height);
            self.text_quad_renderer.prepare(
                &self.device,
                &self.queue,
                &text_entries,
                &transformed_indices,
                self.scale_factor,
            )
        } else {
            Vec::new()
        };

        // Prepare image quads
        self.image_quad_renderer.begin_frame();
        let image_quads: Vec<PreparedImageQuad> = if !image_commands.is_empty() {
            self.image_quad_renderer
                .set_screen_size(self.screen_width, self.screen_height);
            self.image_quad_renderer.prepare(
                &self.device,
                &self.queue,
                &image_commands,
                self.scale_factor,
            )
        } else {
            Vec::new()
        };

        // Ensure we have enough capacity
        let total_instances = shape_instances.len() + overlay_instances.len();
        self.ensure_instance_capacity(total_instances);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Renderer Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Renderer Render Pass"),
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

            // Draw images (after shapes, before text)
            if !image_quads.is_empty() {
                self.image_quad_renderer
                    .render(&mut render_pass, &image_quads);
            }

            // Draw text layer (between images and overlay)
            // Only render non-transformed text via glyphon
            let has_non_transformed_text =
                !text_entries.is_empty() && transformed_indices.len() < text_entries.len();
            if has_non_transformed_text {
                self.text_state.render(&mut render_pass, &self.device);
            }

            // Draw transformed text as textured quads
            if !text_quads.is_empty() {
                log::debug!("Renderer: Rendering {} text quads", text_quads.len());
                self.text_quad_renderer
                    .render(&mut render_pass, &text_quads);
            }

            // Draw overlay shapes (after text, for effects like ripples)
            if !overlay_instances.is_empty() {
                // Re-set the shape pipeline (text/image renderers may have changed it)
                render_pass.set_pipeline(&self.pipeline);
                render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

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
                gradient,
            } => {
                let scale = self.scale_factor;

                let mut instance = ShapeInstance::from_rect(
                    [
                        rect.x * scale,
                        rect.y * scale,
                        rect.width * scale,
                        rect.height * scale,
                    ],
                    [color.r, color.g, color.b, color.a],
                    radius * scale,
                    *curvature,
                )
                .with_transform(&cmd.world_transform, scale);

                if let Some(b) = border {
                    instance = instance.with_border(b, scale);
                }
                if let Some(s) = shadow {
                    instance = instance.with_shadow(s, scale);
                }
                if let Some(g) = gradient {
                    instance = instance.with_gradient(g);
                }
                if let Some(ref clip) = cmd.clip {
                    instance = instance.with_clip(clip, scale, cmd.clip_is_local);
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

                let mut instance = ShapeInstance::from_rect(
                    [rect_x, rect_y, size, size],
                    [color.r, color.g, color.b, color.a],
                    radius * scale, // Full radius = circle
                    1.0,            // Circular corners
                )
                .with_transform(&cmd.world_transform, scale);

                if let Some(ref clip) = cmd.clip {
                    instance = instance.with_clip(clip, scale, cmd.clip_is_local);
                }

                Some(instance)
            }
            // Text commands are handled separately via command_to_text_entry
            DrawCommand::Text { .. } => None,
            // Image commands are handled separately via ImageQuadRenderer
            DrawCommand::Image { .. } => None,
        }
    }

    /// Convert a text command to a TextEntry for text rendering.
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
                // Convert WorldClip to Rect for text clipping
                let clip_rect = cmd.clip.as_ref().map(|clip| clip.rect);

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
