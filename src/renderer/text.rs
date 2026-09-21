use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, ColorMode, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue};

use crate::widgets::Rect;
use crate::widgets::font::FontWeight;

use super::types::TextEntry;

/// Compute a cache key for a text buffer based on content and styling.
/// Two entries with the same key produce identical glyphon Buffers.
fn text_buffer_key(entry: &TextEntry, scale_factor: f32) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    entry.text.hash(&mut hasher);
    (entry.font_size * scale_factor).to_bits().hash(&mut hasher);
    entry.font_weight.hash(&mut hasher);
    entry.font_family.hash(&mut hasher);
    ((entry.rect.width.max(200.0)) * scale_factor)
        .to_bits()
        .hash(&mut hasher);
    ((entry.rect.height.max(50.0)) * scale_factor)
        .to_bits()
        .hash(&mut hasher);
    hasher.finish()
}

/// The buffer glyphon is given to shape a text of this layout box in, in
/// physical pixels.
///
/// The floors are glyphon's: a buffer narrower than the text it holds wraps it,
/// and a measured box can come back a hair narrower than the thing it measured.
///
/// It is a function and not four inline multiplications because a text's frost
/// is shaped separately from the text — see [`text_mask`](super::text_mask) —
/// and two shapings that disagree break their lines in different places, which
/// puts frost beside a letter that wrapped somewhere else. The sibling for the
/// transformed path is [`text_quad::shaping_buffer`](super::text_quad::shaping_buffer).
pub(super) fn shaping_buffer(rect: crate::widgets::Rect, scale: f32) -> (f32, f32) {
    (rect.width.max(200.0) * scale, rect.height.max(50.0) * scale)
}

pub struct TextRenderState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Used for viewport and atlas construction
    cache: Cache,
    atlas: TextAtlas,
    /// One renderer per draw group that contains text.
    ///
    /// glyphon draws everything a `TextRenderer` prepared in a single call,
    /// so text that must appear between two other groups needs a renderer of
    /// its own. Grown on demand and reused across frames; a UI with one group
    /// keeps exactly one, as before.
    text_renderers: Vec<TextRenderer>,
    buffers: Vec<Buffer>,
    viewport: Viewport,
    /// Cache of shaped text buffers from the previous frame, keyed by
    /// content+style hash. Avoids expensive Unicode analysis and glyph
    /// shaping for unchanged text. Multi-valued: the same label can appear
    /// several times in a frame (e.g. repeated list items) — with a
    /// single-entry cache the first occurrence consumed the only buffer and
    /// every duplicate re-shaped from scratch, every frame, forever.
    buffer_cache: HashMap<u64, Vec<Buffer>>,
    /// Keys for current frame's buffers (parallel to `self.buffers`), used to
    /// repopulate `buffer_cache` at the start of the next frame.
    frame_keys: Vec<u64>,
}

/// A text's layout box, in world pixels.
///
/// What the culling check has always compared against a clip, lifted out so
/// the routing question below can ask it too. Not outset: widening it here
/// would widen what gets culled, and the two questions want different
/// margins — see the call that adds one.
fn laid_out_in(entry: &TextEntry) -> Rect {
    let (p1x, p1y) = entry.transform.transform_point(entry.rect.x, entry.rect.y);
    let (p2x, p2y) = entry.transform.transform_point(
        entry.rect.x + entry.rect.width,
        entry.rect.y + entry.rect.height,
    );
    Rect::new(
        p1x.min(p2x),
        p1y.min(p2y),
        (p2x - p1x).abs(),
        (p2y - p1y).abs(),
    )
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
            text_renderers: vec![text_renderer],
            buffers: Vec::new(),
            viewport,
            buffer_cache: HashMap::new(),
            frame_keys: Vec::new(),
        }
    }

    /// Start a frame: recycle last frame's shaped buffers into the cache.
    ///
    /// Split out of [`prepare_layer`](Self::prepare_layer) because a frame may
    /// prepare several groups and the recycling has to happen once for all.
    pub fn begin_frame(&mut self, queue: &Queue, screen: (u32, u32)) {
        for (key, buffer) in self.frame_keys.drain(..).zip(self.buffers.drain(..)) {
            self.buffer_cache.entry(key).or_default().push(buffer);
        }
        // Once per frame, not once per group: every group renders into the
        // same viewport.
        self.viewport.update(
            queue,
            Resolution {
                width: screen.0,
                height: screen.1,
            },
        );
    }

    /// Drop shaped buffers no group asked for. Call once every group has been
    /// prepared — whatever is still cached went unused this frame.
    pub fn end_frame(&mut self) {
        self.buffer_cache.clear();
    }

    /// Prepare one draw group's non-transformed text.
    ///
    /// `slot` selects the renderer, and with it when this text lands relative
    /// to the other groups; see [`render_slot`](Self::render_slot).
    /// Returns the indices, within `texts`, that need the textured-quad path.
    pub fn prepare_layer(
        &mut self,
        slot: usize,
        device: &Device,
        queue: &Queue,
        texts: &[TextEntry],
        screen: (u32, u32),
        scale_factor: f32,
    ) -> Vec<usize> {
        let (screen_width, screen_height) = screen;
        while self.text_renderers.len() <= slot {
            self.text_renderers.push(TextRenderer::new(
                &mut self.atlas,
                device,
                MultisampleState::default(),
                None,
            ));
        }
        // Buffers accumulate across the frame's groups; this is where this
        // group's own shaped buffers begin.
        let buffers_start = self.buffers.len();

        // Collect indices of texts that have non-trivial transforms (for texture-based rendering)
        let mut transformed_indices = Vec::new();
        // Collect indices of texts that are completely outside their clip region (to skip entirely)
        let mut culled_indices = HashSet::new();

        for (idx, entry) in texts.iter().enumerate() {
            // Text scaled to nothing is skipped before any work is done for
            // it. The scale comes from the row norms, not from the diagonal:
            // `a` and `d` are both `cos θ` for a rotation, so reading them as
            // the scale calls a quarter turn a collapse and throws the text
            // away at exactly 90° and 270° — and only there, which is why it
            // survived so long. `extract_scale_components` is what the rest of
            // the renderer already asks.
            if !entry.transform.is_identity() && !entry.transform.is_translation_only() {
                let (sx, sy) = entry.transform.extract_scale_components();
                if sx.abs() < 1e-3 || sy.abs() < 1e-3 {
                    culled_indices.insert(idx);
                    continue;
                }
            }

            // Skip text that is completely outside its clip region (culling optimization)
            // Check this FIRST so culled texts don't get rendered via texture path either
            let laid_out = laid_out_in(entry);
            if let Some(clip) = &entry.clip.map(|c| c.world_aabb()) {
                let text_left = laid_out.x;
                let text_top = laid_out.y;
                let text_right = laid_out.x + laid_out.width;
                let text_bottom = laid_out.y + laid_out.height;

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
            //
            // And anything glyphon cannot cut. `TextBounds` is four integers,
            // so the only clip it can honour is an upright rectangle: a turned
            // one, or a corner this text actually reaches into, has to be
            // tested in the clip's own space, and the quad is what does that
            // (#405). Asked of where the two answers *differ* rather than
            // whether they can, because a quad costs a rasterization and an
            // upload per text — a label in the middle of a rounded card is cut
            // the same by the box and stays with glyphon.
            let moved = !entry.transform.is_identity() && !entry.transform.is_translation_only();
            // Glyphs overshoot the box they were laid out in — descenders,
            // italics, accents — and ink that overshoots into a corner is ink
            // the box would not have cut. Half the font size is the slack
            // `command_to_text_backdrop` allows for the same reason, and it
            // errs the safe way here: too generous sends a text down the quad
            // path the box would have cut correctly, costing a rasterization;
            // too tight leaves ink outside its clip, which is the defect.
            let ink = laid_out.outset(entry.font_size * 0.5);
            let box_will_not_do = entry
                .clip
                .is_some_and(|clip| !clip.box_cuts_like_the_shape(ink));
            if moved || box_will_not_do {
                transformed_indices.push(idx);
                continue; // Skip transformed text in direct rendering
            }

            // Check buffer cache before expensive text shaping
            let key = text_buffer_key(entry, scale_factor);
            let cached = self
                .buffer_cache
                .get_mut(&key)
                .and_then(|buffers| buffers.pop());
            let buffer = if let Some(cached) = cached {
                cached
            } else {
                // Cache miss — create and shape a new buffer
                let scaled_font_size = entry.font_size * scale_factor;
                let (size, line_height) =
                    crate::renderer::text_measurer::shapeable_metrics(scaled_font_size);
                let mut buffer =
                    Buffer::new(&mut self.font_system, Metrics::new(size, line_height));
                let (buffer_width, buffer_height) = shaping_buffer(entry.rect, scale_factor);
                buffer.set_size(
                    &mut self.font_system,
                    Some(buffer_width),
                    Some(buffer_height),
                );
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
                buffer
            };
            self.frame_keys.push(key);
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

        // Destructured so the text areas can borrow the buffers immutably
        // while `prepare` takes the font system and atlas mutably.
        let Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            buffers,
            text_renderers,
            ..
        } = self;

        let text_areas: Vec<TextArea> = non_transformed_texts
            .iter()
            .zip(buffers[buffers_start..].iter())
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
                // The box around the clip, because `TextBounds` is four
                // integers and glyphon has nowhere to put a shape. That is
                // exact here rather than approximate: a text this box would
                // cut differently from the shape never reaches this point —
                // `box_cuts_like_the_shape` sent it down the quad path
                // instead, which tests in the clip's own space (#405).
                let bounds = if let Some(clip_rect) = &entry.clip.map(|c| c.world_aabb()) {
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

        let result = text_renderers[slot].prepare(
            device,
            queue,
            font_system,
            atlas,
            viewport,
            text_areas,
            swash_cache,
        );

        if let Err(e) = result {
            log::error!("Text prepare failed: {:?}", e);
        }

        transformed_indices
    }

    /// Draw the text prepared into `slot`.
    pub fn render_slot<'a>(&'a self, slot: usize, pass: &mut wgpu::RenderPass<'a>) {
        if let Some(renderer) = self.text_renderers.get(slot) {
            renderer
                .render(&self.atlas, &self.viewport, pass)
                .expect("Failed to render text");
        }
    }
}

#[cfg(test)]
mod shaping_buffer_tests {
    use super::shaping_buffer;
    use crate::widgets::Rect;

    /// The floors are in logical pixels and the scale is applied after them, so
    /// a small box on a HiDPI screen gets the floor at that screen's density
    /// rather than a floor that shrinks as the screen gets finer.
    #[test]
    fn the_floor_is_logical_and_the_scale_comes_after_it() {
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 50.0, 10.0), 1.0),
            (200.0, 50.0)
        );
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 50.0, 10.0), 2.0),
            (400.0, 100.0)
        );
    }

    /// Above the floors the buffer is the box, scaled — and scaled, not offset,
    /// which is the whole of what makes it agree with a glyph drawn at that
    /// scale.
    #[test]
    fn a_box_above_the_floor_is_carried_by_the_scale_alone() {
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 1.0),
            (300.0, 80.0)
        );
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 2.0),
            (600.0, 160.0)
        );
        // The floor is logical, so a scale below 1 can carry the buffer under
        // it: 80 clears the floor of 50 and then halves to 40.
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 0.5),
            (150.0, 40.0)
        );
    }

    /// The two rules are not the same rule, which is why a coverage mask has to
    /// pick one rather than assume. If this ever passes, `text_mask` no longer
    /// has a choice to get wrong — and the comment saying it does is stale.
    #[test]
    fn the_transformed_shaper_asks_for_something_else() {
        let rect = Rect::new(0.0, 0.0, 72.0, 80.0);
        assert_ne!(
            shaping_buffer(rect, 4.0),
            super::super::text_quad::shaping_buffer(rect, 4.0),
            "a 72-point box: 400 texels through this one and 317 through the other"
        );
    }
}
