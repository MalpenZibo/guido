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
use self::text_texture::{TextTextureRenderer, QUALITY_MULTIPLIER};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
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
    /// Custom transform origin in logical screen coordinates, if any
    pub transform_origin: Option<(f32, f32)>,
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
                // Scale the transform origin for HiDPI
                scaled_rect.transform_origin = rect
                    .transform_origin
                    .map(|(x, y)| (x * self.scale_factor, y * self.scale_factor));
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

    /// Create vertex and index GPU buffers for a shape
    fn create_gpu_buffers(
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u16],
        label: &str,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{} Vertex Buffer", label)),
            contents: bytemuck::cast_slice(vertices),
            usage: BufferUsages::VERTEX,
        });
        let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{} Index Buffer", label)),
            contents: bytemuck::cast_slice(indices),
            usage: BufferUsages::INDEX,
        });
        (vb, ib)
    }

    pub fn create_paint_context(&mut self) -> PaintContext {
        PaintContext {
            shapes: Vec::new(),
            texts: Vec::new(),
            overlay_shapes: Vec::new(),
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

                // Calculate display rect - divide only by QUALITY_MULTIPLIER to preserve transform_scale.
                // The texture was rendered at: original_size * scale_factor * transform_scale * QUALITY_MULTIPLIER
                // We want display size: original_size * scale_factor * transform_scale
                let display_width = tex.width as f32 / QUALITY_MULTIPLIER;
                let display_height = tex.height as f32 / QUALITY_MULTIPLIER;

                // Get the scale factor from the transform (we pre-scaled the text for this)
                let transform_scale = tex.entry.transform.extract_scale();

                // Calculate the original center position in physical pixels
                let original_center_x = tex.entry.rect.x * self.scale_factor
                    + tex.entry.rect.width * self.scale_factor / 2.0;
                let original_center_y = tex.entry.rect.y * self.scale_factor
                    + tex.entry.rect.height * self.scale_factor / 2.0;

                // If there's scale with custom transform_origin, the center moves
                // offset = (center - origin) * (scale - 1)
                // This is needed because shapes get scaled outward from origin,
                // but we pre-scale text so we need to manually offset the position
                let (center_offset_x, center_offset_y) =
                    if let Some((ox, oy)) = tex.entry.transform_origin {
                        let scaled_ox = ox * self.scale_factor;
                        let scaled_oy = oy * self.scale_factor;
                        let offset_x = (original_center_x - scaled_ox) * (transform_scale - 1.0);
                        let offset_y = (original_center_y - scaled_oy) * (transform_scale - 1.0);
                        (offset_x, offset_y)
                    } else {
                        // Default center origin - no offset needed
                        (0.0, 0.0)
                    };

                // Position the quad centered at the original text center,
                // adjusted for any scale-induced offset
                let display_rect = Rect::new(
                    original_center_x + center_offset_x - display_width / 2.0,
                    original_center_y + center_offset_y - display_height / 2.0,
                    display_width,
                    display_height,
                );

                // Scale transform_origin to physical pixels (same as shapes do)
                let scaled_origin = tex
                    .entry
                    .transform_origin
                    .map(|(x, y)| (x * self.scale_factor, y * self.scale_factor));

                // Get transform with rotation and translation (no scale since text is pre-scaled)
                // and scale the translation component for HiDPI
                let mut quad_transform = tex.entry.transform.without_scale();
                quad_transform.scale_translation(self.scale_factor);

                // Create the textured quad with rotation + translation.
                // Pass the transform_origin so to_vertices applies the same centering logic as shapes.
                let quad = TexturedQuad::new(display_rect, quad_transform, scaled_origin);

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
                let (vb, ib) = Self::create_gpu_buffers(&self.device, &vertices, &indices, "Shape");
                Some((vb, ib, indices.len()))
            })
            .collect();

        // Scale overlay shapes and create GPU buffers (rendered after text)
        let overlay_gpu_buffers: Vec<_> = paint_ctx
            .overlay_shapes
            .iter()
            .filter_map(|shape| {
                let (vertices, indices) = self.scale_shape(shape);
                if vertices.is_empty() || indices.is_empty() {
                    return None;
                }
                let (vb, ib) =
                    Self::create_gpu_buffers(&self.device, &vertices, &indices, "Overlay Shape");
                Some((vb, ib, indices.len()))
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

            // Draw overlay shapes (rendered after text, for effects like ripples)
            if !overlay_gpu_buffers.is_empty() {
                render_pass.set_pipeline(&self.pipeline);
                for (vb, ib, index_count) in &overlay_gpu_buffers {
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
    /// Overlay shapes rendered after text (for effects like ripples)
    overlay_shapes: Vec<Shape>,
    /// Clip stack for clipping children to container bounds
    /// Each entry is (clip_rect, corner_radius, curvature)
    clip_stack: Vec<(Rect, f32, f32)>,
    /// Transform stack for composing parent→child transformations
    /// Each entry is (transform, Option<origin_point>) where origin_point
    /// is Some((x, y)) if a custom transform origin should be used, None for default (center)
    transform_stack: Vec<(Transform, Option<(f32, f32)>)>,
}

impl PaintContext {
    /// Create a new PaintContext with pre-allocated capacity to avoid per-frame allocations
    pub fn with_capacity(shapes: usize, texts: usize, overlay: usize) -> Self {
        Self {
            shapes: Vec::with_capacity(shapes),
            texts: Vec::with_capacity(texts),
            overlay_shapes: Vec::with_capacity(overlay),
            clip_stack: Vec::with_capacity(4),
            transform_stack: Vec::with_capacity(4),
        }
    }

    /// Clear all buffers for reuse, preserving allocated capacity
    pub fn clear(&mut self) {
        self.shapes.clear();
        self.texts.clear();
        self.overlay_shapes.clear();
        self.clip_stack.clear();
        self.transform_stack.clear();
    }

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
        let (transform, transform_origin) = self.current_transform_with_origin();

        self.texts.push(TextEntry {
            text: text.to_string(),
            rect,
            color,
            font_size,
            clip_rect,
            transform,
            transform_origin,
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

    /// Push a transform onto the stack (will be centered at shape's center)
    pub fn push_transform(&mut self, transform: Transform) {
        let composed = if let Some((current, _)) = self.transform_stack.last() {
            current.then(&transform)
        } else {
            transform
        };
        self.transform_stack.push((composed, None));
    }

    /// Push a transform with a custom origin point (in logical screen coordinates)
    pub fn push_transform_with_origin(
        &mut self,
        transform: Transform,
        origin_x: f32,
        origin_y: f32,
    ) {
        let composed = if let Some((current, _)) = self.transform_stack.last() {
            current.then(&transform)
        } else {
            transform
        };
        self.transform_stack
            .push((composed, Some((origin_x, origin_y))));
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

    /// Get the current transform with its custom origin (if any)
    fn current_transform_with_origin(&self) -> (Transform, Option<(f32, f32)>) {
        self.transform_stack
            .last()
            .cloned()
            .unwrap_or((Transform::IDENTITY, None))
    }

    /// Apply the current transform from the stack to a shape
    fn apply_current_transform(&self, shape: &mut impl Transformable) {
        let (transform, origin) = self.current_transform_with_origin();
        shape.set_transform(transform, origin);
    }

    /// Helper to apply clip, transform, and push a rounded rect shape
    fn push_rounded_rect(&mut self, mut shape: RoundedRect) {
        shape.clip = self.current_clip();
        self.apply_current_transform(&mut shape);
        self.shapes.push(Shape::RoundedRect(shape));
    }

    /// Helper to apply clip, transform, and push an overlay rounded rect shape
    fn push_overlay_rounded_rect(&mut self, mut shape: RoundedRect) {
        shape.clip = self.current_clip();
        self.apply_current_transform(&mut shape);
        self.overlay_shapes.push(Shape::RoundedRect(shape));
    }

    /// Draw a circle as an overlay (rendered after text).
    /// The circle is drawn centered at (cx, cy) with the given radius.
    pub fn draw_overlay_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Color) {
        // A circle is a rounded rect where corner_radius = width/2 = height/2
        let rect = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
        self.push_overlay_rounded_rect(RoundedRect::new(rect, color, radius));
    }

    /// Draw a circle as an overlay with a specific clip region.
    /// Used for ripple effects that need to be clipped to container bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_overlay_circle_clipped(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        color: Color,
        clip_rect: Rect,
        clip_radius: f32,
        clip_curvature: f32,
    ) {
        let rect = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
        let mut shape = RoundedRect::new(rect, color, radius);
        // Set explicit clip for this shape
        shape.clip = Some(ClipRegion {
            rect: clip_rect,
            radius: clip_radius,
            curvature: clip_curvature,
        });
        self.apply_current_transform(&mut shape);
        self.overlay_shapes.push(Shape::RoundedRect(shape));
    }

    /// Draw a circle as an overlay with a clip region that can have its own transform.
    /// Used for ripple effects on transformed containers - the ripple uses screen coordinates
    /// but the clip region needs to match the transformed container bounds.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_overlay_circle_clipped_with_transform(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        color: Color,
        clip_rect: Rect,
        clip_radius: f32,
        clip_curvature: f32,
        clip_transform: Option<(Transform, TransformOrigin)>,
    ) {
        let rect = Rect::new(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
        let mut shape = RoundedRect::new(rect, color, radius);

        // Set explicit clip for this shape
        shape.clip = Some(ClipRegion {
            rect: clip_rect,
            radius: clip_radius,
            curvature: clip_curvature,
        });

        // Apply transform to both shape and clip if provided
        // This makes the ripple appear at the correct screen position
        // while clipping to the transformed container bounds
        if let Some((transform, origin)) = clip_transform {
            let (origin_x, origin_y) = origin.resolve(clip_rect);
            shape.transform = transform;
            shape.transform_origin = Some((origin_x, origin_y));
        }

        self.overlay_shapes.push(Shape::RoundedRect(shape));
    }
}
