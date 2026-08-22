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
use super::commands::{CornerRadii, DrawCommand};
use super::flatten::{CommandLayer, FlattenedCommand};
use super::gpu::{QUAD_INDICES, QUAD_VERTICES, QuadVertex, ShaderUniforms, ShapeInstance};
use super::gpu_context::SurfaceState;
use super::image_quad::{ImageQuadRenderer, PreparedImageQuad};
use super::text::TextRenderState;
use super::text_mask::{MaskSpec, TextMaskRenderer};
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

    /// Glyph coverage for texts that frost their own backdrop.
    text_mask: TextMaskRenderer,

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
        let text_mask = TextMaskRenderer::new(format);

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
            text_mask,
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
                        } else if let Some(frost) = command_to_text_backdrop(command, scale) {
                            // The mask is rasterized and submitted on its own
                            // encoder, so it is ready by the time this frame's
                            // encoder reaches the composite below.
                            if let Some(mask) =
                                self.text_mask.mask(&self.device, &self.queue, &frost.spec)
                            {
                                self.backdrop.apply_masked(
                                    &self.device,
                                    &mut encoder,
                                    &frost.region,
                                    &mask,
                                );
                                // After the blur and before the glyphs: a
                                // contour on the glass, not under it.
                                if let Some((color, width)) = frost.outline {
                                    self.backdrop.apply_outline(
                                        &self.device,
                                        &mut encoder,
                                        &frost.region,
                                        &mask,
                                        color,
                                        width,
                                    );
                                }
                            }
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
        self.text_mask.begin_frame();
        self.text_state.begin_frame(
            &self.queue,
            (self.screen_width as u32, self.screen_height as u32),
        );

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

/// The clip a command was flattened under, in physical pixels.
///
/// The axis-aligned rect of it: a rounded or rotated clip is approximated by
/// its box, which is one pixel of slack at the corners against a blurred
/// rectangle where the content has been scrolled away.
fn clip_rect(cmd: &FlattenedCommand, scale: f32) -> Option<Rect> {
    let clip = cmd.clip.as_ref()?.rect;
    Some(Rect::new(
        clip.x * scale,
        clip.y * scale,
        clip.width * scale,
        clip.height * scale,
    ))
}

/// Resolve a backdrop command to the physical-pixel region it filters.
fn command_to_backdrop_region(cmd: &FlattenedCommand, scale: f32) -> Option<BackdropRegion> {
    let DrawCommand::BackdropBlur {
        rect,
        sources,
        radius,
        corner_radii,
        curvature,
    } = &*cmd.command
    else {
        return None;
    };
    // The compositor half of the same command is published as a `wl_region`
    // instead; this pass filters only what the surface has drawn itself.
    if !sources.contains(crate::backdrop::BackdropSources::SURFACE) {
        return None;
    }

    // The effect works on axis-aligned pixels, so a rotated container gets the
    // bounding box of its rotated rect — the mask still cuts the right shape
    // out of it for the common translation-only case. Shared with the region
    // published to the compositor, which is the same shape by definition.
    let (world, world_radii) = cmd.world_rounded_rect(*rect, *corner_radii);
    // The mask shader takes one radius per corner, so an unevenly scaled corner
    // is approximated here rather than on the way in — the region published to
    // the compositor keeps both axes, because a `wl_region` can hold the shape.
    let world_radii = world_radii.to_circular();

    Some(BackdropRegion {
        rect: Rect::new(
            world.x * scale,
            world.y * scale,
            world.width * scale,
            world.height * scale,
        ),
        radius: radius * scale,
        radii: world_radii.scaled(scale),
        curvature: *curvature,
        clip: clip_rect(cmd, scale),
    })
}

/// A frosted text resolved to the region it filters and the mask that cuts it.
struct TextBackdrop<'a> {
    region: BackdropRegion,
    spec: MaskSpec<'a>,
    /// The contour to draw around the coverage, in physical pixels.
    outline: Option<(Color, f32)>,
}

/// Resolve a text backdrop command to its region and the mask to shape it with.
///
/// The region is snapped to whole pixels so the mask is one texel per pixel of
/// it; whatever the snap moved is handed back to the mask as the glyph origin,
/// which is why a text half a pixel off the grid still frosts its own shape.
fn command_to_text_backdrop(cmd: &FlattenedCommand, scale: f32) -> Option<TextBackdrop<'_>> {
    let DrawCommand::TextBackdropBlur {
        text,
        stroke,
        rect,
        radius,
        font_size,
        font_family,
        font_weight,
    } = &*cmd.command
    else {
        return None;
    };

    // A rotated or scaled text is drawn from a texture, at an angle an
    // axis-aligned mask cannot follow. Skipping is the honest answer: a frost
    // that sits beside its letters is worse than none.
    if !cmd.world_transform.is_translation_only() {
        return None;
    }

    let (world_x, world_y) = cmd.world_transform.transform_point(rect.x, rect.y);
    // Glyphs overshoot their layout box — descenders, italics, marks — and the
    // slack here is the one the flattener already uses for a text's bounds. A
    // contour reaches further still, and what falls outside the region is not
    // drawn at all.
    let slack = font_size * 0.5 + stroke.map(|s| s.width).unwrap_or(0.0);
    let left = (world_x - slack) * scale;
    let top = (world_y - slack) * scale;
    let right = (world_x + rect.width + slack) * scale;
    let bottom = (world_y + rect.height + slack) * scale;

    let x = left.floor();
    let y = top.floor();
    let width = (right.ceil() - x).max(1.0);
    let height = (bottom.ceil() - y).max(1.0);

    Some(TextBackdrop {
        region: BackdropRegion {
            rect: Rect::new(x, y, width, height),
            radius: radius * scale,
            // The shape is entirely the mask's; these are what the rectangular
            // composite would have used.
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            clip: clip_rect(cmd, scale),
        },
        spec: MaskSpec {
            text,
            font_size: *font_size,
            font_family,
            font_weight: *font_weight,
            logical: (rect.width, rect.height),
            size: (width as u32, height as u32),
            offset: (world_x * scale - x, world_y * scale - y),
            scale_factor: scale,
        },
        outline: stroke.map(|s| (s.color, s.width * scale)),
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
        DrawCommand::BackdropBlur { .. } | DrawCommand::TextBackdropBlur { .. } => None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    use crate::renderer::flatten::RenderLayer;
    use crate::transform::Transform;
    use crate::widgets::FontFamily;

    fn frosted(rect: Rect, transform: Transform) -> FlattenedCommand {
        FlattenedCommand {
            command: Rc::new(DrawCommand::TextBackdropBlur {
                text: "09:41".to_owned(),
                stroke: None,
                rect,
                radius: 10.0,
                font_size: 20.0,
                font_family: FontFamily::default(),
                font_weight: Default::default(),
            }),
            world_transform: transform,
            world_transform_origin: None,
            layer: RenderLayer::Backdrop,
            clip: None,
            clip_is_local: false,
        }
    }

    /// The mask is one texel per pixel of the region, which only works if the
    /// region lands on the pixel grid. What the snap moves has to come back as
    /// the glyph origin, or the frost sits beside its own letters.
    #[test]
    fn the_region_is_snapped_and_the_remainder_goes_to_the_mask() {
        let cmd = frosted(Rect::new(10.3, 20.6, 100.0, 30.0), Transform::IDENTITY);
        let frost = command_to_text_backdrop(&cmd, 1.0).expect("a frost");

        let rect = frost.region.rect;
        assert_eq!(rect.x, rect.x.floor(), "x is a whole pixel, got {}", rect.x);
        assert_eq!(rect.y, rect.y.floor(), "y is a whole pixel, got {}", rect.y);
        assert_eq!(rect.width, rect.width.floor());
        assert_eq!(rect.height, rect.height.floor());
        assert_eq!(frost.spec.size, (rect.width as u32, rect.height as u32));

        // The glyph origin, measured from the snapped corner, still lands on
        // the text's own position.
        assert!((rect.x + frost.spec.offset.0 - 10.3).abs() < 1e-3);
        assert!((rect.y + frost.spec.offset.1 - 20.6).abs() < 1e-3);
    }

    /// Descenders and italics reach past the layout box, and so must the frost:
    /// the same slack the flattener gives a text's bounds.
    #[test]
    fn the_region_covers_more_than_the_layout_box() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        let frost = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        assert!(frost.region.rect.width >= 110.0, "{:?}", frost.region.rect);
        assert!(frost.region.rect.height >= 40.0, "{:?}", frost.region.rect);
    }

    #[test]
    fn the_scale_factor_reaches_the_region_and_the_radius() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        let one = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        let two = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
        assert_eq!(two.region.radius, one.region.radius * 2.0);
        assert!(two.region.rect.width >= one.region.rect.width * 2.0 - 1.0);
        assert_eq!(two.spec.scale_factor, 2.0);
    }

    /// A translated text is still axis-aligned, so the mask can follow it.
    #[test]
    fn a_translated_text_is_frosted_where_it_ends_up() {
        let cmd = frosted(
            Rect::new(10.0, 20.0, 100.0, 30.0),
            Transform::translate(40.0, 5.0),
        );
        let frost = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        assert!((frost.region.rect.x + frost.spec.offset.0 - 50.0).abs() < 1e-3);
        assert!((frost.region.rect.y + frost.spec.offset.1 - 25.0).abs() < 1e-3);
    }

    /// A frost inside a scroll view must not paint where the text has been
    /// scrolled away to: the clip is what the effect is allowed to write.
    #[test]
    fn the_clip_reaches_the_region() {
        let mut cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        cmd.clip = Some(crate::renderer::flatten::WorldClip {
            rect: Rect::new(0.0, 0.0, 200.0, 200.0),
            corner_radius: 0.0,
            curvature: 1.0,
        });
        let frost = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
        assert_eq!(
            frost.region.clip,
            Some(Rect::new(0.0, 0.0, 400.0, 400.0)),
            "in physical pixels, like the region it bounds"
        );
    }

    /// A rotated or scaled one is not: the mask is rasterized square, and a
    /// frost beside its letters is worse than no frost.
    #[test]
    fn a_rotated_text_is_left_alone() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::rotate(0.4));
        assert!(command_to_text_backdrop(&cmd, 1.0).is_none());
    }
}
