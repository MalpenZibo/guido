//! Backdrop blur: filtering the render target in place.
//!
//! The effect reads pixels that have already been drawn, which a render pass
//! cannot do to its own attachment. So the frame is drawn into an offscreen
//! colour target, each blur ends the pass, filters its region, and the pass
//! resumes with `LoadOp::Load`; the target is blitted to the swapchain once at
//! the end. Frames with no backdrop blur skip all of it and draw straight to
//! the swapchain, so the machinery costs nothing when unused.
//!
//! Per region: downsample into a working texture (rendering into a smaller
//! target with a linear sampler is already a box filter), two separable
//! gaussian passes, then composite back masked by the container's rounded
//! shape. Working at a quarter resolution is what keeps a wide radius cheap —
//! the same reason compositors downsample before blurring.

use wgpu::util::DeviceExt;

use crate::widgets::Rect;

use super::commands::CornerRadii;

/// How much the working texture is shrunk before blurring.
///
/// The downsample is a filter in its own right, so the gaussian afterwards
/// only smooths what the shrink left behind — at a sixteenth of the fragments.
const WORKING_SCALE: u32 = 4;

/// Frames without any backdrop effect before the offscreen target is freed.
const IDLE_FRAMES_BEFORE_RELEASE: u32 = 120;

/// Cap on gaussian taps per axis. Beyond this the shrink is doing the work
/// anyway, and an unbounded loop would let one huge radius stall a frame.
const MAX_TAPS: f32 = 24.0;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    src_rect: [f32; 4],
    direction: [f32; 2],
    sigma: f32,
    taps: f32,
    dst_size: [f32; 2],
    curvature: f32,
    _pad: f32,
    radii: [f32; 4],
}

/// A blur to apply, resolved to physical pixels.
#[derive(Debug, Clone, Copy)]
pub struct BackdropRegion {
    /// Region on the target, in physical pixels.
    pub rect: Rect,
    /// Blur radius in physical pixels.
    pub radius: f32,
    pub radii: CornerRadii,
    pub curvature: f32,
}

impl BackdropRegion {
    /// Clamp to the target so a card running off-screen cannot ask for a
    /// viewport wgpu will reject.
    fn clamped(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        let x = self.rect.x.floor().max(0.0) as u32;
        let y = self.rect.y.floor().max(0.0) as u32;
        let right = (self.rect.x + self.rect.width).ceil().max(0.0) as u32;
        let bottom = (self.rect.y + self.rect.height).ceil().max(0.0) as u32;
        let right = right.min(width);
        let bottom = bottom.min(height);
        if x >= right || y >= bottom {
            return None;
        }
        Some((x, y, right - x, bottom - y))
    }
}

/// Offscreen target plus the working textures the blur ping-pongs between.
struct Targets {
    width: u32,
    height: u32,
    /// Size of the working textures, `WORKING_SCALE` smaller than the scene.
    work_width: u32,
    work_height: u32,
    // The textures are held only to keep their views alive; everything is
    // addressed through the views.
    #[allow(dead_code)]
    scene: wgpu::Texture,
    /// Where the frame is drawn when any backdrop blur is present.
    scene_view: wgpu::TextureView,
    #[allow(dead_code)]
    working: [wgpu::Texture; 2],
    working_views: [wgpu::TextureView; 2],
}

pub struct BackdropRenderer {
    format: wgpu::TextureFormat,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    downsample: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    targets: Option<Targets>,
    idle_frames: u32,
}

impl BackdropRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Backdrop Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("backdrop_shader.wgsl").into()),
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Backdrop Sampler"),
            // Clamped so a tap near the edge smears the border rather than
            // pulling in whatever sits outside the region.
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Backdrop Bind Group Layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Backdrop Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });

        let pipeline = |label: &str, entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        // The composite is the only step that blends: it lays the blurred
        // patch over the target, faded out by the shape mask at the edges.
        // Everything else writes a fresh working texture.
        let composite_blend = wgpu::BlendState {
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
        };

        Self {
            format,
            sampler,
            downsample: pipeline("Backdrop Downsample", "fs_downsample", None),
            blur: pipeline("Backdrop Blur", "fs_blur", None),
            composite: pipeline("Backdrop Composite", "fs_composite", Some(composite_blend)),
            blit: pipeline("Backdrop Blit", "fs_downsample", None),
            bind_group_layout,
            targets: None,
            idle_frames: 0,
        }
    }

    /// Allocate the offscreen target, or reallocate it after a resize.
    pub fn ensure_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let stale = match &self.targets {
            Some(targets) => targets.width != width || targets.height != height,
            None => true,
        };
        if stale {
            self.targets = Some(Targets::new(device, self.format, width, height));
        }
        self.idle_frames = 0;
    }

    /// The view the frame should be drawn into. `None` until
    /// [`ensure_targets`](Self::ensure_targets) has run.
    pub fn scene_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|targets| &targets.scene_view)
    }

    /// Note a frame that needed no backdrop effect, releasing the offscreen
    /// target once enough of them go by.
    ///
    /// Not released on the first such frame: a menu that blurs while open
    /// would otherwise reallocate a surface-sized target every time it opens,
    /// which costs far more than holding it across a few idle frames.
    pub fn note_unused(&mut self) {
        if self.targets.is_none() {
            return;
        }
        self.idle_frames += 1;
        if self.idle_frames > IDLE_FRAMES_BEFORE_RELEASE {
            self.targets = None;
            self.idle_frames = 0;
        }
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        params: Params,
    ) -> wgpu::BindGroup {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Backdrop Params"),
            contents: bytemuck::cast_slice(&[params]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Backdrop Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: buffer.as_entire_binding(),
                },
            ],
        })
    }

    /// Run one full-screen-triangle pass into `target` over `viewport`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        bind_group: &wgpu::BindGroup,
        viewport: (u32, u32, u32, u32),
        label: &str,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let (x, y, w, h) = viewport;
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Blur one region of the scene target, in place.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        region: &BackdropRegion,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let Some((x, y, width, height)) = region.clamped(targets.width, targets.height) else {
            return;
        };

        let work_width = (width / WORKING_SCALE).max(1);
        let work_height = (height / WORKING_SCALE).max(1);

        // Radius shrinks with the working texture; the downsample already
        // spent the rest of it.
        let sigma = (region.radius / WORKING_SCALE as f32 / 2.0).max(0.5);
        let taps = (sigma * 2.5).ceil().min(MAX_TAPS);

        let scene_rect = [
            x as f32 / targets.width as f32,
            y as f32 / targets.height as f32,
            width as f32 / targets.width as f32,
            height as f32 / targets.height as f32,
        ];
        // Only the working textures' top-left corner holds this region, so
        // reads have to stay inside it — normalised against the working size,
        // not the scene's.
        let work_rect = [
            0.0,
            0.0,
            work_width as f32 / targets.work_width as f32,
            work_height as f32 / targets.work_height as f32,
        ];

        let base = Params {
            src_rect: scene_rect,
            direction: [0.0, 0.0],
            sigma,
            taps,
            dst_size: [width as f32, height as f32],
            curvature: region.curvature,
            _pad: 0.0,
            radii: region.radii.to_array(),
        };

        // 1. Scene region → working[0], shrunk.
        let bind = self.bind(device, &targets.scene_view, base);
        self.pass(
            encoder,
            &self.downsample,
            &targets.working_views[0],
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &bind,
            (0, 0, work_width, work_height),
            "Backdrop Downsample",
        );

        // 2. Two separable gaussian passes, ping-ponging. The step is a texel
        // of the *working* texture, which is what the blur samples — stepping
        // by a scene texel would blur `WORKING_SCALE` times too narrowly.
        let texel_x = 1.0 / targets.work_width as f32;
        let texel_y = 1.0 / targets.work_height as f32;
        for (index, direction) in [[texel_x, 0.0], [0.0, texel_y]].into_iter().enumerate() {
            let bind = self.bind(
                device,
                &targets.working_views[index],
                Params {
                    src_rect: work_rect,
                    direction,
                    ..base
                },
            );
            self.pass(
                encoder,
                &self.blur,
                &targets.working_views[1 - index],
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &bind,
                (0, 0, work_width, work_height),
                "Backdrop Blur",
            );
        }

        // 3. Back over the scene, masked to the container's shape.
        let bind = self.bind(
            device,
            &targets.working_views[0],
            Params {
                src_rect: work_rect,
                ..base
            },
        );
        self.pass(
            encoder,
            &self.composite,
            &targets.scene_view,
            wgpu::LoadOp::Load,
            &bind,
            (x, y, width, height),
            "Backdrop Composite",
        );
    }

    /// Copy the offscreen scene onto the swapchain.
    pub fn present(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let bind = self.bind(
            device,
            &targets.scene_view,
            Params {
                src_rect: [0.0, 0.0, 1.0, 1.0],
                direction: [0.0, 0.0],
                sigma: 0.0,
                taps: 0.0,
                dst_size: [targets.width as f32, targets.height as f32],
                curvature: 1.0,
                _pad: 0.0,
                radii: [0.0; 4],
            },
        );
        self.pass(
            encoder,
            &self.blit,
            target,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &bind,
            (0, 0, targets.width, targets.height),
            "Backdrop Blit",
        );
    }
}

impl Targets {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let make = |label: &str, width: u32, height: u32| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };

        let scene = make("Backdrop Scene", width, height);
        let scene_view = scene.create_view(&Default::default());
        // One allocation serves every card on screen — each writes only the
        // corner it needs — and at `WORKING_SCALE` down it costs a sixteenth
        // of a full-size target.
        let work_width = (width / WORKING_SCALE).max(1);
        let work_height = (height / WORKING_SCALE).max(1);
        let working = [
            make("Backdrop Working 0", work_width, work_height),
            make("Backdrop Working 1", work_width, work_height),
        ];
        let working_views = [
            working[0].create_view(&Default::default()),
            working[1].create_view(&Default::default()),
        ];

        Self {
            width,
            height,
            work_width,
            work_height,
            scene,
            scene_view,
            working,
            working_views,
        }
    }
}
