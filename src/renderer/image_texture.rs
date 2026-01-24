//! Image texture loading and caching for GPU rendering.
//!
//! This module handles loading raster images (PNG, JPEG, GIF, WebP) and
//! SVG vector graphics, uploading them to GPU textures for rendering.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::Arc;

use wgpu::{Device, Extent3d, Queue, Texture, TextureDimension, TextureFormat, TextureUsages};

use crate::widgets::image::ImageSource;

/// Quality multiplier for SVG rasterization.
/// Higher values produce crisper SVGs when scaled up.
pub const SVG_QUALITY_MULTIPLIER: f32 = 2.0;

/// A loaded image texture ready for rendering.
pub struct ImageTexture {
    /// The GPU texture containing the image
    pub texture: Texture,
    /// View for sampling the texture
    pub view: wgpu::TextureView,
    /// Width of the texture in pixels
    pub width: u32,
    /// Height of the texture in pixels
    pub height: u32,
    /// Original intrinsic size of the image (for layout)
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
    /// The scale factor used when rendering SVGs
    pub render_scale: f32,
}

/// Cache key for image textures.
#[derive(Clone, Debug)]
struct CacheKey {
    /// Hash of the source
    source_hash: u64,
    /// Scale at which the image was rendered (for SVGs)
    render_scale: u32, // Quantized to reduce cache entries
}

impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash && self.render_scale == other.render_scale
    }
}

impl Eq for CacheKey {}

impl Hash for CacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source_hash.hash(state);
        self.render_scale.hash(state);
    }
}

/// Cached image entry with LRU tracking.
struct CachedImage {
    texture: ImageTexture,
    last_used_frame: u64,
}

/// Image texture renderer with caching.
pub struct ImageTextureRenderer {
    cache: HashMap<CacheKey, CachedImage>,
    current_frame: u64,
    /// Maximum number of cached textures
    max_cache_size: usize,
}

impl ImageTextureRenderer {
    pub fn new(_format: TextureFormat) -> Self {
        Self {
            cache: HashMap::new(),
            current_frame: 0,
            max_cache_size: 64,
        }
    }

    /// Advance the frame counter (call once per frame).
    pub fn begin_frame(&mut self) {
        self.current_frame += 1;

        // Evict old entries if cache is too large
        if self.cache.len() > self.max_cache_size {
            self.evict_oldest();
        }
    }

    /// Evict the least recently used entries until under the limit.
    fn evict_oldest(&mut self) {
        let target_size = self.max_cache_size / 2;
        while self.cache.len() > target_size {
            // Find the oldest entry
            let oldest_key = self
                .cache
                .iter()
                .min_by_key(|(_, v)| v.last_used_frame)
                .map(|(k, _)| k.clone());

            if let Some(key) = oldest_key {
                self.cache.remove(&key);
            } else {
                break;
            }
        }
    }

    /// Hash bytes with improved sampling for collision resistance.
    ///
    /// For small images (<1024 bytes), hashes the entire content.
    /// For larger images, samples first/middle/last 256 bytes each.
    fn hash_bytes(bytes: &[u8], hasher: &mut impl Hasher) {
        bytes.len().hash(hasher);
        if bytes.len() < 1024 {
            bytes.hash(hasher);
            return;
        }
        // Sample: first 256 + middle 256 + last 256 bytes
        let sample = 256;
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

    /// Get or create a texture for the given image source.
    pub fn get_or_create(
        &mut self,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        source: &ImageSource,
        transform_scale: f32,
        scale_factor: f32,
    ) -> Option<&ImageTexture> {
        // Calculate render scale for SVGs
        let is_svg = source.is_svg();
        let render_scale = if is_svg {
            transform_scale * scale_factor * SVG_QUALITY_MULTIPLIER
        } else {
            1.0
        };

        // Quantize scale to reduce cache entries (round to 0.25 increments)
        let quantized_scale = (render_scale * 4.0).round() as u32;

        let source_hash = Self::hash_source(source);
        let key = CacheKey {
            source_hash,
            render_scale: if is_svg { quantized_scale } else { 0 },
        };

        // Check if we already have this texture cached
        if self.cache.contains_key(&key) {
            // Update last used frame and return
            if let Some(entry) = self.cache.get_mut(&key) {
                entry.last_used_frame = self.current_frame;
            }
            return self.cache.get(&key).map(|e| &e.texture);
        }

        // Load and create texture (not in cache)
        // Use Rgba8Unorm to pass colors through without sRGB conversion
        // (the framebuffer handles sRGB encoding)
        let format = TextureFormat::Rgba8Unorm;
        let texture = match source {
            ImageSource::Path(path) => Self::load_raster_file(&format, device, queue, path),
            ImageSource::Bytes(bytes) => Self::load_raster_bytes(&format, device, queue, bytes),
            ImageSource::SvgPath(path) => {
                Self::load_svg_file(&format, device, queue, path, render_scale)
            }
            ImageSource::SvgBytes(bytes) => {
                Self::load_svg_bytes(&format, device, queue, bytes, render_scale)
            }
        };

        if let Some(tex) = texture {
            self.cache.insert(
                key.clone(),
                CachedImage {
                    texture: tex,
                    last_used_frame: self.current_frame,
                },
            );
            return self.cache.get(&key).map(|e| &e.texture);
        }

        None
    }

    /// Get intrinsic dimensions of an image source without loading the full texture.
    pub fn get_intrinsic_size(&self, source: &ImageSource) -> Option<(u32, u32)> {
        crate::image_metadata::get_intrinsic_size(source)
    }

    /// Load a raster image from a file.
    fn load_raster_file(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        path: &Path,
    ) -> Option<ImageTexture> {
        let img = image::open(path).ok()?;
        Self::upload_raster(format, device, queue, &img.to_rgba8())
    }

    /// Load a raster image from bytes.
    fn load_raster_bytes(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        bytes: &[u8],
    ) -> Option<ImageTexture> {
        let img = image::load_from_memory(bytes).ok()?;
        Self::upload_raster(format, device, queue, &img.to_rgba8())
    }

    /// Upload raw RGBA data to a GPU texture.
    ///
    /// Returns the texture and view, or None if dimensions are zero.
    fn upload_rgba_texture(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        data: &[u8],
        width: u32,
        height: u32,
        label: &str,
    ) -> Option<(Texture, wgpu::TextureView)> {
        if width == 0 || height == 0 {
            return None;
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
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
            data,
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
        Some((texture, view))
    }

    /// Upload a raster image to a GPU texture.
    fn upload_raster(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        rgba: &image::RgbaImage,
    ) -> Option<ImageTexture> {
        let (width, height) = rgba.dimensions();
        let (texture, view) = Self::upload_rgba_texture(
            format,
            device,
            queue,
            rgba.as_raw(),
            width,
            height,
            "Image Texture",
        )?;

        Some(ImageTexture {
            texture,
            view,
            width,
            height,
            intrinsic_width: width,
            intrinsic_height: height,
            render_scale: 1.0,
        })
    }

    /// Load an SVG from a file and rasterize it.
    fn load_svg_file(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        path: &Path,
        scale: f32,
    ) -> Option<ImageTexture> {
        let data = std::fs::read(path).ok()?;
        Self::load_svg_bytes(format, device, queue, &data, scale)
    }

    /// Load an SVG from bytes and rasterize it.
    fn load_svg_bytes(
        format: &TextureFormat,
        device: &Arc<Device>,
        queue: &Arc<Queue>,
        bytes: &[u8],
        scale: f32,
    ) -> Option<ImageTexture> {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let size = tree.size();

        let intrinsic_width = size.width() as u32;
        let intrinsic_height = size.height() as u32;

        // Calculate scaled dimensions
        let scaled_width = (size.width() * scale).ceil() as u32;
        let scaled_height = (size.height() * scale).ceil() as u32;

        // Create a pixmap for rendering
        let mut pixmap = resvg::tiny_skia::Pixmap::new(scaled_width, scaled_height)?;

        // Create transform for scaling
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);

        // Render the SVG
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        // Upload to GPU - note: pixmap is already premultiplied alpha RGBA
        let (texture, view) = Self::upload_rgba_texture(
            format,
            device,
            queue,
            pixmap.data(),
            scaled_width,
            scaled_height,
            "SVG Texture",
        )?;

        Some(ImageTexture {
            texture,
            view,
            width: scaled_width,
            height: scaled_height,
            intrinsic_width,
            intrinsic_height,
            render_scale: scale,
        })
    }
}
