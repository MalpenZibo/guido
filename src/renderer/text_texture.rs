//! Text-to-texture rendering for transformed text.
//!
//! This module renders text to offscreen textures, which can then be
//! displayed as transformed quads. This allows text to follow parent
//! container transforms (rotation, scale, etc.) while maintaining
//! crisp rendering quality.

use std::sync::Arc;

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{
    Device, Extent3d, MultisampleState, Queue, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView,
};

use super::TextEntry;

/// A rendered text texture ready for display as a transformed quad.
pub struct TextTexture {
    /// The GPU texture containing the rendered text
    pub texture: Texture,
    /// View for sampling the texture
    pub view: TextureView,
    /// The original text entry this was rendered from
    pub entry: TextEntry,
    /// Width of the texture in pixels
    pub width: u32,
    /// Height of the texture in pixels
    pub height: u32,
    /// The scale factor used when rendering (for crisp text)
    pub render_scale: f32,
}

/// Renders text to offscreen textures for transform support.
pub struct TextTextureRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Required for atlas and viewport lifetime
    cache: Cache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    format: TextureFormat,
}

impl TextTextureRenderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            cache,
            atlas,
            text_renderer,
            viewport,
            format,
        }
    }

    /// Render a text entry to an offscreen texture.
    ///
    /// The text is rendered at `font_size * transform_scale * scale_factor * quality_multiplier`
    /// for crisp quality when the texture is displayed with the transform applied.
    pub fn render_to_texture(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        entry: &TextEntry,
        scale_factor: f32,
    ) -> TextTexture {
        // Quality multiplier for supersampling - renders text at higher resolution
        // for better quality when displayed. 2.0 provides good quality without
        // excessive memory usage.
        const QUALITY_MULTIPLIER: f32 = 2.0;

        // Extract the scale from the transform for crisp rendering
        let transform_scale = entry.transform.extract_scale().max(1.0);

        // Calculate the effective scale for text rendering (includes quality multiplier)
        let effective_scale = scale_factor * transform_scale * QUALITY_MULTIPLIER;

        // Scale font size for crisp rendering at the transformed size
        let scaled_font_size = entry.font_size * effective_scale;

        // Create a buffer for text measurement and rendering
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(scaled_font_size, scaled_font_size * 1.2),
        );

        // Set buffer size to match the entry rect (no arbitrary minimums that would offset text)
        let buffer_width = entry.rect.width * effective_scale;
        let buffer_height = entry.rect.height * effective_scale;

        buffer.set_size(
            &mut self.font_system,
            Some(buffer_width),
            Some(buffer_height),
        );
        buffer.set_text(
            &mut self.font_system,
            &entry.text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, true);

        // Use the original rect dimensions for the texture size to preserve layout centering.
        // The text will be rendered at top-left (with padding), matching glyphon's behavior.
        let padding = 4.0 * effective_scale;
        let tex_width = ((buffer_width + padding * 2.0).ceil() as u32).max(1);
        let tex_height = ((buffer_height + padding * 2.0).ceil() as u32).max(1);

        // Create the offscreen texture
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

        // Update viewport for this texture size
        self.viewport.update(
            queue,
            Resolution {
                width: tex_width,
                height: tex_height,
            },
        );

        // Create text area for rendering
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

        // Prepare text for rendering
        let result = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            vec![text_area],
            &mut self.swash_cache,
        );

        if let Err(e) = result {
            log::error!("Text texture prepare failed: {:?}", e);
        }

        // Create encoder and render to texture
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
                        // Clear to transparent
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

            // Render the text
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut render_pass)
                .expect("Failed to render text to texture");
        }

        queue.submit(std::iter::once(encoder.finish()));

        TextTexture {
            texture,
            view,
            entry: entry.clone(),
            width: tex_width,
            height: tex_height,
            render_scale: effective_scale,
        }
    }
}
