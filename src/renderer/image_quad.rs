//! Textured quad rendering for images with transform support.
//!
//! This module renders images as textured quads with full transform support
//! (rotation, scale, translate). Textures are cached for performance.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, Buffer as WgpuBuffer, Device, Extent3d, Queue, RenderPass, Texture,
    TextureDimension, TextureFormat, TextureUsages,
};

use super::commands::DrawCommand;
use super::constants::{IMAGE_HASH_SAMPLE_SIZE, SVG_QUALITY_MULTIPLIER};
use super::flatten::FlattenedCommand;
use super::gpu::NO_CLIP_RECT;
use super::textured_quad::{QuadDraw, TexturedQuadPipeline};
use super::textured_vertex::TexturedVertex;
use crate::widgets::Rect;
use crate::widgets::image::{ContentFit, ImageSource};

/// A prepared image quad ready for rendering.
pub struct PreparedImageQuad {
    #[allow(dead_code)] // Kept alive for GPU usage
    texture: Arc<CachedTexture>,
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
    #[allow(dead_code)] // Kept alive for GPU usage
    texture: Texture,
    view: wgpu::TextureView,
    /// Original intrinsic dimensions
    intrinsic_width: u32,
    intrinsic_height: u32,
    /// Last frame this texture was used
    last_used_frame: u64,
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
    texture_cache: HashMap<CacheKey, Arc<CachedTexture>>,
    current_frame: u64,
    max_cache_size: usize,
}

impl ImageQuadRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        Self {
            quad: TexturedQuadPipeline::new(device, format, "ImageQuad"),
            texture_cache: HashMap::new(),
            current_frame: 0,
            max_cache_size: 64,
        }
    }

    /// Update screen dimensions for NDC conversion.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.quad.set_screen_size(width, height);
    }

    /// Begin a new frame (for cache management).
    pub fn begin_frame(&mut self) {
        self.current_frame += 1;

        // Evict old entries if cache is too large
        if self.texture_cache.len() > self.max_cache_size {
            self.evict_oldest();
        }
    }

    /// Evict the least recently used entries until under the limit.
    fn evict_oldest(&mut self) {
        let target_size = self.max_cache_size / 2;
        while self.texture_cache.len() > target_size {
            let oldest_key = self
                .texture_cache
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k.clone());

            if let Some(key) = oldest_key {
                self.texture_cache.remove(&key);
            } else {
                break;
            }
        }
    }

    /// Hash bytes with improved sampling for collision resistance.
    fn hash_bytes(bytes: &[u8], hasher: &mut impl Hasher) {
        bytes.len().hash(hasher);
        if bytes.len() < 1024 {
            bytes.hash(hasher);
            return;
        }
        // Sample: first + middle + last bytes for collision resistance
        let sample = IMAGE_HASH_SAMPLE_SIZE;
        bytes[..sample].hash(hasher);
        let mid = bytes.len() / 2 - sample / 2;
        bytes[mid..mid + sample].hash(hasher);
        bytes[bytes.len() - sample..].hash(hasher);
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
                Self::hash_bytes(bytes, &mut hasher);
            }
            ImageSource::Rgba {
                width,
                height,
                pixels,
            } => {
                "rgba".hash(&mut hasher);
                width.hash(&mut hasher);
                height.hash(&mut hasher);
                Self::hash_bytes(pixels, &mut hasher);
            }
            ImageSource::SvgPath(path) => {
                "svg_path".hash(&mut hasher);
                path.hash(&mut hasher);
            }
            ImageSource::SvgBytes(bytes) => {
                "svg_bytes".hash(&mut hasher);
                Self::hash_bytes(bytes, &mut hasher);
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
        transform_scale: f32,
        scale_factor: f32,
        svg_target: Option<(f32, f32)>,
    ) -> Option<Arc<CachedTexture>> {
        let is_svg = source.is_svg();
        let render_scale = if is_svg {
            transform_scale * scale_factor * SVG_QUALITY_MULTIPLIER
        } else {
            1.0
        };

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
        if let Some(cached) = self.texture_cache.get_mut(&key) {
            // Update last used frame via Arc::get_mut if possible
            if let Some(inner) = Arc::get_mut(cached) {
                inner.last_used_frame = self.current_frame;
            }
            return Some(cached.clone());
        }

        // Load and create texture
        let texture = self.load_texture(device, queue, source, render_scale, svg_target)?;

        let cached = Arc::new(texture);
        self.texture_cache.insert(key, cached.clone());
        Some(cached)
    }

    /// Load and upload a texture to the GPU.
    fn load_texture(
        &self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        render_scale: f32,
        svg_target: Option<(f32, f32)>,
    ) -> Option<CachedTexture> {
        // Use Rgba8Unorm to pass colors through without sRGB conversion
        let format = TextureFormat::Rgba8Unorm;

        match source {
            ImageSource::Path(path) => {
                // Decode failures must be loud: a missing decoder feature
                // (e.g. `webp` disabled) or a bad file otherwise degrades to
                // a silently empty box.
                let img = match image::open(path) {
                    Ok(img) => img,
                    Err(e) => {
                        log::warn!("Failed to decode image {}: {e}", path.display());
                        return None;
                    }
                };
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                self.upload_raster(device, queue, &format, width, height, rgba.as_raw())
            }
            ImageSource::Bytes(bytes) => {
                let img = match image::load_from_memory(bytes) {
                    Ok(img) => img,
                    Err(e) => {
                        log::warn!("Failed to decode in-memory image: {e}");
                        return None;
                    }
                };
                let rgba = img.to_rgba8();
                let (width, height) = rgba.dimensions();
                self.upload_raster(device, queue, &format, width, height, rgba.as_raw())
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
            last_used_frame: self.current_frame,
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
            last_used_frame: self.current_frame,
        })
    }

    /// Prepare image commands for rendering.
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        commands: &[FlattenedCommand],
        scale_factor: f32,
    ) -> Vec<PreparedImageQuad> {
        commands
            .iter()
            .filter_map(|cmd| self.prepare_single(device, queue, cmd, scale_factor))
            .collect()
    }

    /// Prepare a single image command.
    fn prepare_single(
        &mut self,
        device: &Device,
        queue: &Queue,
        cmd: &FlattenedCommand,
        scale_factor: f32,
    ) -> Option<PreparedImageQuad> {
        let (source, rect, content_fit) = match &*cmd.command {
            DrawCommand::Image {
                source,
                rect,
                content_fit,
            } => (source, rect, content_fit),
            _ => return None,
        };

        // Extract scale from transform for SVG quality
        let transform_scale = cmd.world_transform.extract_scale().max(1.0);

        // Rasterize SVGs at the size they are displayed at; ContentFit::None
        // shows the image at intrinsic size, so no target there.
        let svg_target = (*content_fit != ContentFit::None).then_some((rect.width, rect.height));

        // Get or create the texture
        let cached = self.get_or_create_texture(
            device,
            queue,
            source,
            transform_scale,
            scale_factor,
            svg_target,
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

        // Extract clip data (scale to physical pixels)
        let (clip_rect, clip_params) = if let Some(ref clip) = cmd.clip {
            (
                [
                    clip.rect.x * scale_factor,
                    clip.rect.y * scale_factor,
                    clip.rect.width * scale_factor,
                    clip.rect.height * scale_factor,
                ],
                [
                    clip.corner_radius.top_left * scale_factor,
                    clip.corner_radius.top_right * scale_factor,
                    clip.corner_radius.bottom_right * scale_factor,
                    clip.corner_radius.bottom_left * scale_factor,
                ],
            )
        } else {
            // No clipping
            (NO_CLIP_RECT, [0.0; 4])
        };

        // Transform corners from local to screen coordinates
        let vertices = self.compute_vertices(
            &display_rect,
            &cmd.world_transform,
            uv,
            scale_factor,
            clip_rect,
            clip_params,
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
        clip_rect: [f32; 4],
        clip_params: [f32; 4],
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

        // Convert to NDC and create vertices with clip data
        [
            TexturedVertex {
                position: self.quad.to_ndc(screen_corners[0].0, screen_corners[0].1),
                uv: [u_min, v_min],
                screen_pos: [screen_corners[0].0, screen_corners[0].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: self.quad.to_ndc(screen_corners[1].0, screen_corners[1].1),
                uv: [u_max, v_min],
                screen_pos: [screen_corners[1].0, screen_corners[1].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: self.quad.to_ndc(screen_corners[2].0, screen_corners[2].1),
                uv: [u_min, v_max],
                screen_pos: [screen_corners[2].0, screen_corners[2].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: self.quad.to_ndc(screen_corners[3].0, screen_corners[3].1),
                uv: [u_max, v_max],
                screen_pos: [screen_corners[3].0, screen_corners[3].1],
                clip_rect,
                clip_params,
            },
        ]
    }

    /// Render the prepared image quads.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>, quads: &'a [PreparedImageQuad]) {
        self.quad.draw(render_pass, quads);
    }
}
