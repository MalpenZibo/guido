use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue};

use crate::widgets::{Color, Rect};

pub struct TextRenderState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Used for viewport and atlas construction
    cache: Cache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    buffers: Vec<Buffer>,
    viewport: Viewport,
}

impl TextRenderState {
    pub fn new(device: &Device, queue: &Queue, format: wgpu::TextureFormat) -> Self {
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
            buffers: Vec::new(),
            viewport,
        }
    }

    pub fn prepare_text(
        &mut self,
        device: &Device,
        queue: &Queue,
        texts: &[(String, Rect, Color, f32)],
        screen_width: u32,
        screen_height: u32,
        scale_factor: f32,
    ) {
        self.buffers.clear();

        log::trace!(
            "prepare_text: screen={}x{}, scale={}, texts={}",
            screen_width,
            screen_height,
            scale_factor,
            texts.len()
        );

        for (text, rect, _color, font_size) in texts {
            // Use logical font size - glyphon will scale via TextArea.scale parameter
            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(*font_size, *font_size * 1.2),
            );
            // Use logical dimensions - glyphon will scale
            buffer.set_size(
                &mut self.font_system,
                Some(rect.width.max(200.0)),
                Some(rect.height.max(50.0)),
            );
            buffer.set_text(
                &mut self.font_system,
                text,
                &Attrs::new().family(Family::SansSerif),
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut self.font_system, true);
            self.buffers.push(buffer);
        }

        let text_areas: Vec<TextArea> = texts
            .iter()
            .zip(self.buffers.iter())
            .map(|((_text, rect, color, _), buffer)| {
                // Use logical positions with small offset for font metrics
                let left_offset = 2.0;
                let left = rect.x + left_offset;
                let top = rect.y;
                TextArea {
                    buffer,
                    left,
                    top,
                    scale: scale_factor, // Let glyphon handle the HiDPI scaling
                    bounds: TextBounds {
                        left: 0,
                        top: 0,
                        right: screen_width as i32,
                        bottom: screen_height as i32,
                    },
                    default_color: GlyphonColor::rgba(
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        (color.a * 255.0) as u8,
                    ),
                    custom_glyphs: &[],
                }
            })
            .collect();

        // Update viewport with current screen dimensions
        self.viewport.update(
            queue,
            Resolution {
                width: screen_width,
                height: screen_height,
            },
        );

        let result = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            text_areas,
            &mut self.swash_cache,
        );

        if let Err(e) = result {
            log::error!("Text prepare failed: {:?}", e);
        }
    }

    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, _device: &Device) {
        self.text_renderer
            .render(&self.atlas, &self.viewport, pass)
            .expect("Failed to render text");
    }
}
