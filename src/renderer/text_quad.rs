//! Textured quad rendering for transformed text.
//!
//! This module renders text to offscreen textures and displays them as
//! transformed quads. This is used when text has rotation or scale transforms
//! that glyphon cannot handle directly.
//!
//! Vertex positions are computed on the CPU and passed as pre-computed
//! NDC coordinates to the shader.

use std::sync::Arc;

use glyphon::{Color as GlyphonColor, Resolution, TextArea, TextBounds};
use wgpu::{
    BindGroup, Device, Extent3d, Queue, RenderPass, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages,
};

use super::constants::{TEXT_BUFFER_MARGIN_MULTIPLIER, TEXT_SUPERSAMPLE, TEXT_TEXTURE_PADDING};
use super::layer_blur::IDLE_FRAMES_BEFORE_RELEASE;
use super::layer_pass::ANY_SURFACE;
use super::text_mask::Shaper;
use super::textured_quad::{QuadDraw, TexturedQuadPipeline};
use super::textured_vertex::{NO_TINT, QuadClip, TexturedVertex};
use super::tree::NodeId;
use super::types::TextEntry;
use crate::render_stats::Pipeline;
use crate::widgets::{Rect, TextAlign};

/// How many rasterized texts the cache holds before a frame keeps only its
/// own, whatever their idle frames say.
const CACHE_BOUND: usize = 512;

/// Margin multiplier is imported from constants
const TEXT_MARGIN: f32 = TEXT_BUFFER_MARGIN_MULTIPLIER;

/// The height a transformed text is shaped in when it is not cut, in texture
/// pixels — `scale_factor * TEXT_SUPERSAMPLE` of them per logical pixel: the
/// room its texture has, the box widened by [`TEXT_MARGIN`].
///
/// Its width is no business of this path's. A text is shaped in the width it
/// was laid out in, as every path shapes it — see
/// [`text::line_width`](super::text::line_width) — and placed in its box by
/// [`text::paint_offset`](super::text::paint_offset). The untransformed path's
/// height is [`text::buffer_height`](super::text::buffer_height), and a frosted
/// text's mask is shaped in whichever of the two draws its letters — see
/// [`text_mask`](super::text_mask).
pub(super) fn buffer_height(rect: Rect, effective_scale: f32) -> f32 {
    texture_room(rect, effective_scale).1
}

/// The room a transformed text is rasterized into, before padding: its box
/// widened by [`TEXT_MARGIN`].
fn texture_room(rect: Rect, effective_scale: f32) -> (f32, f32) {
    (
        rect.width * effective_scale * TEXT_MARGIN,
        rect.height * effective_scale * TEXT_MARGIN,
    )
}

/// A prepared text quad ready for rendering: the texture it samples, and
/// where its corners are among the frame's quads.
pub struct PreparedTextQuad {
    cached: std::rc::Rc<CachedTextTexture>,
    quad: u32,
}

impl QuadDraw for PreparedTextQuad {
    fn bind_group(&self) -> &std::rc::Rc<BindGroup> {
        &self.cached.bind_group
    }

    fn quad(&self) -> u32 {
        self.quad
    }
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
    bind_group: std::rc::Rc<BindGroup>,
}

/// A rasterized text in the cache, and how long it has waited for a frame to
/// draw it again.
///
/// Aged as a layer's texture is — see `LayerTextures::end_frame` — by the
/// frames of the surface it was last drawn on, and released after
/// [`IDLE_FRAMES_BEFORE_RELEASE`] of them; after [`ANY_SURFACE`] times as many
/// of anybody's, which is what lets go of one whose surface has gone.
struct CacheEntry {
    texture: std::rc::Rc<CachedTextTexture>,
    /// Whether the frame being drawn draws it.
    drawn: bool,
    surface: NodeId,
    idle: u32,
    idle_anywhere: u32,
}

/// Everything that affects the rasterized pixels of a text texture: the text
/// and how it is drawn, and the box it is drawn in.
#[derive(PartialEq, Eq, Hash)]
struct TextCacheKey {
    look: TextLook,
    tex_width: u32,
    tex_height: u32,
    /// The box's own width, which the texture's is only rounded from: it is
    /// what a line starting at the right is drawn against.
    box_width_bits: u32,
    /// The cut, which decides what is drawn at all.
    fit: Option<super::text_measurer::LineFitKey>,
}

/// A text and how it is drawn, whatever box it is drawn in.
#[derive(Clone, PartialEq, Eq, Hash)]
struct TextLook {
    text: std::rc::Rc<str>,
    font_size_bits: u32,
    weight: u16,
    style: crate::widgets::FontStyle,
    features: crate::widgets::FontFeatures,
    family: crate::widgets::FontFamily,
    line_height: (u8, u32),
    letter_spacing_bits: u32,
    align: TextAlign,
    color: [u8; 4],
}

/// Renderer for transformed text as textured quads.
pub struct TextQuadRenderer {
    // Rasterized text textures reused across frames
    text_cache: rustc_hash::FxHashMap<TextCacheKey, CacheEntry>,
    /// The texts this frame rasterized, in whatever box.
    rasterized: Vec<TextLook>,
    /// The glyphon side, built on the first texture rasterized and not before.
    shaper: Option<Shaper>,

    /// Pipeline, layout, sampler and index buffer — shared with the image
    /// renderer, which draws the same quad from a different texture.
    quad: TexturedQuadPipeline,

    // Texture format
    format: TextureFormat,
}

impl TextQuadRenderer {
    pub fn new(
        device: &Device,
        format: TextureFormat,
        clip_records: &wgpu::BindGroupLayout,
    ) -> Self {
        Self {
            quad: TexturedQuadPipeline::new(device, format, clip_records, "TextQuad"),
            text_cache: rustc_hash::FxHashMap::default(),
            rasterized: Vec::new(),
            shaper: None,
            format,
        }
    }

    /// Forget the last frame's quads.
    pub fn begin_frame(&mut self) {
        self.quad.begin_frame();
    }

    /// Age every text this frame of `surface` did not draw, and release what
    /// has waited long enough — or has been drawn again in another box.
    ///
    /// A text whose box animates is rasterized again at every size the box
    /// passes through, and the size before is one no frame will ask for
    /// again: kept for the idle frames, a panel that opens holds a texture for
    /// each of its frames. So a text this frame rasterized anew releases its
    /// other boxes on the same surface that this frame did not draw (#913).
    ///
    /// Past [`CACHE_BOUND`] entries, only what this frame drew is kept.
    pub fn end_frame(&mut self, surface: NodeId) {
        let overflowing = self.text_cache.len() > CACHE_BOUND;
        let rasterized = std::mem::take(&mut self.rasterized);
        self.text_cache.retain(|key, entry| {
            if std::mem::take(&mut entry.drawn) {
                entry.surface = surface;
                entry.idle = 0;
                entry.idle_anywhere = 0;
                return true;
            }
            if overflowing {
                return false;
            }
            entry.idle_anywhere += 1;
            if entry.surface == surface {
                if rasterized.contains(&key.look) {
                    return false;
                }
                entry.idle += 1;
            }
            entry.idle <= IDLE_FRAMES_BEFORE_RELEASE
                && entry.idle_anywhere <= IDLE_FRAMES_BEFORE_RELEASE * ANY_SURFACE
        });
        self.rasterized = rasterized;
        self.rasterized.clear();
    }

    /// Write this frame's text quads to the GPU, once they are all prepared.
    pub fn upload(&mut self, device: &Device, queue: &Queue) {
        self.quad.upload(device, queue);
    }

    /// Update screen dimensions for NDC conversion.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.quad.set_screen_size(width, height);
    }

    /// Prepare text entries for rendering as textured quads, appending them to
    /// `out`.
    ///
    /// The caller's buffer rather than one of ours, for the reason
    /// [`ImageQuadRenderer::prepare`](super::image_quad::ImageQuadRenderer::prepare)
    /// gives: the frame keeps one `Vec` of quads that every group addresses by
    /// range.
    pub fn prepare(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        entries: &[TextEntry],
        indices: &[usize],
        scale_factor: f32,
        out: &mut Vec<PreparedTextQuad>,
    ) {
        out.extend(indices.iter().map(|&idx| {
            let entry = &entries[idx];
            self.render_text_to_quad(device, queue, entry, scale_factor)
        }));
    }

    /// Render a single text entry to a textured quad.
    fn render_text_to_quad(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        entry: &TextEntry,
        scale_factor: f32,
    ) -> PreparedTextQuad {
        // Rasterize at fixed resolution: scale_factor * TEXT_SUPERSAMPLE.
        // The transform's scale/rotation is applied via GPU quad vertices, not baked into the texture.
        // This prevents atlas churn during scale animations (each frame would otherwise create new entries).
        let effective_scale = scale_factor * TEXT_SUPERSAMPLE;

        // Scale font size for crisp rendering
        let scaled_font_size = entry.font_size * effective_scale;

        // Texture dimensions are deterministic from the entry, so they can
        // key the cache before any shaping happens
        let (room_width, room_height) = texture_room(entry.rect, effective_scale);
        let padding = TEXT_TEXTURE_PADDING * effective_scale;
        let tex_width = ((room_width + padding * 2.0).ceil() as u32).max(1);
        let tex_height = ((room_height + padding * 2.0).ceil() as u32).max(1);

        let weight = super::text::drawn_weight(entry.font_weight);

        let cache_key = TextCacheKey {
            look: TextLook {
                text: entry.text.clone(),
                font_size_bits: scaled_font_size.to_bits(),
                weight: weight.0,
                style: entry.font_style,
                features: entry.font_features,
                family: entry.font_family,
                line_height: entry.line_height.key(),
                letter_spacing_bits: entry.letter_spacing.to_bits(),
                align: entry.align,
                color: [
                    (entry.color.r * 255.0) as u8,
                    (entry.color.g * 255.0) as u8,
                    (entry.color.b * 255.0) as u8,
                    (entry.color.a * 255.0) as u8,
                ],
            },
            tex_width,
            tex_height,
            box_width_bits: entry.rect.width.to_bits(),
            fit: entry.fit.map(|fit| fit.key()),
        };

        if let Some(kept) = self.text_cache.get_mut(&cache_key) {
            kept.drawn = true;
            let cached = kept.texture.clone();
            return self.build_quad(entry, cached, tex_width, tex_height, padding, scale_factor);
        }

        // Cache miss: shape and rasterize
        let format = self.format;
        let shaper = self
            .shaper
            .get_or_insert_with(|| Shaper::new(device, queue, format));
        let box_width = entry.rect.width * effective_scale;
        let buffer = super::text::shape(
            &mut shaper.font_system,
            &entry.text,
            scaled_font_size,
            entry.font_family,
            entry.font_weight,
            entry.font_style,
            entry.font_features,
            entry.line_height,
            entry.letter_spacing,
            entry.align,
            (
                Some(box_width),
                Some(buffer_height(entry.rect, effective_scale)),
            ),
            entry.fit,
            effective_scale,
        );

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
            format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Update viewport
        shaper.viewport.update(
            queue,
            Resolution {
                width: tex_width,
                height: tex_height,
            },
        );

        // Create text area
        let text_area = TextArea {
            buffer: &buffer,
            left: padding + super::text::paint_offset(&buffer, box_width, entry.align),
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

        shaper.draw(device, queue, text_area, &view, "Text Texture");

        // Create bind group
        let bind_group = self
            .quad
            .bind_texture(device, &view, "Text Texture Bind Group");

        let cached = std::rc::Rc::new(CachedTextTexture {
            texture,
            bind_group,
        });
        self.rasterized.push(cache_key.look.clone());
        self.text_cache.insert(
            cache_key,
            CacheEntry {
                texture: cached.clone(),
                drawn: true,
                surface: 0,
                idle: 0,
                idle_anywhere: 0,
            },
        );
        self.build_quad(entry, cached, tex_width, tex_height, padding, scale_factor)
    }

    /// Build the per-frame quad (vertices depend on position/transform, the
    /// texture itself is cached).
    #[allow(clippy::too_many_arguments)]
    fn build_quad(
        &mut self,
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
        // The texture was rendered at (scale_factor * TEXT_SUPERSAMPLE) resolution.
        // We divide by TEXT_SUPERSAMPLE to get logical-pixel dimensions, then the
        // world_transform applies scaling/rotation via transform_point() on quad corners.

        // Calculate display size: divide by TEXT_SUPERSAMPLE only.
        // The texture is rendered at (scale_factor * TEXT_SUPERSAMPLE) resolution.
        // The transform's scale is applied by transform_point() on quad corners, not here.
        let total_scale = TEXT_SUPERSAMPLE;
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
        // An array, as the image quad beside it already builds the same four:
        // `collect` here put four tuples on the heap once per transformed text
        // per frame, on the cache-hit path as well as the miss.
        let screen_corners: [(f32, f32); 4] = local_corners.map(|(x, y)| {
            let (sx, sy) = entry.transform.transform_point(x, y);
            (sx * scale_factor, sy * scale_factor)
        });

        // The clip's own space, like the image quad beside it: a turned clip
        // cuts the turned shape, and a rounded or squircle one cuts its corners
        // rather than the box they sit in. The corners above are in physical
        // world pixels and this is not — `QuadClip::resolved` folds the scale into
        // its map, which is what keeps the rect in the units the widget wrote.
        let clip = entry.clip.as_ref().map_or(QuadClip::NONE, |clip| {
            QuadClip::resolved(clip, scale_factor)
        });

        let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let vertices: [TexturedVertex; 4] = std::array::from_fn(|i| {
            let (x, y) = screen_corners[i];
            TexturedVertex::corner(
                self.quad.to_ndc(x, y),
                uvs[i],
                (x, y),
                &clip,
                entry.opacity,
                NO_TINT,
            )
        });

        PreparedTextQuad {
            quad: self.quad.push(vertices),
            cached,
        }
    }

    /// How many texts are rasterized and kept.
    #[cfg(feature = "testing")]
    pub(super) fn texture_count(&self) -> usize {
        self.text_cache.len()
    }

    /// Render the prepared text quads.
    pub fn render<'a>(
        &'a self,
        render_pass: &mut RenderPass<'a>,
        quads: &'a [PreparedTextQuad],
        clip_records: &'a wgpu::BindGroup,
    ) {
        self.quad
            .draw(render_pass, quads, clip_records, Pipeline::TextQuads);
    }
}

/// The rasterized-texture cache gives back what a frame stopped asking for.
///
/// Every entry holds a GPU texture and a bind group, so a cache that only ever
/// grows is a leak that the compositor, not the test suite, finds out about.
/// Idle frames bound it in the ordinary case (see
/// `a_texture_nobody_draws_is_released`); this is the bound past them: once
/// the cache is over [`CACHE_BOUND`](super::CACHE_BOUND), a frame keeps only
/// what it drew, so a frame that draws more texts than that keeps all of its
/// own, and the frame after gives back what it no longer asks for.
#[cfg(test)]
mod the_texture_cache_gives_back_what_a_frame_stopped_asking_for {
    use super::{CACHE_BOUND as BOUND, TextQuadRenderer};
    use crate::renderer::GpuContext;
    use crate::renderer::text::test_entry;
    use crate::transform::Transform;
    use crate::widgets::Rect;

    #[test]
    fn a_frame_that_overflows_it_keeps_only_its_own() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut renderer = TextQuadRenderer::new(
            &gpu.device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            super::super::clip_records::ClipRecords::new(&gpu.device).layout(),
        );
        renderer.set_screen_size(200.0, 100.0);

        // Small enough that a texture apiece is cheap; the count is the point.
        let box_ = Rect::new(0.0, 0.0, 6.0, 6.0);
        let labelled = |text: String| {
            let mut entry = test_entry(box_, Transform::scale(2.0));
            entry.text = text.into();
            entry
        };

        let crowd: Vec<_> = (0..=BOUND).map(|i| labelled(i.to_string())).collect();
        let every = (0..crowd.len()).collect::<Vec<_>>();
        let mut quads = Vec::new();
        renderer.begin_frame();
        renderer.prepare(&gpu.device, &gpu.queue, &crowd, &every, 1.0, &mut quads);
        renderer.end_frame(0);
        assert_eq!(
            renderer.text_cache.len(),
            BOUND + 1,
            "one frame past the bound, and every texture in it is this frame's"
        );

        quads.clear();
        let after = [labelled("a label the frame before never asked for".into())];
        renderer.begin_frame();
        renderer.prepare(&gpu.device, &gpu.queue, &after, &[0], 1.0, &mut quads);
        renderer.end_frame(0);
        assert_eq!(
            renderer.text_cache.len(),
            1,
            "the frame before rasterized {BOUND} textures nobody wants now"
        );
    }
}

/// A text nobody draws gives its texture back, and one drawn again in
/// another box gives back the box before (#913).
#[cfg(test)]
mod a_texture_nobody_draws_is_released {
    use super::TextQuadRenderer;
    use crate::renderer::GpuContext;
    use crate::renderer::layer_blur::IDLE_FRAMES_BEFORE_RELEASE;
    use crate::renderer::text::test_entry;
    use crate::renderer::types::TextEntry;
    use crate::transform::Transform;
    use crate::widgets::Rect;

    const BAR: u64 = 1;
    const PANEL: u64 = 2;

    fn renderer(gpu: &GpuContext) -> TextQuadRenderer {
        let mut renderer = TextQuadRenderer::new(
            &gpu.device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            super::super::clip_records::ClipRecords::new(&gpu.device).layout(),
        );
        renderer.set_screen_size(200.0, 100.0);
        renderer
    }

    /// One frame of `surface`, drawing `texts`.
    fn frame(gpu: &GpuContext, renderer: &mut TextQuadRenderer, surface: u64, texts: &[TextEntry]) {
        renderer.begin_frame();
        let every = (0..texts.len()).collect::<Vec<_>>();
        let mut quads = Vec::new();
        renderer.prepare(&gpu.device, &gpu.queue, texts, &every, 1.0, &mut quads);
        drop(quads);
        renderer.end_frame(surface);
    }

    fn turned(height: f32) -> TextEntry {
        test_entry(Rect::new(0.0, 0.0, 20.0, height), Transform::scale(2.0))
    }

    #[test]
    fn a_text_whose_box_animates_holds_one_texture() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut renderer = renderer(&gpu);
        for height in 1..40 {
            frame(&gpu, &mut renderer, BAR, &[turned(height as f32)]);
            assert_eq!(
                renderer.texture_count(),
                1,
                "a box {height} tall, and the boxes before it are still kept"
            );
        }
    }

    /// Two boxes of one text drawn in one frame are both drawn, and keep
    /// theirs; and a box on another surface is not one this surface replaced.
    #[test]
    fn a_text_in_two_boxes_drawn_together_or_apart_keeps_both() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut renderer = renderer(&gpu);
        frame(&gpu, &mut renderer, BAR, &[turned(10.0), turned(20.0)]);
        frame(&gpu, &mut renderer, PANEL, &[turned(30.0)]);
        frame(&gpu, &mut renderer, BAR, &[turned(10.0), turned(20.0)]);
        assert_eq!(renderer.texture_count(), 3);
    }

    /// Released after its own surface's idle frames, not another's: a still
    /// text beside a surface that animates is not drawn again every two
    /// seconds.
    #[test]
    fn a_text_no_frame_draws_is_released_after_its_surface_idles() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut renderer = renderer(&gpu);
        frame(&gpu, &mut renderer, BAR, &[turned(10.0)]);
        for _ in 0..2 * IDLE_FRAMES_BEFORE_RELEASE {
            frame(&gpu, &mut renderer, PANEL, &[]);
        }
        assert_eq!(
            renderer.texture_count(),
            1,
            "another surface's frames aged it"
        );

        for _ in 0..IDLE_FRAMES_BEFORE_RELEASE {
            frame(&gpu, &mut renderer, BAR, &[]);
        }
        assert_eq!(
            renderer.texture_count(),
            1,
            "released before its idle frames ran out"
        );
        frame(&gpu, &mut renderer, BAR, &[]);
        assert_eq!(renderer.texture_count(), 0, "kept past its idle frames");
    }
}

/// The glyph atlas behind the textures holds one text's glyphs, not every
/// text's — see `the_glyph_atlas_holds_a_frame_not_a_history` in `text.rs`, whose pages this
/// draws turned instead of upright.
#[cfg(test)]
mod the_glyph_atlas_holds_a_text_not_a_history {
    use super::{Shaper, TextQuadRenderer};
    use crate::renderer::GpuContext;
    use crate::renderer::text::the_glyph_atlas_holds_a_frame_not_a_history::{
        holds_one_page, page,
    };
    use crate::transform::Transform;

    /// The atlas a fresh renderer holds after drawing `frames`, one frame
    /// each — and only the atlas: the textures it rasterized are given back
    /// before the device is asked.
    fn atlas_after(gpu: &GpuContext, frames: &[String]) -> isize {
        let before = gpu.texture_bytes();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut renderer = TextQuadRenderer::new(
            &gpu.device,
            format,
            super::super::clip_records::ClipRecords::new(&gpu.device).layout(),
        );
        let mut shaper = Shaper::new(&gpu.device, &gpu.queue, format);
        shaper.font_system = crate::renderer::text::vendored_font_system();
        renderer.shaper = Some(shaper);
        renderer.set_screen_size(1200.0, 60.0);
        let mut quads = Vec::new();
        for text in frames {
            renderer.begin_frame();
            quads.clear();
            let texts = [page(text, Transform::rotate_degrees(10.0))];
            renderer.prepare(&gpu.device, &gpu.queue, &texts, &[0], 1.0, &mut quads);
            renderer.upload(&gpu.device, &gpu.queue);
        }
        quads.clear();
        renderer.text_cache.clear();
        gpu.texture_bytes() - before
    }

    #[test]
    fn a_turned_text_that_changes_every_frame_needs_no_more_atlas_than_one_frame() {
        holds_one_page(atlas_after);
    }
}

/// Most applications never turn a text, and a renderer that is never asked to
/// draw one builds none of what drawing one takes: no font system, which scans
/// every font on the machine to construct, and no atlas, which is two textures
/// before it holds a glyph.
#[cfg(test)]
mod nothing_turned_nothing_built {
    use super::TextQuadRenderer;
    use crate::renderer::GpuContext;

    #[test]
    fn a_renderer_that_draws_no_turned_text_holds_no_atlas() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let before = gpu.texture_bytes();
        let mut renderer = TextQuadRenderer::new(
            &gpu.device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            super::super::clip_records::ClipRecords::new(&gpu.device).layout(),
        );
        let mut quads = Vec::new();
        for _ in 0..3 {
            renderer.begin_frame();
            renderer.prepare(&gpu.device, &gpu.queue, &[], &[], 1.0, &mut quads);
            renderer.upload(&gpu.device, &gpu.queue);
        }
        assert_eq!(
            gpu.texture_bytes() - before,
            0,
            "three frames without a turned text, and an atlas was made for one"
        );
    }
}

#[cfg(test)]
mod buffer_height_tests {
    use super::{TEXT_MARGIN, buffer_height};
    use crate::widgets::Rect;

    /// No floor on this path — a transformed text is rasterized into a texture
    /// of its own size — and the margin is a factor on the box, not a border
    /// added to it, so it grows with the text rather than mattering only to
    /// small ones.
    #[test]
    fn the_margin_is_a_factor_on_the_box_and_the_scale_multiplies_both() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(buffer_height(rect, 1.0), 40.0 * TEXT_MARGIN);
        assert_eq!(buffer_height(rect, 4.0), 160.0 * TEXT_MARGIN);
        // A short box stays short: nothing floors it up to 50 the way the
        // untransformed path does.
        assert!(buffer_height(Rect::new(0.0, 0.0, 10.0, 4.0), 1.0) < 10.0);
    }
}
