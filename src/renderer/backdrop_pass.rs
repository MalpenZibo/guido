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
//!
//! The mask is the only part a caller can replace. [`apply`](BackdropRenderer::apply)
//! shapes the result with the region's own corners; [`apply_masked`](BackdropRenderer::apply_masked)
//! takes a coverage texture instead, which is what lets glyphs be the window
//! rather than a box — see [`text_mask`](super::text_mask).
//!
//! What the passes read is kept from frame to frame, as the shape pipeline's
//! instances are: their uniforms are slots of one buffer, written once when the
//! frame is finished and grown only past what it has held, and a bind group is
//! made the first time a texture is bound and kept while frames go on binding
//! it — see [`Bindings`].

use std::cell::RefCell;

use bytemuck::Zeroable;

use crate::render_stats::{self, GpuObject, Pipeline};
use crate::shape::PlacedShape;
use crate::widgets::{Color, Rect};

use super::clip::ResolvedClip;
use super::clip_records::ClipRecord;

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

/// Slots the uniform buffer starts with: a frosted panel's four passes and the
/// blit, with room for a second panel before it has to grow.
const INITIAL_SLOTS: u64 = 16;

/// The size every pass binds of the uniform buffer, at its own offset.
const PARAMS_SIZE: wgpu::BufferSize = wgpu::BufferSize::new(size_of::<Params>() as u64).unwrap();

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    src_rect: [f32; 4],
    direction: [f32; 2],
    sigma: f32,
    taps: f32,
    dst_size: [f32; 2],
    curvature: f32,
    _pad1: f32,
    radii: [f32; 4],
    /// The shape the mask cuts, `[x, y, width, height]`, in its own space.
    shape_rect: [f32; 4],
    /// Where this viewport sits on the target, in physical pixels — the
    /// fragment's uv is relative to the viewport and the shape is not.
    viewport_origin: [f32; 2],
    /// Size of the coverage mask in texels, for the passes that read one.
    mask_size: [f32; 2],
    /// Target physical pixels to shape space: `[a, b, tx, c, d, ty]`.
    to_shape: [f32; 6],
    _pad3: [f32; 2],
    /// Colour of the contour `fs_outline` draws, and how far out it reaches
    /// from the glyph edge in mask texels.
    stroke_color: [f32; 4],
    stroke_width: f32,
    /// Where the clip continues in the frame's clip records, or
    /// [`NO_CHAIN`](super::clip::NO_CHAIN) — see
    /// [`ResolvedClip`](super::clip::ResolvedClip).
    clip_chain: u32,
    _pad4: [f32; 2],
    /// The clip, in its own space: `[x, y, width, height]`. Negative extents
    /// are the no-clip sentinel and zero extents the lets-nothing-through one,
    /// as in [`NO_CLIP_RECT`](super::gpu::NO_CLIP_RECT) — the same two spellings
    /// the shape pipeline and the textured quad already read.
    clip_rect: [f32; 4],
    clip_radii: [f32; 4],
    /// Target physical pixels to clip space, and the clip's curvature.
    to_clip: [f32; 6],
    clip_curvature: f32,
    /// How opaque the composite and the contour are laid over the target —
    /// the fade the container's ancestors ask for.
    opacity: f32,
}

/// A rasterized coverage mask and how big it is, which the view alone does not
/// say — the passes that read one need its texel size to step by a texel.
#[derive(Clone, Copy)]
pub struct CoverageMask<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: (u32, u32),
}

/// A blur to apply: the shape it is cut to, and what it was flattened under.
///
/// Everything the pass works in is derived from these rather than stored beside
/// them. The viewport used to be a field, computed by the caller as the world
/// box of the very shape it travelled with — two values for one fact, free to
/// drift, and nothing that would have said so (#402).
#[derive(Debug, Clone, Copy)]
pub struct BackdropRegion {
    /// The shape the effect is cut to: the rect its widget declared, its
    /// corners, and the transform that placed it — all in the logical units it
    /// was written in.
    ///
    /// A `PlacedShape` and not four loose fields, because that is what a clip
    /// is and what the region tessellator takes, and this is the same object
    /// (#198). The masked composite reads coverage over the same rect instead
    /// of cutting corners out of it, so for a frosted text the corners are
    /// zero — which says "a plain frame", not "a field nobody filled".
    pub shape: PlacedShape,
    /// Physical pixels per logical one. The pass needs it to place `shape` on
    /// the target and to scale the radius; `shape` itself must stay logical.
    pub scale: f32,
    /// Blur radius, logical like the shape.
    pub radius: f32,
    /// What the effect is allowed to write: the clip it was flattened under,
    /// if any. The mask says which pixels of the region are filtered; this says
    /// which of them are on show at all.
    ///
    /// Shapes, like the one above and for the same reason: the first and the
    /// chain of those it could not be merged with. Each is tested per fragment
    /// in its own space, so a rounded scroller keeps its corners and a turned
    /// one keeps its edges; the viewport is still narrowed by the box they all
    /// leave open, because a viewport is four integers and narrowing it is
    /// free.
    pub clip: Option<ResolvedClip>,
    /// How opaque the result is laid back over the target: what the
    /// container's own opacity and every ancestor's multiply to. A frosted
    /// panel fading out takes its frost with it.
    pub opacity: f32,
}

impl BackdropRegion {
    /// The box on the target the effect may touch, in physical pixels.
    ///
    /// Derived from the shape rather than handed in beside it: a viewport has
    /// no choice but to be a box — under a rotation it is larger than the shape
    /// — and the mask is what cuts the difference back off.
    pub(crate) fn viewport(&self) -> Rect {
        let world = self.shape.world_aabb();
        Rect::new(
            world.x * self.scale,
            world.y * self.scale,
            world.width * self.scale,
            world.height * self.scale,
        )
    }

    /// Clamp to the target so a card running off-screen cannot ask for a
    /// viewport wgpu will reject.
    fn clamped(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        // The clip comes first: a region scrolled out of its container has
        // nothing on show, and filtering it would leave a blurred rectangle
        // where the content is not.
        let rect = self.viewport();
        let mut left = rect.x;
        let mut top = rect.y;
        let mut right_f = rect.x + rect.width;
        let mut bottom_f = rect.y + rect.height;
        if let Some(clip) = &self.clip {
            // The box every clip leaves open, which is all a viewport can be.
            // What the box over-claims is cut back off per fragment by the
            // shader, the same way the shape's own box is.
            let clip = clip.open;
            left = left.max(clip.x * self.scale);
            top = top.max(clip.y * self.scale);
            right_f = right_f.min((clip.x + clip.width) * self.scale);
            bottom_f = bottom_f.min((clip.y + clip.height) * self.scale);
        }

        let x = left.floor().max(0.0) as u32;
        let y = top.floor().max(0.0) as u32;
        let right = right_f.ceil().max(0.0) as u32;
        let bottom = bottom_f.ceil().max(0.0) as u32;
        let right = right.min(width);
        let bottom = bottom.min(height);
        if x >= right || y >= bottom {
            return None;
        }
        Some((x, y, right - x, bottom - y))
    }

    /// The half of the uniform that says where the shape is.
    ///
    /// Every pass that cuts something — the corners, a coverage mask, the
    /// contour around one — works by carrying its fragment from the target
    /// back into the shape's own space, so they all need the same fields and
    /// none of them should spell them out again.
    fn shape_params(&self, origin: (u32, u32), mask: Option<CoverageMask<'_>>) -> Option<Params> {
        // A placement that collapses has no inverse, so there is no way back
        // into the shape's own space and nothing to cut. The effect is skipped,
        // which is what a zero-area shape deserves.
        let to_shape = self.shape.to_local(self.scale)?;
        let rect = self.shape.rect;
        // The two sentinels the shape pipeline already uses, spelled the same
        // way: negative extents mean there is no clip, zero extents mean one
        // that lets nothing through. A placement that collapses is the second —
        // it has no inverse, so there is no space to test a fragment in, and
        // the honest answer is that nothing shows rather than everything.
        let clip = match &self.clip {
            Some(clip) => ClipRecord::new(&clip.first, clip.rest, self.scale),
            None => ClipRecord::NONE,
        };
        Some(Params {
            curvature: self.shape.curvature,
            radii: self.shape.radii.to_array(),
            shape_rect: [rect.x, rect.y, rect.width, rect.height],
            viewport_origin: [origin.0 as f32, origin.1 as f32],
            to_shape: to_shape.data,
            mask_size: mask.map_or([1.0, 1.0], |m| [m.size.0 as f32, m.size.1 as f32]),
            clip_rect: clip.rect,
            clip_radii: clip.radii,
            clip_curvature: clip.curvature,
            to_clip: clip.inverse,
            clip_chain: clip.next,
            opacity: self.opacity,
            ..Params::zeroed()
        })
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
    /// The same three bindings plus the coverage texture the masked composite
    /// reads. Kept apart so the three shape passes bind nothing they never use.
    mask_bind_group_layout: wgpu::BindGroupLayout,
    downsample: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    composite_mask: wgpu::RenderPipeline,
    outline: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    targets: Option<Targets>,
    idle_frames: u32,
    /// Behind a cell because the passes take `&self`: every pass stages its
    /// uniforms and looks its bind group up, and none of them owns the
    /// renderer.
    bindings: RefCell<Bindings>,
}

/// Where a pass's uniforms are: the bind group that reads them, and their
/// offset in the buffer it binds.
struct Bound {
    group: wgpu::BindGroup,
    offset: u32,
}

/// The uniforms of every pass a frame runs, and the bind groups that read
/// them.
///
/// Each pass takes the next slot of one buffer, read through a dynamic offset,
/// so a bind group says only which textures a pass reads: one per texture, or
/// per texture and mask, made the first time it is bound and kept while frames
/// go on binding it. The slots are gathered here and written in one
/// `write_buffer` when the frame is [finished](BackdropRenderer::finish),
/// because each write is a staging buffer of wgpu's own and a frame runs
/// several passes per backdrop.
struct Bindings {
    queue: wgpu::Queue,
    /// `None` until a pass needs it, and again once the targets are released.
    buffer: Option<wgpu::Buffer>,
    /// A slot's size: the uniforms, padded to the offset alignment the device
    /// asks of a dynamic offset.
    stride: u64,
    /// This frame's slots, in the order the passes took them.
    staged: Vec<u8>,
    kept: Vec<KeptGroup>,
}

struct KeptGroup {
    view: wgpu::TextureView,
    mask: Option<wgpu::TextureView>,
    group: wgpu::BindGroup,
    /// Bound by the frame in progress. A group the last frame that blurred did
    /// not bind is let go, so a texture no longer drawn is not kept alive by it.
    used: bool,
}

impl Bindings {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        Self {
            queue: queue.clone(),
            buffer: None,
            stride: PARAMS_SIZE.get().next_multiple_of(align),
            staged: Vec::new(),
            kept: Vec::new(),
        }
    }

    /// Take the next slot for `params`, growing the buffer if every slot is
    /// taken, and answer the slot's offset.
    fn stage(&mut self, device: &wgpu::Device, params: Params) -> u32 {
        let capacity = self.buffer.as_ref().map_or(0, wgpu::Buffer::size);
        if self.staged.len() as u64 == capacity {
            self.grow(device, capacity);
        }
        let offset = self.staged.len();
        self.staged.extend_from_slice(bytemuck::bytes_of(&params));
        self.staged.resize(offset + self.stride as usize, 0);
        offset as u32
    }

    /// Replace the buffer with one twice the size.
    ///
    /// The frame goes on at the slot it reached, so by the time it finishes
    /// the buffer holds all of it and the next frame like it fits. The passes
    /// already recorded keep the buffer they were bound to, so what they
    /// staged is written to it first. Every kept bind group names the old
    /// buffer, so they all go with it.
    fn grow(&mut self, device: &wgpu::Device, capacity: u64) {
        if let Some(buffer) = &self.buffer {
            self.queue.write_buffer(buffer, 0, &self.staged);
        }
        let size = (capacity * 2).max(INITIAL_SLOTS * self.stride);
        self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Backdrop Params"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        render_stats::record_backdrop_allocation(GpuObject::Buffer);
        self.kept.clear();
    }

    fn release(&mut self) {
        self.buffer = None;
        self.staged = Vec::new();
        self.kept.clear();
    }
}

impl BackdropRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        clip_records: &wgpu::BindGroupLayout,
    ) -> Self {
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
                        has_dynamic_offset: true,
                        min_binding_size: Some(PARAMS_SIZE),
                    },
                    count: None,
                },
            ],
        });

        // The masked composite reads one more texture; everything else would
        // only be declaring a binding it never touches.
        let mask_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Backdrop Mask Bind Group Layout"),
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
                            has_dynamic_offset: true,
                            min_binding_size: Some(PARAMS_SIZE),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                ],
            });

        // Group 1 is the frame's clip chains, which only the composites walk:
        // the downsample, the blur and the blit read no clip and bind none.
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Backdrop Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let clipped_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Backdrop Clipped Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout), Some(clip_records)],
            immediate_size: 0,
        });
        let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Backdrop Mask Pipeline Layout"),
            bind_group_layouts: &[Some(&mask_bind_group_layout), Some(clip_records)],
            immediate_size: 0,
        });

        let build = |label: &str,
                     entry: &str,
                     blend: Option<wgpu::BlendState>,
                     layout: &wgpu::PipelineLayout| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
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
        let pipeline = |label: &str, entry: &str, blend: Option<wgpu::BlendState>| {
            build(label, entry, blend, &layout)
        };

        // The composite is the only step that blends: it lays the blurred
        // patch over the target, faded out by the shape mask at the edges.
        // Everything else writes a fresh working texture.
        let composite_blend = super::gpu::STRAIGHT_ALPHA_BLEND;

        Self {
            format,
            sampler,
            downsample: pipeline("Backdrop Downsample", "fs_downsample", None),
            blur: pipeline("Backdrop Blur", "fs_blur", None),
            composite: build(
                "Backdrop Composite",
                "fs_composite",
                Some(composite_blend),
                &clipped_layout,
            ),
            composite_mask: build(
                "Backdrop Composite Mask",
                "fs_composite_mask",
                Some(composite_blend),
                &mask_layout,
            ),
            outline: build(
                "Text Outline",
                "fs_outline",
                Some(composite_blend),
                &mask_layout,
            ),
            blit: pipeline("Backdrop Blit", "fs_downsample", None),
            bind_group_layout,
            mask_bind_group_layout,
            targets: None,
            idle_frames: 0,
            bindings: RefCell::new(Bindings::new(device, queue)),
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
            self.bindings.get_mut().release();
            self.idle_frames = 0;
        }
    }

    /// Write the uniforms this frame's passes staged, before the frame's
    /// commands are submitted, and let go of the bind groups it did not use.
    ///
    /// A frame that blurred nothing lets go of nothing: the targets outlive a
    /// few such frames on purpose, and so do the groups that bind them.
    pub fn finish(&self) {
        let bindings = &mut *self.bindings.borrow_mut();
        let Some(buffer) = &bindings.buffer else {
            return;
        };
        if bindings.staged.is_empty() {
            return;
        }
        bindings.queue.write_buffer(buffer, 0, &bindings.staged);
        bindings.staged.clear();
        bindings
            .kept
            .retain_mut(|kept| std::mem::take(&mut kept.used));
    }

    /// Stage `params` for a pass reading `view`, and `mask` for the masked
    /// passes, and answer where the pass finds them.
    fn bind(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        mask: Option<&wgpu::TextureView>,
        params: Params,
    ) -> Bound {
        let bindings = &mut *self.bindings.borrow_mut();
        let offset = bindings.stage(device, params);
        let buffer = bindings.buffer.as_ref().expect("staging made the buffer");
        let index = bindings
            .kept
            .iter()
            .position(|kept| kept.view == *view && kept.mask.as_ref() == mask)
            .unwrap_or_else(|| {
                let group = self.make_group(device, buffer, view, mask);
                bindings.kept.push(KeptGroup {
                    view: view.clone(),
                    mask: mask.cloned(),
                    group,
                    used: false,
                });
                bindings.kept.len() - 1
            });
        let kept = &mut bindings.kept[index];
        kept.used = true;
        Bound {
            group: kept.group.clone(),
            offset,
        }
    }

    fn make_group(
        &self,
        device: &wgpu::Device,
        buffer: &wgpu::Buffer,
        view: &wgpu::TextureView,
        mask: Option<&wgpu::TextureView>,
    ) -> wgpu::BindGroup {
        let mut entries = vec![
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
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer,
                    offset: 0,
                    size: Some(PARAMS_SIZE),
                }),
            },
        ];
        // The masked passes read the coverage texture as well.
        let (layout, label) = match mask {
            Some(mask) => {
                entries.push(wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(mask),
                });
                (&self.mask_bind_group_layout, "Backdrop Mask Bind Group")
            }
            None => (&self.bind_group_layout, "Backdrop Bind Group"),
        };
        render_stats::record_backdrop_allocation(GpuObject::BindGroup);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &entries,
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
        bind: &Bound,
        clip_records: Option<&wgpu::BindGroup>,
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
        pass.set_bind_group(0, &bind.group, &[bind.offset]);
        if let Some(clip_records) = clip_records {
            pass.set_bind_group(1, clip_records, &[]);
        }
        pass.draw(0..3, 0..1);
        render_stats::record_draw_calls(Pipeline::Backdrop, 1);
    }

    /// Blur one region of the scene target, in place, cut out by the region's
    /// own corners.
    pub fn apply(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        region: &BackdropRegion,
        clip_records: &wgpu::BindGroup,
    ) {
        self.filter(device, encoder, region, clip_records, None);
    }

    /// The same, cut out by `mask`'s alpha instead of the corners.
    ///
    /// The mask covers `region.shape` in the space `region.to_shape` lands in,
    /// and each fragment is carried back there to find its texel — so what the
    /// mask leaves opaque is what shows the blur, wherever the shape has been
    /// turned or stretched to. The region's radii are ignored: the shape is
    /// entirely the mask's.
    pub fn apply_masked(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        region: &BackdropRegion,
        clip_records: &wgpu::BindGroup,
        mask: CoverageMask<'_>,
    ) {
        self.filter(device, encoder, region, clip_records, Some(mask));
    }

    /// Draw a contour just outside `mask`'s coverage.
    ///
    /// A stroke, for the one case the cheap approximation cannot serve: copies
    /// of the glyphs drawn under the fill ring the letter *and* fill it, which
    /// is invisible under an opaque fill and opaque over frost. This dilates
    /// the coverage instead and draws only the band the dilate added.
    ///
    /// Ordered after the frost and before the glyphs, so the letter keeps what
    /// the frost put in it.
    pub fn apply_outline(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        region: &BackdropRegion,
        clip_records: &wgpu::BindGroup,
        mask: CoverageMask<'_>,
        (color, width): (Color, f32),
    ) {
        let Some(targets) = &self.targets else {
            return;
        };
        let Some(viewport) = region.clamped(targets.width, targets.height) else {
            return;
        };
        let (x, y, dst_width, dst_height) = viewport;

        let params = Params {
            src_rect: [0.0, 0.0, 1.0, 1.0],
            dst_size: [dst_width as f32, dst_height as f32],
            stroke_color: [color.r, color.g, color.b, color.a],
            stroke_width: width,
            ..match region.shape_params((x, y), Some(mask)) {
                Some(params) => params,
                None => return,
            }
        };
        // Binding 0 goes unread by this pipeline; the working texture fills it
        // because the scene is the target and may not be bound as a resource
        // at the same time.
        let bind = self.bind(device, &targets.working_views[0], Some(mask.view), params);
        self.pass(
            encoder,
            &self.outline,
            &targets.scene_view,
            wgpu::LoadOp::Load,
            &bind,
            Some(clip_records),
            viewport,
            "Text Outline",
        );
    }

    fn filter(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        region: &BackdropRegion,
        clip_records: &wgpu::BindGroup,
        mask: Option<CoverageMask<'_>>,
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
        let sigma = (region.radius * region.scale / WORKING_SCALE as f32 / 2.0).max(0.5);
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
            sigma,
            taps,
            dst_size: [width as f32, height as f32],
            ..match region.shape_params((x, y), mask) {
                Some(params) => params,
                None => return,
            }
        };

        // 1. Scene region → working[0], shrunk.
        let bind = self.bind(device, &targets.scene_view, None, base);
        self.pass(
            encoder,
            &self.downsample,
            &targets.working_views[0],
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &bind,
            None,
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
                None,
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
                None,
                (0, 0, work_width, work_height),
                "Backdrop Blur",
            );
        }

        // 3. Back over the scene, cut out by the shape or by the mask.
        let params = Params {
            src_rect: work_rect,
            ..base
        };
        let (pipeline, bind, label) = match mask {
            Some(mask) => (
                &self.composite_mask,
                self.bind(device, &targets.working_views[0], Some(mask.view), params),
                "Backdrop Composite Mask",
            ),
            None => (
                &self.composite,
                self.bind(device, &targets.working_views[0], None, params),
                "Backdrop Composite",
            ),
        };
        self.pass(
            encoder,
            pipeline,
            &targets.scene_view,
            wgpu::LoadOp::Load,
            &bind,
            Some(clip_records),
            (x, y, width, height),
            label,
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
            None,
            // The blit runs `fs_downsample`, which reads `src_rect` and
            // nothing else. A `curvature` and a `dst_size` were set here too
            // and went unread — copied from the passes that do read them, and
            // kept by nobody noticing, until a mutant deleted one and no test
            // objected.
            Params {
                src_rect: [0.0, 0.0, 1.0, 1.0],
                ..Params::zeroed()
            },
        );
        self.pass(
            encoder,
            &self.blit,
            target,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &bind,
            None,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// The uniform is one layout written twice, and the chain head took four
    /// bytes that were padding: the shader has to read it, and everything
    /// after it, from the bytes the struct wrote them to.
    #[test]
    fn the_shader_reads_the_clip_where_the_params_put_it() {
        let (wgsl, size) = super::super::gpu::instance_layout::wgsl_fields(
            "src/renderer/backdrop_shader.wgsl",
            "Params",
        );
        let at = |name: &str| {
            wgsl.iter()
                .find(|(field, _)| field == name)
                .unwrap_or_else(|| panic!("`Params` in the shader has no `{name}`"))
                .1
        };
        assert_eq!(at("stroke_width"), offset_of!(Params, stroke_width));
        assert_eq!(at("clip_chain"), offset_of!(Params, clip_chain));
        assert_eq!(at("clip_rect"), offset_of!(Params, clip_rect));
        assert_eq!(at("opacity"), offset_of!(Params, opacity));
        assert_eq!(size, size_of::<Params>());
    }
}
