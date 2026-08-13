//! GPU rendering for the render tree using instanced rendering.
//!
//! This module uses a single draw call per layer to render all shapes,
//! significantly reducing CPU-GPU communication overhead.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline, ShaderModule,
};

use super::backdrop_pass::{BackdropRegion, BackdropRenderer};
use super::commands::DrawCommand;
use super::flatten::{CommandLayer, FlattenedCommand};
use super::gpu::{QUAD_INDICES, QUAD_VERTICES, QuadVertex, ShaderUniforms, ShapeInstance};
use super::gpu_context::SurfaceState;
use super::image_quad::{ImageQuadRenderer, PreparedImageQuad};
use super::text::TextRenderState;
use super::text_quad::{PreparedTextQuad, TextQuadRenderer};
use super::types::TextEntry;
use crate::widgets::{Color, Rect};

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

    // Reusable per-frame buffers (cleared and reused each frame to avoid allocations)
    /// Every group's shape and overlay instances, addressed by range.
    shape_instance_buf: Vec<ShapeInstance>,
    text_entry_buf: Vec<TextEntry>,
    image_quads: Vec<PreparedImageQuad>,
    text_quads: Vec<PreparedTextQuad>,
    backdrop: BackdropRenderer,

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

        let backdrop = BackdropRenderer::new(&device, format);

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
            shape_instance_buf: Vec::new(),
            text_entry_buf: Vec::new(),
            image_quads: Vec::new(),
            text_quads: Vec::new(),
            backdrop,
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
    ///
    /// Returns `true` if a frame was actually presented. On `false` (lost/
    /// outdated swapchain, out of memory) nothing reached the screen — the
    /// caller must keep its dirty state so the content is repainted on the
    /// next frame instead of showing stale pixels.
    pub fn render(
        &mut self,
        surface: &mut SurfaceState,
        commands: &[FlattenedCommand],
        layers: &[CommandLayer],
        clear_color: Color,
    ) -> bool {
        let output = match surface.surface.get_current_texture() {
            Ok(output) => output,
            Err(wgpu::SurfaceError::Lost) => {
                surface.resize(surface.width(), surface.height());
                return false;
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Out of GPU memory");
                return false;
            }
            Err(e) => {
                log::error!("Surface error: {:?}", e);
                return false;
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

        let prepared = self.prepare_layers(commands, layers);

        // A backdrop effect reads pixels already drawn, which a pass cannot do
        // to its own attachment: the frame goes to an offscreen target and is
        // blitted over at the end. Frames without one draw straight to the
        // swapchain and never allocate it.
        let uses_backdrop = layers.iter().any(|layer| !layer.backdrop.is_empty());
        if uses_backdrop {
            self.backdrop.ensure_targets(
                &self.device,
                surface.width().max(1),
                surface.height().max(1),
            );
        } else {
            self.backdrop.note_unused();
        }

        // Instances for every group live in one buffer; the groups address it
        // by range, which is what keeps their draws in order.
        self.ensure_instance_capacity(self.shape_instance_buf.len());
        if !self.shape_instance_buf.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.shape_instance_buf),
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Renderer Encoder"),
            });

        let scene_view = uses_backdrop.then(|| self.backdrop.scene_view()).flatten();
        let target = scene_view.unwrap_or(&view);

        let scale = self.scale_factor;
        {
            let clear = wgpu::LoadOp::Clear(wgpu::Color {
                r: clear_color.r as f64,
                g: clear_color.g as f64,
                b: clear_color.b as f64,
                a: clear_color.a as f64,
            });
            let mut load = clear;
            let mut render_pass = begin_pass(&mut encoder, target, load);
            load = wgpu::LoadOp::Load;

            // Groups are drawn in order; within a group, bucket order. The
            // shape pipeline is re-bound per draw because the image and text
            // renderers replace it.
            for (index, layer) in prepared.iter().enumerate() {
                if !layers[index].backdrop.is_empty() {
                    // The effect samples the target, so the pass has to end
                    // and its contents be stored before it can run.
                    drop(render_pass);
                    for command in &commands[layers[index].backdrop.clone()] {
                        if let Some(region) = command_to_backdrop_region(command, scale) {
                            self.backdrop.apply(&self.device, &mut encoder, &region);
                        }
                    }
                    render_pass = begin_pass(&mut encoder, target, load);
                }

                if !layer.shapes.is_empty() {
                    self.bind_shape_pipeline(&mut render_pass);
                    render_pass.draw_indexed(0..6, 0, layer.shapes.clone());
                }

                if !layer.images.is_empty() {
                    self.image_quad_renderer
                        .render(&mut render_pass, &self.image_quads[layer.images.clone()]);
                }

                if let Some(slot) = layer.text_slot {
                    self.text_state.render_slot(slot, &mut render_pass);
                }

                if !layer.text_quads.is_empty() {
                    self.text_quad_renderer
                        .render(&mut render_pass, &self.text_quads[layer.text_quads.clone()]);
                }

                if !layer.overlay.is_empty() {
                    self.bind_shape_pipeline(&mut render_pass);
                    render_pass.draw_indexed(0..6, 0, layer.overlay.clone());
                }
            }
        }

        if uses_backdrop {
            self.backdrop.present(&self.device, &mut encoder, &view);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        true
    }

    fn bind_shape_pipeline(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    }

    /// Resolve every group's GPU work before the pass opens.
    ///
    /// Uploads cannot happen inside a render pass, so all shaping, atlas
    /// packing and buffer writes are done here and the pass only issues draws.
    fn prepare_layers(
        &mut self,
        commands: &[FlattenedCommand],
        layers: &[CommandLayer],
    ) -> Vec<PreparedLayer> {
        let scale = self.scale_factor;

        self.shape_instance_buf.clear();
        self.image_quads.clear();
        self.text_quads.clear();
        self.image_quad_renderer.begin_frame();
        self.text_state.begin_frame();

        let mut prepared = Vec::with_capacity(layers.len());
        // Each group with directly-rendered text needs its own glyphon
        // renderer, so slots are handed out only to groups that have some.
        let mut next_text_slot = 0;

        for layer in layers {
            let shapes_start = self.shape_instance_buf.len() as u32;
            self.shape_instance_buf.extend(
                commands[layer.shapes.clone()]
                    .iter()
                    .filter_map(|c| command_to_instance(c, scale)),
            );
            let shapes = shapes_start..self.shape_instance_buf.len() as u32;

            let images_start = self.image_quads.len();
            if !layer.images.is_empty() {
                self.image_quad_renderer
                    .set_screen_size(self.screen_width, self.screen_height);
                let quads = self.image_quad_renderer.prepare(
                    &self.device,
                    &self.queue,
                    &commands[layer.images.clone()],
                    scale,
                );
                self.image_quads.extend(quads);
            }
            let images = images_start..self.image_quads.len();

            self.text_entry_buf.clear();
            self.text_entry_buf.extend(
                commands[layer.text.clone()]
                    .iter()
                    .filter_map(command_to_text_entry),
            );
            let text_quads_start = self.text_quads.len();
            let mut text_slot = None;
            if !self.text_entry_buf.is_empty() {
                let slot = next_text_slot;
                next_text_slot += 1;
                let transformed = self.text_state.prepare_layer(
                    slot,
                    &self.device,
                    &self.queue,
                    &self.text_entry_buf,
                    (self.screen_width as u32, self.screen_height as u32),
                    scale,
                );
                // Rotated and scaled text goes through the textured-quad path
                // instead, to keep the glyphon atlas stable.
                if transformed.len() < self.text_entry_buf.len() {
                    text_slot = Some(slot);
                }
                if !transformed.is_empty() {
                    self.text_quad_renderer
                        .set_screen_size(self.screen_width, self.screen_height);
                    let quads = self.text_quad_renderer.prepare(
                        &self.device,
                        &self.queue,
                        &self.text_entry_buf,
                        &transformed,
                        scale,
                    );
                    self.text_quads.extend(quads);
                }
            }
            let text_quads = text_quads_start..self.text_quads.len();

            let overlay_start = self.shape_instance_buf.len() as u32;
            self.shape_instance_buf.extend(
                commands[layer.overlay.clone()]
                    .iter()
                    .filter_map(|c| command_to_instance(c, scale)),
            );
            let overlay = overlay_start..self.shape_instance_buf.len() as u32;

            prepared.push(PreparedLayer {
                shapes,
                images,
                text_slot,
                text_quads,
                overlay,
            });
        }

        self.text_state.end_frame();
        prepared
    }
}

/// One draw group's GPU work, addressed by range into the renderer's
/// per-frame buffers.
struct PreparedLayer {
    shapes: std::ops::Range<u32>,
    images: std::ops::Range<usize>,
    /// Glyphon renderer holding this group's directly-rendered text.
    text_slot: Option<usize>,
    text_quads: std::ops::Range<usize>,
    overlay: std::ops::Range<u32>,
}

/// Open a colour-only render pass over `target`.
fn begin_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &'a wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Renderer Render Pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// Resolve a backdrop command to the physical-pixel region it filters.
fn command_to_backdrop_region(cmd: &FlattenedCommand, scale: f32) -> Option<BackdropRegion> {
    let DrawCommand::BackdropBlur {
        rect,
        radius,
        corner_radii,
        curvature,
    } = &*cmd.command
    else {
        return None;
    };

    // The effect works on axis-aligned pixels, so a rotated container gets the
    // bounding box of its rotated rect — the mask still cuts the right shape
    // out of it for the common translation-only case.
    let corners = [
        cmd.world_transform.transform_point(rect.x, rect.y),
        cmd.world_transform
            .transform_point(rect.x + rect.width, rect.y),
        cmd.world_transform
            .transform_point(rect.x, rect.y + rect.height),
        cmd.world_transform
            .transform_point(rect.x + rect.width, rect.y + rect.height),
    ];
    let min_x = corners.iter().map(|c| c.0).fold(f32::INFINITY, f32::min);
    let max_x = corners
        .iter()
        .map(|c| c.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_y = corners.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let max_y = corners
        .iter()
        .map(|c| c.1)
        .fold(f32::NEG_INFINITY, f32::max);

    Some(BackdropRegion {
        rect: Rect::new(
            min_x * scale,
            min_y * scale,
            (max_x - min_x) * scale,
            (max_y - min_y) * scale,
        ),
        radius: radius * scale,
        radii: corner_radii.scaled(scale),
        curvature: *curvature,
    })
}

/// Convert a single flattened command to a shape instance.
fn command_to_instance(cmd: &FlattenedCommand, scale: f32) -> Option<ShapeInstance> {
    match &*cmd.command {
        DrawCommand::RoundedRect {
            rect,
            color,
            radius,
            curvature,
            border,
            shadow,
            gradient,
        } => {
            let mut instance = ShapeInstance::from_rect(
                [
                    rect.x * scale,
                    rect.y * scale,
                    rect.width * scale,
                    rect.height * scale,
                ],
                [color.r, color.g, color.b, color.a],
                radius.scaled(scale).to_array(),
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
            let rect_x = (center.0 - radius) * scale;
            let rect_y = (center.1 - radius) * scale;
            let size = radius * 2.0 * scale;

            let mut instance = ShapeInstance::from_rect(
                [rect_x, rect_y, size, size],
                [color.r, color.g, color.b, color.a],
                [radius * scale; 4], // Full radius = circle
                1.0,                 // Circular corners
            )
            .with_transform(&cmd.world_transform, scale);

            if let Some(ref clip) = cmd.clip {
                instance = instance.with_clip(clip, scale, cmd.clip_is_local);
            }

            Some(instance)
        }
        // Text commands are handled separately via command_to_text_entry
        DrawCommand::Text { .. } => None,
        // Filters the target rather than adding geometry; handled between
        // draw groups, not as an instance.
        DrawCommand::BackdropBlur { .. } => None,
        // Image commands are handled separately via ImageQuadRenderer
        DrawCommand::Image { .. } => None,
    }
}

/// Convert a text command to a TextEntry for text rendering.
fn command_to_text_entry(cmd: &FlattenedCommand) -> Option<TextEntry> {
    match &*cmd.command {
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
