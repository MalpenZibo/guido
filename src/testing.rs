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
//! for as many surfaces as the loop will carry, keeps what each was asked, and
//! never talks to anything. A test says what the compositor said and then reads
//! both halves — what the widgets became, and what each surface asked for.
//!
//! Surfaces declared before the loop runs come from [`Headless::surface`], the
//! way `App::add_surface` declares them. After it is running they come from
//! guido's own `spawn_surface` and go away through `surface_handle(id).close()`,
//! and [`Headless::step`] is what drains the command either one queues — so a
//! test spawns a surface the way an application does, not the way a harness
//! would let it.
//!
//! It is not a compositor and cannot be. What it proves is guido's half; what
//! niri does with an exclusive zone of 50 is still a question no test here can
//! settle.

use std::time::Instant;

use crate::reactive;
use crate::renderer::{GpuContext, RenderTarget, Renderer};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::surface_manager::{ManagedSurface, SurfaceManager};
use crate::tree::{Tree, WidgetId};
use crate::widgets::{Event, MouseButton, Widget};
use crate::{Frame, LoopContext, Platform, Surface, iterate};

/// The compositor's half of one surface: what it has said, and what it has
/// been asked for.
#[derive(Default)]
struct RecordedSurface {
    /// The surface this one hangs from, for a popup. On the surface rather than
    /// in a map beside it, so a parent link cannot outlive its surface — which
    /// is also where `WaylandState` keeps it.
    parent: Option<SurfaceId>,
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

/// The compositor's half of a *connection*: every surface it holds, and the
/// order it was told to build and tear them down in.
///
/// Lists rather than counts, because the order is what is asserted.
#[derive(Default)]
struct Recorder {
    surfaces: rustc_hash::FxHashMap<SurfaceId, RecordedSurface>,
    created: Vec<SurfaceId>,
    destroyed: Vec<SurfaceId>,
}

impl Recorder {
    fn get(&self, id: SurfaceId) -> &RecordedSurface {
        self.surfaces.get(&id).unwrap_or_else(|| missing(id))
    }

    fn get_mut(&mut self, id: SurfaceId) -> &mut RecordedSurface {
        self.surfaces.get_mut(&id).unwrap_or_else(|| missing(id))
    }
}

/// The two ways to name a surface that is not there, said once.
fn missing(id: SurfaceId) -> ! {
    panic!("surface {id:?} was never declared, or has been closed")
}

impl Surface for &mut RecordedSurface {
    fn open_frame(&mut self) -> Option<Frame> {
        if !self.configured {
            return None;
        }
        Some(Frame {
            events: std::mem::take(&mut self.events),
            scale_factor: self.scale,
            width: self.width,
            height: self.height,
            // Never pending: a driver that had to wait for a callback nobody
            // sends would step once and stop.
            frame_callback_pending: false,
            force_render_surface: !self.first_frame_presented,
        })
    }

    fn configured_size(&self) -> Option<(u32, u32)> {
        self.configured.then_some((self.width, self.height))
    }

    fn scale_factor(&self) -> Option<f32> {
        self.configured.then_some(self.scale)
    }

    fn set_size(&mut self, width: u32, height: u32) {
        self.sizes_asked.push((width, height));
    }

    fn set_exclusive_zone(&mut self, zone: i32) {
        self.exclusive_zones.push(zone);
    }

    fn request_frame_callback(&mut self) {
        self.frame_callbacks += 1;
    }

    fn mark_frame_callback_pending(&mut self) {
        self.first_frame_presented = true;
    }
}

impl Platform for Recorder {
    type Surface<'a> = &'a mut RecordedSurface;

    fn surface(&mut self, id: SurfaceId) -> Option<&mut RecordedSurface> {
        self.surfaces.get_mut(&id)
    }

    fn create_surface(&mut self, id: SurfaceId, config: &crate::surface::SurfaceConfig) {
        // The same function the layer-shell path uses, so what a surface says
        // at birth cannot differ between the two.
        let declared = crate::surface::initial_declaration(config);
        self.created.push(id);
        self.surfaces.insert(
            id,
            RecordedSurface {
                scale: 1.0,
                sizes_asked: vec![declared.asked],
                exclusive_zones: vec![declared.exclusive_zone],
                ..Default::default()
            },
        );
    }

    /// A popup is a surface that knows what it hangs from. The size is the
    /// compositor's answer, so it arrives configured — a real one would wait a
    /// round trip, and nothing here is waiting for anything.
    fn create_popup(
        &mut self,
        id: SurfaceId,
        parent: SurfaceId,
        _config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        self.created.push(id);
        self.surfaces.insert(
            id,
            RecordedSurface {
                parent: Some(parent),
                width: size.0,
                height: size.1,
                scale: 1.0,
                configured: true,
                ..Default::default()
            },
        );
        true
    }

    fn destroy_surface(&mut self, id: SurfaceId) {
        self.destroyed.push(id);
        self.surfaces.remove(&id);
    }

    /// Deepest first, which is the order the protocol demands and the order the
    /// loop is supposed to close them in.
    fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        for (&child, surface) in &self.surfaces {
            if surface.parent == Some(root) {
                out.extend(self.popup_descendants_bottom_up(child));
                out.push(child);
            }
        }
        out
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

/// One application and its surfaces, stepped by hand.
pub struct Headless {
    gpu: &'static GpuContext,
    tree: Tree,
    renderer: Option<Renderer>,
    surfaces: SurfaceManager,
    host: Recorder,
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
            host: Recorder::default(),
            layout_roots: rustc_hash::FxHashMap::default(),
        })
    }

    /// Declare a surface before the loop runs, as `App::add_surface` does.
    ///
    /// The id it returns is what `spawn_popup` wants for a parent, and what
    /// every accessor here is asked about.
    pub fn surface<W, F>(&mut self, config: SurfaceConfig, widget_fn: F) -> SurfaceId
    where
        W: Widget + 'static,
        F: FnOnce() -> W,
    {
        let (widget, owner) = reactive::with_owner(|| Box::new(widget_fn()) as Box<dyn Widget>);
        let id = SurfaceId::next();

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
        id
    }

    /// Say what the compositor confirmed for one surface. Until this is called
    /// there is no size to draw at and [`step`](Self::step) does nothing for it
    /// — which is what an unconfigured surface does in the real loop.
    pub fn configure(&mut self, id: SurfaceId, width: u32, height: u32, scale: f32) {
        let surface = self.host.get_mut(id);
        surface.width = width;
        surface.height = height;
        surface.scale = scale;
        surface.configured = true;
    }

    /// Queue a press and a release at a point on one surface, in logical
    /// coordinates.
    ///
    /// They are delivered by the next [`step`](Self::step), because that is when
    /// the frame that carries them opens — the same order the compositor's own
    /// events arrive in.
    pub fn click(&mut self, id: SurfaceId, x: f32, y: f32) {
        self.click_at(id, x, y, Instant::now());
    }

    /// [`click`](Self::click), at a moment you name — so a gesture can be
    /// played through the application at the speed it is meant to have.
    pub fn click_at(&mut self, id: SurfaceId, x: f32, y: f32, now: Instant) {
        let surface = self.host.get_mut(id);
        surface.events.push((
            now,
            Event::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            },
        ));
        surface.events.push((
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
    /// Returns why the loop ended, or `None` if it did not: closing the last
    /// surface is `Some(ExitReason::Quit)`. Nothing here stops a test stepping
    /// again afterwards, and nothing good comes of it — the real loop returns.
    ///
    /// The moment is the caller's, so an animation can be walked through
    /// without sleeping.
    pub fn step(&mut self) -> Option<crate::ExitReason> {
        self.step_at(Instant::now())
    }

    /// [`step`](Self::step), at a moment you name.
    pub fn step_at(&mut self, at: Instant) -> Option<crate::ExitReason> {
        let ctx = LoopContext {
            wayland_state: &mut self.host,
            surface_manager: &mut self.surfaces,
            gpu_context: self.gpu,
            renderer: &mut self.renderer,
        };
        iterate(ctx, &mut self.tree, &mut self.layout_roots, Some(at))
    }

    fn root(&self, id: SurfaceId) -> WidgetId {
        self.surfaces
            .get(id)
            .unwrap_or_else(|| missing(id))
            .widget_id
    }

    fn target(&self, id: SurfaceId) -> &RenderTarget {
        self.surfaces
            .get(id)
            .and_then(|s| s.wgpu_surface.as_ref())
            .expect("no target; step once after configuring")
    }

    /// The size a surface's root widget was measured at, in logical pixels.
    pub fn root_size(&self, id: SurfaceId) -> (f32, f32) {
        let bounds = self.tree.get_bounds(self.root(id)).unwrap_or_default();
        (bounds.width, bounds.height)
    }

    /// The size of the buffer a surface's last frame was drawn into, in
    /// physical pixels — the logical size times the scale the compositor
    /// confirmed.
    pub fn physical_size(&self, id: SurfaceId) -> (u32, u32) {
        let target = self.target(id);
        (target.width(), target.height())
    }

    /// Every reservation a surface has asked for, oldest first. A frame that
    /// republishes one it already sent is not the same as one that says nothing,
    /// and only a list can tell them apart.
    pub fn exclusive_zones_asked(&self, id: SurfaceId) -> &[i32] {
        &self.host.get(id).exclusive_zones
    }

    /// The sizes a surface has asked for, oldest first.
    pub fn sizes_asked(&self, id: SurfaceId) -> &[(u32, u32)] {
        &self.host.get(id).sizes_asked
    }

    /// How many frames a surface has presented and had its callback re-armed.
    pub fn frames_presented(&self, id: SurfaceId) -> u32 {
        self.host.get(id).frame_callbacks
    }

    /// Every surface the compositor was told to build, oldest first — including
    /// the ones an application spawned at runtime, and popups.
    pub fn surfaces_created(&self) -> &[SurfaceId] {
        &self.host.created
    }

    /// Every surface the compositor was told to tear down, in the order it was
    /// told.
    pub fn surfaces_destroyed(&self) -> &[SurfaceId] {
        &self.host.destroyed
    }

    /// The colour at one pixel of a surface's last frame, in physical
    /// coordinates.
    pub fn read_pixel(&self, id: SurfaceId, x: u32, y: u32) -> [u8; 4] {
        match self.target(id) {
            RenderTarget::Offscreen(offscreen) => offscreen.read_pixel(x, y),
            RenderTarget::Swapchain(_) => panic!("a headless surface has no swapchain"),
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
