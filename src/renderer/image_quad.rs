//! Textured quad rendering for images with transform support.
//!
//! This module renders images as textured quads with full transform support
//! (rotation, scale, translate). Textures are cached for performance.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer as WgpuBuffer, Device, Extent3d, Queue, RenderPass,
    RenderPipeline, Sampler, Texture, TextureDimension, TextureFormat, TextureUsages,
};

use super::commands::DrawCommand;
use super::constants::{IMAGE_HASH_SAMPLE_SIZE, SVG_QUALITY_MULTIPLIER};
use super::flatten::FlattenedCommand;
use super::textured_vertex::{TexturedVertex, to_ndc};
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

/// Renderer for images as textured quads.
pub struct ImageQuadRenderer {
    // Quad rendering pipeline
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,

    // Shared index buffer (vertices are per-quad)
    index_buffer: WgpuBuffer,

    // Texture cache
    texture_cache: HashMap<CacheKey, Arc<CachedTexture>>,
    current_frame: u64,
    max_cache_size: usize,

    // Screen dimensions for NDC conversion
    screen_width: f32,
    screen_height: f32,
}

impl ImageQuadRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        // Load shader from dedicated file
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ImageQuad Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("textured_quad_shader.wgsl").into()),
        });

        // Create texture bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ImageQuad Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ImageQuad Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        // Create render pipeline
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ImageQuad Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TexturedVertex::desc()],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Create sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ImageQuad Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Create index buffer
        let indices: [u16; 6] = [0, 1, 2, 1, 3, 2];
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ImageQuad Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            index_buffer,
            texture_cache: HashMap::new(),
            current_frame: 0,
            max_cache_size: 64,
            screen_width: 800.0,
            screen_height: 600.0,
        }
    }

    /// Update screen dimensions for NDC conversion.
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
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
    fn get_or_create_texture(
        &mut self,
        device: &Device,
        queue: &Queue,
        source: &ImageSource,
        transform_scale: f32,
        scale_factor: f32,
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
        let key = CacheKey {
            source_hash,
            render_scale: if is_svg { quantized_scale } else { 0 },
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
        let texture = self.load_texture(device, queue, source, render_scale)?;

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
    ) -> Option<CachedTexture> {
        // Use Rgba8Unorm to pass colors through without sRGB conversion
        let format = TextureFormat::Rgba8Unorm;

        match source {
            ImageSource::Path(path) => {
                let img = image::open(path).ok()?;
                let rgba = img.to_rgba8();
                self.upload_raster(device, queue, &format, &rgba)
            }
            ImageSource::Bytes(bytes) => {
                let img = image::load_from_memory(bytes).ok()?;
                let rgba = img.to_rgba8();
                self.upload_raster(device, queue, &format, &rgba)
            }
            ImageSource::SvgPath(path) => {
                let data = std::fs::read(path).ok()?;
                self.load_svg(device, queue, &format, &data, render_scale)
            }
            ImageSource::SvgBytes(bytes) => {
                self.load_svg(device, queue, &format, bytes, render_scale)
            }
        }
    }

    /// Upload a raster image to GPU.
    fn upload_raster(
        &self,
        device: &Device,
        queue: &Queue,
        format: &TextureFormat,
        rgba: &image::RgbaImage,
    ) -> Option<CachedTexture> {
        let (width, height) = rgba.dimensions();
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
            rgba.as_raw(),
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

    /// Load and rasterize an SVG.
    fn load_svg(
        &self,
        device: &Device,
        queue: &Queue,
        format: &TextureFormat,
        bytes: &[u8],
        scale: f32,
    ) -> Option<CachedTexture> {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()?;
        let size = tree.size();

        let intrinsic_width = size.width() as u32;
        let intrinsic_height = size.height() as u32;

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
        commands: &[&FlattenedCommand],
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
        let (source, rect, content_fit) = match &cmd.command {
            DrawCommand::Image {
                source,
                rect,
                content_fit,
            } => (source, rect, content_fit),
            _ => return None,
        };

        // Extract scale from transform for SVG quality
        let transform_scale = cmd.world_transform.extract_scale().max(1.0);

        // Get or create the texture
        let cached =
            self.get_or_create_texture(device, queue, source, transform_scale, scale_factor)?;

        // Create bind group
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ImageQuad Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cached.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

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
                [clip.corner_radius * scale_factor, clip.curvature, 0.0, 0.0],
            )
        } else {
            // No clipping (width/height = 0 disables clipping in shader)
            ([0.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0])
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
                position: to_ndc(
                    screen_corners[0].0,
                    screen_corners[0].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [u_min, v_min],
                screen_pos: [screen_corners[0].0, screen_corners[0].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[1].0,
                    screen_corners[1].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [u_max, v_min],
                screen_pos: [screen_corners[1].0, screen_corners[1].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[2].0,
                    screen_corners[2].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [u_min, v_max],
                screen_pos: [screen_corners[2].0, screen_corners[2].1],
                clip_rect,
                clip_params,
            },
            TexturedVertex {
                position: to_ndc(
                    screen_corners[3].0,
                    screen_corners[3].1,
                    self.screen_width,
                    self.screen_height,
                ),
                uv: [u_max, v_max],
                screen_pos: [screen_corners[3].0, screen_corners[3].1],
                clip_rect,
                clip_params,
            },
        ]
    }

    /// Render the prepared image quads.
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>, quads: &'a [PreparedImageQuad]) {
        if quads.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        for quad in quads {
            render_pass.set_bind_group(0, &quad.bind_group, &[]);
            render_pass.set_vertex_buffer(0, quad.vertex_buffer.slice(..));
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }
}
