use std::collections::HashSet;

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue};

use super::TextEntry;
use crate::widgets::font::FontWeight;

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

    /// Prepare non-transformed text for rendering directly to screen.
    /// Returns a list of indices of texts that have transforms and need special handling.
    pub fn prepare_text(
        &mut self,
        device: &Device,
        queue: &Queue,
        texts: &[TextEntry],
        screen_width: u32,
        screen_height: u32,
        scale_factor: f32,
    ) -> Vec<usize> {
        self.buffers.clear();

        // Collect indices of texts that have non-trivial transforms (for texture-based rendering)
        let mut transformed_indices = Vec::new();
        // Collect indices of texts that are completely outside their clip region (to skip entirely)
        let mut culled_indices = HashSet::new();

        for (idx, entry) in texts.iter().enumerate() {
            // Skip text that is completely outside its clip region (culling optimization)
            // Check this FIRST so culled texts don't get rendered via texture path either
            if let Some(clip) = &entry.clip_rect {
                // Get text position (applying translation if any)
                let (tx, ty) = if entry.transform.is_identity() {
                    (0.0, 0.0)
                } else {
                    (entry.transform.tx(), entry.transform.ty())
                };
                let text_left = entry.rect.x + tx;
                let text_top = entry.rect.y + ty;
                let text_right = text_left + entry.rect.width;
                let text_bottom = text_top + entry.rect.height;

                let clip_right = clip.x + clip.width;
                let clip_bottom = clip.y + clip.height;

                // Check if text is completely outside clip region
                let outside = text_right <= clip.x
                    || text_left >= clip_right
                    || text_bottom <= clip.y
                    || text_top >= clip_bottom;

                if outside {
                    // Text is completely outside clip - skip it entirely
                    culled_indices.insert(idx);
                    continue;
                }
            }

            // Check if text has a transform that affects rendering
            if entry.transform.has_rotation_or_scale() {
                transformed_indices.push(idx);
                continue; // Skip transformed text in direct rendering
            }

            // Scale the font size for HiDPI rendering
            let scaled_font_size = entry.font_size * scale_factor;

            let mut buffer = Buffer::new(
                &mut self.font_system,
                Metrics::new(scaled_font_size, scaled_font_size * 1.2),
            );
            // Give more space for the text buffer, scaled
            buffer.set_size(
                &mut self.font_system,
                Some((entry.rect.width.max(200.0)) * scale_factor),
                Some((entry.rect.height.max(50.0)) * scale_factor),
            );
            // Use entry's font properties, defaulting to NORMAL weight if default (0)
            let weight = if entry.font_weight == FontWeight::default() {
                FontWeight::NORMAL
            } else {
                entry.font_weight
            };
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
            self.buffers.push(buffer);
        }

        // Filter to only non-transformed, non-culled texts for TextArea creation
        // Use HashSet for O(1) lookup instead of Vec::contains which is O(n)
        let transformed_set: HashSet<_> = transformed_indices.iter().copied().collect();
        let non_transformed_texts: Vec<_> = texts
            .iter()
            .enumerate()
            .filter(|(idx, _)| !transformed_set.contains(idx) && !culled_indices.contains(idx))
            .map(|(_, entry)| entry)
            .collect();

        let text_areas: Vec<TextArea> = non_transformed_texts
            .iter()
            .zip(self.buffers.iter())
            .map(|(entry, buffer)| {
                // Apply translation from transform (if any) to position
                let (tx, ty) = if entry.transform.is_identity() {
                    (0.0, 0.0)
                } else {
                    (entry.transform.tx(), entry.transform.ty())
                };

                // Scale positions for HiDPI rendering
                let scaled_left = (entry.rect.x + tx) * scale_factor;
                let scaled_top = (entry.rect.y + ty) * scale_factor;

                // Use clip rect if provided, otherwise use full screen
                // Clip bounds stay in screen space - don't apply transform translation
                // (text position is transformed, but clip region should remain fixed)
                let bounds = if let Some(clip_rect) = &entry.clip_rect {
                    TextBounds {
                        left: (clip_rect.x * scale_factor) as i32,
                        top: (clip_rect.y * scale_factor) as i32,
                        right: ((clip_rect.x + clip_rect.width) * scale_factor) as i32,
                        bottom: ((clip_rect.y + clip_rect.height) * scale_factor) as i32,
                    }
                } else {
                    TextBounds {
                        left: 0,
                        top: 0,
                        right: screen_width as i32,
                        bottom: screen_height as i32,
                    }
                };

                TextArea {
                    buffer,
                    left: scaled_left,
                    top: scaled_top,
                    scale: 1.0, // Buffer is already scaled, no additional scaling needed
                    bounds,
                    default_color: GlyphonColor::rgba(
                        (entry.color.r * 255.0) as u8,
                        (entry.color.g * 255.0) as u8,
                        (entry.color.b * 255.0) as u8,
                        (entry.color.a * 255.0) as u8,
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

        transformed_indices
    }

    pub fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, _device: &Device) {
        self.text_renderer
            .render(&self.atlas, &self.viewport, pass)
            .expect("Failed to render text");
    }
}
