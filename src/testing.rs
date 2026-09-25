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
//! The connection has a half of its own, and the recorder answers for that
//! too: [`Headless::connect_output`] plugs a monitor in,
//! [`disconnect_output`](Headless::disconnect_output) takes it away, and
//! [`grant_lock`](Headless::grant_lock) and
//! [`finish_lock`](Headless::finish_lock) answer the session lock the
//! application asked for. Outputs and the lock were watched by a person with
//! a spare screen until #424.
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

use crate::outputs::{self, OutputId, OutputInfo};
use crate::platform::LockEvent;
use crate::platform::outputs::OutputRegistry;
use crate::reactive;
use crate::renderer::{GpuContext, RenderTarget, Renderer};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::surface_manager::{ManagedSurface, SurfaceManager};
use crate::tree::{Tree, WidgetId};
use crate::widgets::{Event, MouseButton, Rect, Widget};
use crate::{Frame, LoopContext, Platform, Surface, iterate};

/// The compositor's half of one surface: what it has said, and what it has
/// been asked for.
#[derive(Default)]
struct RecordedSurface {
    /// The surface this one hangs from, for a popup. On the surface rather than
    /// in a map beside it, so a parent link cannot outlive its surface — which
    /// is also where `WaylandState` keeps it.
    parent: Option<SurfaceId>,
    /// Whether this popup took an explicit grab. Kept because
    /// `conflicting_grab_popups` cannot be answered without it, and a recorder
    /// that throws it away leaves that path with no sensor at all.
    grab: bool,
    width: u32,
    height: u32,
    scale: f32,
    configured: bool,
    events: Vec<(Instant, Event)>,
    first_frame_presented: bool,
    exclusive_zones: Vec<i32>,
    /// Every input region asked for, oldest first. Kept whole rather than
    /// resolved, because "no region at all" and "a region holding nothing" are
    /// opposite instructions that a list of rectangles cannot tell apart.
    input_regions: Vec<Option<Vec<Rect>>>,
    /// Every derived region published, oldest first — the base a surface takes
    /// input in, and what the frame declared about it.
    input_requests: Vec<crate::region::InputRegionRequest>,
    /// Every blur region the loop handed over, oldest first — an empty one is
    /// a withdrawal, and is kept.
    blur_regions: Vec<Vec<Rect>>,
    sizes_asked: Vec<(u32, u32)>,
    /// Every logical size the surface declared its buffer stands for, oldest
    /// first. A list, because what is asserted is that one was declared for
    /// each buffer the frame path resolved and never for a frame that resolved
    /// the same one again.
    viewport_destinations: Vec<(u32, u32)>,
    frame_callbacks: u32,
}

/// The compositor's half of the session lock: whether it is holding a grant,
/// what it has been asked to cover, and what it has said about it.
#[derive(Default)]
struct RecordedLock {
    /// Whether a lock object exists: asked for, and not yet handed back. What
    /// refuses a second request, as `active_lock.is_some()` does on a
    /// compositor.
    asked: bool,
    /// Whether the compositor has granted it. Asking does not make it so — the
    /// grant is a thing the compositor says, and here the test says it,
    /// through [`Headless::grant_lock`].
    granted: bool,
    /// Every lock surface asked for, oldest first: the id, the output it was
    /// to cover, and whether it was accepted. Refused asks are kept, because a
    /// refusal that repeats is #422 — a monitor that has gone being asked for
    /// a cover sixty times a second — and only the asks can show it.
    requests: Vec<(SurfaceId, OutputId, bool)>,
    events: Vec<LockEvent>,
}

/// The compositor's half of a *connection*: every surface it holds, the
/// monitors it is advertising, the lock it has granted, and the order it was
/// told to build and tear surfaces down in.
///
/// Lists rather than counts, because the order is what is asserted.
#[derive(Default)]
struct Recorder {
    surfaces: rustc_hash::FxHashMap<SurfaceId, RecordedSurface>,
    created: Vec<SurfaceId>,
    destroyed: Vec<SurfaceId>,
    /// The connectors the compositor is advertising, in the order they were
    /// plugged in — the globals, in `OutputRegistry`'s terms.
    connectors: Vec<String>,
    /// Which of them has which id, and the only place one is minted. The same
    /// registry `WaylandState` keys by `ObjectId`, keyed here by connector
    /// name: the id policy has one definition, so a harness cannot go on
    /// agreeing with itself after the real one has changed.
    outputs: OutputRegistry<String>,
    lock: RecordedLock,
    /// Every cursor shape the seat was handed, oldest first. A list, because a
    /// shape asked for again on each enter is the thing asserted, and a last
    /// value cannot count.
    cursors: Vec<crate::reactive::CursorIcon>,
    /// What the seat offers as the clipboard and as the primary selection:
    /// another application's copy, or the application's own, handed back the
    /// way a compositor hands a new selection to every client.
    clipboard: Option<String>,
    primary: Option<String>,
    /// Every read the seat was asked for, oldest first.
    selection_reads: Vec<crate::reactive::SelectionKind>,
}

impl Recorder {
    /// A new selection, and what the application is told of it.
    fn offer(&mut self, kind: crate::reactive::SelectionKind, text: String) {
        *self.selection(kind) = Some(text);
        if kind == crate::reactive::SelectionKind::Clipboard {
            crate::reactive::clipboard::set_clipboard_offered(true);
        }
    }

    fn selection(&mut self, kind: crate::reactive::SelectionKind) -> &mut Option<String> {
        match kind {
            crate::reactive::SelectionKind::Clipboard => &mut self.clipboard,
            crate::reactive::SelectionKind::Primary => &mut self.primary,
        }
    }

    fn get(&self, id: SurfaceId) -> &RecordedSurface {
        self.surfaces.get(&id).unwrap_or_else(|| missing(id))
    }

    fn get_mut(&mut self, id: SurfaceId) -> &mut RecordedSurface {
        self.surfaces.get_mut(&id).unwrap_or_else(|| missing(id))
    }

    /// Advertise a monitor, as `new_output` does: mint it an id, then publish
    /// the list.
    fn connect_output(&mut self, name: &str) -> OutputId {
        self.connectors.push(name.to_string());
        let id = self.outputs.add(name.to_string());
        self.publish_outputs();
        id
    }

    /// Take one away, as `output_destroyed` does: forget the mapping, drop
    /// what pointed at it, publish what is left.
    ///
    /// The mapping goes before the global, and the list is published in
    /// between, because that is the order a compositor does it in: when
    /// `output_destroyed` drops the mapping, sctk's `outputs()` still holds
    /// the dying `wl_output` for one more round. That gap is where #422's
    /// phantom was minted, so a recorder that closed it by hand would be
    /// publishing a list nothing had to filter.
    fn disconnect_output(&mut self, id: OutputId) {
        let Some(at) = self
            .connectors
            .iter()
            .position(|name| self.outputs.id_for(name) == Some(id))
        else {
            return;
        };
        self.outputs.remove(&self.connectors[at]);
        outputs::output_removed(id);
        self.publish_outputs();
        self.connectors.remove(at);
    }

    /// Rebuild the reactive list from the registry, as `sync_outputs` does.
    ///
    /// Through `outputs::sync_outputs`, which is the Wayland handler's own way
    /// in — outputs never reach `Platform`, so a recorder that writes the
    /// signal plays the same part rather than a new one.
    fn publish_outputs(&self) {
        outputs::sync_outputs(self.outputs.connected(
            self.connectors.iter().map(|name| (name.clone(), name)),
            |name, id| Some(OutputInfo::named(id, name)),
        ));
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

    /// A compositor with `wp_viewporter`: without one the loop's declaration
    /// goes nowhere and the recorder would be keeping an empty list.
    fn set_viewport_destination(&mut self, width: u32, height: u32) {
        self.viewport_destinations.push((width, height));
    }

    fn set_input_region(&mut self, rects: Option<&[Rect]>) {
        self.input_regions.push(rects.map(<[Rect]>::to_vec));
    }

    fn sync_input_region(&mut self, request: &crate::region::InputRegionRequest, _commit: bool) {
        self.input_requests.push(request.clone());
    }

    fn has_published_blur(&self) -> bool {
        self.blur_regions
            .last()
            .is_some_and(|rects| !rects.is_empty())
    }

    fn sync_blur_region(&mut self, rects: Vec<crate::region::RegionRect>, _commit: bool) {
        self.blur_regions.push(
            rects
                .iter()
                .map(|r| Rect::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32))
                .collect(),
        );
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

    /// A compositor with `ext-background-effect-v1`: without it the loop never
    /// asks for a blur region, and the recorder would have nothing to keep.
    fn supports_blur_region(&self) -> bool {
        true
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
                input_regions: Vec::from_iter(declared.input_region.map(Some)),
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
        config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        self.created.push(id);
        self.surfaces.insert(
            id,
            RecordedSurface {
                parent: Some(parent),
                grab: config.grab,
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

    /// The same rule the Wayland platform answers with — the recorder's job is
    /// to know who hangs from whom, not to have an opinion about the order.
    fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        crate::descendants_bottom_up(
            root,
            self.surfaces
                .iter()
                .filter_map(|(id, surface)| surface.parent.map(|parent| (*id, parent))),
        )
    }

    /// Likewise: which popups hold a grab is the recorder's to know, what to do
    /// about it is not.
    fn conflicting_grab_popups(&self, new_parent: SurfaceId) -> Vec<SurfaceId> {
        crate::conflicting_grabs(
            new_parent,
            self.surfaces.iter().filter_map(|(id, surface)| {
                surface.parent.map(|parent| (*id, parent, surface.grab))
            }),
        )
    }

    /// A compositor with `ext-session-lock-v1` and no other lock client, which
    /// refuses a second request the way the real one does. The lock is asked
    /// for here and granted later, by [`Headless::grant_lock`] — the round
    /// trip is the reason `LockState::Locking` exists.
    fn start_session_lock(&mut self) -> bool {
        if self.lock.asked {
            return false;
        }
        self.lock.asked = true;
        true
    }

    /// Cover one output. The two refusals are the compositor's own: no lock
    /// to hang the surface on, and a monitor that is not there — the second is
    /// what a lock asking for a departed output's cover runs into, over and
    /// over, and it is kept rather than merely refused.
    fn create_lock_surface(&mut self, id: SurfaceId, output: OutputId) -> bool {
        let accepted = self.lock.asked
            && self
                .connectors
                .iter()
                .any(|name| self.outputs.id_for(name) == Some(output));
        self.lock.requests.push((id, output, accepted));
        if !accepted {
            return false;
        }
        self.created.push(id);
        // Born with nothing, as a lock surface is: its size and its scale both
        // arrive with the compositor's configure, and until one does there is
        // no frame for either to be wrong in.
        self.surfaces.insert(id, RecordedSurface::default());
        true
    }

    fn set_cursor(&mut self, cursor: crate::reactive::CursorIcon) {
        self.cursors.push(cursor);
    }

    fn set_clipboard(&mut self, text: String) {
        self.offer(crate::reactive::SelectionKind::Clipboard, text);
    }

    fn set_primary(&mut self, text: String) {
        self.offer(crate::reactive::SelectionKind::Primary, text);
    }

    /// Answered at once: the reader thread and its pipe are the half a
    /// recorder cannot stand in for.
    fn read_selection(&mut self, kind: crate::reactive::SelectionKind, token: u64) {
        self.selection_reads.push(kind);
        let offered = self.selection(kind).clone();
        crate::reactive::clipboard::answer_paste(token, offered);
    }

    fn take_lock_events(&mut self) -> Vec<LockEvent> {
        std::mem::take(&mut self.lock.events)
    }

    /// The lock object goes back with the session, as `unlock_and_destroy`
    /// hands it back — and no `Finished` follows a clean unlock.
    fn unlock_session(&mut self) {
        self.lock.asked = false;
        self.lock.granted = false;
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
    gpu: crate::renderer::GpuSlot,
    tree: Tree,
    renderer: Option<Renderer>,
    surfaces: SurfaceManager,
    host: Recorder,
    layout_roots: rustc_hash::FxHashMap<WidgetId, Vec<WidgetId>>,
    quit_on_last_surface: bool,
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
        let gpu = crate::renderer::GpuSlot::Shared(shared_device()?);
        // The application's own scope, as `App::run` makes one: what outlives
        // every widget — a decoded image's entry, a global signal — is filed
        // under it rather than under whichever widget asked first.
        reactive::create_root_owner();
        Some(Self {
            gpu,
            tree: Tree::new(),
            renderer: None,
            surfaces: SurfaceManager::new(),
            host: Recorder::default(),
            layout_roots: rustc_hash::FxHashMap::default(),
            quit_on_last_surface: true,
        })
    }

    /// Hold a device of this application's own, made and let go as
    /// `App::run` does, rather than the one every application in a test
    /// binary shares and nothing lets go of.
    pub fn own_gpu(&mut self) {
        self.gpu = crate::renderer::GpuSlot::lazy();
    }

    /// Stand where a machine without a usable Vulkan adapter stands.
    pub fn without_gpu(&mut self) {
        self.gpu = crate::renderer::GpuSlot::Absent;
    }

    /// Whether the compositor was ever asked to lock the session.
    pub fn lock_asked(&self) -> bool {
        self.host.lock.asked
    }

    /// Whether a device and a renderer are held right now.
    pub fn holds_gpu(&self) -> bool {
        self.gpu.held().is_some() && self.renderer.is_some()
    }

    /// What [`App::quit_on_last_surface`](crate::App::quit_on_last_surface)
    /// says: whether closing the last surface ends the loop.
    pub fn quit_on_last_surface(&mut self, quit: bool) {
        self.quit_on_last_surface = quit;
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

    /// Plug a monitor in, under the connector name it would report. Returns
    /// the id it was minted, which is what
    /// [`SurfaceConfig::output`](crate::surface::SurfaceConfig::output) pins a
    /// surface to and what [`disconnect_output`](Self::disconnect_output)
    /// takes back.
    ///
    /// The id is the compositor's to hand out rather than the caller's, which
    /// is why this takes a name and not the whole [`OutputInfo`]: ids are
    /// minted once and never reused, and an application that could choose one
    /// could choose one that has already been a monitor.
    pub fn connect_output(&mut self, name: &str) -> OutputId {
        self.host.connect_output(name)
    }

    /// Unplug one. The reactive list loses it, and so does everything that
    /// said which output a surface was on.
    pub fn disconnect_output(&mut self, id: OutputId) {
        self.host.disconnect_output(id)
    }

    /// Say the compositor mapped a surface onto an output, which is what a
    /// `wl_surface` enter event says and what
    /// [`surface_output`](crate::outputs::surface_output) reports afterwards.
    pub fn enter_output(&mut self, surface: SurfaceId, output: OutputId) {
        outputs::surface_entered_output(surface, output);
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
        let now = Instant::now();
        self.event_at(id, Event::mouse_down(x, y, MouseButton::Left), now);
        self.event_at(id, Event::mouse_up(x, y, MouseButton::Left), now);
    }

    /// Queue any event for one surface, at a moment you name — so a gesture
    /// can be played through the application at the speed it is meant to
    /// have. A widget handed it reads that moment from
    /// [`Tree::event_instant`](crate::tree::Tree::event_instant).
    ///
    /// Delivered by the next [`step`](Self::step), in the order queued, as
    /// [`click`](Self::click)'s are.
    pub fn event_at(&mut self, id: SurfaceId, event: Event, at: Instant) {
        self.host.get_mut(id).events.push((at, event));
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
            gpu: &mut self.gpu,
            renderer: &mut self.renderer,
            quit_on_last_surface: self.quit_on_last_surface,
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

    /// The name of the adapter every surface here draws with.
    ///
    /// A harness that measures has to be able to say what produced the number:
    /// the same frame costs one thing on a GPU and another on lavapipe, and a
    /// timing that does not name its adapter is a claim about neither.
    pub fn adapter_name(&self) -> &str {
        self.gpu.held().map_or("", |gpu| &gpu.adapter_info.name)
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

    /// Every input region a surface has asked for, oldest first. `None` is the
    /// whole surface, and an empty list is a surface that takes no input at
    /// all.
    pub fn input_regions_asked(&self, id: SurfaceId) -> &[Option<Vec<Rect>>] {
        &self.host.get(id).input_regions
    }

    /// Whether input reaches a point of a surface, by the reading the
    /// compositor gives the last region it was handed: the area the surface
    /// takes, then every declaration the frame made, in order.
    ///
    /// A surface whose tree has never declared anything takes input
    /// everywhere, which is what it asked for by saying nothing.
    pub fn input_reaches(&self, id: SurfaceId, x: f32, y: f32) -> bool {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        let surface = self.host.get(id);

        // Before the tree has declared anything, the answer is whatever the
        // surface itself asked for at birth or through a handle — which is
        // "everywhere" only when it asked for nothing.
        let Some(request) = surface.input_requests.last() else {
            return match surface.input_regions.last() {
                None | Some(None) => true,
                Some(Some(rects)) => rects.iter().any(|r| r.contains(x as f32, y as f32)),
            };
        };

        // The compositor's reading of the program it was handed: the
        // area the surface takes, then every declaration in order.
        // Here rather than beside the type, because nothing the library
        // does asks this question — only a test does.
        let mut takes = request.base.iter().any(|r| r.contains(x, y));
        for op in &request.ops {
            if op.rect.contains(x, y) {
                takes = op.takes;
            }
        }
        takes
    }

    /// How many times a surface has published a derived input region. A frame
    /// that changes nothing must not add to this.
    pub fn input_regions_published(&self, id: SurfaceId) -> usize {
        self.host.get(id).input_requests.len()
    }

    /// Every blur region a surface was handed, oldest first, in logical
    /// pixels. An empty one withdraws the last.
    ///
    /// Every call, not every change: a frame that repaints under an unchanged
    /// blur hands it over again, and it is the platform that drops the repeat.
    pub fn blur_regions_asked(&self, id: SurfaceId) -> &[Vec<Rect>] {
        &self.host.get(id).blur_regions
    }

    /// The sizes a surface has asked for, oldest first.
    pub fn sizes_asked(&self, id: SurfaceId) -> &[(u32, u32)] {
        &self.host.get(id).sizes_asked
    }

    /// Every logical size a surface has declared its buffer stands for, oldest
    /// first — what a `wp_viewport` destination says, beside the buffer
    /// [`physical_size`](Self::physical_size) reports. The two are what a
    /// compositor reads together, and a test that reads only one of them
    /// cannot see them disagree.
    pub fn viewport_destinations(&self, id: SurfaceId) -> &[(u32, u32)] {
        &self.host.get(id).viewport_destinations
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

    /// The surfaces the application itself still holds, in the order the ids
    /// were handed out — what the next frame would draw on.
    ///
    /// The other half of [`surfaces_destroyed`](Self::surfaces_destroyed): a
    /// surface the compositor was told to destroy and the application goes on
    /// holding is abandoned rather than torn down, and only these two
    /// together can say so.
    pub fn surfaces_live(&self) -> Vec<SurfaceId> {
        let mut ids: Vec<SurfaceId> = self.surfaces.ids().collect();
        ids.sort_by_key(SurfaceId::raw);
        ids
    }

    /// Every cursor shape the seat was asked for, oldest first. The seat's and
    /// not a surface's: a shape goes out against the pointer's latest enter,
    /// whichever surface that was.
    pub fn cursors_asked(&self) -> &[crate::reactive::CursorIcon] {
        &self.host.cursors
    }

    /// Another application copies `text`: the seat now offers it as `kind`.
    /// Nothing is read until the application pastes.
    pub fn offer_selection(&mut self, kind: crate::reactive::SelectionKind, text: &str) {
        self.host.offer(kind, text.to_owned());
    }

    /// Every selection read the application asked the seat for, oldest first.
    pub fn selection_reads(&self) -> &[crate::reactive::SelectionKind] {
        &self.host.selection_reads
    }

    /// Say the compositor granted the lock the application asked for, which is
    /// what `ext_session_lock_v1.locked` says. Until it does, the application
    /// sits in [`LockState::Locking`](crate::session_lock::LockState::Locking),
    /// with its lock surfaces already asked for — a compositor answers a round
    /// trip later, may wait for those surfaces to draw first, and may refuse
    /// through [`finish_lock`](Self::finish_lock).
    pub fn grant_lock(&mut self) {
        self.host.lock.granted = true;
        self.host.lock.events.push(LockEvent::Locked);
    }

    /// Say the compositor ended the lock, which is what
    /// `ext_session_lock_v1.finished` says: sent in place of `locked` when it
    /// refuses, or later when the lock ends without the application's unlock.
    /// Either way the lock object is gone, as `finished` clears `active_lock`.
    pub fn finish_lock(&mut self) {
        crate::Platform::unlock_session(&mut self.host);
        self.host.lock.events.push(LockEvent::Finished);
    }

    /// Whether the compositor is holding a lock grant. The application's own
    /// view of it is [`session_locked`](crate::session_lock::session_locked);
    /// this is the half that says the unlock reached the compositor.
    pub fn is_locked(&self) -> bool {
        self.host.lock.granted
    }

    /// Every lock surface the compositor was asked for, oldest first: the id,
    /// the output it was to cover, and whether it was accepted. Refused asks
    /// are in it, because a refusal that repeats is the whole of what #422
    /// looked like from here.
    pub fn lock_surface_requests(&self) -> &[(SurfaceId, OutputId, bool)] {
        &self.host.lock.requests
    }

    /// The lock surfaces it accepted, oldest first — the asks above, less the
    /// refusals.
    pub fn lock_surfaces_created(&self) -> Vec<(SurfaceId, OutputId)> {
        self.host
            .lock
            .requests
            .iter()
            .filter(|(_, _, accepted)| *accepted)
            .map(|(id, output, _)| (*id, *output))
            .collect()
    }

    /// The colour at one pixel of a surface's last frame, in physical
    /// coordinates.
    pub fn read_pixel(&self, id: SurfaceId, x: u32, y: u32) -> [u8; 4] {
        match self.target(id) {
            RenderTarget::Offscreen(offscreen) => offscreen.read_pixel(x, y),
            RenderTarget::Swapchain(_) => panic!("a headless surface has no swapchain"),
        }
    }

    /// Hold this application's image decodes until the hold is released or
    /// dropped, so a frame can be stepped while a raster source is still
    /// pending — which otherwise depends on how fast the worker is.
    pub fn hold_image_decodes(&self) -> DecodeHold {
        crate::image_decode::hold()
    }

    /// Block until every image decode this application started has finished
    /// and queued its result. The result is applied by the next
    /// [`step`](Self::step), as the loop applies any background write.
    ///
    /// Panics if the decodes are held.
    pub fn wait_for_image_decodes(&self) {
        crate::image_decode::wait();
    }

    /// How many image decodes this application has started.
    pub fn image_decodes_started(&self) -> u64 {
        crate::image_decode::started()
    }

    /// How many bytes of decoded pixels the image cache is holding — the
    /// pixels waiting for the renderer, which lets go of them once uploaded.
    pub fn image_bytes_held(&self) -> usize {
        crate::image_decode::held_bytes()
    }

    /// Drop every image texture the renderer holds, as eviction would, so a
    /// test can ask what happens when one that was uploaded is needed again.
    pub fn forget_image_textures(&mut self) {
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.forget_image_textures();
        }
    }
}

pub use crate::image_decode::DecodeHold;

#[cfg(test)]
mod the_two_refusals_the_loop_cannot_reach {
    use super::*;

    /// A cover is refused without a lock to hang it on, and refused for a
    /// monitor that is not there.
    ///
    /// Beside the recorder rather than in `tests/headless_app.rs` because the
    /// loop reaches neither: it asks for a cover only while a lock is
    /// held, and only for an output the list holds. What makes them
    /// load-bearing anyway is the case where the list and the registry
    /// disagree — the second refusal is what #422's phantom ran into, sixty
    /// times a second.
    #[test]
    fn a_cover_is_refused_without_a_lock_and_for_a_monitor_that_is_not_there() {
        let mut recorder = Recorder::default();
        let screen = recorder.connect_output("eDP-1");
        let phantom = OutputId::from_raw(9);

        assert!(
            !recorder.create_lock_surface(SurfaceId::next(), screen),
            "no lock has been asked for yet"
        );

        assert!(recorder.start_session_lock());
        assert!(
            !recorder.create_lock_surface(SurfaceId::next(), phantom),
            "an output the registry never gave that id to"
        );
        assert!(
            recorder.create_lock_surface(SurfaceId::next(), screen),
            "and the monitor that is really there is covered"
        );

        let accepted: Vec<bool> = recorder
            .lock
            .requests
            .iter()
            .map(|(_, _, accepted)| *accepted)
            .collect();
        assert_eq!(
            accepted,
            [false, false, true],
            "every ask is kept, refused or not"
        );
    }
}

/// An application forgets what it left on its thread, so the next one there
/// starts clean — `reset_thread_state`, the same sequence `App::drop` runs and
/// in the same place.
///
/// `App` has done this since it existed; `Headless` never did, because every
/// test in `tests/` builds one application and cargo gives each test a thread
/// of its own, so no second application ever ran on a thread that had already
/// held one. `benches/scroll_list` is the first caller that does, and the worst
/// of what it inherited was the `Unregister` jobs a dropped tree queues: the
/// next application's widgets are handed the same ids, and answer to them. Its
/// list of two hundred rows painted two children a frame.
///
/// What it does *not* do is clear the process-wide half — the wake flag, the
/// ingress sender. `App::drop` may, because a program has one application; this
/// must not, because a test binary runs several at once and clearing them
/// reaches into somebody else's.
impl Drop for Headless {
    fn drop(&mut self) {
        // The surfaces first. Each disposes its reactive owner as it goes, and
        // an owner disposed after the arena is wiped names somebody else's
        // signals.
        drop(std::mem::take(&mut self.surfaces));
        // And the renderer, whose textures report their eviction to the image
        // cache as they go: dropped after the reset, those reports would be
        // left for the next application on this thread to act on.
        drop(self.renderer.take());
        crate::reset_thread_state(&mut self.tree);
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
            std::ptr::eq(first.gpu.held().unwrap(), second.gpu.held().unwrap()),
            "a second application built a context of its own"
        );
    }
}

#[cfg(test)]
mod the_next_application_on_this_thread {
    use super::*;
    use crate::layout::Flex;
    use crate::widgets::{Color, container};

    /// The list two applications in a row each put on a surface, white on a
    /// black surface so that a row which is there and a row which is not are
    /// one pixel apart.
    fn rows() -> impl Widget + 'static {
        container()
            .layout(Flex::column())
            .children((0..200).map(|_| {
                container()
                    .width(40.0)
                    .height(10.0)
                    .background(Color::WHITE)
            }))
    }

    /// A second application does not inherit the first one's teardown.
    ///
    /// Dropping a tree queues an `Unregister` job per widget, and the next
    /// application's widgets are handed the same ids: without the reset, the
    /// second application's rows were unregistered by the first's departure
    /// before they had drawn once, and its surface came up black. Every test in
    /// `tests/` builds one application, and cargo gives each test a thread of
    /// its own, so nothing asked this until a benchmark played one script
    /// twice.
    #[test]
    fn does_not_inherit_the_last_one_tearing_its_widgets_down() {
        let Some(mut first) = crate::or_skip(Headless::new()) else {
            return;
        };
        let white = drew_a_row(&mut first);
        assert_eq!(white, [255, 255, 255, 255], "the first application's row");
        drop(first);

        let mut second = Headless::new().expect("the first one had an adapter");
        assert_eq!(
            drew_a_row(&mut second),
            white,
            "the second application's row is not the one the first one drew"
        );
    }

    /// One surface, one frame, and the colour where the third row should be.
    fn drew_a_row(app: &mut Headless) -> [u8; 4] {
        let id = app.surface(
            SurfaceConfig::new()
                .width(100)
                .height(200)
                .background_color(Color::BLACK),
            rows,
        );
        app.configure(id, 100, 200, 1.0);
        app.step();
        app.read_pixel(id, 20, 25)
    }
}
