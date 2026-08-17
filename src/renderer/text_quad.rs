//! Textured quad rendering for transformed text.
//!
//! This module renders text to offscreen textures and displays them as
//! transformed quads. This is used when text has rotation or scale transforms
//! that glyphon cannot handle directly.
//!
//! Vertex positions are computed on the CPU and passed as pre-computed
//! NDC coordinates to the shader.

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, ColorMode, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, Buffer as WgpuBuffer, Device, Extent3d, MultisampleState, Queue, RenderPass,
    Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

use super::constants::{TEXT_BUFFER_MARGIN_MULTIPLIER, TEXT_TEXTURE_PADDING};
use super::gpu::NO_CLIP_RECT;
use super::textured_quad::TexturedQuadPipeline;
use super::textured_vertex::{TexturedVertex, to_ndc};
use super::types::TextEntry;
use crate::widgets::font::FontWeight;

/// Quality multiplier for supersampling text textures.
const QUALITY_MULTIPLIER: f32 = 2.0;

/// Margin multiplier is imported from constants
const TEXT_MARGIN: f32 = TEXT_BUFFER_MARGIN_MULTIPLIER;

/// A prepared text quad ready for rendering.
pub struct PreparedTextQuad {
    cached: std::rc::Rc<CachedTextTexture>,
    /// Vertex buffer with pre-computed vertices in NDC
    vertex_buffer: WgpuBuffer,
}

/// A rasterized text texture, reused across frames.
///
/// Rasterizing text is the expensive step: shaping, an offscreen texture,
/// a render pass and a bind group per text. Without caching, every frame
/// re-created all of it for every visible text — content-heavy surfaces
/// (a weather menu, long lists) reached >1s of GPU work per frame and
/// never got a frame out.
struct CachedTextTexture {
    #[allow(dead_code)] // Kept alive for GPU usage
    texture: Texture,
    bind_group: BindGroup,
    last_used: std::cell::Cell<u64>,
}

/// Everything that affects the rasterized pixels of a text texture.
#[derive(PartialEq, Eq, Hash)]
struct TextCacheKey {
    text: String,
    font_size_bits: u32,
    weight: u16,
    family: crate::widgets::FontFamily,
    color: [u8; 4],
    tex_width: u32,
    tex_height: u32,
}

/// Renderer for transformed text as textured quads.
pub struct TextQuadRenderer {
    // Rasterized text textures reused across frames
    text_cache: std::collections::HashMap<TextCacheKey, std::rc::Rc<CachedTextTexture>>,
    frame_gen: u64,
    // Text rendering (glyphon-based)
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Kept alive for text rendering
    cache: Cache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,

    /// Pipeline, layout, sampler and index buffer — shared with the image
    /// renderer, which draws the same quad from a different texture.
    quad: TexturedQuadPipeline,

    // Texture format
    format: TextureFormat,

    // Screen dimensions for NDC conversion
    screen_width: f32,
    screen_height: f32,
}

impl TextQuadRenderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        // Initialize text rendering components
        let mut font_system = FontSystem::new();
        for data in crate::get_registered_fonts() {
            font_system
                .db_mut()
                .load_font_source(glyphon::fontdb::Source::Binary(data));
        }
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::with_color_mode(device, queue, &cache, format, ColorMode::Web);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        Self {
            quad: TexturedQuadPipeline::new(device, format, "TextQuad"),
            font_system,
            text_cache: std::collections::HashMap::new(),
            frame_gen: 0,
            swash_cache,
            cache,
            atlas,
            text_renderer,
            viewport,
            format,
            screen_width: 800.0,
            screen_height: 600.0,
        }
    }

    /// Update screen dimensions for NDC conversion.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Prepare text entries for rendering as textured quads.
    pub fn prepare(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        entries: &[TextEntry],
        indices: &[usize],
        scale_factor: f32,
    ) -> Vec<PreparedTextQuad> {
        self.frame_gen += 1;
        let quads = indices
            .iter()
            .map(|&idx| {
                let entry = &entries[idx];
                self.render_text_to_quad(device, queue, entry, scale_factor)
            })
            .collect();
        // Bounded cache: on overflow keep only the textures this frame used
        if self.text_cache.len() > 512 {
            let current = self.frame_gen;
            self.text_cache.retain(|_, c| c.last_used.get() == current);
        }
        quads
    }

    /// Render a single text entry to a textured quad.
    fn render_text_to_quad(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        entry: &TextEntry,
        scale_factor: f32,
    ) -> PreparedTextQuad {
        // Rasterize at fixed resolution: scale_factor * QUALITY_MULTIPLIER.
        // The transform's scale/rotation is applied via GPU quad vertices, not baked into the texture.
        // This prevents atlas churn during scale animations (each frame would otherwise create new entries).
        let effective_scale = scale_factor * QUALITY_MULTIPLIER;

        // Scale font size for crisp rendering
        let scaled_font_size = entry.font_size * effective_scale;

        // Texture dimensions are deterministic from the entry, so they can
        // key the cache before any shaping happens
        let buffer_width = entry.rect.width * effective_scale * TEXT_MARGIN;
        let buffer_height = entry.rect.height * effective_scale * TEXT_MARGIN;
        let padding = TEXT_TEXTURE_PADDING * effective_scale;
        let tex_width = ((buffer_width + padding * 2.0).ceil() as u32).max(1);
        let tex_height = ((buffer_height + padding * 2.0).ceil() as u32).max(1);

        let weight = if entry.font_weight == FontWeight::default() {
            FontWeight::NORMAL
        } else {
            entry.font_weight
        };

        let cache_key = TextCacheKey {
            text: entry.text.clone(),
            font_size_bits: scaled_font_size.to_bits(),
            weight: weight.0,
            family: entry.font_family.clone(),
            color: [
                (entry.color.r * 255.0) as u8,
                (entry.color.g * 255.0) as u8,
                (entry.color.b * 255.0) as u8,
                (entry.color.a * 255.0) as u8,
            ],
            tex_width,
            tex_height,
        };

        if let Some(cached) = self.text_cache.get(&cache_key) {
            cached.last_used.set(self.frame_gen);
            let cached = cached.clone();
            return self.build_quad(
                device,
                entry,
                cached,
                tex_width,
                tex_height,
                padding,
                scale_factor,
            );
        }

        // Cache miss: shape and rasterize
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(scaled_font_size, scaled_font_size * 1.2),
        );
        buffer.set_size(
            &mut self.font_system,
            Some(buffer_width),
            Some(buffer_height),
        );
        buffer.set_text(
            &mut self.font_system,
            &entry.text,
            &Attrs::new()
                .family(entry.font_family.to_cosmic())
                .weight(weight.to_cosmic()),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, true);

        // Create offscreen texture
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Text Texture"),
            size: Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Update viewport
        self.viewport.update(
            queue,
            Resolution {
                width: tex_width,
                height: tex_height,
            },
        );

        // Create text area
        let text_area = TextArea {
            buffer: &buffer,
            left: padding,
            top: padding,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: tex_width as i32,
                bottom: tex_height as i32,
            },
            default_color: GlyphonColor::rgba(
                (entry.color.r * 255.0) as u8,
                (entry.color.g * 255.0) as u8,
                (entry.color.b * 255.0) as u8,
                (entry.color.a * 255.0) as u8,
            ),
            custom_glyphs: &[],
        };

        // Prepare and render text to texture
        if let Err(e) = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            vec![text_area],
            &mut self.swash_cache,
        ) {
            log::error!("Text texture prepare failed: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Texture Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Texture Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut render_pass)
                .expect("Failed to render text to texture");
        }

        queue.submit(std::iter::once(encoder.finish()));

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Texture Bind Group"),
            layout: &self.quad.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.quad.sampler),
                },
            ],
        });

        let cached = std::rc::Rc::new(CachedTextTexture {
            texture,
            bind_group,
            last_used: std::cell::Cell::new(self.frame_gen),
        });
        self.text_cache.insert(cache_key, cached.clone());
        self.build_quad(
            device,
            entry,
            cached,
            tex_width,
            tex_height,
            padding,
            scale_factor,
        )
    }

    /// Build the per-frame quad (vertices depend on position/transform, the
    /// texture itself is cached).
    #[allow(clippy::too_many_arguments)]
    fn build_quad(
        &self,
        device: &Arc<Device>,
        entry: &TextEntry,
        cached: std::rc::Rc<CachedTextTexture>,
        tex_width: u32,
        tex_height: u32,
        padding: f32,
        scale_factor: f32,
    ) -> PreparedTextQuad {
        // The entry.rect is in LOCAL coordinates. We need to apply the world_transform
        // to get screen coordinates. The world_transform already includes everything:
        // parent translations, rotations, scales, and center_at adjustments.
        //
        // The texture was rendered at (scale_factor * QUALITY_MULTIPLIER) resolution.
        // We divide by QUALITY_MULTIPLIER to get logical-pixel dimensions, then the
        // world_transform applies scaling/rotation via transform_point() on quad corners.

        // Calculate display size: divide by QUALITY_MULTIPLIER only.
        // The texture is rendered at (scale_factor * QUALITY_MULTIPLIER) resolution.
        // The transform's scale is applied by transform_point() on quad corners, not here.
        let total_scale = QUALITY_MULTIPLIER;
        let display_width = tex_width as f32 / total_scale;
        let display_height = tex_height as f32 / total_scale;

        // Padding in logical pixels
        let display_padding = padding / total_scale;

        // Get the local rect corners with padding adjustment
        // The texture has padding, so we need to expand the display rect accordingly
        let local_left = entry.rect.x - display_padding / scale_factor;
        let local_top = entry.rect.y - display_padding / scale_factor;
        let local_right = local_left + display_width / scale_factor;
        let local_bottom = local_top + display_height / scale_factor;

        // Local corners of the text area (with padding included)
        let local_corners = [
            (local_left, local_top),     // top-left
            (local_right, local_top),    // top-right
            (local_left, local_bottom),  // bottom-left
            (local_right, local_bottom), // bottom-right
        ];

        // Apply the full world_transform to get screen coordinates (logical)
        // Then multiply by scale_factor to get physical pixels
        let screen_corners: Vec<(f32, f32)> = local_corners
            .iter()
            .map(|&(x, y)| {
                let (sx, sy) = entry.transform.transform_point(x, y);
                (sx * scale_factor, sy * scale_factor)
            })
            .collect();

        // Extract clip data (scale to physical pixels)
        // Note: entry.clip_rect is in logical pixels (world coordinates)
        let (clip_rect, clip_params) = if let Some(ref clip) = entry.clip_rect {
            (
                [
                    clip.x * scale_factor,
                    clip.y * scale_factor,
                    clip.width * scale_factor,
                    clip.height * scale_factor,
                ],
                [0.0, 1.0, 0.0, 0.0], // No corner radius for text clip (uses rect from TextEntry)
            )
        } else {
            // No clipping
            (NO_CLIP_RECT, [0.0, 1.0, 0.0, 0.0])
        };

        // Convert to NDC and create vertices with clip data
        let vertices = [
            TexturedVertex {
                position: to_ndc(
                    screen_corners[0].0,
                    screen_corners[0].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [0.0, 0.0],
                screen_pos: [screen_corners[0].0, screen_corners[0].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[1].0,
                    screen_corners[1].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [1.0, 0.0],
                screen_pos: [screen_corners[1].0, screen_corners[1].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[2].0,
                    screen_corners[2].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [0.0, 1.0],
                screen_pos: [screen_corners[2].0, screen_corners[2].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[3].0,
                    screen_corners[3].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [1.0, 1.0],
                screen_pos: [screen_corners[3].0, screen_corners[3].1],
                clip_rect,
                clip_params,
            },
        ];

        // Create vertex buffer with the vertices already initialized
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TextQuad Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        PreparedTextQuad {
            cached,
            vertex_buffer,
        }
    }

    /// Render the prepared text quads.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>, quads: &'a [PreparedTextQuad]) {
        if quads.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.quad.pipeline);
        render_pass.set_index_buffer(self.quad.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for quad in quads {
            render_pass.set_bind_group(0, &quad.cached.bind_group, &[]);
            render_pass.set_vertex_buffer(0, quad.vertex_buffer.slice(..));
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }
}
