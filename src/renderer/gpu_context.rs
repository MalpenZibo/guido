use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use wgpu::{Device, Instance, Queue, Surface, SurfaceConfiguration};

pub struct GpuContext {
    pub instance: Instance,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
}

impl Default for GpuContext {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuContext {
    pub fn new() -> Self {
        Self::try_new().expect("no usable Vulkan adapter (see log for the reason)")
    }

    /// The same context, for a caller that has something better to do than die
    /// when there is no GPU to be had: a test that skips itself, a tool that
    /// reports. The reason is logged at error level either way.
    pub fn try_new() -> Option<Self> {
        let instance = Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: false,
            })) {
                Ok(adapter) => adapter,
                Err(e) => {
                    log::error!("No Vulkan adapter: {e}");
                    return None;
                }
            };

        let (device, queue) =
            match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("Guido Device"),
                required_features: wgpu::Features::TEXTURE_FORMAT_16BIT_NORM,
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
                experimental_features: wgpu::ExperimentalFeatures::default(),
                trace: wgpu::Trace::Off,
            })) {
                Ok(pair) => pair,
                Err(e) => {
                    log::error!(
                        "Adapter {:?} cannot make a device: {e}",
                        adapter.get_info().name
                    );
                    return None;
                }
            };

        Some(Self {
            instance,
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    pub fn create_surface<W>(&self, window: W, width: u32, height: u32) -> SurfaceState
    where
        W: HasWindowHandle + HasDisplayHandle,
    {
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::from_window(&window).unwrap())
                .expect("Failed to create surface")
        };

        // Get surface capabilities and use preferred format
        let caps = surface.get_capabilities(
            &pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .unwrap(),
        );

        // Select a renderable format - prefer Bgra8Unorm or Rgba8Unorm for compatibility
        let format = caps
            .formats
            .iter()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .copied()
            .unwrap_or_else(|| {
                // Fallback to first format that is not 16-bit
                caps.formats
                    .iter()
                    .find(|f| !matches!(f, wgpu::TextureFormat::Rgba16Unorm))
                    .copied()
                    .unwrap_or(caps.formats[0])
            });

        log::info!("Using surface format: {:?}", format);

        let config = SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps
                .alpha_modes
                .iter()
                .find(|m| **m == wgpu::CompositeAlphaMode::PreMultiplied)
                .copied()
                .unwrap_or_else(|| {
                    caps.alpha_modes
                        .first()
                        .copied()
                        .unwrap_or(wgpu::CompositeAlphaMode::Auto)
                }),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&self.device, &config);

        SurfaceState {
            surface,
            config,
            device: self.device.clone(),
            queue: self.queue.clone(),
        }
    }
}

pub struct SurfaceState {
    pub surface: Surface<'static>,
    pub config: SurfaceConfiguration,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
}

impl SurfaceState {
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn width(&self) -> u32 {
        self.config.width
    }

    pub fn height(&self) -> u32 {
        self.config.height
    }
}

/// Where a drawn frame lands.
///
/// A swapchain is a compositor's: it hands out a texture, takes it back, and
/// shows it. A texture is the caller's, and nobody shows it — which is the only
/// difference, and the reason a frame can be inspected without a compositor at
/// all. Everything upstream of here draws the same commands either way.
pub enum RenderTarget {
    /// The compositor's swapchain. Acquired per frame, presented after.
    Swapchain(SurfaceState),
    /// A texture the caller owns and can read back.
    #[cfg(any(test, feature = "testing"))]
    Offscreen(OffscreenTarget),
}

/// A texture a frame is drawn into, with the handles needed to read it back.
///
/// The texture is the only state: it already knows its size and its format, so
/// storing either beside it would be a second copy of a fact that cannot drift
/// while it is derived.
#[cfg(any(test, feature = "testing"))]
pub struct OffscreenTarget {
    pub texture: wgpu::Texture,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
}

#[cfg(any(test, feature = "testing"))]
impl OffscreenTarget {
    /// The colour at one pixel, in the texture's own format.
    ///
    /// Reading a target back is the whole reason to draw into one, so it lives
    /// here rather than beside whoever asks: the row padding wgpu requires and
    /// the map-then-poll order are the sort of thing that is written correctly
    /// once and copied wrongly after.
    pub fn read_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let (width, height) = (self.texture.width(), self.texture.height());
        let bytes_per_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("guido offscreen readback"),
            size: (bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("readback never completed");
        let data = buffer.slice(..).get_mapped_range();
        let at = (y * bytes_per_row + x * 4) as usize;
        [data[at], data[at + 1], data[at + 2], data[at + 3]]
    }
}

impl RenderTarget {
    /// A target of `width` by `height` physical pixels, drawn into and kept.
    ///
    /// `COPY_SRC` because a target nobody presents is only useful if it can be
    /// read, and `Rgba8Unorm` because eight bits a channel makes an exact byte
    /// a fair thing to assert.
    #[cfg(any(test, feature = "testing"))]
    pub fn offscreen(gpu: &GpuContext, width: u32, height: u32) -> Self {
        Self::Offscreen(OffscreenTarget {
            texture: offscreen_texture(&gpu.device, width, height),
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
        })
    }

    pub fn width(&self) -> u32 {
        match self {
            Self::Swapchain(s) => s.width(),
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => o.texture.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Self::Swapchain(s) => s.height(),
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => o.texture.height(),
        }
    }

    /// A swapchain is reconfigured; a texture is replaced.
    ///
    /// Not a no-op for the texture, however tempting: `resolve_geometry` asks
    /// whether the target matches the frame and calls this when it does not, so
    /// a target that quietly declined would report a resize every frame for
    /// ever, repainting the whole tree each time and defeating the incremental
    /// paint the rest of the pipeline is built around.
    pub fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::Swapchain(s) => s.resize(width, height),
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => {
                if width > 0 && height > 0 {
                    o.texture = offscreen_texture(&o.device, width, height);
                }
            }
        }
    }

    pub fn device(&self) -> &Arc<Device> {
        match self {
            Self::Swapchain(s) => &s.device,
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => &o.device,
        }
    }

    pub fn queue(&self) -> &Arc<Queue> {
        match self {
            Self::Swapchain(s) => &s.queue,
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => &o.queue,
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Swapchain(s) => s.config.format,
            #[cfg(any(test, feature = "testing"))]
            Self::Offscreen(o) => o.texture.format(),
        }
    }
}

#[cfg(any(test, feature = "testing"))]
fn offscreen_texture(device: &Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("guido offscreen target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
