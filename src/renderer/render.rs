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

        let prepared = self.prepare_layers(commands, layers);

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

/// A logical rect in physical pixels.
///
/// Every rect the backdrop pass is handed has made this trip, and it is the one
/// place the convention is written down: the pass works in physical pixels, and
/// the shapes it cuts against do not.
fn to_physical(rect: Rect, scale: f32) -> Rect {
    Rect::new(
        rect.x * scale,
        rect.y * scale,
        rect.width * scale,
        rect.height * scale,
    )
}

/// The clip a command was flattened under, in physical pixels.
///
/// The axis-aligned world box of it: a rounded or rotated clip is approximated
/// by that box, which is one pixel of slack at the corners against a blurred
/// rectangle where the content has been scrolled away — and a good deal more
/// than that against a turned one, which is #401.
fn clip_rect(cmd: &FlattenedCommand, scale: f32) -> Option<Rect> {
    Some(to_physical(cmd.clip.as_ref()?.world_aabb(), scale))
}

/// Where a placed shape lands for the backdrop pass.
///
/// The *viewport* is the box around the shape, because a viewport has no other
/// shape to be: it says which pixels the pass may touch, and wgpu takes four
/// integers. The *shape* travels in its own space, alongside the map back into
/// it, and the pass carries each fragment there before deciding what to cut —
/// which is how a turned container frosts what it drew rather than an upright
/// rounded rect the size of its box, and how a turned text's coverage stays on
/// its letters.
///
/// Both callers construct it the same way, and that is the point: the viewport
/// is derived from the very shape it accompanies, so the two cannot drift.
fn backdrop_region(
    shape: PlacedShape,
    radius: f32,
    cmd: &FlattenedCommand,
    scale: f32,
) -> Option<BackdropRegion> {
    Some(BackdropRegion {
        rect: to_physical(shape.world_aabb(), scale),
        radius: radius * scale,
        // Logical, like the clip's rect and for the same reason: `to_shape`
        // lands a physical fragment in the shape's own *logical* space, because
        // the surface scale is folded into the map. Scaling it here as well
        // applies it twice — and at scale 1 the two conventions agree, so the
        // pairing has to be got right somewhere a golden can see it.
        shape: shape.rect,
        to_shape: shape.to_local(scale)?,
        radii: shape.radii,
        curvature: shape.curvature,
        clip: clip_rect(cmd, scale),
    })
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
    backdrop_region(
        PlacedShape::placed(*rect, *corner_radii, *curvature, cmd.world_transform),
        *radius,
        cmd,
        scale,
    )
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
    )?;

    Some(TextBackdrop {
        region,
        spec: MaskSpec {
            text,
            font_size: *font_size,
            font_family,
            font_weight: *font_weight,
            // Shaped by whichever path will draw the glyphs over the frost:
            // the two break their lines in different places, and the frost has
            // to break its own where the letters do.
            buffer: if cmd.world_transform.is_translation_only() {
                super::text::shaping_buffer(*rect, density)
            } else {
                super::text_quad::shaping_buffer(*rect, density)
            },
            size: (width as u32, height as u32),
            offset: (slack * density, slack * density),
            density,
        },
        outline: stroke.map(|s| (s.color, s.width * density)),
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
                instance = instance.with_clip(clip, scale);
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
                instance = instance.with_clip(clip, scale);
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
        } => {
            // glyphon clips to four integers, so text gets the world box of
            // the clip and not its shape. A turned clip over text is #199.
            let clip_rect = cmd.clip.as_ref().map(PlacedShape::world_aabb);

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
            let frame = frost.region.shape;
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
        assert!(frost.region.rect.width >= 110.0, "{:?}", frost.region.rect);
        assert!(frost.region.rect.height >= 40.0, "{:?}", frost.region.rect);
    }

    #[test]
    fn the_scale_factor_reaches_the_region_the_radius_and_the_density() {
        let cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        let one = command_to_text_backdrop(&cmd, 1.0).expect("a frost");
        let two = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
        assert_eq!(two.region.radius, one.region.radius * 2.0);
        assert!(two.region.rect.width >= one.region.rect.width * 2.0 - 1.0);
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
        assert!((moved.region.rect.x - at_rest.region.rect.x - 40.0).abs() < 1e-3);
        assert!((moved.region.rect.y - at_rest.region.rect.y - 5.0).abs() < 1e-3);
    }

    /// A frost inside a scroll view must not paint where the text has been
    /// scrolled away to: the clip is what the effect is allowed to write.
    #[test]
    fn the_clip_reaches_the_region() {
        let mut cmd = frosted(Rect::new(10.0, 20.0, 100.0, 30.0), Transform::IDENTITY);
        cmd.clip = Some(crate::shape::PlacedShape {
            rect: Rect::new(0.0, 0.0, 200.0, 200.0),
            radii: CornerRadii::uniform(0.0),
            curvature: 1.0,
            placement: Transform::IDENTITY,
        });
        let frost = command_to_text_backdrop(&cmd, 2.0).expect("a frost");
        assert_eq!(
            frost.region.clip,
            Some(Rect::new(0.0, 0.0, 400.0, 400.0)),
            "in physical pixels, like the region it bounds"
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

        let frame = frost.region.shape;
        let turned = cmd.world_transform.map_rect(frame);
        assert!(
            (frost.region.rect.x - turned.x).abs() < 1e-3
                && (frost.region.rect.width - turned.width).abs() < 1e-3,
            "the viewport is the box around the turned frame: {:?} vs {turned:?}",
            frost.region.rect
        );
        assert!(
            turned.width > frame.width,
            "a turn widens the box it needs, or this proves nothing"
        );

        // The frame's own corner, carried from the target back into the text's
        // space, lands on the frame — which is what the composite does to find
        // its mask texel.
        let to_shape = frost.region.to_shape;
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
        assert_eq!(large.region.shape, small.region.shape);
        assert!(
            (large.region.rect.width - small.region.rect.width / 1.6 * 3.0).abs() < 1e-2,
            "{:?} did not grow with the scale against {:?}",
            large.region.rect,
            small.region.rect
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
}
