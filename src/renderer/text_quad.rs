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
    Cache, Color as GlyphonColor, ColorMode, FontSystem, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, Buffer as WgpuBuffer, Device, Extent3d, MultisampleState, Queue, RenderPass,
    Texture, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

use super::constants::{TEXT_BUFFER_MARGIN_MULTIPLIER, TEXT_SUPERSAMPLE, TEXT_TEXTURE_PADDING};
use super::textured_quad::{QuadDraw, TexturedQuadPipeline};
use super::textured_vertex::{QuadClip, TexturedVertex};
use super::types::TextEntry;
use crate::widgets::font::FontWeight;
use crate::widgets::{Rect, TextAlign};

/// Margin multiplier is imported from constants
const TEXT_MARGIN: f32 = TEXT_BUFFER_MARGIN_MULTIPLIER;

/// The buffer glyphon is given to shape a transformed text in, in texture
/// pixels — `scale_factor * TEXT_SUPERSAMPLE` of them per logical pixel.
///
/// Wider than the layout box by [`TEXT_MARGIN`], which is the slack the
/// rasterizer wants and the reason this path cannot simply borrow
/// [`text::shaping_buffer`](super::text::shaping_buffer): the two shape the
/// same text in different buffers and break their lines in different places.
/// A frosted text's coverage mask has to be shaped in whichever of the two will
/// draw the glyphs over it — see [`text_mask`](super::text_mask).
///
/// An aligned text is shaped at exactly its box's width instead, since that is
/// the width cosmic-text aligns its lines across; the texture keeps the margin
/// either way — see [`texture_room`] — as room for the ink.
pub(super) fn shaping_buffer(rect: Rect, effective_scale: f32, align: TextAlign) -> (f32, f32) {
    let (width, height) = texture_room(rect, effective_scale);
    match align {
        TextAlign::Start => (width, height),
        _ => (rect.width * effective_scale, height),
    }
}

/// The room a transformed text is rasterized into, before padding: its box
/// widened by [`TEXT_MARGIN`], whatever its alignment.
fn texture_room(rect: Rect, effective_scale: f32) -> (f32, f32) {
    (
        rect.width * effective_scale * TEXT_MARGIN,
        rect.height * effective_scale * TEXT_MARGIN,
    )
}

/// A prepared text quad ready for rendering.
pub struct PreparedTextQuad {
    cached: std::rc::Rc<CachedTextTexture>,
    /// Vertex buffer with pre-computed vertices in NDC
    vertex_buffer: WgpuBuffer,
}

impl QuadDraw for PreparedTextQuad {
    fn bind_group(&self) -> &BindGroup {
        &self.cached.bind_group
    }

    fn vertex_buffer(&self) -> &WgpuBuffer {
        &self.vertex_buffer
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
    align: TextAlign,
    color: [u8; 4],
    tex_width: u32,
    tex_height: u32,
}

/// Renderer for transformed text as textured quads.
pub struct TextQuadRenderer {
    // Rasterized text textures reused across frames
    text_cache: rustc_hash::FxHashMap<TextCacheKey, std::rc::Rc<CachedTextTexture>>,
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
            text_cache: rustc_hash::FxHashMap::default(),
            frame_gen: 0,
            swash_cache,
            cache,
            atlas,
            text_renderer,
            viewport,
            format,
        }
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
        self.frame_gen += 1;
        out.extend(indices.iter().map(|&idx| {
            let entry = &entries[idx];
            self.render_text_to_quad(device, queue, entry, scale_factor)
        }));
        // Bounded cache: on overflow keep only the textures this frame used
        if self.text_cache.len() > 512 {
            let current = self.frame_gen;
            self.text_cache.retain(|_, c| c.last_used.get() == current);
        }
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

        let weight = if entry.font_weight == FontWeight::default() {
            FontWeight::NORMAL
        } else {
            entry.font_weight
        };

        let cache_key = TextCacheKey {
            text: entry.text.clone(),
            font_size_bits: scaled_font_size.to_bits(),
            weight: weight.0,
            family: entry.font_family,
            align: entry.align,
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
        let buffer = super::text::shape(
            &mut self.font_system,
            &entry.text,
            scaled_font_size,
            entry.font_family,
            entry.font_weight,
            entry.align,
            shaping_buffer(entry.rect, effective_scale, entry.align),
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
        let bind_group = self
            .quad
            .bind_texture(device, &view, "Text Texture Bind Group");

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
        // world pixels and this is not — `QuadClip::shape` folds the scale into
        // its map, which is what keeps the rect in the units the widget wrote.
        let clip = entry
            .clip
            .as_ref()
            .map_or(QuadClip::NONE, |clip| QuadClip::shape(clip, scale_factor));

        let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
        let vertices: [TexturedVertex; 4] = std::array::from_fn(|i| {
            let (x, y) = screen_corners[i];
            TexturedVertex::corner(self.quad.to_ndc(x, y), uvs[i], (x, y), &clip, entry.opacity)
        });

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
        self.quad.draw(render_pass, quads);
    }
}

/// The rasterized-texture cache gives back what a frame stopped asking for.
///
/// Every entry holds a GPU texture and a bind group, so a cache that only ever
/// grows is a leak that the compositor, not the test suite, finds out about.
/// What bounds it is the frame counter `prepare` advances: the sweep keeps the
/// entries whose `last_used` is *this* frame, so a counter that stopped moving
/// would match every entry ever made and keep the lot — and the sweep would
/// still run, still look like eviction, and evict nothing.
///
/// The sweep only runs once the cache is over its bound, so two frames of one
/// text each never reach it and nothing about the counter is observable there.
/// Overflowing it is the whole setup.
#[cfg(test)]
mod the_texture_cache_gives_back_what_a_frame_stopped_asking_for {
    use super::TextQuadRenderer;
    use crate::renderer::GpuContext;
    use crate::renderer::text::test_entry;
    use crate::transform::Transform;
    use crate::widgets::Rect;

    /// The size `prepare` lets the cache pass before it sweeps.
    const BOUND: usize = 512;

    #[test]
    fn a_frame_that_overflows_it_keeps_only_its_own() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut renderer =
            TextQuadRenderer::new(&gpu.device, &gpu.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
        renderer.set_screen_size(200.0, 100.0);

        // Small enough that a texture apiece is cheap; the count is the point.
        let box_ = Rect::new(0.0, 0.0, 6.0, 6.0);
        let labelled = |text: String| {
            let mut entry = test_entry(box_, Transform::scale(2.0));
            entry.text = text;
            entry
        };

        let crowd: Vec<_> = (0..=BOUND).map(|i| labelled(i.to_string())).collect();
        let every = (0..crowd.len()).collect::<Vec<_>>();
        let mut quads = Vec::new();
        renderer.prepare(&gpu.device, &gpu.queue, &crowd, &every, 1.0, &mut quads);
        assert_eq!(
            renderer.text_cache.len(),
            BOUND + 1,
            "one frame past the bound, and every texture in it is this frame's"
        );

        quads.clear();
        let after = [labelled("a label the frame before never asked for".into())];
        renderer.prepare(&gpu.device, &gpu.queue, &after, &[0], 1.0, &mut quads);
        assert_eq!(
            renderer.text_cache.len(),
            1,
            "the frame before rasterized {BOUND} textures nobody wants now"
        );
    }
}

#[cfg(test)]
mod shaping_buffer_tests {
    use super::{TEXT_MARGIN, shaping_buffer};
    use crate::widgets::{Rect, TextAlign};

    /// No floor on this path — a transformed text is rasterized into a texture
    /// of its own size — and the margin is a factor on the box, not a border
    /// added to it, so it grows with the text rather than mattering only to
    /// small ones.
    #[test]
    fn the_margin_is_a_factor_on_the_box_and_the_scale_multiplies_both() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(
            shaping_buffer(rect, 1.0, TextAlign::Start),
            (100.0 * TEXT_MARGIN, 40.0 * TEXT_MARGIN)
        );
        assert_eq!(
            shaping_buffer(rect, 4.0, TextAlign::Start),
            (400.0 * TEXT_MARGIN, 160.0 * TEXT_MARGIN)
        );
        // A tiny box stays tiny: nothing floors it up to 200 the way the
        // untransformed path does.
        assert!(shaping_buffer(Rect::new(0.0, 0.0, 10.0, 4.0), 1.0, TextAlign::Start).0 < 20.0);
    }
}
