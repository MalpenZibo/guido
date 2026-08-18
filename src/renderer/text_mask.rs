//! Glyph coverage as a texture: the shape a frosted text cuts out of its
//! background.
//!
//! A container's backdrop blur is masked by a rounded rectangle, which the
//! composite shader can evaluate from four radii. Glyphs have no such formula —
//! the only thing that knows their shape is the rasterizer. So a text that asks
//! for a backdrop blur is shaped once into a texture, in white on nothing, and
//! the composite reads its alpha where it would otherwise evaluate an SDF.
//!
//! The mask is rasterized to cover exactly the region being filtered, one texel
//! per physical pixel, so the composite can sample it with the destination uv.
//! Shaping mirrors [`TextRenderState`](super::text::TextRenderState) exactly —
//! same metrics, same buffer size — because the glyphs drawn over the frost come
//! from there, and two shapings that disagree would show as a halo.
//!
//! Colour is deliberately absent from the cache key: the frost of a white label
//! and a red one is the same hole.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, ColorMode, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

use crate::widgets::FontFamily;
use crate::widgets::font::FontWeight;

/// Masks kept before the unused ones are dropped.
///
/// A surface frosting more than a handful of texts at once is not the case this
/// is for — each one costs a pass break — so the ceiling is low and the sweep
/// keeps whatever the current frame touched.
const MAX_CACHED: usize = 32;

/// Refuse a mask larger than this per side. A frost is an effect on a label, and
/// an accidental one over a full-screen text would allocate a second framebuffer
/// with no warning.
const MAX_SIDE: u32 = 2048;

/// What a mask has to be rasterized from.
pub struct MaskSpec<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub font_family: &'a FontFamily,
    pub font_weight: FontWeight,
    /// The text's layout box in logical pixels — the buffer is sized from it,
    /// exactly as the on-screen shaping is.
    pub logical: (f32, f32),
    /// Mask size in physical pixels: the region the composite will cover.
    pub size: (u32, u32),
    /// Where the glyph origin sits inside that region, in physical pixels.
    /// Sub-pixel, because the region is snapped to the pixel grid and the text
    /// is not.
    pub offset: (f32, f32),
    pub scale_factor: f32,
}

#[derive(PartialEq, Eq, Hash)]
struct MaskKey {
    text: String,
    font_size_bits: u32,
    weight: u16,
    family: FontFamily,
    width: u32,
    height: u32,
    /// The sub-pixel offset, in quarter pixels. Quantised because it varies
    /// continuously while a surface is dragged, and a mask per unique float
    /// would never hit.
    offset: (i32, i32),
}

struct CachedMask {
    #[allow(dead_code)] // Held to keep the view alive.
    texture: wgpu::Texture,
    view: Rc<wgpu::TextureView>,
    last_used: Cell<u64>,
}

/// Rasterizes glyph coverage, and remembers it.
pub struct TextMaskRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Held for the atlas and viewport.
    cache: Cache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
    format: TextureFormat,
    masks: HashMap<MaskKey, Rc<CachedMask>>,
    frame_gen: u64,
}

impl TextMaskRenderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        for data in crate::get_registered_fonts() {
            font_system
                .db_mut()
                .load_font_source(glyphon::fontdb::Source::Binary(data));
        }
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::with_color_mode(device, queue, &cache, format, ColorMode::Web);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            cache,
            atlas,
            text_renderer,
            viewport,
            format,
            masks: HashMap::new(),
            frame_gen: 0,
        }
    }

    /// Note a new frame, and drop masks nothing has asked for in a while.
    pub fn begin_frame(&mut self) {
        self.frame_gen += 1;
        if self.masks.len() > MAX_CACHED {
            let current = self.frame_gen;
            self.masks
                .retain(|_, mask| mask.last_used.get() + 1 >= current);
        }
    }

    /// The coverage of `spec`, rasterized or recalled.
    ///
    /// Renders and submits on its own encoder, so the mask is ready before the
    /// frame that samples it is submitted.
    pub fn mask(
        &mut self,
        device: &Device,
        queue: &Queue,
        spec: &MaskSpec<'_>,
    ) -> Option<Rc<wgpu::TextureView>> {
        let (width, height) = spec.size;
        if spec.text.is_empty() || width == 0 || height == 0 {
            return None;
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            log::warn!(
                "text backdrop blur skipped: mask would be {width}x{height}, over the {MAX_SIDE} limit"
            );
            return None;
        }

        let font_size = spec.font_size * spec.scale_factor;
        let weight = if spec.font_weight == FontWeight::default() {
            FontWeight::NORMAL
        } else {
            spec.font_weight
        };
        let key = MaskKey {
            text: spec.text.to_owned(),
            font_size_bits: font_size.to_bits(),
            weight: weight.0,
            family: spec.font_family.clone(),
            width,
            height,
            offset: (
                (spec.offset.0 * 4.0).round() as i32,
                (spec.offset.1 * 4.0).round() as i32,
            ),
        };

        if let Some(cached) = self.masks.get(&key) {
            cached.last_used.set(self.frame_gen);
            return Some(Rc::clone(&cached.view));
        }

        // Shaped the way the on-screen text is shaped, or the hole would not be
        // the shape of the letters that land in it.
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size, font_size * 1.2),
        );
        buffer.set_size(
            &mut self.font_system,
            Some(spec.logical.0.max(200.0) * spec.scale_factor),
            Some(spec.logical.1.max(50.0) * spec.scale_factor),
        );
        buffer.set_text(
            &mut self.font_system,
            spec.text,
            &Attrs::new()
                .family(spec.font_family.to_cosmic())
                .weight(weight.to_cosmic()),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, true);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Mask"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());

        self.viewport.update(queue, Resolution { width, height });

        let area = TextArea {
            buffer: &buffer,
            left: spec.offset.0,
            top: spec.offset.1,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            },
            // Opaque white: what is wanted is coverage, and a translucent text
            // would otherwise thin its own hole.
            default_color: GlyphonColor::rgb(255, 255, 255),
            custom_glyphs: &[],
        };

        if let Err(e) = self.text_renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            vec![area],
            &mut self.swash_cache,
        ) {
            log::error!("text mask prepare failed: {e:?}");
            return None;
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Text Mask Encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Text Mask Pass"),
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
            if let Err(e) = self
                .text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
            {
                log::error!("text mask render failed: {e:?}");
            }
        }
        queue.submit(std::iter::once(encoder.finish()));

        let view = Rc::new(view);
        self.masks.insert(
            key,
            Rc::new(CachedMask {
                texture,
                view: Rc::clone(&view),
                last_used: Cell::new(self.frame_gen),
            }),
        );
        Some(view)
    }
}
