use std::collections::HashSet;

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, ColorMode, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue};

use crate::widgets::font::FontWeight;

use super::types::TextEntry;

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
            // Skip text invisible due to zero scale — avoids all rendering work
            if !entry.transform.is_identity() && !entry.transform.is_translation_only() {
                let (sx, sy) = (entry.transform.data[0], entry.transform.data[5]);
                if sx.abs() < 1e-3 || sy.abs() < 1e-3 {
                    culled_indices.insert(idx);
                    continue;
                }
            }

            // Skip text that is completely outside its clip region (culling optimization)
            // Check this FIRST so culled texts don't get rendered via texture path either
            if let Some(clip) = &entry.clip_rect {
                // Get text bounding box in world space (same coordinate system as clip rect)
                let (p1x, p1y) = entry.transform.transform_point(entry.rect.x, entry.rect.y);
                let (p2x, p2y) = entry.transform.transform_point(
                    entry.rect.x + entry.rect.width,
                    entry.rect.y + entry.rect.height,
                );
                let text_left = p1x.min(p2x);
                let text_top = p1y.min(p2y);
                let text_right = p1x.max(p2x);
                let text_bottom = p1y.max(p2y);

                let clip_right = clip.x + clip.width;
                let clip_bottom = clip.y + clip.height;

                // Check if text is completely outside clip region
                // Use small epsilon to avoid floating point precision issues at boundaries
                let epsilon = 0.1;
                let outside = text_right < clip.x - epsilon
                    || text_left > clip_right + epsilon
                    || text_bottom < clip.y - epsilon
                    || text_top > clip_bottom + epsilon;

                if outside {
                    // Text is completely outside clip - skip it entirely
                    culled_indices.insert(idx);
                    continue;
                }
            }

            // Route all non-translation transforms (rotation, scale) to TextQuadRenderer.
            // This keeps the glyphon atlas stable — only identity/translation text goes through it.
            if !entry.transform.is_identity() && !entry.transform.is_translation_only() {
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
                // Position text in world space using the full transform.
                // This is consistent with the clip rect (also in world space via
                // transform_clip_to_world), so TextBounds clipping works correctly.
                let (screen_x, screen_y) =
                    entry.transform.transform_point(entry.rect.x, entry.rect.y);

                // Scale positions for HiDPI rendering
                let scaled_left = screen_x * scale_factor;
                let scaled_top = screen_y * scale_factor;

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
