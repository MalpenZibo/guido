//! Textured quad rendering for images with transform support.
//!
//! This module renders images as textured quads with full transform support
//! (rotation, scale, translate). Textures are cached for performance.

use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, Buffer as WgpuBuffer, Device, Extent3d, Queue, RenderPass, Texture,
    TextureDimension, TextureFormat, TextureUsages,
};

use super::commands::DrawCommand;
use super::constants::SVG_QUALITY_MULTIPLIER;
use super::flatten::FlattenedCommand;
use super::textured_quad::{QuadDraw, TexturedQuadPipeline};
use super::textured_vertex::{QuadClip, TexturedVertex};
use crate::image_decode::{DecodeKey, DecodedImage, hash_sampled};
use crate::widgets::Rect;
use crate::widgets::image::{ContentFit, ImageSource};

/// A prepared image quad ready for rendering.
pub struct PreparedImageQuad {
    #[allow(dead_code)] // Kept alive for GPU usage
    texture: Rc<CachedTexture>,
    bind_group: BindGroup,
    /// Vertex buffer with pre-computed vertices in NDC
    vertex_buffer: WgpuBuffer,
}

impl QuadDraw for PreparedImageQuad {
    fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }

    fn vertex_buffer(&self) -> &WgpuBuffer {
        &self.vertex_buffer
    }
}

/// Cached texture data.
struct CachedTexture {
    texture: Texture,
    view: wgpu::TextureView,
    /// Original intrinsic dimensions
    intrinsic_width: u32,
    intrinsic_height: u32,
    /// When a frame last drew it. A `Cell` because the frame that draws it
    /// holds it through an `Rc` already.
    last_used: Cell<Instant>,
    /// The raster source it was uploaded from, whose decoded pixels went with
    /// the upload: the decode cache is told when this texture goes.
    decoded_from: Option<DecodeKey>,
}

impl CachedTexture {
    /// What it costs on the GPU, counted against the cache's budget.
    fn bytes(&self) -> usize {
        texture_bytes(&self.texture)
    }
}

impl Drop for CachedTexture {
    fn drop(&mut self) {
        if let Some(key) = self.decoded_from.take() {
            crate::image_decode::texture_evicted(key);
        }
    }
}

/// Cache key for image textures.
#[derive(Clone, Debug)]
struct CacheKey {
    /// Hash of the source
    source_hash: u64,
    /// Rasterization variant (for SVGs): quantized scale + target size.
    /// Raster images always use 0 — they decode at intrinsic size.
    svg_variant: u64,
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash && self.svg_variant == other.svg_variant
    }
}

impl Eq for CacheKey {}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source_hash.hash(state);
        self.svg_variant.hash(state);
    }
}

/// Renderer for images as textured quads.
pub struct ImageQuadRenderer {
    /// Pipeline, layout, sampler and index buffer — shared with the text
    /// renderer, which draws the same quad from a different texture.
    quad: TexturedQuadPipeline,

    // Texture cache
    texture_cache: FxHashMap<CacheKey, Rc<CachedTexture>>,
    /// When the frame being prepared started: what a texture drawn in it is
    /// stamped with.
    frame_started: Instant,
    /// The sum of every cached texture's `bytes`.
    cached_bytes: usize,
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
            texture_cache: FxHashMap::default(),
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
    }

    /// Bring the cache back under `budget` bytes, once the frame has drawn
    /// what it draws.
    ///
    /// After the frame rather than before it, so every texture this frame drew
    /// is stamped as drawn now: before it, a surface that sat idle for longer
    /// than [`KEEP_UNUSED`] would have its own images evicted by the frame
    /// about to draw them.
    pub fn trim(&mut self, budget: usize) {
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
            }
        }
    }

    /// Drop every texture, as eviction does, and say so as eviction does.
    #[cfg(feature = "testing")]
    pub(crate) fn forget_textures(&mut self) {
        self.texture_cache.clear();
        self.cached_bytes = 0;
    }

    /// Hash an image source for cache lookup.
    fn hash_source(source: &ImageSource) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();

        match source {
            ImageSource::Path(path) => {
                "path".hash(&mut hasher);
                path.hash(&mut hasher);
            }
            ImageSource::Bytes(bytes) => {
                "bytes".hash(&mut hasher);
                hash_sampled(bytes, &mut hasher);
            }
            ImageSource::Rgba {
                width,
                height,
                pixels,
            } => {
                "rgba".hash(&mut hasher);
                width.hash(&mut hasher);
                height.hash(&mut hasher);
                hash_sampled(pixels, &mut hasher);
            }
            ImageSource::SvgPath(path) => {
                "svg_path".hash(&mut hasher);
                path.hash(&mut hasher);
            }
            ImageSource::SvgBytes(bytes) => {
                "svg_bytes".hash(&mut hasher);
                hash_sampled(bytes, &mut hasher);
            }
        }

        hasher.finish()
    }

    /// Get or create a cached texture for the given source.
    ///
    /// `svg_target` is the widget rect the SVG will be displayed in
    /// (logical pixels): the rasterization is sized to it instead of the
    /// SVG's intrinsic size, so a 16px icon costs a 16px raster no matter
    /// how large its viewBox is. `None` (ContentFit::None) keeps the
    /// intrinsic size, which is what that fit mode displays.
    fn get_or_create_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        render_scale: f32,
        svg_target: Option<(f32, f32)>,
        decoded: Option<&DecodedImage>,
    ) -> Option<Rc<CachedTexture>> {
        let is_svg = source.is_svg();

        // Quantize scale to reduce cache entries (round to 0.25 increments)
        let quantized_scale = (render_scale * 4.0).round() as u32;

        let source_hash = Self::hash_source(source);
        // SVG rasterization variant: scale + quantized target size (the
        // same icon shown at 16px and 48px needs two textures).
        let svg_variant = if is_svg {
            let (qw, qh) = match svg_target {
                Some((w, h)) => (w.round().max(1.0) as u64, h.round().max(1.0) as u64),
                None => (0, 0),
            };
            (quantized_scale as u64) << 40 | qw << 20 | qh
        } else {
            0
        };
        let key = CacheKey {
            source_hash,
            svg_variant,
        };

        // Check if we already have this texture cached
        if let Some(cached) = self.texture_cache.get(&key) {
            cached.last_used.set(self.frame_started);
            return Some(cached.clone());
        }

        // Load and create texture
        let mut texture =
            self.load_texture(device, queue, source, render_scale, svg_target, decoded)?;
        if decoded.is_some() {
            texture.decoded_from = DecodeKey::of(source);
        }

        let cached = Rc::new(texture);
        self.cached_bytes += cached.bytes();
        self.texture_cache.insert(key, cached.clone());
        Some(cached)
    }

    /// Load and upload a texture to the GPU.
    ///
    /// A raster `Path` or `Bytes` source arrives already decoded — the worker
    /// in `image_decode` did that off the frame — so this only uploads, and
    /// taking the pixels to upload them is what drops them from the cache.
    /// Pixels that were already taken are gone: this draws nothing and reports
    /// the texture missing, which sends the source back to the worker.
    /// Decoding here instead is the stall that module exists to remove.
    fn load_texture(
        &self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        render_scale: f32,
        svg_target: Option<(f32, f32)>,
        decoded: Option<&DecodedImage>,
    ) -> Option<CachedTexture> {
        // Use Rgba8Unorm to pass colors through without sRGB conversion
        let format = TextureFormat::Rgba8Unorm;

        match source {
            ImageSource::Path(_) | ImageSource::Bytes(_) => {
                let Some(pixels) = decoded.and_then(DecodedImage::take) else {
                    crate::image_decode::texture_missing(source);
                    return None;
                };
                self.upload_raster(
                    device,
                    queue,
                    &format,
                    pixels.width,
                    pixels.height,
                    &pixels.rgba,
                )
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
                self.upload_raster(device, queue, &format, *width, *height, pixels)
            }
            ImageSource::SvgPath(path) => {
                let data = match std::fs::read(path) {
                    Ok(data) => data,
                    Err(e) => {
                        log::warn!("Failed to read SVG {}: {e}", path.display());
                        return None;
                    }
                };
                self.load_svg(device, queue, &format, &data, render_scale, svg_target)
            }
            ImageSource::SvgBytes(bytes) => {
                self.load_svg(device, queue, &format, bytes, render_scale, svg_target)
            }
        }
    }

    /// Upload raw RGBA8 pixel data to GPU.
    fn upload_raster(
        &self,
        device: &Device,
        queue: &Queue,
        format: &TextureFormat,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Option<CachedTexture> {
        if width == 0 || height == 0 {
            return None;
        }

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
            format: *format,
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

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Some(CachedTexture {
            texture,
            view,
            intrinsic_width: width,
            intrinsic_height: height,
            last_used: Cell::new(self.frame_started),
            decoded_from: None,
        })
    }

    /// Fallback when the `svg` feature is disabled: SVG sources fail to
    /// decode with a warning instead of failing to compile.
    #[cfg(not(feature = "svg"))]
    fn load_svg(
        &self,
        _device: &Device,
        _queue: &Queue,
        _format: &TextureFormat,
        _bytes: &[u8],
        _scale: f32,
        _target: Option<(f32, f32)>,
    ) -> Option<CachedTexture> {
        log::warn!("SVG image used but the `svg` feature is disabled");
        None
    }

    /// Load and rasterize an SVG.
    ///
    /// With a `target` (the widget rect in logical pixels) the raster is
    /// sized to what will actually be displayed rather than the SVG's
    /// intrinsic size — a 16px weather icon with a 104px viewBox costs a
    /// 16px raster, not a 104px one. This is both faster (rasterization
    /// cost scales with pixels) and sharper (no GPU minification).
    #[cfg(feature = "svg")]
    fn load_svg(
        &self,
        device: &Device,
        queue: &Queue,
        format: &TextureFormat,
        bytes: &[u8],
        scale: f32,
        target: Option<(f32, f32)>,
    ) -> Option<CachedTexture> {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let size = tree.size();

        let intrinsic_width = size.width() as u32;
        let intrinsic_height = size.height() as u32;

        // Fit the raster to the display target, preserving aspect ratio
        // (contain). `scale` already carries transform scale, HiDPI factor
        // and the quality multiplier.
        let scale = match target {
            Some((tw, th)) if tw >= 1.0 && th >= 1.0 => {
                scale * (tw / size.width()).min(th / size.height())
            }
            _ => scale,
        };

        // Calculate scaled dimensions
        let scaled_width = (size.width() * scale).ceil() as u32;
        let scaled_height = (size.height() * scale).ceil() as u32;

        if scaled_width == 0 || scaled_height == 0 {
            return None;
        }

        // Create a pixmap for rendering
        let mut pixmap = resvg::tiny_skia::Pixmap::new(scaled_width, scaled_height)?;

        // Create transform for scaling
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);

        // Render the SVG
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        // Upload to GPU
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("SVG Texture"),
            size: Extent3d {
                width: scaled_width,
                height: scaled_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: *format,
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
            pixmap.data(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * scaled_width),
                rows_per_image: Some(scaled_height),
            },
            Extent3d {
                width: scaled_width,
                height: scaled_height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Some(CachedTexture {
            texture,
            view,
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
        let (source, decoded, rect, content_fit) = match &*cmd.command {
            DrawCommand::Image {
                source,
                decoded,
                rect,
                content_fit,
            } => (source, decoded, rect, content_fit),
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

        // Create bind group
        let bind_group = self
            .quad
            .bind_texture(device, &cached.view, "ImageQuad Bind Group");

        // Calculate display rect and UV coordinates based on content fit
        let (display_rect, uv) = self.calculate_display_rect_and_uv(
            rect,
            cached.intrinsic_width,
            cached.intrinsic_height,
            *content_fit,
        );

        // An image is cut in the clip's own space, so a turned clip cuts the
        // turned shape.
        let clip = cmd
            .clip()
            .map_or(QuadClip::NONE, |clip| QuadClip::shape(&clip, scale_factor));

        let vertices = self.compute_vertices(
            &display_rect,
            &cmd.world_transform,
            uv,
            scale_factor,
            clip,
            cmd.opacity,
        );

        // Create vertex buffer
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ImageQuad Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Some(PreparedImageQuad {
            texture: cached,
            bind_group,
            vertex_buffer,
        })
    }

    /// Calculate the display rect and UV coordinates based on content fit.
    fn calculate_display_rect_and_uv(
        &self,
        rect: &Rect,
        intrinsic_width: u32,
        intrinsic_height: u32,
        content_fit: ContentFit,
    ) -> (Rect, (f32, f32, f32, f32)) {
        let img_width = intrinsic_width as f32;
        let img_height = intrinsic_height as f32;
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
        opacity: f32,
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
            TexturedVertex::corner(self.quad.to_ndc(x, y), uvs[i], (x, y), &clip, opacity)
        })
    }

    /// Render the prepared image quads.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>, quads: &'a [PreparedImageQuad]) {
        self.quad.draw(render_pass, quads);
    }
}
