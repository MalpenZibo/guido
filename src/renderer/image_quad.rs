//! Textured quad rendering for images with transform support.
//!
//! This module renders images as textured quads with full transform support
//! (rotation, scale, translate). Textures are cached for performance.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use wgpu::{
    BindGroup, Device, Extent3d, Queue, RenderPass, Texture, TextureDimension, TextureFormat,
    TextureUsages,
};

use super::commands::DrawCommand;
use super::constants::SVG_QUALITY_MULTIPLIER;
use super::flatten::FlattenedCommand;
use super::image_atlas::{AtlasEntry, ImageAtlas};
use super::textured_quad::{QuadDraw, TexturedQuadPipeline};
use super::textured_vertex::{NO_TINT, QuadClip, TexturedVertex};
use crate::image_decode::{DecodeKey, DecodedImage, Extent, SampledBytes};
use crate::render_stats::Pipeline;
use crate::widgets::Color;
use crate::widgets::Rect;
use crate::widgets::image::{ContentFit, ImageSource};

/// A prepared image quad ready for rendering: the texture it samples, and
/// where its corners are among the frame's quads.
pub struct PreparedImageQuad {
    texture: Rc<CachedTexture>,
    quad: u32,
}

impl QuadDraw for PreparedImageQuad {
    fn bind_group(&self) -> &Rc<BindGroup> {
        &self.texture.bind_group
    }

    fn quad(&self) -> u32 {
        self.quad
    }
}

/// Where a cached image's texels live.
enum Backing {
    /// A texture of its own: an image larger than an atlas page takes.
    Own(Texture),
    /// A place on a shared page, given back when this is dropped.
    Atlas(AtlasEntry),
}

/// Cached texture data.
struct CachedTexture {
    backing: Backing,
    /// The bind group it is drawn through: its own texture's, or its page's.
    /// Made with the texture or the page and kept with it, so drawing it again
    /// makes nothing, and every image on one page draws through the same one.
    bind_group: Rc<BindGroup>,
    /// Its texels within that texture, `[x, y, width, height]` in texture
    /// coordinates — all of it, for a texture of its own.
    uv_rect: [f32; 4],
    /// Original intrinsic dimensions
    intrinsic_width: f32,
    intrinsic_height: f32,
    /// When a frame last drew it. A `Cell` because the frame that draws it
    /// holds it through an `Rc` already.
    last_used: Cell<Instant>,
    /// The source and raster it was uploaded from, whose pixels went with the
    /// upload: the decode cache is told when this texture goes.
    decoded_from: Option<(DecodeKey, Extent)>,
}

impl CachedTexture {
    /// What it costs on the GPU, counted against the cache's budget.
    fn bytes(&self) -> usize {
        match &self.backing {
            Backing::Own(texture) => texture_bytes(texture),
            Backing::Atlas(entry) => entry.bytes(),
        }
    }
}

impl Drop for CachedTexture {
    fn drop(&mut self) {
        if let Some((key, extent)) = self.decoded_from.take() {
            crate::image_decode::texture_evicted(key, extent);
        }
    }
}

/// Cache key for image textures: the source, and which of its rasters.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    source: SourceKey,
    extent: Extent,
}

/// Which source a texture is of.
///
/// A source that decodes is its decode entry: `DecodeKey` settled whether two
/// sources are one when the entry was made, so finding its texture reads none
/// of its bytes. Raw pixels have no entry and are their own buffer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum SourceKey {
    Decoded(DecodedImage),
    Rgba(RgbaKey),
}

impl SourceKey {
    /// `None` for a source that decodes and has no entry to name it by.
    fn of(source: &ImageSource, decoded: Option<&DecodedImage>) -> Option<Self> {
        match source {
            ImageSource::Rgba {
                width,
                height,
                pixels,
            } => Some(Self::Rgba(RgbaKey {
                width: *width,
                height: *height,
                pixels: SampledBytes(pixels.clone()),
            })),
            _ => decoded.cloned().map(Self::Decoded),
        }
    }

    /// Whether nothing but this key holds the source, so no image can draw it
    /// again. Only raw pixels can be orphaned: a decode entry outlives every
    /// texture of it.
    fn orphaned(&self) -> bool {
        matches!(self, Self::Rgba(key) if Arc::strong_count(&key.pixels.0) == 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RgbaKey {
    width: u32,
    height: u32,
    pixels: SampledBytes,
}

/// The pixel size to rasterize an SVG of `intrinsic` size at.
///
/// With a `target` — the box it is shown in, in logical pixels — the raster
/// is fitted to that box, preserving aspect ratio, rather than to the SVG's
/// intrinsic size: a 16px weather icon with a 104px viewBox costs a 16px
/// raster, not a 104px one, which is both faster and sharper. `scale` carries
/// transform scale, HiDPI factor and the quality multiplier.
///
/// Both are quantized first — the scale to quarters, the box to whole pixels —
/// so a scale animation asks for a raster per step rather than per frame.
fn svg_extent(
    (width, height): (f32, f32),
    scale: f32,
    target: Option<(f32, f32)>,
) -> Option<(u32, u32)> {
    let scale = (scale * 4.0).round() / 4.0;
    let scale = match target {
        Some((tw, th)) if tw >= 1.0 && th >= 1.0 => {
            scale * (tw.round() / width).min(th.round() / height)
        }
        _ => scale,
    };
    let extent = ((width * scale).ceil(), (height * scale).ceil());
    (extent.0 >= 1.0 && extent.1 >= 1.0).then_some((extent.0 as u32, extent.1 as u32))
}

/// A texture of its own, for an image too large for an atlas page.
///
/// `Rgba8Unorm`, like the pages, so colours pass through without an sRGB
/// conversion.
fn own_texture(device: &Device, queue: &Queue, width: u32, height: u32, rgba: &[u8]) -> Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Image Texture"),
        size: Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture
}

/// Renderer for images as textured quads.
pub struct ImageQuadRenderer {
    /// Pipeline, layout, sampler and index buffer — shared with the text
    /// renderer, which draws the same quad from a different texture.
    quad: TexturedQuadPipeline,

    /// The pages small images are packed onto.
    atlas: ImageAtlas,
    // Texture cache
    texture_cache: FxHashMap<CacheKey, Rc<CachedTexture>>,
    /// The size of the raster each SVG source was last drawn from: what is
    /// drawn, stretched, while a new size is on its way.
    last_svg_raster: FxHashMap<DecodedImage, Extent>,
    /// When the frame being prepared started: what a texture drawn in it is
    /// stamped with.
    frame_started: Instant,
    /// The sum of every cached texture's `bytes`.
    cached_bytes: usize,
}

/// What a quad's vertices carry for its opacity and its tint. A tint's own
/// alpha fades the image as an opacity would.
fn opacity_and_tint(opacity: f32, tint: Option<Color>) -> (f32, [f32; 4]) {
    match tint {
        Some(c) => (opacity * c.a, [c.r, c.g, c.b, 1.0]),
        None => (opacity, NO_TINT),
    }
}

/// How long a texture no frame has drawn is kept once the cache is past its
/// budget. One drawn more recently is never evicted — the cache grows past its
/// budget instead — because a raster texture's pixels went with its upload: an
/// image still in view whose texture was evicted is blank until it is decoded
/// again, and evicting it every frame would decode it every frame.
const KEEP_UNUSED: Duration = Duration::from_secs(1);

/// The bytes `texture` holds on the GPU, every mip level of it.
fn texture_bytes(texture: &Texture) -> usize {
    let size = texture.size();
    let texel = texture.format().block_copy_size(None).unwrap_or(4) as usize;
    (0..texture.mip_level_count())
        .map(|level| {
            let width = (size.width >> level).max(1) as usize;
            let height = (size.height >> level).max(1) as usize;
            width * height * texel
        })
        .sum()
}

impl ImageQuadRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        Self {
            quad: TexturedQuadPipeline::new(device, format, "ImageQuad"),
            atlas: ImageAtlas::default(),
            texture_cache: FxHashMap::default(),
            last_svg_raster: FxHashMap::default(),
            frame_started: Instant::now(),
            cached_bytes: 0,
        }
    }

    /// Update screen dimensions for NDC conversion.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.quad.set_screen_size(width, height);
    }

    /// Begin a new frame: what a texture drawn in it is stamped with.
    pub fn begin_frame(&mut self) {
        self.frame_started = Instant::now();
        self.quad.begin_frame();
    }

    /// Write this frame's image quads to the GPU, once they are all prepared.
    pub fn upload(&mut self, device: &Device, queue: &Queue) {
        self.quad.upload(device, queue);
    }

    /// Bring the cache back under `budget` bytes, once the frame has drawn
    /// what it draws.
    ///
    /// After the frame rather than before it, so every texture this frame drew
    /// is stamped as drawn now: before it, a surface that sat idle for longer
    /// than [`KEEP_UNUSED`] would have its own images evicted by the frame
    /// about to draw them.
    pub fn trim(&mut self, budget: usize) {
        // Before the budget is looked at: a page empties when the last frame's
        // quads let go of what was evicted from it, which is not this call.
        self.atlas.drop_empty_pages();
        // Under the budget too: the key holds the pixels, and would keep a
        // buffer the application let go for as long as its texture stayed.
        self.texture_cache.retain(|key, texture| {
            let orphaned = key.source.orphaned();
            if orphaned {
                self.cached_bytes -= texture.bytes();
            }
            !orphaned
        });
        if self.cached_bytes <= budget {
            return;
        }
        let mut idle: Vec<(Instant, CacheKey)> = self
            .texture_cache
            .iter()
            .filter(|(_, texture)| {
                self.frame_started
                    .saturating_duration_since(texture.last_used.get())
                    > KEEP_UNUSED
            })
            .map(|(key, texture)| (texture.last_used.get(), key.clone()))
            .collect();
        idle.sort_unstable_by_key(|(last_used, _)| *last_used);
        for (_, key) in idle {
            if self.cached_bytes <= budget {
                break;
            }
            if let Some(texture) = self.texture_cache.remove(&key) {
                self.cached_bytes -= texture.bytes();
                if let SourceKey::Decoded(decoded) = &key.source
                    && self.last_svg_raster.get(decoded) == Some(&key.extent)
                {
                    self.last_svg_raster.remove(decoded);
                }
            }
        }
    }

    /// How many textures are cached.
    #[cfg(feature = "testing")]
    pub(crate) fn texture_count(&self) -> usize {
        self.texture_cache.len()
    }

    /// Drop every texture, as eviction does, and say so as eviction does.
    #[cfg(feature = "testing")]
    pub(crate) fn forget_textures(&mut self) {
        self.texture_cache.clear();
        self.last_svg_raster.clear();
        self.cached_bytes = 0;
    }

    /// How many atlas pages there are.
    #[cfg(feature = "testing")]
    pub(crate) fn atlas_pages(&self) -> usize {
        self.atlas.pages()
    }

    /// Get or create a cached texture for the given source.
    ///
    /// `svg_target` is the widget rect the SVG will be displayed in (logical
    /// pixels), which its raster is sized to — see [`svg_extent`]. `None`
    /// (ContentFit::None) keeps the intrinsic size, which is what that fit
    /// mode displays.
    ///
    /// An SVG with no raster of that size yet draws one now if it is small;
    /// a larger one is asked of the worker, and is drawn from the raster it
    /// was last drawn from until the new one lands.
    fn get_or_create_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        render_scale: f32,
        svg_target: Option<(f32, f32)>,
        decoded: Option<&DecodedImage>,
    ) -> Option<Rc<CachedTexture>> {
        let extent = match decoded.and_then(DecodedImage::svg_size) {
            Some(size) => Some(svg_extent(size, render_scale, svg_target)?),
            None => None,
        };
        let key = CacheKey {
            source: SourceKey::of(source, decoded)?,
            extent,
        };

        let texture = match self.texture_cache.get_key_value(&key) {
            Some((cached_key, cached)) => {
                let cached = cached.clone();
                // Equal pixels in a new buffer, and nothing else holds the old
                // one: keyed by the new one from now on, so the next lookup is
                // settled by the pointer and `trim` keeps a texture still drawn.
                if cached_key.source.orphaned() {
                    self.texture_cache.remove(&key);
                    self.texture_cache.insert(key.clone(), cached.clone());
                }
                Some(cached)
            }
            None => self.load_texture(device, queue, source, key.clone(), decoded),
        };
        let svg = extent.and(decoded);
        let texture = match texture {
            Some(texture) => {
                if let Some(svg) = svg
                    && self.last_svg_raster.get(svg) != Some(&extent)
                {
                    self.last_svg_raster.insert(svg.clone(), extent);
                }
                texture
            }
            None => {
                let last = svg
                    .and_then(|svg| self.last_svg_raster.get(svg))
                    .and_then(|&extent| {
                        self.texture_cache.get(&CacheKey {
                            source: key.source.clone(),
                            extent,
                        })
                    });
                match (last, decoded) {
                    (Some(last), _) => last.clone(),
                    (None, Some(decoded)) if extent.is_some() => {
                        decoded.drew_nothing();
                        return None;
                    }
                    (None, _) => return None,
                }
            }
        };
        texture.last_used.set(self.frame_started);
        Some(texture)
    }

    /// Upload a texture to the GPU, and cache it under `key`.
    ///
    /// A raster `Path` or `Bytes` source arrives already decoded and an SVG
    /// already rasterized — the worker in `image_decode` did that off the
    /// frame — so this only uploads, and taking the pixels to upload them is
    /// what drops them from the cache. A raster source whose pixels were
    /// already taken has none left: this draws nothing and reports the texture
    /// missing, which sends the source back to the worker. An SVG with none
    /// of this size rasterizes them here if they are few, and asks the worker
    /// for them if not. Decoding a raster image or rasterizing a large SVG
    /// here instead is the stall that module exists to remove.
    fn load_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        key: CacheKey,
        decoded: Option<&DecodedImage>,
    ) -> Option<Rc<CachedTexture>> {
        let mut texture = match source {
            ImageSource::Path(_) | ImageSource::Bytes(_) => {
                let Some(pixels) = decoded.and_then(DecodedImage::take) else {
                    crate::image_decode::texture_missing(source);
                    return None;
                };
                self.upload_raster(device, queue, pixels.width, pixels.height, &pixels.rgba)?
            }
            ImageSource::Rgba {
                width,
                height,
                pixels,
            } => {
                let expected = (*width as usize)
                    .checked_mul(*height as usize)
                    .and_then(|v| v.checked_mul(4));
                if Some(pixels.len()) != expected {
                    log::warn!(
                        "Rgba image source size mismatch: {}x{} expects {:?} bytes, got {}",
                        width,
                        height,
                        expected,
                        pixels.len()
                    );
                    return None;
                }
                // Raw pixels are straight; the pipeline takes them
                // premultiplied.
                let pixels = crate::image_decode::premultiplied(pixels);
                self.upload_raster(device, queue, *width, *height, &pixels)?
            }
            ImageSource::SvgPath(_) | ImageSource::SvgBytes(_) => {
                let decoded = decoded?;
                let pixels = decoded.take_or_rasterize(key.extent)?;
                self.store(
                    device,
                    queue,
                    pixels.width,
                    pixels.height,
                    &pixels.rgba,
                    decoded.svg_size()?,
                )?
            }
        };
        texture.decoded_from = decoded.map(|decoded| (decoded.key().clone(), key.extent));

        let cached = Rc::new(texture);
        self.cached_bytes += cached.bytes();
        self.texture_cache.insert(key, cached.clone());
        Some(cached)
    }

    /// Upload raw RGBA8 pixel data to GPU.
    fn upload_raster(
        &mut self,
        device: &Device,
        queue: &Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Option<CachedTexture> {
        if width == 0 || height == 0 {
            return None;
        }

        self.store(
            device,
            queue,
            width,
            height,
            rgba,
            (width as f32, height as f32),
        )
    }

    /// Put `rgba`, `width` by `height` texels, where it will be drawn from: on
    /// an atlas page if it is small enough for one, in a texture of its own if
    /// not. `intrinsic` is the size the image lays out at, which for an SVG is
    /// not the size it was rasterized at.
    fn store(
        &mut self,
        device: &Device,
        queue: &Queue,
        width: u32,
        height: u32,
        rgba: &[u8],
        (intrinsic_width, intrinsic_height): (f32, f32),
    ) -> Option<CachedTexture> {
        let (backing, bind_group, uv_rect) = match self
            .atlas
            .insert(device, queue, &self.quad, width, height, rgba)
        {
            Some((entry, uv_rect)) => {
                let bind_group = entry.bind_group().clone();
                (Backing::Atlas(entry), bind_group, uv_rect)
            }
            None => {
                let texture = own_texture(device, queue, width, height, rgba);
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = self
                    .quad
                    .bind_texture(device, &view, "ImageQuad Bind Group");
                (Backing::Own(texture), bind_group, [0.0, 0.0, 1.0, 1.0])
            }
        };
        Some(CachedTexture {
            backing,
            bind_group,
            uv_rect,
            intrinsic_width,
            intrinsic_height,
            last_used: Cell::new(self.frame_started),
            decoded_from: None,
        })
    }

    /// Prepare image commands for rendering, appending them to `out`.
    ///
    /// The caller's buffer rather than one of ours: every group's quads end up
    /// in the same frame-long `Vec` addressed by range, so a `Vec` returned
    /// here would be allocated per group only to be drained into that one.
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        commands: &[FlattenedCommand],
        scale_factor: f32,
        out: &mut Vec<PreparedImageQuad>,
    ) {
        out.extend(
            commands
                .iter()
                .filter_map(|cmd| self.prepare_single(device, queue, cmd, scale_factor)),
        );
    }

    /// Prepare a single image command.
    fn prepare_single(
        &mut self,
        device: &Device,
        queue: &Queue,
        cmd: &FlattenedCommand,
        scale_factor: f32,
    ) -> Option<PreparedImageQuad> {
        let (source, decoded, rect, content_fit, tint) = match &*cmd.command {
            DrawCommand::Image {
                source,
                decoded,
                rect,
                content_fit,
                tint,
            } => (source, decoded, rect, content_fit, tint),
            _ => return None,
        };

        // Extract scale from transform for SVG quality
        let transform_scale = cmd.world_transform.extract_scale().max(1.0);

        // Rasterize SVGs at the size they are displayed at; ContentFit::None
        // shows the image at intrinsic size, so no target there.
        let svg_target = (*content_fit != ContentFit::None).then_some((rect.width, rect.height));

        // Get or create the texture
        // An SVG is rasterised at the scale it is shown at; a raster image is
        // uploaded at its own size.
        let render_scale = if source.is_svg() {
            transform_scale * scale_factor * SVG_QUALITY_MULTIPLIER
        } else {
            1.0
        };

        let cached = self.get_or_create_texture(
            device,
            queue,
            source,
            render_scale,
            svg_target,
            decoded.as_ref(),
        )?;

        // Calculate display rect and UV coordinates based on content fit
        let (display_rect, (u_min, v_min, u_max, v_max)) = self.calculate_display_rect_and_uv(
            rect,
            cached.intrinsic_width,
            cached.intrinsic_height,
            *content_fit,
        );
        // From the image's own coordinates to the texture it is drawn from,
        // which for one on an atlas page is a corner of the page.
        let [x, y, width, height] = cached.uv_rect;
        let uv = (
            x + u_min * width,
            y + v_min * height,
            x + u_max * width,
            y + v_max * height,
        );

        // An image is cut in the clip's own space, so a turned clip cuts the
        // turned shape.
        let clip = cmd
            .clip()
            .map_or(QuadClip::NONE, |clip| QuadClip::shape(&clip, scale_factor));

        let corners = self.compute_vertices(
            &display_rect,
            &cmd.world_transform,
            uv,
            scale_factor,
            clip,
            opacity_and_tint(cmd.opacity, *tint),
        );

        Some(PreparedImageQuad {
            quad: self.quad.push(corners),
            texture: cached,
        })
    }

    /// Calculate the display rect and UV coordinates based on content fit.
    fn calculate_display_rect_and_uv(
        &self,
        rect: &Rect,
        intrinsic_width: f32,
        intrinsic_height: f32,
        content_fit: ContentFit,
    ) -> (Rect, (f32, f32, f32, f32)) {
        let img_width = intrinsic_width;
        let img_height = intrinsic_height;
        let img_aspect = img_width / img_height;
        let widget_aspect = rect.width / rect.height;

        match content_fit {
            ContentFit::Fill => {
                // Stretch to fill - use full rect and full UV
                (*rect, (0.0, 0.0, 1.0, 1.0))
            }
            ContentFit::Contain => {
                // Fit within bounds, preserving aspect ratio (letterbox/pillarbox)
                let (scaled_w, scaled_h) = if widget_aspect > img_aspect {
                    // Widget is wider - fit to height, center horizontally
                    (rect.height * img_aspect, rect.height)
                } else {
                    // Widget is taller - fit to width, center vertically
                    (rect.width, rect.width / img_aspect)
                };
                let offset_x = (rect.width - scaled_w) / 2.0;
                let offset_y = (rect.height - scaled_h) / 2.0;
                (
                    Rect::new(rect.x + offset_x, rect.y + offset_y, scaled_w, scaled_h),
                    (0.0, 0.0, 1.0, 1.0),
                )
            }
            ContentFit::Cover => {
                // Cover bounds, cropping as needed (adjust UV to crop)
                let (u_min, v_min, u_max, v_max) = if widget_aspect > img_aspect {
                    // Widget is wider - crop top/bottom
                    let visible_height = img_aspect / widget_aspect;
                    let v_offset = (1.0 - visible_height) / 2.0;
                    (0.0, v_offset, 1.0, v_offset + visible_height)
                } else {
                    // Widget is taller - crop left/right
                    let visible_width = widget_aspect / img_aspect;
                    let u_offset = (1.0 - visible_width) / 2.0;
                    (u_offset, 0.0, u_offset + visible_width, 1.0)
                };
                (*rect, (u_min, v_min, u_max, v_max))
            }
            ContentFit::None => {
                // Use intrinsic size, centered in widget
                let offset_x = (rect.width - img_width) / 2.0;
                let offset_y = (rect.height - img_height) / 2.0;
                (
                    Rect::new(rect.x + offset_x, rect.y + offset_y, img_width, img_height),
                    (0.0, 0.0, 1.0, 1.0),
                )
            }
        }
    }

    /// Compute vertex positions by applying world transform to local corners.
    fn compute_vertices(
        &self,
        rect: &Rect,
        world_transform: &crate::transform::Transform,
        uv: (f32, f32, f32, f32),
        scale_factor: f32,
        clip: QuadClip,
        (opacity, tint): (f32, [f32; 4]),
    ) -> [TexturedVertex; 4] {
        // Get local rect corners
        let local_corners = [
            (rect.x, rect.y),                            // top-left
            (rect.x + rect.width, rect.y),               // top-right
            (rect.x, rect.y + rect.height),              // bottom-left
            (rect.x + rect.width, rect.y + rect.height), // bottom-right
        ];

        // Apply world_transform to get screen coordinates (in logical pixels)
        // Then multiply by scale_factor to get physical pixels
        let screen_corners: [(f32, f32); 4] = [
            {
                let (sx, sy) =
                    world_transform.transform_point(local_corners[0].0, local_corners[0].1);
                (sx * scale_factor, sy * scale_factor)
            },
            {
                let (sx, sy) =
                    world_transform.transform_point(local_corners[1].0, local_corners[1].1);
                (sx * scale_factor, sy * scale_factor)
            },
            {
                let (sx, sy) =
                    world_transform.transform_point(local_corners[2].0, local_corners[2].1);
                (sx * scale_factor, sy * scale_factor)
            },
            {
                let (sx, sy) =
                    world_transform.transform_point(local_corners[3].0, local_corners[3].1);
                (sx * scale_factor, sy * scale_factor)
            },
        ];

        let (u_min, v_min, u_max, v_max) = uv;
        let uvs = [
            [u_min, v_min],
            [u_max, v_min],
            [u_min, v_max],
            [u_max, v_max],
        ];

        std::array::from_fn(|i| {
            let (x, y) = screen_corners[i];
            TexturedVertex::corner(self.quad.to_ndc(x, y), uvs[i], (x, y), &clip, opacity, tint)
        })
    }

    /// Render the prepared image quads.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>, quads: &'a [PreparedImageQuad]) {
        self.quad.draw(render_pass, quads, Pipeline::Images);
    }
}
