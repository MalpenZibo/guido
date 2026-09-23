//! Glyph coverage as a texture: the shape a frosted text cuts out of its
//! background.
//!
//! A container's backdrop blur is masked by a rounded rectangle, which the
//! composite shader can evaluate from four radii. Glyphs have no such formula —
//! the only thing that knows their shape is the rasterizer. So a text that asks
//! for a backdrop blur is shaped once into a texture, in white on nothing, and
//! the composite reads its alpha where it would otherwise evaluate an SDF.
//!
//! The mask is rasterized in the text's **own space** — the one the layout box
//! is written in, before any transform — and the composite carries each
//! fragment back into that space to find its texel, so the coverage is turned
//! and stretched with the letters rather than laid over them square. What the
//! transform decides is only where the mask is read, never how it is made, so
//! one rasterization serves a whole animation.
//!
//! Shaping mirrors whatever will draw the glyphs over the frost exactly — same
//! metrics, same buffer — because two shapings that disagree break their lines
//! in different places, and the frost then lands beside a letter that wrapped
//! elsewhere. There are two to mirror, not one: [`text`](super::text) draws an
//! untransformed text and [`text_quad`](super::text_quad) a transformed one,
//! and their buffers are not the same size — a 72-point box at 30px shapes in
//! 400 texels through one and 317 through the other. So the caller hands the
//! buffer in rather than this module guessing which is about to be used.
//!
//! Colour is deliberately absent from the cache key: the frost of a white label
//! and a red one is the same hole.

use std::cell::Cell;
use std::rc::Rc;

use glyphon::{
    Cache, Color as GlyphonColor, ColorMode, FontSystem, Resolution, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};
use rustc_hash::FxHashMap;
use wgpu::{Device, MultisampleState, Queue, TextureFormat};

use crate::widgets::font::FontWeight;
use crate::widgets::{FontFamily, TextAlign};

/// Masks kept before the unused ones are dropped.
///
/// A surface frosting more than a handful of texts at once is not the case this
/// is for — each one costs a pass break — so the ceiling is low and the sweep
/// keeps whatever the current frame touched.
const MAX_CACHED: usize = 32;

/// Refuse a mask larger than this per side, in texels. The logical text that
/// allows is this over the density, so a HiDPI screen halves it and a transform
/// halves it again. A frost is an effect on a label, and an accidental one over
/// a full-screen text would allocate a second framebuffer with no warning.
const MAX_SIDE: u32 = 2048;

/// What a mask has to be rasterized from.
pub struct MaskSpec<'a> {
    pub text: &'a str,
    pub font_size: f32,
    pub font_family: FontFamily,
    pub font_weight: FontWeight,
    /// Where each line sits across the buffer, as the letters over the frost
    /// have it.
    pub align: TextAlign,
    /// The buffer to shape in, in the same texels as everything else here.
    ///
    /// Handed in rather than derived, because the rule belongs to whichever
    /// path will draw the glyphs and there are two of them — see the module
    /// documentation.
    pub buffer: (f32, f32),
    /// Mask size in texels: the frame the composite reads it over.
    pub size: (u32, u32),
    /// Where the glyph origin sits inside that frame, in texels — the slack the
    /// frame carries on every side so a descender or a contour has somewhere to
    /// reach.
    pub offset: (f32, f32),
    /// Texels per logical pixel: the surface scale, and
    /// [`TEXT_SUPERSAMPLE`](super::constants::TEXT_SUPERSAMPLE) times it when
    /// the transform stretches the mask over more pixels than it has texels.
    ///
    /// Whether it stretches, never by how much — so the mask a scale animation
    /// reads is rasterized twice over its length rather than once a frame.
    pub density: f32,
}

#[derive(PartialEq, Eq, Hash)]
struct MaskKey {
    text: String,
    font_size_bits: u32,
    weight: u16,
    family: FontFamily,
    align: TextAlign,
    width: u32,
    height: u32,
    /// Rounded, and in the key because it decides where the lines break: two
    /// masks alike in every other field can still be shaped differently.
    buffer: (u32, u32),
    /// The glyph origin inside the frame, in quarter texels. Quantised because
    /// it follows a stroke width, and a mask per unique float would never hit.
    offset: (i32, i32),
}

struct CachedMask {
    #[allow(dead_code)] // Held to keep the view alive.
    texture: wgpu::Texture,
    view: Rc<wgpu::TextureView>,
    last_used: Cell<u64>,
}

/// The glyphon side, built on the first frosted text and not before.
///
/// A `FontSystem` scans the machine's fonts to construct, which is not a cost
/// to put on every app for an effect most of them never ask for.
struct Shaper {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Held for the atlas and viewport.
    cache: Cache,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    viewport: Viewport,
}

impl Shaper {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
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
        }
    }
}

/// Rasterizes glyph coverage, and remembers it.
pub struct TextMaskRenderer {
    shaper: Option<Shaper>,
    format: TextureFormat,
    masks: FxHashMap<MaskKey, Rc<CachedMask>>,
    frame_gen: u64,
}

impl TextMaskRenderer {
    pub fn new(format: TextureFormat) -> Self {
        Self {
            shaper: None,
            format,
            masks: FxHashMap::default(),
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

        let font_size = spec.font_size * spec.density;
        let weight = if spec.font_weight == FontWeight::default() {
            FontWeight::NORMAL
        } else {
            spec.font_weight
        };
        let key = MaskKey {
            text: spec.text.to_owned(),
            font_size_bits: font_size.to_bits(),
            weight: weight.0,
            family: spec.font_family,
            align: spec.align,
            width,
            height,
            buffer: (spec.buffer.0 as u32, spec.buffer.1 as u32),
            offset: (
                (spec.offset.0 * 4.0).round() as i32,
                (spec.offset.1 * 4.0).round() as i32,
            ),
        };

        if let Some(cached) = self.masks.get(&key) {
            cached.last_used.set(self.frame_gen);
            return Some(Rc::clone(&cached.view));
        }

        let format = self.format;
        let shaper = self
            .shaper
            .get_or_insert_with(|| Shaper::new(device, queue, format));

        // Shaped the way the on-screen text is shaped, or the hole would not be
        // the shape of the letters that land in it.
        let buffer = super::text::shape(
            &mut shaper.font_system,
            spec.text,
            font_size,
            spec.font_family,
            spec.font_weight,
            spec.align,
            spec.buffer,
        );

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
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());

        shaper.viewport.update(queue, Resolution { width, height });

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

        if let Err(e) = shaper.text_renderer.prepare(
            device,
            queue,
            &mut shaper.font_system,
            &mut shaper.atlas,
            &shaper.viewport,
            vec![area],
            &mut shaper.swash_cache,
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
            if let Err(e) = shaper
                .text_renderer
                .render(&shaper.atlas, &shaper.viewport, &mut pass)
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
