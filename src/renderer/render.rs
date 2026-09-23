//! GPU rendering for the render tree using instanced rendering.
//!
//! This module uses a single draw call per layer to render all shapes,
//! significantly reducing CPU-GPU communication overhead.

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, Device, Queue, RenderPipeline, ShaderModule,
};

use super::backdrop_pass::{BackdropRegion, BackdropRenderer, CoverageMask};
use super::commands::{CornerRadii, DrawCommand};
use super::constants::TEXT_SUPERSAMPLE;
use super::flatten::{CommandLayer, FlattenedCommand};
use super::gpu::{QUAD_INDICES, QUAD_VERTICES, QuadVertex, ShaderUniforms, ShapeInstance};
use super::gpu_context::RenderTarget;
use super::image_quad::{ImageQuadRenderer, PreparedImageQuad};
use super::text::TextRenderState;
use super::text_mask::{MaskSpec, TextMaskRenderer};
use super::text_quad::{PreparedTextQuad, TextQuadRenderer};
use super::types::TextEntry;
use crate::shape::PlacedShape;
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
    /// What each group's draws address, one entry per group, rebuilt per frame.
    prepared_layers: Vec<PreparedLayer>,
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // The instances, read by `@builtin(instance_index)` rather
                // than fetched as vertex attributes — which is what lifts the
                // sixteen-attribute ceiling the layout had reached (#398).
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
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

        // Create initial instance buffer (will be resized as needed)
        let initial_capacity = 256;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Renderer Instance Buffer"),
            size: (initial_capacity * std::mem::size_of::<ShapeInstance>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_bind_group = Self::make_bind_group(
            &device,
            &bind_group_layout,
            &uniform_buffer,
            &instance_buffer,
        );

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
            prepared_layers: Vec::new(),
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
            bind_group_layouts: &[Some(bind_group_layout)],
            immediate_size: 0,
        });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Renderer Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                buffers: &[QuadVertex::desc()],
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

    /// The uniforms, and the instances the vertex stage indexes into.
    ///
    /// Rebuilt whenever the instance buffer is, because a bind group holds the
    /// buffer it was made from — which a vertex buffer binding did not, since
    /// that one was named at draw time.
    fn make_bind_group(
        device: &wgpu::Device,
        layout: &BindGroupLayout,
        uniforms: &Buffer,
        instances: &Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Renderer Bind Group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
            ],
        })
    }

    /// Ensure instance buffer has enough capacity.
    fn ensure_instance_capacity(&mut self, count: usize) {
        if count > self.instance_buffer_capacity {
            // Double capacity or use count, whichever is larger
            let new_capacity = (self.instance_buffer_capacity * 2).max(count);
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Renderer Instance Buffer"),
                size: (new_capacity * std::mem::size_of::<ShapeInstance>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_buffer_capacity = new_capacity;
            self.uniform_bind_group = Self::make_bind_group(
                &self.device,
                &self.bind_group_layout,
                &self.uniform_buffer,
                &self.instance_buffer,
            );
        }
    }

    /// Drop every image texture, as eviction does.
    #[cfg(feature = "testing")]
    pub(crate) fn forget_image_textures(&mut self) {
        self.image_quad_renderer.forget_textures();
    }

    /// Draw into whatever this surface points at, and hand it over.
    ///
    /// `false` means nothing reached the target and the frame should be drawn
    /// again — a lost or outdated swapchain, which is common right after a
    /// resize. A texture cannot fail that way: it is already there.
    pub fn render(
        &mut self,
        target: &mut RenderTarget,
        commands: &[FlattenedCommand],
        layers: &[CommandLayer],
        clear_color: Color,
    ) -> bool {
        // Two spellings of one dispatch, and both are the only one clippy
        // accepts for the build they belong to: the offscreen target exists
        // only where something reads pixels back, so with it compiled out this
        // is a one-variant enum and a `match` on it is a `let`. Written out
        // rather than silenced, because the lint is right in both builds.
        #[cfg(any(test, feature = "testing"))]
        let surface = match target {
            RenderTarget::Offscreen(offscreen) => {
                let view = offscreen
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let width = offscreen.texture.width();
                let height = offscreen.texture.height();
                self.render_to_view(&view, width, height, commands, layers, clear_color);
                return true;
            }
            RenderTarget::Swapchain(surface) => surface,
        };
        #[cfg(not(any(test, feature = "testing")))]
        let RenderTarget::Swapchain(surface) = target;
        let output = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost => {
                surface.resize(surface.width(), surface.height());
                return false;
            }
            other => {
                log::error!("Surface error: {:?}", other);
                return false;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.render_to_view(
            &view,
            surface.width(),
            surface.height(),
            commands,
            layers,
            clear_color,
        );

        output.present();
        true
    }

    /// Render flattened commands into a texture view the caller owns.
    ///
    /// This is everything `render` does once it holds a swapchain texture, with
    /// the acquire and the present taken off both ends. A caller that has its
    /// own target — an offscreen texture in a test, with no compositor and no
    /// surface anywhere — gets the same pixels the screen would have shown.
    ///
    /// `width` and `height` are the target's size in physical pixels; the
    /// backdrop targets are allocated against them.
    pub fn render_to_view(
        &mut self,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        commands: &[FlattenedCommand],
        layers: &[CommandLayer],
        clear_color: Color,
    ) {
        // Update uniform buffer with current screen size (in logical pixels)
        let uniforms =
            ShaderUniforms::new(self.screen_width, self.screen_height, self.scale_factor);
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));

        self.prepare_layers(commands, layers);

        // A backdrop effect reads pixels already drawn, which a pass cannot do
        // to its own attachment: the frame goes to an offscreen target and is
        // blitted over at the end. Frames without one draw straight to the
        // swapchain and never allocate it.
        let uses_backdrop = layers.iter().any(|layer| !layer.backdrop.is_empty());
        if uses_backdrop {
            self.backdrop
                .ensure_targets(&self.device, width.max(1), height.max(1));
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
        let target = scene_view.unwrap_or(view);

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
            for (index, layer) in self.prepared_layers.iter().enumerate() {
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
                            if let Some(view) =
                                self.text_mask.mask(&self.device, &self.queue, &frost.spec)
                            {
                                let mask = CoverageMask {
                                    view: &view,
                                    size: frost.spec.size,
                                };
                                self.backdrop.apply_masked(
                                    &self.device,
                                    &mut encoder,
                                    &frost.region,
                                    mask,
                                );
                                // After the blur and before the glyphs: a
                                // contour on the glass, not under it.
                                if let Some((color, width)) = frost.outline {
                                    self.backdrop.apply_outline(
                                        &self.device,
                                        &mut encoder,
                                        &frost.region,
                                        mask,
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
            self.backdrop.present(&self.device, &mut encoder, view);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    fn bind_shape_pipeline(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.uniform_bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
    }

    /// Resolve every group's GPU work before the pass opens, into
    /// `prepared_layers`.
    ///
    /// Uploads cannot happen inside a render pass, so all shaping, atlas
    /// packing and buffer writes are done here and the pass only issues draws.
    fn prepare_layers(&mut self, commands: &[FlattenedCommand], layers: &[CommandLayer]) {
        let scale = self.scale_factor;

        self.shape_instance_buf.clear();
        self.image_quads.clear();
        self.text_quads.clear();
        self.prepared_layers.clear();
        self.image_quad_renderer.begin_frame();
        self.text_mask.begin_frame();
        self.text_state.begin_frame(
            &self.queue,
            (self.screen_width as u32, self.screen_height as u32),
        );

        // Each group with directly-rendered text needs its own glyphon
        // renderer, so slots are handed out only to groups that have some.
        let mut next_text_slot = 0;

        for layer in layers {
            let shapes_start = self.shape_instance_buf.len() as u32;
            self.shape_instance_buf.extend(
                commands[layer.shapes.clone()]
                    .iter()
                    .filter_map(|c| faded_instance(c, scale)),
            );
            let shapes = shapes_start..self.shape_instance_buf.len() as u32;

            let images_start = self.image_quads.len();
            if !layer.images.is_empty() {
                self.image_quad_renderer
                    .set_screen_size(self.screen_width, self.screen_height);
                self.image_quad_renderer.prepare(
                    &self.device,
                    &self.queue,
                    &commands[layer.images.clone()],
                    scale,
                    &mut self.image_quads,
                );
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
                    self.text_quad_renderer.prepare(
                        &self.device,
                        &self.queue,
                        &self.text_entry_buf,
                        transformed,
                        scale,
                        &mut self.text_quads,
                    );
                }
            }
            let text_quads = text_quads_start..self.text_quads.len();

            let overlay_start = self.shape_instance_buf.len() as u32;
            self.shape_instance_buf.extend(
                commands[layer.overlay.clone()]
                    .iter()
                    .filter_map(|c| faded_instance(c, scale)),
            );
            let overlay = overlay_start..self.shape_instance_buf.len() as u32;

            self.prepared_layers.push(PreparedLayer {
                shapes,
                images,
                text_slot,
                text_quads,
                overlay,
            });
        }

        self.image_quad_renderer.trim(crate::image_cache_budget());
        self.text_state.end_frame();
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

/// Where a placed shape lands for the backdrop pass.
///
/// Four values that all follow from the shape and the scale, so the pass is
/// given those and works the rest out: the viewport it may touch, the map back
/// into the shape's own space, and the radius in pixels. They used to be
/// fields, and the viewport was the world box of the very shape beside it —
/// one fact written twice, free to drift, with nothing that would have said so.
fn backdrop_region(
    shape: PlacedShape,
    radius: f32,
    cmd: &FlattenedCommand,
    scale: f32,
) -> BackdropRegion {
    BackdropRegion {
        shape,
        scale,
        radius,
        // The clip itself, not the box around it: the pass narrows its viewport
        // by the box and then cuts the difference back off per fragment, the
        // same two steps it already takes for the shape.
        clip: cmd.clip(),
        opacity: cmd.opacity,
    }
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

    // The corners are the container's own, so the mask is cut where they are
    // circles and its sides are its sides.
    Some(backdrop_region(
        PlacedShape::placed(*rect, *corner_radii, *curvature, cmd.world_transform),
        *radius,
        cmd,
        scale,
    ))
}

/// A frosted text resolved to the region it filters and the mask that cuts it.
struct TextBackdrop<'a> {
    region: BackdropRegion,
    spec: MaskSpec<'a>,
    /// The contour to draw around the coverage, in mask texels — the space it
    /// is dilated in, so it arrives on the target stretched with the letters.
    outline: Option<(Color, f32)>,
}

/// Resolve a text backdrop command to its region and the mask to shape it with.
///
/// The mask covers the text's layout box plus slack, in the text's **own**
/// space, and the region is the box around wherever the transform puts that
/// frame — the shape a container's blur carries, with a coverage texture where
/// its corners would be. The composite carries each fragment back into the
/// frame to read it, so a turned or stretched text keeps a frost that follows
/// its letters; and where the mask sits is all the transform decides, so an
/// animating one reuses a rasterization rather than asking for a new one every
/// frame.
fn command_to_text_backdrop(cmd: &FlattenedCommand, scale: f32) -> Option<TextBackdrop<'_>> {
    let DrawCommand::TextBackdropBlur {
        text,
        stroke,
        rect,
        radius,
        font_size,
        font_family,
        font_weight,
        fit,
        align,
    } = &*cmd.command
    else {
        return None;
    };

    // Glyphs overshoot their layout box — descenders, italics, marks — and the
    // slack here is the one the flattener already uses for a text's bounds. A
    // contour reaches further still, and what falls outside the frame is not
    // drawn at all.
    let slack = font_size * 0.5 + stroke.map(|s| s.width).unwrap_or(0.0);
    let box_of_ink = rect.outset(slack);

    // Texels per logical pixel. A text the transform only moves is read back at
    // the density it was made, so the surface scale is the whole of it; one the
    // transform stretches is read back across more pixels than it has texels,
    // and takes the supersample the glyph textures take for the same reason.
    //
    // A boolean and not the transform's own magnitude, so the mask survives an
    // animation: a scale sweeping 1.0 -> 1.6 crosses this once and rasterizes
    // twice, where a density that tracked the stretch would rasterize afresh
    // every frame.
    //
    // Both flat rules were tried and both are worse, in half the cells each.
    // Pixels of half-covered glyph edge over the six cells of
    // `frosted_text_follows_its_letters_at_scale_2x` — fewer is tighter:
    //
    //                at rest  scale 1.6  stretch  rotate  clipped  wrapped
    //     this rule      244       2052      989     747      692     1677
    //     flat 2x        295       2052      989     747      698     1677
    //     scale only     244       2264     1128     875      692     2102
    //
    // The numbers live here and nowhere else; what the reference and the skill
    // carry is the shape of the result and a pointer back.
    let density = if cmd.world_transform.is_translation_only() {
        scale
    } else {
        scale * TEXT_SUPERSAMPLE
    };
    let width = (box_of_ink.width * density).ceil().max(1.0);
    let height = (box_of_ink.height * density).ceil().max(1.0);

    // The frame is the texture's extent and not the ink's: a whole number of
    // texels, so the rect the shader reads the mask over and the rect the mask
    // was drawn into are the same rect. Sizing it from the ink instead leaves
    // the mask displayed short by `ceil(w·density)/(w·density)` — a per cent or
    // so, which is half a pixel of frost beside its letters at the frame's
    // edges, and the one failure this whole change is about.
    let frame = Rect::new(
        box_of_ink.x,
        box_of_ink.y,
        width / density,
        height / density,
    );

    // Cornerless: the masked composite reads coverage over this frame and never
    // asks the rounded-rect SDF, so the shape here is entirely the mask's.
    let region = backdrop_region(
        PlacedShape::placed(frame, CornerRadii::uniform(0.0), 1.0, cmd.world_transform),
        *radius,
        cmd,
        scale,
    );

    Some(TextBackdrop {
        region,
        spec: MaskSpec {
            text,
            font_size: *font_size,
            font_family: *font_family,
            font_weight: *font_weight,
            fit: *fit,
            align: *align,
            // Shaped by whichever path will draw the glyphs over the frost:
            // the two break their lines in different places, and the frost has
            // to break its own where the letters do.
            buffer: if cmd.world_transform.is_translation_only() {
                super::text::shaping_buffer(*rect, density, *align)
            } else {
                super::text_quad::shaping_buffer(*rect, density, *align)
            },
            size: (width as u32, height as u32),
            offset: (slack * density, slack * density),
            density,
        },
        outline: stroke.map(|s| (s.color, s.width * density)),
    })
}

/// A command's shape instance, at the opacity it was flattened under — faded
/// here, once, so no arm below can forget to.
fn faded_instance(cmd: &FlattenedCommand, scale: f32) -> Option<ShapeInstance> {
    command_to_instance(cmd, scale).map(|instance| instance.faded(cmd.opacity))
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
            if let Some(clip) = cmd.clip() {
                instance = instance.with_clip(&clip, scale);
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

            if let Some(clip) = cmd.clip() {
                instance = instance.with_clip(&clip, scale);
            }

            Some(instance)
        }
        // Text commands are handled separately via command_to_text_entry
        DrawCommand::Text { .. } => None,
        // Filters the target rather than adding geometry; handled between
        // draw groups, not as an instance.
        DrawCommand::BackdropBlur { .. } | DrawCommand::TextBackdropBlur { .. } => None,
        // Says where input goes, not what is drawn.
        DrawCommand::InputRegion { .. } => None,
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
            fit,
            align,
        } => Some(TextEntry {
            text: text.clone(),
            rect: *rect,
            color: *color,
            font_size: *font_size,
            font_family: *font_family,
            font_weight: *font_weight,
            fit: *fit,
            align: *align,
            opacity: cmd.opacity,
            clip: cmd.clip(),
            transform: cmd.world_transform,
        }),
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
                fit: None,
                align: Default::default(),
            }),
            world_transform: transform,
            layer: RenderLayer::Backdrop,
            opacity: 1.0,
            clip: None,
        }
    }

    /// The mask is read over `shape`, with the glyph origin `offset` texels
    /// into it. Those two have to meet at the text's own position, or the
    /// frost sits beside its letters — and they meet there in the text's own
    /// space, which is why no transform can pull them apart.
    #[test]
    fn the_glyph_origin_in_the_mask_is_where_the_text_is() {
        for transform in [
            Transform::IDENTITY,
            Transform::translate(40.0, 5.0),
            Transform::scale(2.5),
            Transform::scale_xy(0.5, 3.0),
            Transform::rotate(0.4),
        ] {
            let cmd = frosted(Rect::new(10.3, 20.6, 100.0, 30.0), transform);
            let frost = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
            let frame = frost.region.shape.rect;
            let origin = (
                frame.x + frost.spec.offset.0 / frost.spec.density,
                frame.y + frost.spec.offset.1 / frost.spec.density,
            );
            assert!(
                (origin.0 - 10.3).abs() < 1e-3 && (origin.1 - 20.6).abs() < 1e-3,
                "{origin:?} is not the text's own origin under {transform:?}"
            );
            // Exactly, not to the nearest texel: the shader reads the mask
            // over `frame` and divides by its width, so a frame wider than the
            // texture it stands for displays the coverage short.
            assert_eq!(
                (
                    frame.width * frost.spec.density,
                    frame.height * frost.spec.density
                ),
                (frost.spec.size.0 as f32, frost.spec.size.1 as f32),
                "the frame and the texture are the same extent under {transform:?}"
            );
        }
    }

    /// Descenders and italics reach past the layout box, and so must the frost:
    /// the same slack the flattener gives a text's bounds.
    #[test]
    fn the_region_covers_more_than_the_layout_box() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        let frost = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        assert!(
            frost.region.viewport().width >= 110.0,
            "{:?}",
            frost.region.viewport()
        );
        assert!(
            frost.region.viewport().height >= 40.0,
            "{:?}",
            frost.region.viewport()
        );
    }

    /// The viewport is the box around the shape it travels with, at every
    /// placement — which is a fact about one value now rather than an agreement
    /// between two.
    ///
    /// It used to be a field the caller filled in with the world box of the
    /// very shape beside it. Nothing checked they matched, and a turned shape
    /// is exactly where they would have stopped matching quietly.
    #[test]
    fn the_viewport_is_the_box_around_the_shape() {
        for transform in [
            Transform::IDENTITY,
            Transform::translate(40.0, 5.0),
            Transform::scale(2.5),
            Transform::rotate_degrees(30.0),
        ] {
            let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), transform);
            for scale in [1.0_f32, 2.0] {
                let frost = command_to_text_backdrop(&cmd, scale).expect("a frost");
                let world = frost.region.shape.world_aabb();
                let viewport = frost.region.viewport();
                assert!(
                    (viewport.x - world.x * scale).abs() < 1e-3
                        && (viewport.y - world.y * scale).abs() < 1e-3
                        && (viewport.width - world.width * scale).abs() < 1e-3
                        && (viewport.height - world.height * scale).abs() < 1e-3,
                    "{transform:?} at scale {scale}: {viewport:?} is not \
                     {world:?} in physical pixels"
                );
            }
        }
    }

    /// The surface scale reaches the viewport and the mask's density. It does
    /// *not* reach the radius or the shape: those stay logical, and the scale
    /// travels beside them so the pass can apply it once, where it also places
    /// the shape. Scaling them here as well would apply it twice.
    #[test]
    fn the_scale_factor_reaches_the_viewport_and_the_density_and_nothing_twice() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        let one = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        let two = command_to_text_backdrop(&cmd, 2.0).expect("a frost");

        assert_eq!(two.region.scale, 2.0);
        assert_eq!(
            two.region.radius, one.region.radius,
            "the radius is logical, and the scale is carried rather than baked in"
        );
        assert_eq!(
            two.region.shape.rect, one.region.shape.rect,
            "and so is the shape"
        );
        assert!(two.region.viewport().width >= one.region.viewport().width * 2.0 - 1.0);
        assert_eq!(two.spec.density, one.spec.density * 2.0);
    }

    /// A translation moves the viewport and leaves the mask alone, which is
    /// what lets a dragged surface reuse the one it already has.
    #[test]
    fn a_translated_text_is_frosted_where_it_ends_up() {
        let rect = Rect::new(10.0, 20.0, 100.0, 30.0);
        let at_rest = frosted(rect, Transform::IDENTITY);
        let at_rest = command_to_text_backdrop(&at_rest, 1.0).expect("a frost");
        let moved = frosted(rect, Transform::translate(40.0, 5.0));
        let moved = command_to_text_backdrop(&moved, 1.0).expect("a frost");

        assert_eq!(moved.spec.size, at_rest.spec.size);
        assert_eq!(moved.spec.offset, at_rest.spec.offset);
        assert!((moved.region.viewport().x - at_rest.region.viewport().x - 40.0).abs() < 1e-3);
        assert!((moved.region.viewport().y - at_rest.region.viewport().y - 5.0).abs() < 1e-3);
    }

    /// A frost inside a scroll view must not paint where the text has been
    /// scrolled away to: the clip is what the effect is allowed to write.
    ///
    /// It arrives as the shape it is, corners and placement and all, rather
    /// than as the box around it — the pass narrows its viewport by the box and
    /// cuts the rest per fragment.
    #[test]
    fn the_clip_reaches_the_region_as_a_shape() {
        let clip = crate::shape::PlacedShape {
            rect: Rect::new(0.0, 0.0, 200.0, 200.0),
            radii: CornerRadii::uniform(16.0),
            curvature: 1.0,
            placement: Transform::rotate_degrees(20.0),
        };
        let mut cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        cmd.clip = Some(crate::renderer::clip::ClipRef::placed(clip));

        let frost = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
        let arrived = frost.region.clip.expect("a clip");
        assert_eq!(arrived.rect, clip.rect, "logical, like the shape beside it");
        assert_eq!(
            arrived.radii.top_left, 16.0,
            "with its corners, which a box would have squared off"
        );
        assert_eq!(
            arrived.placement, clip.placement,
            "and its placement, which a box would have flattened"
        );
    }

    /// A turned text is frosted too, and along its letters: the viewport is
    /// the box around the turned frame, and the frame carried back through
    /// `to_shape` is where the mask is read — so the coverage turns with the
    /// glyphs instead of being skipped.
    #[test]
    fn a_turned_text_is_frosted_along_its_letters() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::rotate(0.4));
        let frost = command_to_text_backdrop(&cmd, 1.0).expect("a frost");

        let frame = frost.region.shape.rect;
        let turned = cmd.world_transform.map_rect(frame);
        assert!(
            (frost.region.viewport().x - turned.x).abs() < 1e-3
                && (frost.region.viewport().width - turned.width).abs() < 1e-3,
            "the viewport is the box around the turned frame: {:?} vs {turned:?}",
            frost.region.viewport()
        );
        assert!(
            turned.width > frame.width,
            "a turn widens the box it needs, or this proves nothing"
        );

        // The frame's own corner, carried from the target back into the text's
        // space, lands on the frame — which is what the composite does to find
        // its mask texel.
        let to_shape = frost
            .region
            .shape
            .to_local(frost.region.scale)
            .expect("invertible");
        let (x, y) = cmd.world_transform.transform_point(frame.x, frame.y);
        let (back_x, back_y) = to_shape.transform_point(x, y);
        assert!((back_x - frame.x).abs() < 1e-2 && (back_y - frame.y).abs() < 1e-2);
    }

    /// The mask is rasterized in the text's own space, as the glyphs of a
    /// transformed text already are, and how much the transform stretches it is
    /// no part of that. So every frame of a scale animation asks for the same
    /// mask while its viewport grows — one rasterization over the length of it,
    /// rather than one per size it passes through.
    #[test]
    fn a_scale_grows_the_viewport_and_leaves_the_mask_alone() {
        let rect = Rect::new(10.0, 20.0, 100.0, 30.0);
        let small = frosted(rect, Transform::scale(1.6));
        let small = command_to_text_backdrop(&small, 1.0).expect("a frost");
        let large = frosted(rect, Transform::scale(3.0));
        let large = command_to_text_backdrop(&large, 1.0).expect("a frost");

        assert_eq!(large.spec.size, small.spec.size);
        assert_eq!(large.spec.density, small.spec.density);
        assert_eq!(large.region.shape.rect, small.region.shape.rect);
        assert!(
            (large.region.viewport().width - small.region.viewport().width / 1.6 * 3.0).abs()
                < 1e-2,
            "{:?} did not grow with the scale against {:?}",
            large.region.viewport(),
            small.region.viewport()
        );
    }

    /// A text the transform only moves is read back at the density it was made,
    /// and takes no supersample for it. One the transform stretches is read
    /// back across more pixels than it has texels, and does — measured at the
    /// glyph edge in `frosted_text_follows_its_letters_at_scale_2x`, it is the
    /// wrong way round in both directions otherwise.
    #[test]
    fn only_a_stretched_text_pays_for_the_supersample() {
        let rect = Rect::new(10.0, 20.0, 100.0, 30.0);
        let moved = frosted(rect, Transform::translate(40.0, 5.0));
        let moved = command_to_text_backdrop(&moved, 2.0).expect("a frost");
        let turned = frosted(rect, Transform::rotate(0.4));
        let turned = command_to_text_backdrop(&turned, 2.0).expect("a frost");

        assert_eq!(moved.spec.density, 2.0, "the surface scale, and no more");
        assert_eq!(turned.spec.density, 2.0 * TEXT_SUPERSAMPLE);
    }

    /// The frame's groups are prepared into a buffer the renderer keeps.
    ///
    /// The one container this change moved out of a local and into the
    /// renderer, and so the one a later edit could put back without anything
    /// noticing: `prepare_layers` fills it and the draw loop reads it a
    /// hundred lines later, which compiles either way. Capacity across two
    /// frames is what says it is still the same `Vec` — the mechanism, for the
    /// reason the sibling test in `text.rs` gives.
    ///
    /// Five groups and then one, and neither number is arbitrary. A `Vec` of a
    /// 64-byte element starts at capacity 4 however few things are pushed into
    /// it, so a first frame of two groups and a rebuilt second frame of one
    /// both read 4 and the test would pass against the very regression it is
    /// here for. Five crosses that step to 8; one rebuilt would come back at
    /// 4.
    #[test]
    fn the_frame_s_groups_are_prepared_into_a_buffer_the_renderer_keeps() {
        /// What `Vec` allocates for the first push of an element this size.
        const MIN_CAP: usize = 4;
        assert!(std::mem::size_of::<PreparedLayer>() <= 1024);

        let Some(gpu) = crate::or_skip(crate::renderer::GpuContext::try_new()) else {
            return;
        };
        let mut renderer = Renderer::new(
            gpu.device.clone(),
            gpu.queue.clone(),
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let mut target = RenderTarget::offscreen(&gpu, 32, 32);

        let box_at = |x: f32| FlattenedCommand {
            command: Rc::new(DrawCommand::RoundedRect {
                rect: Rect::new(x, 0.0, 10.0, 10.0),
                color: crate::widgets::Color::WHITE,
                radius: CornerRadii::uniform(0.0),
                curvature: 1.0,
                border: None,
                shadow: None,
                gradient: None,
            }),
            world_transform: Transform::IDENTITY,
            layer: RenderLayer::Shapes,
            opacity: 1.0,
            clip: None,
        };
        let commands: [FlattenedCommand; 5] = std::array::from_fn(|i| box_at(i as f32 * 12.0));
        let group = |shapes: std::ops::Range<usize>| CommandLayer {
            backdrop: 0..0,
            shapes,
            images: 0..0,
            text: 0..0,
            overlay: 0..0,
        };
        let layers: [CommandLayer; 5] = std::array::from_fn(|i| group(i..i + 1));

        let mut frame = |renderer: &mut Renderer, layers: &[CommandLayer]| {
            renderer.render(
                &mut target,
                &commands,
                layers,
                crate::widgets::Color::TRANSPARENT,
            );
        };

        frame(&mut renderer, &layers);
        let first = renderer.prepared_layers.capacity();
        assert!(
            first > MIN_CAP,
            "five groups should have grown past a fresh `Vec`'s floor: {first}"
        );

        // One group this time. A `Vec` built per frame would come back at that
        // floor; one that was cleared still has room for five.
        frame(&mut renderer, &layers[..1]);
        assert_eq!(
            renderer.prepared_layers.capacity(),
            first,
            "the second frame reused the first frame's buffer"
        );
    }
}
