pub mod context;
pub mod pipeline;
pub mod primitives;
pub mod text;

use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{BufferUsages, Device, Queue, RenderPipeline};

use self::primitives::{Circle, ClipRegion, Gradient, RoundedRect, Vertex};
use self::text::TextRenderState;
use crate::widgets::{Color, Rect};

pub use context::{GpuContext, SurfaceState};

/// Enum to hold different shape types for rendering
#[derive(Debug, Clone)]
enum Shape {
    RoundedRect(RoundedRect),
    Circle(Circle),
}

pub struct Renderer {
    device: Arc<Device>,
    queue: Arc<Queue>,
    pipeline: RenderPipeline,
    text_state: TextRenderState,
    screen_width: f32,
    screen_height: f32,
    scale_factor: f32,
}

impl Renderer {
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, format: wgpu::TextureFormat) -> Self {
        let pipeline = pipeline::create_render_pipeline(&device, format);
        let text_state = TextRenderState::new(&device, &queue, format);

        Self {
            device,
            queue,
            pipeline,
            text_state,
            screen_width: 1.0,
            screen_height: 1.0,
            scale_factor: 1.0,
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
            overlay_shapes: Vec::new(),
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

        // Collect vertices and indices for each shape separately to ensure proper draw order
        let mut shape_buffers: Vec<(Vec<Vertex>, Vec<u16>)> = Vec::new();

        for shape in &paint_ctx.shapes {
            let (vertices, indices) = match shape {
                Shape::RoundedRect(rect) => {
                    // Scale rect coordinates for HiDPI rendering
                    let scaled_clip = rect.clip.as_ref().map(|c| ClipRegion {
                        rect: Rect::new(
                            c.rect.x * self.scale_factor,
                            c.rect.y * self.scale_factor,
                            c.rect.width * self.scale_factor,
                            c.rect.height * self.scale_factor,
                        ),
                        radius: c.radius * self.scale_factor,
                        curvature: c.curvature, // Preserve curvature (doesn't scale)
                    });
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
                    scaled_rect.gradient = rect.gradient.clone(); // Preserve gradient
                    scaled_rect.curvature = rect.curvature; // Preserve curvature (doesn't scale)
                    scaled_rect.border_width = rect.border_width * self.scale_factor;
                    scaled_rect.border_color = rect.border_color;
                    // Scale shadow parameters
                    scaled_rect.shadow = primitives::Shadow {
                        offset: (
                            rect.shadow.offset.0 * self.scale_factor,
                            rect.shadow.offset.1 * self.scale_factor,
                        ),
                        blur: rect.shadow.blur * self.scale_factor,
                        spread: rect.shadow.spread * self.scale_factor,
                        color: rect.shadow.color,
                    };
                    scaled_rect.to_vertices(self.screen_width, self.screen_height)
                }
                Shape::Circle(circle) => {
                    // Scale circle coordinates for HiDPI rendering
                    let scaled_clip = circle.clip.as_ref().map(|c| ClipRegion {
                        rect: Rect::new(
                            c.rect.x * self.scale_factor,
                            c.rect.y * self.scale_factor,
                            c.rect.width * self.scale_factor,
                            c.rect.height * self.scale_factor,
                        ),
                        radius: c.radius * self.scale_factor,
                        curvature: c.curvature, // Preserve curvature (doesn't scale)
                    });
                    let mut scaled_circle = Circle::new(
                        circle.center_x * self.scale_factor,
                        circle.center_y * self.scale_factor,
                        circle.radius * self.scale_factor,
                        circle.color,
                    );
                    scaled_circle.clip = scaled_clip;
                    scaled_circle.to_vertices(self.screen_width, self.screen_height)
                }
            };
            shape_buffers.push((vertices, indices));
        }

        // Collect overlay vertices and indices (rendered after text)
        let mut overlay_vertices: Vec<Vertex> = Vec::new();
        let mut overlay_indices: Vec<u16> = Vec::new();

        for shape in &paint_ctx.overlay_shapes {
            let (vertices, indices) = match shape {
                Shape::RoundedRect(rect) => {
                    let scaled_clip = rect.clip.as_ref().map(|c| ClipRegion {
                        rect: Rect::new(
                            c.rect.x * self.scale_factor,
                            c.rect.y * self.scale_factor,
                            c.rect.width * self.scale_factor,
                            c.rect.height * self.scale_factor,
                        ),
                        radius: c.radius * self.scale_factor,
                        curvature: c.curvature, // Preserve curvature (doesn't scale)
                    });
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
                    scaled_rect.gradient = rect.gradient.clone(); // Preserve gradient
                    scaled_rect.curvature = rect.curvature; // Preserve curvature (doesn't scale)
                    scaled_rect.border_width = rect.border_width * self.scale_factor;
                    scaled_rect.border_color = rect.border_color;
                    // Scale shadow parameters
                    scaled_rect.shadow = primitives::Shadow {
                        offset: (
                            rect.shadow.offset.0 * self.scale_factor,
                            rect.shadow.offset.1 * self.scale_factor,
                        ),
                        blur: rect.shadow.blur * self.scale_factor,
                        spread: rect.shadow.spread * self.scale_factor,
                        color: rect.shadow.color,
                    };
                    scaled_rect.to_vertices(self.screen_width, self.screen_height)
                }
                Shape::Circle(circle) => {
                    let scaled_clip = circle.clip.as_ref().map(|c| ClipRegion {
                        rect: Rect::new(
                            c.rect.x * self.scale_factor,
                            c.rect.y * self.scale_factor,
                            c.rect.width * self.scale_factor,
                            c.rect.height * self.scale_factor,
                        ),
                        radius: c.radius * self.scale_factor,
                        curvature: c.curvature, // Preserve curvature (doesn't scale)
                    });
                    let mut scaled_circle = Circle::new(
                        circle.center_x * self.scale_factor,
                        circle.center_y * self.scale_factor,
                        circle.radius * self.scale_factor,
                        circle.color,
                    );
                    scaled_circle.clip = scaled_clip;
                    scaled_circle.to_vertices(self.screen_width, self.screen_height)
                }
            };
            let base_index = overlay_vertices.len() as u16;
            overlay_vertices.extend(vertices);
            overlay_indices.extend(indices.iter().map(|i| i + base_index));
        }

        // Create overlay buffers
        let overlay_vertex_buffer = if !overlay_vertices.is_empty() {
            Some(
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Overlay Vertex Buffer"),
                        contents: bytemuck::cast_slice(&overlay_vertices),
                        usage: BufferUsages::VERTEX,
                    }),
            )
        } else {
            None
        };

        let overlay_index_buffer = if !overlay_indices.is_empty() {
            Some(
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Overlay Index Buffer"),
                        contents: bytemuck::cast_slice(&overlay_indices),
                        usage: BufferUsages::INDEX,
                    }),
            )
        } else {
            None
        };

        // Prepare text
        if !paint_ctx.texts.is_empty() {
            self.text_state.prepare_text(
                &self.device,
                &self.queue,
                &paint_ctx.texts,
                self.screen_width as u32,
                self.screen_height as u32,
                self.scale_factor,
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Pre-create all shape buffers to keep them alive during render pass
        let shape_gpu_buffers: Vec<_> = shape_buffers
            .iter()
            .filter(|(v, i)| !v.is_empty() && !i.is_empty())
            .map(|(vertices, indices)| {
                let vb = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Shape Vertex Buffer"),
                        contents: bytemuck::cast_slice(vertices),
                        usage: BufferUsages::VERTEX,
                    });
                let ib = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Shape Index Buffer"),
                        contents: bytemuck::cast_slice(indices),
                        usage: BufferUsages::INDEX,
                    });
                (vb, ib, indices.len())
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

            // Draw text
            if !paint_ctx.texts.is_empty() {
                self.text_state.render(&mut render_pass, &self.device);
            }

            // Draw overlay shapes (on top of text)
            if let (Some(vb), Some(ib)) = (&overlay_vertex_buffer, &overlay_index_buffer) {
                render_pass.set_pipeline(&self.pipeline);
                render_pass.set_vertex_buffer(0, vb.slice(..));
                render_pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
                render_pass.draw_indexed(0..overlay_indices.len() as u32, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }
}

pub struct PaintContext {
    shapes: Vec<Shape>,
    texts: Vec<(String, Rect, Color, f32)>,
    /// Shapes to render after text (for effects like ripples over text)
    overlay_shapes: Vec<Shape>,
}

impl PaintContext {
    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::new(rect, color, 0.0)));
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::new(rect, color, radius)));
    }

    /// Draw a rounded rectangle with custom curvature
    pub fn draw_rounded_rect_with_curvature(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        curvature: f32,
    ) {
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::with_curvature(
                rect, color, radius, curvature,
            )));
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
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::with_gradient(
                rect, gradient, radius,
            )));
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
        let mut rounded_rect = RoundedRect::with_gradient(rect, gradient, radius);
        rounded_rect.curvature = curvature;
        self.shapes.push(Shape::RoundedRect(rounded_rect));
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
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::border_only(
                rect,
                corner_radius,
                border_width,
                color,
            )));
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
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::border_only_with_curvature(
                rect,
                corner_radius,
                border_width,
                color,
                curvature,
            )));
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
        self.shapes
            .push(Shape::RoundedRect(RoundedRect::with_border(
                rect,
                fill_color,
                radius,
                border_width,
                border_color,
            )));
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
        let mut rounded_rect =
            RoundedRect::with_border(rect, fill_color, radius, border_width, border_color);
        rounded_rect.curvature = curvature;
        self.shapes.push(Shape::RoundedRect(rounded_rect));
    }

    /// Draw a circle at the given center point with the specified radius
    pub fn draw_circle(&mut self, center_x: f32, center_y: f32, radius: f32, color: Color) {
        self.shapes.push(Shape::Circle(Circle::new(
            center_x, center_y, radius, color,
        )));
    }

    /// Draw a circle clipped to a bounding rectangle with optional rounded corners
    pub fn draw_circle_clipped(
        &mut self,
        center_x: f32,
        center_y: f32,
        radius: f32,
        color: Color,
        clip_rect: Rect,
        clip_corner_radius: f32,
    ) {
        let clip = ClipRegion {
            rect: clip_rect,
            radius: clip_corner_radius,
            curvature: 2.0, // Default to circular clipping
        };
        self.shapes.push(Shape::Circle(Circle::with_clip(
            center_x, center_y, radius, color, clip,
        )));
    }

    /// Draw a rounded rectangle with a shadow
    pub fn draw_rounded_rect_with_shadow(
        &mut self,
        rect: Rect,
        color: Color,
        radius: f32,
        shadow: primitives::Shadow,
    ) {
        let mut rounded_rect = RoundedRect::new(rect, color, radius);
        rounded_rect.shadow = shadow;
        self.shapes.push(Shape::RoundedRect(rounded_rect));
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
        let mut rounded_rect = RoundedRect::with_curvature(rect, color, radius, curvature);
        rounded_rect.shadow = shadow;
        self.shapes.push(Shape::RoundedRect(rounded_rect));
    }

    pub fn draw_text(&mut self, text: &str, rect: Rect, color: Color, font_size: f32) {
        self.texts.push((text.to_string(), rect, color, font_size));
    }

    /// Draw a circle as an overlay (rendered on top of text)
    pub fn draw_overlay_circle(&mut self, center_x: f32, center_y: f32, radius: f32, color: Color) {
        self.overlay_shapes.push(Shape::Circle(Circle::new(
            center_x, center_y, radius, color,
        )));
    }

    /// Draw a circle as an overlay, clipped to a bounding rectangle
    pub fn draw_overlay_circle_clipped(
        &mut self,
        center_x: f32,
        center_y: f32,
        radius: f32,
        color: Color,
        clip_rect: Rect,
        clip_corner_radius: f32,
    ) {
        let clip = ClipRegion {
            rect: clip_rect,
            radius: clip_corner_radius,
            curvature: 2.0, // Default to circular clipping
        };
        self.overlay_shapes.push(Shape::Circle(Circle::with_clip(
            center_x, center_y, radius, color, clip,
        )));
    }

    /// Draw a circle as an overlay, clipped to a bounding rectangle with custom curvature
    #[allow(clippy::too_many_arguments)]
    pub fn draw_overlay_circle_clipped_with_curvature(
        &mut self,
        center_x: f32,
        center_y: f32,
        radius: f32,
        color: Color,
        clip_rect: Rect,
        clip_corner_radius: f32,
        clip_curvature: f32,
    ) {
        let clip = ClipRegion {
            rect: clip_rect,
            radius: clip_corner_radius,
            curvature: clip_curvature,
        };
        self.overlay_shapes.push(Shape::Circle(Circle::with_clip(
            center_x, center_y, radius, color, clip,
        )));
    }
}
