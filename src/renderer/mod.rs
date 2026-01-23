pub mod context;
pub mod pipeline;
pub mod primitives;
pub mod text;
mod text_measurer;
pub mod text_texture;

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{BufferUsages, Device, Queue, RenderPipeline};

use self::primitives::{ClipRegion, Gradient, RoundedRect, TexturedQuad, Transformable, Vertex};
use self::text::TextRenderState;
use self::text_texture::TextTextureRenderer;
use crate::transform::Transform;
use crate::widgets::{Color, Rect};

pub use context::{GpuContext, SurfaceState};
pub use text_measurer::measure_text;

/// A text entry for rendering, containing all information needed to render text.
#[derive(Debug, Clone)]
pub struct TextEntry {
    /// The text string to render
    pub text: String,
    /// The bounding rectangle for the text in logical pixels
    pub rect: Rect,
    /// The text color
    pub color: Color,
    /// The font size in logical pixels
    pub font_size: f32,
    /// Optional clip rectangle to constrain text rendering
    pub clip_rect: Option<Rect>,
    /// Transform to apply to this text
    pub transform: Transform,
    /// Whether the transform is already centered around a custom origin
    pub transform_is_centered: bool,
}

/// Enum to hold different shape types for rendering
#[derive(Debug, Clone)]
enum Shape {
    RoundedRect(RoundedRect),
}

pub struct Renderer {
    device: Arc<Device>,
    queue: Arc<Queue>,
    pipeline: RenderPipeline,
    texture_pipeline: RenderPipeline,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    texture_sampler: wgpu::Sampler,
    text_state: TextRenderState,
    text_texture_renderer: TextTextureRenderer,
    screen_width: f32,
    screen_height: f32,
    scale_factor: f32,
    #[allow(dead_code)] // May be used for dynamic pipeline creation
    format: wgpu::TextureFormat,
}

impl Renderer {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, format: wgpu::TextureFormat) -> Self {
        let pipeline = pipeline::create_render_pipeline(&device, format);
        let (texture_pipeline, texture_bind_group_layout) =
            pipeline::create_texture_pipeline(&device, format);
        let text_state = TextRenderState::new(&device, &queue, format);
        let text_texture_renderer = TextTextureRenderer::new(&device, &queue, format);

        // Create sampler for text textures
        let texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Texture Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Self {
            device,
            queue,
            pipeline,
            texture_pipeline,
            texture_bind_group_layout,
            texture_sampler,
            text_state,
            text_texture_renderer,
            screen_width: 1.0,
            screen_height: 1.0,
            scale_factor: 1.0,
            format,
        }
    }

    /// Scale a clip region for HiDPI rendering
    fn scale_clip(&self, clip: &Option<ClipRegion>) -> Option<ClipRegion> {
        clip.as_ref().map(|c| ClipRegion {
            rect: Rect::new(
                c.rect.x * self.scale_factor,
                c.rect.y * self.scale_factor,
                c.rect.width * self.scale_factor,
                c.rect.height * self.scale_factor,
            ),
            radius: c.radius * self.scale_factor,
            curvature: c.curvature,
        })
    }

    /// Scale a shape for HiDPI rendering and convert to vertices
    fn scale_shape(&self, shape: &Shape) -> (Vec<Vertex>, Vec<u16>) {
        match shape {
            Shape::RoundedRect(rect) => {
                let scaled_clip = self.scale_clip(&rect.clip);
                let mut scaled_rect = RoundedRect::new(
                    Rect::new(
                        rect.rect.x * self.scale_factor,
                        rect.rect.y * self.scale_factor,
                        rect.rect.width * self.scale_factor,
                        rect.rect.height * self.scale_factor,
                    ),
                    rect.color,
                    rect.radius * self.scale_factor,
                );
                scaled_rect.clip = scaled_clip;
                scaled_rect.gradient = rect.gradient;
                scaled_rect.curvature = rect.curvature;
                scaled_rect.border_width = rect.border_width * self.scale_factor;
                scaled_rect.border_color = rect.border_color;
                scaled_rect.shadow = primitives::Shadow {
                    offset: (
                        rect.shadow.offset.0 * self.scale_factor,
                        rect.shadow.offset.1 * self.scale_factor,
                    ),
                    blur: rect.shadow.blur * self.scale_factor,
                    spread: rect.shadow.spread * self.scale_factor,
                    color: rect.shadow.color,
                };
                // Scale the transform translation components for HiDPI
                let mut scaled_transform = rect.transform;
                scaled_transform.scale_translation(self.scale_factor);
                scaled_rect.transform = scaled_transform;
                scaled_rect.transform_is_centered = rect.transform_is_centered;
                scaled_rect.to_vertices(self.screen_width, self.screen_height)
            }
        }
    }

    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    pub fn set_scale_factor(&mut self, scale: f32) {
        self.scale_factor = scale;
    }

    pub fn create_paint_context(&mut self) -> PaintContext {
        PaintContext {
            shapes: Vec::new(),
            texts: Vec::new(),
            clip_stack: Vec::new(),
            transform_stack: Vec::new(),
        }
    }

    pub fn render(
        &mut self,
        surface: &mut SurfaceState,
        paint_ctx: &PaintContext,
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

        // Prepare non-transformed text and get indices of transformed texts
        let transformed_text_indices = if !paint_ctx.texts.is_empty() {
            self.text_state.prepare_text(
                &self.device,
                &self.queue,
                &paint_ctx.texts,
                self.screen_width as u32,
                self.screen_height as u32,
                self.scale_factor,
            )
        } else {
            Vec::new()
        };

        // Render transformed text to textures
        let text_textures: Vec<_> = transformed_text_indices
            .iter()
            .map(|&idx| {
                self.text_texture_renderer.render_to_texture(
                    &self.device,
                    &self.queue,
                    &paint_ctx.texts[idx],
                    self.scale_factor,
                )
            })
            .collect();

        // Create bind groups and vertex/index buffers for transformed text quads
        let textured_quad_data: Vec<_> = text_textures
            .iter()
            .filter_map(|tex| {
                // Create bind group for this texture
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Text Texture Bind Group"),
                    layout: &self.texture_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&tex.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.texture_sampler),
                        },
                    ],
                });

                // Calculate display rect - the texture was rendered at an effective scale,
                // so we need to display it at the correct logical size
                let display_width = tex.width as f32 / tex.render_scale * self.scale_factor;
                let display_height = tex.height as f32 / tex.render_scale * self.scale_factor;

                // The text texture has 4.0 logical pixels of padding on each side.
                // We need to offset the position so the text content (not the padding)
                // aligns with where the text should appear.
                let padding_physical = 4.0 * self.scale_factor;

                // Apply HiDPI scaling to position and offset by padding
                let display_rect = Rect::new(
                    tex.entry.rect.x * self.scale_factor - padding_physical,
                    tex.entry.rect.y * self.scale_factor - padding_physical,
                    display_width,
                    display_height,
                );

                // Create the textured quad with just rotation (no scale since text is pre-scaled,
                // no translation since we position the quad explicitly).
                // Always use transform_is_centered=false so the quad centers the transform at the
                // text's own center. This ensures text rotates around its own center regardless
                // of the container's transform_origin.
                let quad = TexturedQuad::new(
                    display_rect,
                    tex.entry.transform.rotation_only(),
                    false, // Center at text's position
                );

                let (vertices, indices) = quad.to_vertices(self.screen_width, self.screen_height);
                if vertices.is_empty() || indices.is_empty() {
                    return None;
                }

                let vb = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Textured Quad Vertex Buffer"),
                        contents: bytemuck::cast_slice(&vertices),
                        usage: BufferUsages::VERTEX,
                    });
                let ib = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Textured Quad Index Buffer"),
                        contents: bytemuck::cast_slice(&indices),
                        usage: BufferUsages::INDEX,
                    });

                Some((bind_group, vb, ib, indices.len()))
            })
            .collect();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Scale shapes and create GPU buffers in a single pass, filtering empty shapes
        let shape_gpu_buffers: Vec<_> = paint_ctx
            .shapes
            .iter()
            .filter_map(|shape| {
                let (vertices, indices) = self.scale_shape(shape);
                if vertices.is_empty() || indices.is_empty() {
                    return None;
                }
                Some((
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Shape Vertex Buffer"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: BufferUsages::VERTEX,
                        }),
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Shape Index Buffer"),
                            contents: bytemuck::cast_slice(&indices),
                            usage: BufferUsages::INDEX,
                        }),
                    indices.len(),
                ))
            })
            .collect();

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
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

            // Draw shapes (background layer) - one draw call per shape for proper layering
            render_pass.set_pipeline(&self.pipeline);
            for (vb, ib, index_count) in &shape_gpu_buffers {
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..*index_count as u32, 0, 0..1);
            }

            // Draw non-transformed text
            let has_non_transformed_text = !paint_ctx.texts.is_empty()
                && transformed_text_indices.len() < paint_ctx.texts.len();
            if has_non_transformed_text {
                self.text_state.render(&mut render_pass, &self.device);
            }

            // Draw transformed text (textured quads)
            if !textured_quad_data.is_empty() {
                render_pass.set_pipeline(&self.texture_pipeline);
                for (bind_group, vb, ib, index_count) in &textured_quad_data {
                    render_pass.set_bind_group(0, bind_group, &[]);
                    render_pass.set_vertex_buffer(0, vb.slice(..));
                    render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                    render_pass.draw_indexed(0..*index_count as u32, 0, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

pub struct PaintContext {
    shapes: Vec<Shape>,
    /// Text entries with full transform support
    texts: Vec<TextEntry>,
    /// Clip stack for clipping children to container bounds
    /// Each entry is (clip_rect, corner_radius, curvature)
    clip_stack: Vec<(Rect, f32, f32)>,
    /// Transform stack for composing parent→child transformations
    /// Each entry is (transform, is_centered) where is_centered indicates
    /// the transform has already been centered around a custom origin
    transform_stack: Vec<(Transform, bool)>,
}

impl PaintContext {
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.push_rounded_rect(RoundedRect::new(rect, color, 0.0));
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        self.push_rounded_rect(RoundedRect::new(rect, color, radius));
    }

    /// Draw a rounded rectangle with custom curvature
    pub fn draw_rounded_rect_with_curvature(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
    ) {
        self.push_rounded_rect(RoundedRect::with_curvature(rect, color, radius, curvature));
    }

    /// Draw a rounded rectangle with a linear gradient
    pub fn draw_gradient_rect(
        &mut self,
        rect: Rect,
        start_color: Color,
        end_color: Color,
        direction: primitives::GradientDir,
        radius: f32,
    ) {
        let gradient = Gradient {
            start_color,
            end_color,
            direction,
        };
        self.push_rounded_rect(RoundedRect::with_gradient(rect, gradient, radius));
    }

    /// Draw a rounded rectangle with a linear gradient and custom curvature
    pub fn draw_gradient_rect_with_curvature(
        &mut self,
        rect: Rect,
        start_color: Color,
        end_color: Color,
        direction: primitives::GradientDir,
        radius: f32,
        curvature: f32,
    ) {
        let gradient = Gradient {
            start_color,
            end_color,
            direction,
        };
        let mut shape = RoundedRect::with_gradient(rect, gradient, radius);
        shape.curvature = curvature;
        self.push_rounded_rect(shape);
    }

    /// Draw a border frame (hollow rounded rectangle - just the border outline)
    /// Uses SDF-based rendering for crisp anti-aliased borders
    pub fn draw_border_frame(
        &mut self,
        rect: Rect,
        color: Color,
        corner_radius: f32,
        border_width: f32,
    ) {
        self.push_rounded_rect(RoundedRect::border_only(
            rect,
            corner_radius,
            border_width,
            color,
        ));
    }

    /// Draw a border frame with custom curvature
    /// Uses SDF-based rendering for crisp anti-aliased borders
    pub fn draw_border_frame_with_curvature(
        &mut self,
        rect: Rect,
        color: Color,
        corner_radius: f32,
        border_width: f32,
        curvature: f32,
    ) {
        self.push_rounded_rect(RoundedRect::border_only_with_curvature(
            rect,
            corner_radius,
            border_width,
            color,
            curvature,
        ));
    }

    /// Draw a rounded rectangle with both fill and border
    pub fn draw_rounded_rect_with_border(
        &mut self,
        rect: Rect,
        fill_color: Color,
        radius: f32,
        border_width: f32,
        border_color: Color,
    ) {
        self.push_rounded_rect(RoundedRect::with_border(
            rect,
            fill_color,
            radius,
            border_width,
            border_color,
        ));
    }

    /// Draw a rounded rectangle with fill, border, and custom curvature
    pub fn draw_rounded_rect_with_border_and_curvature(
        &mut self,
        rect: Rect,
        fill_color: Color,
        radius: f32,
        border_width: f32,
        border_color: Color,
        curvature: f32,
    ) {
        let mut shape =
            RoundedRect::with_border(rect, fill_color, radius, border_width, border_color);
        shape.curvature = curvature;
        self.push_rounded_rect(shape);
    }

    /// Draw a rounded rectangle with a shadow
    pub fn draw_rounded_rect_with_shadow(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        shadow: primitives::Shadow,
    ) {
        let mut shape = RoundedRect::new(rect, color, radius);
        shape.shadow = shadow;
        self.push_rounded_rect(shape);
    }

    /// Draw a rounded rectangle with a shadow and custom curvature
    pub fn draw_rounded_rect_with_shadow_and_curvature(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
        shadow: primitives::Shadow,
    ) {
        let mut shape = RoundedRect::with_curvature(rect, color, radius, curvature);
        shape.shadow = shadow;
        self.push_rounded_rect(shape);
    }

    pub fn draw_text(&mut self, text: &str, rect: Rect, color: Color, font_size: f32) {
        // Get current clip rect (if any) for text clipping
        let clip_rect = self.clip_stack.last().map(|(rect, _, _)| *rect);
        // Get current transform from the stack
        let (transform, transform_is_centered) = self.current_transform_with_flag();

        self.texts.push(TextEntry {
            text: text.to_string(),
            rect,
            color,
            font_size,
            clip_rect,
            transform,
            transform_is_centered,
        });
    }

    /// Push a clip region onto the stack
    /// All children drawn after this will be clipped to the given bounds
    pub fn push_clip(&mut self, rect: Rect, corner_radius: f32, curvature: f32) {
        self.clip_stack.push((rect, corner_radius, curvature));
    }

    /// Pop a clip region from the stack
    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    /// Get the current clip region if any
    fn current_clip(&self) -> Option<ClipRegion> {
        self.clip_stack
            .last()
            .map(|(rect, radius, curvature)| ClipRegion {
                rect: *rect,
                radius: *radius,
                curvature: *curvature,
            })
    }

    /// Push a transform onto the stack
    /// This transform is composed with the current transform
    pub fn push_transform(&mut self, transform: Transform) {
        let (composed, _) = if let Some((current, _)) = self.transform_stack.last() {
            (current.then(&transform), false)
        } else {
            (transform, false)
        };
        self.transform_stack.push((composed, false));
    }

    /// Push a pre-centered transform onto the stack
    /// Use this when the transform has already been centered around a custom origin point
    pub fn push_centered_transform(&mut self, transform: Transform) {
        let (composed, _) = if let Some((current, _)) = self.transform_stack.last() {
            (current.then(&transform), true)
        } else {
            (transform, true)
        };
        self.transform_stack.push((composed, true));
    }

    /// Pop a transform from the stack
    pub fn pop_transform(&mut self) {
        self.transform_stack.pop();
    }

    /// Get the current composed transform
    pub fn current_transform(&self) -> Transform {
        self.transform_stack
            .last()
            .map(|(t, _)| *t)
            .unwrap_or(Transform::IDENTITY)
    }

    /// Get the current transform with its centered flag
    fn current_transform_with_flag(&self) -> (Transform, bool) {
        self.transform_stack
            .last()
            .copied()
            .unwrap_or((Transform::IDENTITY, false))
    }

    /// Apply the current transform from the stack to a shape
    fn apply_current_transform(&self, shape: &mut impl Transformable) {
        let (transform, is_centered) = self.current_transform_with_flag();
        shape.set_transform(transform, is_centered);
    }

    /// Helper to apply clip, transform, and push a rounded rect shape
    fn push_rounded_rect(&mut self, mut shape: RoundedRect) {
        shape.clip = self.current_clip();
        self.apply_current_transform(&mut shape);
        self.shapes.push(Shape::RoundedRect(shape));
    }
}
