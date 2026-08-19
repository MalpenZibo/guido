use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::{
    Connection, EventQueue, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    data_device_manager::DataDeviceManagerState,
    delegate_compositor, delegate_layer, delegate_registry,
    output::OutputState,
    primary_selection::PrimarySelectionManagerState,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{SeatState, pointer::cursor_shape::CursorShapeManager},
    session_lock::{SessionLockState, SessionLockSurface},
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{XdgShell, popup::Popup},
    },
};
use wayland_backend::sys::client::ObjectId;
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

use std::cell::Cell;
use std::collections::HashMap;

use super::backdrop::Backdrop;
use super::input::InputState;
use super::lock::Lock;
use super::outputs::OutputRegistry;
use super::popups::Popups;
use super::selections::Selections;
use crate::blur::BlurRect;
use crate::outputs::{self};
use crate::surface::SurfaceId;
use crate::widgets::{Event, Rect};

/// The shell role of a surface: an ordinary layer-shell surface or an
/// ext-session-lock-v1 lock surface. Both share the same widget tree, GPU
/// and input pipeline; only creation, configure, and shell requests differ.
pub enum SurfaceRole {
    Layer(LayerSurface),
    /// Kept alive here — dropping it destroys the protocol object.
    Lock(SessionLockSurface),
    /// An xdg popup anchored to another surface. Kept alive here —
    /// dropping it destroys the xdg objects. The config is retained to
    /// rebuild positioners for repositioning (auto-height growth).
    Popup {
        popup: Popup,
        config: crate::surface::PopupConfig,
        /// The surface this popup is anchored to — used to tear down popup
        /// chains in protocol order (children before parents).
        parent: crate::surface::SurfaceId,
    },
}

/// Events from the session-lock protocol, drained by the main loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEvent {
    /// The compositor granted the lock; lock surfaces may be created.
    Locked,
    /// The lock ended: denied outright, or unlocked/aborted later.
    Finished,
}

/// Per-surface state for multi-surface support.
pub struct WaylandSurfaceState {
    /// The shell role (layer surface or session-lock surface)
    pub role: SurfaceRole,
    /// The underlying wl_surface
    pub wl_surface: wl_surface::WlSurface,
    /// Whether the surface has been configured
    pub configured: bool,
    /// Logical width of the surface
    pub width: u32,
    /// Logical height of the surface
    pub height: u32,
    /// Scale factor for HiDPI
    pub scale_factor: f32,
    /// Whether scale factor has been received
    pub scale_factor_received: bool,
    /// Whether the first frame has been presented
    pub first_frame_presented: bool,
    /// Whether a wl_surface.frame callback is in flight. While true, the
    /// compositor has not yet shown the last frame — rendering another one
    /// would outpace it. Cleared by the frame-done handler.
    pub frame_callback_pending: bool,
    /// Pending events for this surface
    pub pending_events: Vec<Event>,
    /// Blur proxy for this surface, created lazily on first use (asking the
    /// manager twice for the same surface is a protocol error).
    pub(super) bg_effect_surface: Option<ExtBackgroundEffectSurfaceV1>,
    /// Last blur rects pushed to the compositor. `None` means nothing has
    /// been pushed yet (also reset when the blur capability changes, so the
    /// region is re-sent if it comes back).
    pub(super) blur_region: Option<Vec<BlurRect>>,
    /// Set when the compositor's blur capability changes, so a surface that is
    /// not repainting knows it owes a region — and, the rest of the time, knows
    /// it does not. See
    /// [`take_blur_resync`](WaylandState::take_blur_resync).
    pub(super) blur_resync_owed: bool,
    /// Height sent in an in-flight popup reposition (cleared by the popup
    /// configure), so repeated content measurements don't re-send it.
    pub(super) pending_popup_height: Option<u32>,
}

impl WaylandSurfaceState {
    /// Create a new surface state.
    pub fn new(
        role: SurfaceRole,
        wl_surface: wl_surface::WlSurface,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            role,
            wl_surface,
            configured: false,
            width,
            height,
            scale_factor: 1.0,
            scale_factor_received: false,
            first_frame_presented: false,
            frame_callback_pending: false,
            pending_events: Vec::new(),
            bg_effect_surface: None,
            blur_region: None,
            blur_resync_owed: false,
            pending_popup_height: None,
        }
    }

    /// Take all pending events (drains the queue)
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
    }
}

thread_local! {
    /// Which surface a [`batch_layer_requests`](WaylandState::batch_layer_requests)
    /// group is open on: its requests hold their commit so the group makes one.
    ///
    /// The *id*, not a flag. The closure is handed a whole `&mut WaylandState`,
    /// so it can address any surface it likes, and a flag suppressed the commit
    /// of every one of them while promising it for exactly one: a request aimed
    /// at another surface stayed double-buffered until something unrelated
    /// committed that surface, which for an idle one is never.
    ///
    /// A dynamic scope rather than a field, because the scope has to survive a
    /// panic. The closure gets `&mut WaylandState`, so a guard restoring a field
    /// could not hold on to it while the closure runs — and a scope left open by
    /// an unwind would be permanent: every later `set_margin` or `set_layer`
    /// would hold its commit for a group that had already ended, and the surface
    /// would stop answering its handle in silence.
    static BATCHING: Cell<Option<SurfaceId>> = const { Cell::new(None) };
}

/// Run `f` inside a batching scope for `id`, and report whether it opened it.
///
/// Restored on the way out however that happens, which is the whole point. A
/// group nested inside one on *another* surface opens its own, because the
/// commit it owes is not the one the outer group will make.
fn batching<R>(id: SurfaceId, f: impl FnOnce() -> R) -> (bool, R) {
    struct Restore(Option<SurfaceId>);
    impl Drop for Restore {
        fn drop(&mut self) {
            BATCHING.with(|b| b.set(self.0));
        }
    }

    let outer = BATCHING.with(|b| b.replace(Some(id)));
    let _restore = Restore(outer);
    (outer != Some(id), f())
}

pub struct WaylandState {
    pub registry_state: RegistryState,
    pub compositor_state: CompositorState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub layer_shell: LayerShell,

    /// Whether the application should exit
    pub exit: bool,
    // Multi-surface tracking
    /// All surfaces indexed by SurfaceId
    pub surfaces: HashMap<SurfaceId, WaylandSurfaceState>,
    /// Lookup from wl_surface ObjectId to SurfaceId
    pub surface_lookup: HashMap<ObjectId, SurfaceId>,
    /// Which surface currently has pointer focus
    pub current_pointer_surface: Option<SurfaceId>,
    /// Which surface currently has keyboard focus
    pub current_keyboard_surface: Option<SurfaceId>,

    /// Stable identity for the compositor's outputs — see [`super::outputs`].
    pub(super) outputs: OutputRegistry,

    /// Compositor-side backdrop blur — see [`super::backdrop`].
    pub(super) backdrop: Backdrop,

    /// xdg popups anchored to a surface — see [`super::popups`].
    pub(super) popups: Popups,

    /// Session lock grant and events — see [`super::lock`].
    pub(super) lock: Lock,

    /// Pointer, touch and keyboard — see [`super::input`].
    pub(super) input: InputState,

    /// Clipboard and primary selection — see [`super::selections`].
    pub(super) selections: Selections,
}

/// Why the platform layer could not start or continue.
///
/// These are ordinary environmental conditions (no Wayland session, a
/// compositor without layer-shell such as GNOME, a dropped connection) —
/// they are reported instead of panicking the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformError {
    /// Could not connect to a Wayland display (no session / wrong env).
    Connect,
    /// Wayland registry initialization failed.
    Registry,
    /// The compositor does not advertise `wl_compositor`.
    MissingCompositor,
    /// The compositor does not support `zwlr_layer_shell_v1` (e.g. GNOME).
    MissingLayerShell,
    /// The Wayland connection failed while the app was running.
    ConnectionLost,
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect => write!(f, "failed to connect to a Wayland display"),
            Self::Registry => write!(f, "failed to initialize the Wayland registry"),
            Self::MissingCompositor => write!(f, "compositor does not advertise wl_compositor"),
            Self::MissingLayerShell => {
                write!(
                    f,
                    "compositor does not support zwlr_layer_shell_v1 (layer shell)"
                )
            }
            Self::ConnectionLost => write!(f, "Wayland connection lost"),
        }
    }
}

impl std::error::Error for PlatformError {}

#[allow(clippy::type_complexity)]
pub fn create_wayland_app(
    loop_handle: LoopHandle<'static, WaylandState>,
) -> Result<
    (
        Connection,
        EventQueue<WaylandState>,
        WaylandState,
        QueueHandle<WaylandState>,
    ),
    PlatformError,
> {
    let connection = Connection::connect_to_env().map_err(|e| {
        log::error!("Failed to connect to Wayland: {e}");
        PlatformError::Connect
    })?;
    let (globals, event_queue) = registry_queue_init::<WaylandState>(&connection).map_err(|e| {
        log::error!("Failed to initialize Wayland registry: {e}");
        PlatformError::Registry
    })?;
    let qh = event_queue.handle();

    let compositor_state =
        CompositorState::bind(&globals, &qh).map_err(|_| PlatformError::MissingCompositor)?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|_| {
        log::error!(
            "This compositor does not support the layer shell protocol; \
             guido surfaces cannot be created"
        );
        PlatformError::MissingLayerShell
    })?;
    let output_state = OutputState::new(&globals, &qh);
    let seat_state = SeatState::new(&globals, &qh);
    let session_lock_state = SessionLockState::new(&globals, &qh);

    // xdg shell for popups anchored to layer surfaces — optional
    let xdg_shell = XdgShell::bind(&globals, &qh).ok();
    if xdg_shell.is_none() {
        log::warn!("xdg_wm_base not available - popups will not work");
    }

    // Initialize data device manager for clipboard support
    let data_device_manager = DataDeviceManagerState::bind(&globals, &qh).ok();
    if data_device_manager.is_none() {
        log::warn!("Data device manager not available - clipboard will not work");
    }

    // Primary selection (select-to-copy / middle-click paste) — optional
    let primary_selection_manager = PrimarySelectionManagerState::bind(&globals, &qh).ok();
    if primary_selection_manager.is_none() {
        log::info!("Primary selection manager not available - middle-click paste will not work");
    }

    // Initialize cursor shape manager for cursor changes
    let cursor_shape_manager = CursorShapeManager::bind(&globals, &qh).ok();
    if cursor_shape_manager.is_none() {
        log::warn!("Cursor shape manager not available - cursor changes will not work");
    }

    // Background effect manager (blur) — optional, no-ops when unsupported
    let bg_effect_manager: Option<ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    if bg_effect_manager.is_none() {
        log::info!("ext-background-effect-v1 not available - background blur will not work");
    }

    let state = WaylandState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        output_state,
        seat_state,
        layer_shell,
        exit: false,
        surfaces: HashMap::new(),
        surface_lookup: HashMap::new(),
        current_pointer_surface: None,
        current_keyboard_surface: None,
        outputs: OutputRegistry::new(),
        backdrop: Backdrop::new(bg_effect_manager),
        popups: Popups::new(xdg_shell),
        lock: Lock::new(session_lock_state),
        input: InputState::new(cursor_shape_manager, loop_handle),
        selections: Selections::new(data_device_manager, primary_selection_manager),
    };

    Ok((connection, event_queue, state, qh))
}

impl WaylandState {
    /// Build a wl_region from logical-coordinate rects, rounded outward so a
    /// fractional widget bound never loses its edge pixels.
    fn build_region(&self, rects: &[Rect]) -> Option<Region> {
        let region = Region::new(&self.compositor_state)
            .map_err(|e| log::warn!("Failed to create wl_region: {e}"))
            .ok()?;
        for r in rects {
            let x = r.x.floor() as i32;
            let y = r.y.floor() as i32;
            let width = (r.x + r.width).ceil() as i32 - x;
            let height = (r.y + r.height).ceil() as i32 - y;
            region.add(x, y, width, height);
        }
        Some(region)
    }

    /// Apply an input region to a wl_surface. `None` restores the default
    /// (the whole surface accepts input); an empty slice is fully
    /// click-through.
    ///
    /// The `Region` is dropped (and the wl_region destroyed) right away:
    /// the protocol copies the region state at `set_input_region` time.
    fn apply_input_region(&self, wl_surface: &wl_surface::WlSurface, rects: Option<&[Rect]>) {
        match rects {
            None => wl_surface.set_input_region(None),
            Some(rects) => {
                let Some(region) = self.build_region(rects) else {
                    return;
                };
                wl_surface.set_input_region(Some(region.wl_region()));
            }
        }
    }

    /// Set the input region for a surface at runtime.
    pub fn set_surface_input_region(&mut self, id: SurfaceId, rects: Option<&[Rect]>) {
        if let Some(surface_state) = self.surfaces.get(&id) {
            self.apply_input_region(&surface_state.wl_surface, rects);
            surface_state.wl_surface.commit();
            log::info!(
                "Surface {:?} input region set to {}",
                id,
                match rects {
                    None => "full surface".to_string(),
                    Some(r) => format!("{} rect(s)", r.len()),
                }
            );
        }
    }
    /// Create a layer surface with a specific SurfaceId.
    pub fn create_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        config: &crate::surface::SurfaceConfig,
    ) {
        // Resolve the requested output; fall back to letting the compositor
        // choose if it was disconnected in the meantime.
        let target_output = config.output.and_then(|oid| {
            let found = self.wl_output_for(oid);
            if found.is_none() {
                log::warn!(
                    "Surface {:?} requested output {:?} which is not connected; \
                     letting the compositor choose",
                    id,
                    oid
                );
            }
            found
        });

        let wl_surface = self.compositor_state.create_surface(qh);
        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            wl_surface.clone(),
            config.layer,
            Some(config.namespace.clone()),
            target_output.as_ref(),
        );

        layer_surface.set_anchor(config.anchor);

        // When anchored to both edges on an axis, the compositor owns
        // that dimension: set it to 0 so it stretches. Content sizing on
        // such an axis can never take effect — warn loudly.
        crate::surface::warn_content_on_stretched_axis(id, config);
        // The same rule the runtime resize uses, so creation and re-anchoring
        // cannot disagree about which axes are the compositor's.
        let (use_width, use_height, _) = crate::surface::resize_request(config, None);
        // Before the honouring: the size the surface believes it has, and the
        // one a reservation resolves against. Through the same helper as the
        // request, not `SurfaceExtent::initial()`, which happens to agree today
        // only because `requested_extent` with no configure falls back to it.
        let initial_width = crate::surface::requested_extent(config.width, None);
        let initial_height = crate::surface::requested_extent(config.height, None);

        layer_surface.set_size(use_width, use_height);
        layer_surface.set_keyboard_interactivity(config.keyboard_interactivity);

        let zone = config.exclusive_zone.resolve(
            config.anchor,
            config.margin,
            initial_width,
            initial_height,
        );
        layer_surface.set_exclusive_zone(zone);

        let margin = config.margin;
        if !margin.is_zero() {
            layer_surface.set_margin(margin.top, margin.right, margin.bottom, margin.left);
        }

        if let Some(rects) = &config.input_region {
            self.apply_input_region(&wl_surface, Some(rects));
        }

        wl_surface.commit();

        // Register in lookup table
        let object_id = wl_surface.id();
        self.surface_lookup.insert(object_id, id);

        // Create and store surface state
        let surface_state = WaylandSurfaceState::new(
            SurfaceRole::Layer(layer_surface),
            wl_surface,
            initial_width,
            initial_height,
        );
        self.surfaces.insert(id, surface_state);

        log::info!(
            "Created surface {:?} with size {}x{}, anchor {:?}, layer {:?}, keyboard {:?}",
            id,
            initial_width,
            initial_height,
            config.anchor,
            config.layer,
            config.keyboard_interactivity
        );
    }

    /// Destroy a surface by its SurfaceId.
    pub fn destroy_surface(&mut self, id: SurfaceId) {
        if let Some(surface_state) = self.surfaces.remove(&id) {
            // Remove from lookup table
            let object_id = surface_state.wl_surface.id();
            self.surface_lookup.remove(&object_id);

            // Destroy the blur proxy before its wl_surface goes away
            if let Some(effect) = surface_state.bg_effect_surface {
                effect.destroy();
            }

            // Clear pointer/keyboard focus if this surface had it
            if self.current_pointer_surface == Some(id) {
                self.current_pointer_surface = None;
            }
            if self.current_keyboard_surface == Some(id) {
                self.current_keyboard_surface = None;
            }

            // Drop reactive output tracking for the closed surface
            outputs::surface_closed(id);

            // The LayerSurface and WlSurface will be destroyed when dropped
            log::info!("Destroyed surface {:?}", id);
        }
    }

    /// Apply several layer-shell requests as one change.
    ///
    /// `with_layer_surface` commits after each
    /// request, which is right for one on its own and wrong for a group:
    /// re-anchoring sends the anchor, the size and the reservation, and three
    /// commits is two of them showing the compositor a surface halfway between
    /// two configurations — before this, an app animating a dock's margin sent a
    /// redundant pair every frame.
    ///
    /// The commit is the group's, so it is subject to the same rule each request
    /// is: only a layer surface has these properties, and a batch aimed at a
    /// session-lock or popup surface applies nothing. Committing anyway would
    /// send a bare commit to exactly the roles the per-request path refuses to
    /// touch.
    pub fn batch_layer_requests(&mut self, id: SurfaceId, f: impl FnOnce(&mut Self)) {
        let (outermost, ()) = batching(id, || f(self));
        if outermost
            && let Some(state) = self.surfaces.get(&id)
            && matches!(state.role, SurfaceRole::Layer(_))
        {
            state.wl_surface.commit();
        }
    }

    /// Helper to modify a surface's layer shell properties and commit.
    /// No-ops (with a warning) on session-lock surfaces, which have none.
    fn with_layer_surface<F>(&mut self, id: SurfaceId, f: F)
    where
        F: FnOnce(&LayerSurface),
    {
        // Only for the surface the open group will commit for. Another one's
        // request has nobody to ride with, so it commits for itself.
        let batching = BATCHING.with(|b| b.get()) == Some(id);
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            match &surface_state.role {
                SurfaceRole::Layer(layer_surface) => {
                    f(layer_surface);
                    if !batching {
                        surface_state.wl_surface.commit();
                    }
                }
                SurfaceRole::Lock(_) | SurfaceRole::Popup { .. } => {
                    log::warn!(
                        "Ignoring layer-shell property change on non-layer surface {:?}",
                        id
                    );
                }
            }
        }
    }

    /// Set the layer for a surface.
    pub fn set_surface_layer(&mut self, id: SurfaceId, layer: Layer) {
        self.with_layer_surface(id, |ls| ls.set_layer(layer));
        log::info!("Surface {:?} layer set to {:?}", id, layer);
    }

    /// Set the keyboard interactivity for a surface.
    pub fn set_surface_keyboard_interactivity(
        &mut self,
        id: SurfaceId,
        mode: KeyboardInteractivity,
    ) {
        self.with_layer_surface(id, |ls| ls.set_keyboard_interactivity(mode));
        log::info!("Surface {:?} keyboard interactivity set to {:?}", id, mode);
    }

    /// Set the anchor edges for a surface.
    pub fn set_surface_anchor(&mut self, id: SurfaceId, anchor: Anchor) {
        self.with_layer_surface(id, |ls| ls.set_anchor(anchor));
        log::info!("Surface {:?} anchor set to {:?}", id, anchor);
    }

    /// Set the size of a surface.
    pub fn set_surface_size(&mut self, id: SurfaceId, width: u32, height: u32) {
        self.with_layer_surface(id, |ls| ls.set_size(width, height));
        log::info!("Surface {:?} size set to {}x{}", id, width, height);
    }

    /// Set the exclusive zone for a surface.
    pub fn set_surface_exclusive_zone(&mut self, id: SurfaceId, zone: i32) {
        self.with_layer_surface(id, |ls| ls.set_exclusive_zone(zone));
        log::info!("Surface {:?} exclusive zone set to {}", id, zone);
    }

    /// Set the margin for a surface.
    pub fn set_surface_margin(&mut self, id: SurfaceId, margin: crate::surface::Margin) {
        self.with_layer_surface(id, |ls| {
            ls.set_margin(margin.top, margin.right, margin.bottom, margin.left)
        });
        log::info!("Surface {:?} margin set to {:?}", id, margin);
    }

    /// Get a surface state by SurfaceId.
    pub fn get_surface(&self, id: SurfaceId) -> Option<&WaylandSurfaceState> {
        self.surfaces.get(&id)
    }

    /// Get a mutable surface state by SurfaceId.
    pub fn get_surface_mut(&mut self, id: SurfaceId) -> Option<&mut WaylandSurfaceState> {
        self.surfaces.get_mut(&id)
    }

    /// Get a surface ID from a wl_surface.
    pub fn surface_id_from_wl_surface(
        &self,
        wl_surface: &wl_surface::WlSurface,
    ) -> Option<SurfaceId> {
        self.surface_lookup.get(&wl_surface.id()).copied()
    }

    /// Check if all surfaces are configured.
    pub fn all_surfaces_configured(&self) -> bool {
        self.surfaces.values().all(|s| s.configured)
    }

    /// Check if any surface needs rendering.
    pub fn any_surface_needs_render(&self) -> bool {
        self.surfaces
            .values()
            .any(|s| !s.first_frame_presented || !s.scale_factor_received)
    }
}

pub struct WaylandWindowWrapper {
    display: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
}

impl WaylandWindowWrapper {
    pub fn new(connection: &Connection, surface: &wl_surface::WlSurface) -> Self {
        // Get raw pointers using wayland-backend's sys module
        // The ObjectId in sys backend has as_ptr() method
        let backend = connection.backend();

        // Get display pointer - this is the wl_display*
        let display_ptr = backend.display_ptr() as *mut std::ffi::c_void;

        // Get surface pointer - need to convert the wayland-client ObjectId to sys ObjectId
        // The surface.id() returns a wayland_backend::client::ObjectId
        // We need to get the raw wl_proxy* pointer from it
        let surface_id = surface.id();
        let surface_ptr = ObjectId::as_ptr(&surface_id) as *mut std::ffi::c_void;

        Self {
            display: display_ptr,
            surface: surface_ptr,
        }
    }
}

unsafe impl Send for WaylandWindowWrapper {}
unsafe impl Sync for WaylandWindowWrapper {}

impl HasDisplayHandle for WaylandWindowWrapper {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = WaylandDisplayHandle::new(
            std::ptr::NonNull::new(self.display).expect("display ptr is null"),
        );
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(handle)) })
    }
}

impl HasWindowHandle for WaylandWindowWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = WaylandWindowHandle::new(
            std::ptr::NonNull::new(self.surface).expect("surface ptr is null"),
        );
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Wayland(handle)) })
    }
}

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        // Find which surface this is for
        if let Some(id) = self.surface_lookup.get(&surface.id()).copied()
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            log::info!("Surface {:?} scale factor changed to: {}", id, new_factor);
            surface_state.scale_factor = new_factor as f32;
            surface_state.scale_factor_received = true;
        }

        // Set the buffer scale on the surface for proper HiDPI rendering
        surface.set_buffer_scale(new_factor);
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if let Some(surface_id) = self.surface_lookup.get(&surface.id()).copied() {
            let output_id = self.ensure_output_id(output);
            log::debug!("Surface {:?} entered output {:?}", surface_id, output_id);
            outputs::surface_entered_output(surface_id, output_id);
        }
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if let Some(surface_id) = self.surface_lookup.get(&surface.id()).copied()
            && let Some(output_id) = self.outputs.output_ids.get(&output.id()).copied()
        {
            log::debug!("Surface {:?} left output {:?}", surface_id, output_id);
            outputs::surface_left_output(surface_id, output_id);
        }
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // The compositor consumed the last frame: this surface may render
        // again. Callbacks are re-armed on every present (see
        // render_surface), so this is the pacing signal for the surface.
        if let Some(id) = self.surface_lookup.get(&surface.id()).copied()
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            surface_state.frame_callback_pending = false;
            if !surface_state.first_frame_presented {
                log::info!(
                    "Surface {:?} first frame presented by compositor - initialization complete",
                    id
                );
                surface_state.first_frame_presented = true;
            }
            // Wake the loop so a dirty surface renders promptly
            crate::jobs::wake_loop();
        }
    }
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        // O(1) lookup via the wl_surface object id
        let closed_id = self.surface_lookup.get(&layer.wl_surface().id()).copied();

        if let Some(id) = closed_id {
            log::info!("Surface {:?} closed by compositor", id);
            // Route through the normal close command so the managed surface
            // is fully cleaned up (widgets, reactive owner, wgpu surface —
            // dropped BEFORE the wl_surface it borrows). Previously only the
            // Wayland-side state was removed, leaking the widget tree and
            // leaving a zombie surface iterated every frame.
            crate::surface::push_surface_command(crate::surface::SurfaceCommand::Close(id));
        }
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        // O(1) lookup via the wl_surface object id (fires on every resize)
        let surface_id = self.surface_lookup.get(&layer.wl_surface().id()).copied();

        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            log::info!(
                "Surface {:?} configure: requested size {:?}, current {}x{}",
                id,
                configure.new_size,
                surface_state.width,
                surface_state.height
            );
            if configure.new_size.0 > 0 {
                surface_state.width = configure.new_size.0;
            }
            if configure.new_size.1 > 0 {
                surface_state.height = configure.new_size.1;
            }
            log::info!(
                "Surface {:?} using size: {}x{}",
                id,
                surface_state.width,
                surface_state.height
            );
            surface_state.configured = true;
        }
    }
}

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(WaylandState);
delegate_layer!(WaylandState);
delegate_registry!(WaylandState);

#[cfg(test)]
mod batching_tests {
    use super::{BATCHING, SurfaceId, batching};

    /// Distinct ids; the counter is global and the numbers do not matter.
    fn surface() -> SurfaceId {
        SurfaceId::next()
    }

    /// A batch is a scope, and a scope that a panic can leave open is not one.
    /// Left open, every later layer-shell request holds its commit for a group
    /// that ended, and the surface stops answering its handle in silence.
    #[test]
    fn a_panic_inside_a_batch_still_closes_it() {
        let escaped = std::panic::catch_unwind(|| {
            batching(surface(), || panic!("a request went wrong"));
        });
        assert!(escaped.is_err(), "the panic still propagates");
        assert!(
            BATCHING.with(|b| b.get()).is_none(),
            "and the scope closed on its way out"
        );
    }

    /// Only the outermost group commits, or a nested batch would commit halfway
    /// through the one containing it.
    #[test]
    fn only_the_outermost_batch_reports_itself() {
        let a = surface();
        let (outermost, inner) = batching(a, || batching(a, || ()).0);
        assert!(outermost, "the outer one opened the scope");
        assert!(!inner, "the inner one found it already open");
        assert!(BATCHING.with(|b| b.get()).is_none());
    }

    /// The scope belongs to a surface, not to the thread. A request aimed at
    /// another surface inside an open group has nobody to ride with: suppressing
    /// its commit leaves it double-buffered until something unrelated commits
    /// that surface, which for an idle one is never.
    #[test]
    fn a_batch_on_one_surface_does_not_hold_another_one_s_commit() {
        let (a, b) = (surface(), surface());
        let mut seen = None;
        let (outer, inner) = batching(a, || {
            seen = Some(BATCHING.with(|s| s.get()));
            batching(b, || ()).0
        });
        assert!(outer, "the group on a commits for a");
        assert!(inner, "and the one on b commits for b, rather than nothing");
        assert_eq!(seen, Some(Some(a)), "inside, the open group is a's");
        assert!(BATCHING.with(|s| s.get()).is_none());
    }
}
