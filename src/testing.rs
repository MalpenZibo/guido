//! An application driven without a compositor.
//!
//! `App::run` needs a Wayland connection to do anything at all: it dials the
//! compositor, waits for a configure, and only then is there a surface to draw
//! on. So everything the application does with what the compositor says — input
//! routing, the frame's phases, what the surface asks for in return — could only
//! be watched by a person running an example and looking.
//!
//! [`Headless`] is the other caller. It holds the same tree, the same renderer
//! and the same frame path, with a recorder where the compositor was: it answers
//! for one surface, keeps what it was asked, and never talks to anything. A test
//! says what the compositor said and then reads both halves — what the widgets
//! became, and what the surface asked for.
//!
//! It is not a compositor and cannot be. What it proves is guido's half; what
//! niri does with an exclusive zone of 50 is still a question no test here can
//! settle.

use std::time::Instant;

use crate::reactive::{self, OwnerId};
use crate::renderer::{GpuContext, RenderTarget, Renderer};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::surface_manager::{ManagedSurface, SurfaceManager};
use crate::tree::{Tree, WidgetId};
use crate::widgets::{Event, MouseButton, Widget};
use crate::{Frame, LoopContext, Platform, Surface, iterate};

/// The compositor's half of one surface: what it has said, and what it has been
/// asked for.
#[derive(Default)]
struct Recorder {
    width: u32,
    height: u32,
    scale: f32,
    configured: bool,
    events: Vec<(Instant, Event)>,
    first_frame_presented: bool,
    exclusive_zones: Vec<i32>,
    sizes_asked: Vec<(u32, u32)>,
    frame_callbacks: u32,
}

/// The one surface a `Headless` has, borrowed for a frame.
struct RecordedSurface<'a>(&'a mut Recorder);

impl Surface for RecordedSurface<'_> {
    fn open_frame(&mut self) -> Option<Frame> {
        if !self.0.configured {
            return None;
        }
        Some(Frame {
            events: std::mem::take(&mut self.0.events),
            scale_factor: self.0.scale,
            width: self.0.width,
            height: self.0.height,
            // Never pending: a driver that had to wait for a callback nobody
            // sends would step once and stop.
            frame_callback_pending: false,
            force_render_surface: !self.0.first_frame_presented,
        })
    }

    fn configured_size(&self) -> Option<(u32, u32)> {
        self.0.configured.then_some((self.0.width, self.0.height))
    }

    fn scale_factor(&self) -> Option<f32> {
        self.0.configured.then_some(self.0.scale)
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.0.sizes_asked.push((width, height));
    }

    fn set_exclusive_zone(&mut self, zone: i32) {
        self.0.exclusive_zones.push(zone);
    }

    fn request_frame_callback(&mut self) {
        self.0.frame_callbacks += 1;
    }

    fn mark_frame_callback_pending(&mut self) {
        self.0.first_frame_presented = true;
    }
}

impl Platform for Recorder {
    type Surface<'a> = RecordedSurface<'a>;

    fn surface(&mut self, _id: SurfaceId) -> Option<RecordedSurface<'_>> {
        Some(RecordedSurface(self))
    }

    /// A texture, where a compositor would hand out a swapchain. The one place
    /// the two implementations differ by doing rather than by recording — and
    /// the reason a surface can be *born* without a compositor rather than only
    /// driven after somebody else built it one.
    fn create_surface(&mut self, _id: SurfaceId, config: &crate::surface::SurfaceConfig) {
        // The same function the layer-shell path uses, so what a surface says
        // at birth cannot differ between the two.
        let declared = crate::surface::initial_declaration(config);
        self.sizes_asked.push(declared.asked);
        self.exclusive_zones.push(declared.exclusive_zone);
    }

    fn create_render_target(
        &self,
        _id: SurfaceId,
        gpu: &crate::renderer::GpuContext,
        size: (u32, u32),
    ) -> Option<crate::renderer::RenderTarget> {
        Some(crate::renderer::RenderTarget::offscreen(
            gpu, size.0, size.1,
        ))
    }
}

/// One application, one surface, stepped by hand.
pub struct Headless {
    gpu: &'static GpuContext,
    tree: Tree,
    renderer: Option<Renderer>,
    surfaces: SurfaceManager,
    id: Option<SurfaceId>,
    host: Recorder,
    owner: Option<OwnerId>,
    layout_roots: rustc_hash::FxHashMap<WidgetId, Vec<WidgetId>>,
}

/// One device for the whole test binary.
///
/// An instance, an adapter request and a queue, and every application in a
/// process can share one set. It outlives them all because a `wgpu::Device` is
/// cheapest when nothing has to decide when to stop holding it, and a test
/// binary ends soon enough for that to be the whole of the lifetime question.
///
/// One device is not most of what starting an application costs. Measured on
/// lavapipe: 48ms here, against 76ms to build the `Renderer` that every
/// application needs one of anyway. So this is a fifth off a test binary, not an
/// order of magnitude.
///
/// The absence of an adapter is cached too: without that, a machine with no GPU
/// re-discovers it once per test, which is the slow way to skip.
fn shared_device() -> Option<&'static GpuContext> {
    static GPU: std::sync::OnceLock<Option<GpuContext>> = std::sync::OnceLock::new();
    GPU.get_or_init(GpuContext::try_new).as_ref()
}

impl Headless {
    /// `None` where there is no GPU adapter at all — a frame has to land
    /// somewhere, and the somewhere is a texture this allocates.
    pub fn new() -> Option<Self> {
        Some(Self {
            gpu: shared_device()?,
            tree: Tree::new(),
            renderer: None,
            surfaces: SurfaceManager::new(),
            id: None,
            host: Recorder {
                scale: 1.0,
                ..Default::default()
            },
            owner: None,
            layout_roots: rustc_hash::FxHashMap::default(),
        })
    }

    /// Declare the surface, as `App::add_surface` does. One only, for now.
    pub fn surface<W, F>(&mut self, config: SurfaceConfig, widget_fn: F)
    where
        W: Widget + 'static,
        F: FnOnce() -> W,
    {
        let (widget, owner) = reactive::with_owner(|| Box::new(widget_fn()) as Box<dyn Widget>);
        self.owner = Some(owner);

        let id = SurfaceId::next();
        self.id = Some(id);

        // Through the trait, not beside it: a fixed-size bar declares its
        // reservation once, at birth, and if the driver wrote that number down
        // itself the recorder would be holding an answer rather than a request.
        self.host.create_surface(id, &config);
        self.surfaces.add(ManagedSurface::new(
            id,
            config,
            widget,
            owner,
            &mut self.tree,
        ));
    }

    /// Say what the compositor confirmed. Until this is called there is no size
    /// to draw at, and [`step`](Self::step) does nothing — which is what an
    /// unconfigured surface does in the real loop.
    pub fn configure(&mut self, width: u32, height: u32, scale: f32) {
        self.host.width = width;
        self.host.height = height;
        self.host.scale = scale;
        self.host.configured = true;
    }

    /// Queue a press and a release at a point, in logical coordinates.
    ///
    /// They are delivered by the next [`step`](Self::step), because that is when
    /// the frame that carries them opens — the same order the compositor's own
    /// events arrive in.
    pub fn click(&mut self, x: f32, y: f32) {
        self.click_at(x, y, Instant::now());
    }

    /// [`click`](Self::click), at a moment you name — so a gesture can be
    /// played through the application at the speed it is meant to have.
    pub fn click_at(&mut self, x: f32, y: f32, now: Instant) {
        self.host.events.push((
            now,
            Event::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            },
        ));
        self.host.events.push((
            now,
            Event::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            },
        ));
    }

    /// One frame: open it, route what is queued, measure, paint, present.
    ///
    /// The moment is the caller's, so an animation can be walked through
    /// without sleeping.
    pub fn step(&mut self) {
        self.step_at(Instant::now());
    }

    /// [`step`](Self::step), at a moment you name.
    pub fn step_at(&mut self, at: Instant) {
        let ctx = LoopContext {
            wayland_state: &mut self.host,
            surface_manager: &mut self.surfaces,
            gpu_context: self.gpu,
            renderer: &mut self.renderer,
        };
        iterate(ctx, &mut self.tree, &mut self.layout_roots, Some(at));
    }

    fn root(&self) -> WidgetId {
        self.surfaces
            .get(self.id.expect("no surface"))
            .expect("no surface")
            .widget_id
    }

    fn target(&self) -> &RenderTarget {
        self.surfaces
            .get(self.id.expect("no surface"))
            .and_then(|s| s.wgpu_surface.as_ref())
            .expect("no target; step once after configuring")
    }

    /// The size the root widget was measured at, in logical pixels.
    pub fn root_size(&self) -> (f32, f32) {
        let bounds = self.tree.get_bounds(self.root()).unwrap_or_default();
        (bounds.width, bounds.height)
    }

    /// The size of the buffer the last frame was drawn into, in physical
    /// pixels — the logical size times the scale the compositor confirmed.
    pub fn physical_size(&self) -> (u32, u32) {
        let target = self.target();
        (target.width(), target.height())
    }

    /// The screen space this surface last asked the compositor to reserve.
    pub fn exclusive_zone_asked(&self) -> Option<i32> {
        self.host.exclusive_zones.last().copied()
    }

    /// Every reservation this surface has asked for, oldest first. A frame that
    /// republishes one it already sent is not the same as one that says nothing,
    /// and only a list can tell them apart.
    pub fn exclusive_zones_asked(&self) -> &[i32] {
        &self.host.exclusive_zones
    }

    /// The sizes this surface has asked for, oldest first.
    pub fn sizes_asked(&self) -> &[(u32, u32)] {
        &self.host.sizes_asked
    }

    /// How many frames have been presented and had their callback re-armed.
    pub fn frames_presented(&self) -> u32 {
        self.host.frame_callbacks
    }

    /// The colour at one pixel of the last frame, in physical coordinates.
    pub fn read_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        match self.target() {
            RenderTarget::Offscreen(offscreen) => offscreen.read_pixel(x, y),
            RenderTarget::Swapchain(_) => panic!("a headless surface has no swapchain"),
        }
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            reactive::dispose_owner_now(owner);
        }
    }
}

#[cfg(test)]
mod one_device_for_the_binary {
    use super::*;

    /// Two applications in one process are handed the same context, so the
    /// second pays nothing for an adapter. Without it #275's list of scenarios
    /// is a list of Vulkan devices.
    ///
    /// The identity is not observable through [`Headless`]'s public surface,
    /// which is why this is a unit test rather than one in
    /// `tests/headless_app.rs`: an accessor added so a test could look would
    /// outlive the test.
    #[test]
    fn two_applications_are_handed_the_same_device() {
        let Some(first) = crate::or_skip(Headless::new()) else {
            return;
        };
        let second = Headless::new().expect("the first one had an adapter");

        assert!(
            std::ptr::eq(first.gpu, second.gpu),
            "a second application built a context of its own"
        );
    }
}
