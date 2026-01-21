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
        let instance = Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("Failed to find GPU adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Guido Device"),
            required_features: wgpu::Features::TEXTURE_FORMAT_16BIT_NORM,
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("Failed to create device");

        Self {
            instance,
            device: Arc::new(device),
            queue: Arc::new(queue),
        }
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
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
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
