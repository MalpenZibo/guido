use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    data_device_manager::DataDeviceManagerState,
    delegate_compositor, delegate_layer, delegate_registry, delegate_session_lock,
    delegate_xdg_popup,
    output::OutputState,
    primary_selection::PrimarySelectionManagerState,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{SeatState, pointer::cursor_shape::CursorShapeManager},
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{
            XdgPositioner, XdgShell,
            popup::{Popup, PopupConfigure, PopupHandler},
        },
    },
};
use wayland_backend::sys::client::ObjectId;
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{
        self, Capability as BgCapability, ExtBackgroundEffectManagerV1,
    },
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

use std::collections::HashMap;

use super::input::InputState;
use super::outputs::OutputRegistry;
use super::selections::Selections;
use crate::blur::BlurRect;
use crate::outputs::{self, OutputId};
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
    bg_effect_surface: Option<ExtBackgroundEffectSurfaceV1>,
    /// Last blur rects pushed to the compositor. `None` means nothing has
    /// been pushed yet (also reset when the blur capability changes, so the
    /// region is re-sent if it comes back).
    blur_region: Option<Vec<BlurRect>>,
    /// Height sent in an in-flight popup reposition (cleared by the popup
    /// configure), so repeated content measurements don't re-send it.
    pending_popup_height: Option<u32>,
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
            pending_popup_height: None,
        }
    }

    /// Take all pending events (drains the queue)
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
    }
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

    // Background effect (blur)
    /// `None` where the protocol is unsupported — the feature simply no-ops.
    bg_effect_manager: Option<ExtBackgroundEffectManagerV1>,
    /// Whether the compositor currently advertises the Blur capability.
    bg_effect_supports_blur: bool,

    // xdg shell (popups anchored to layer surfaces)
    /// `None` where xdg_wm_base is unavailable — popups then fail with a log.
    xdg_shell: Option<XdgShell>,
    /// Monotonic token for xdg_popup.reposition requests.
    reposition_token: u32,

    // Session lock (ext-session-lock-v1)
    session_lock_state: SessionLockState,
    /// The active lock grant. Written when `start_session_lock` succeeds so a
    /// synchronous double-lock check can reject a second call; cleared by
    /// `finished` or `unlock_session`.
    active_lock: Option<SessionLock>,
    /// Lock lifecycle events, drained by the main loop.
    lock_events: Vec<LockEvent>,

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
        bg_effect_manager,
        bg_effect_supports_blur: false,
        xdg_shell,
        reposition_token: 0,
        session_lock_state,
        active_lock: None,
        lock_events: Vec::new(),
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

    /// Request a session lock from the compositor.
    ///
    /// Returns false when a lock is already active or the compositor lacks
    /// ext-session-lock-v1. The grant (or denial) arrives asynchronously as
    /// a [`LockEvent`].
    pub fn start_session_lock(&mut self, qh: &QueueHandle<Self>) -> bool {
        if self.active_lock.is_some() {
            log::warn!("Session lock requested while a lock is already active");
            return false;
        }
        match self.session_lock_state.lock(qh) {
            Ok(lock) => {
                self.active_lock = Some(lock);
                log::info!("Session lock requested");
                true
            }
            Err(e) => {
                log::error!("Compositor does not support ext-session-lock-v1: {e}");
                false
            }
        }
    }

    /// Unlock the session (no-op when not locked).
    pub fn unlock_session(&mut self) {
        if let Some(lock) = self.active_lock.take() {
            lock.unlock();
            log::info!("Session unlocked");
        }
    }

    /// Whether a session lock is currently held and granted.
    pub fn is_session_locked(&self) -> bool {
        self.active_lock.as_ref().is_some_and(|l| l.is_locked())
    }

    /// Create a lock surface for `output` with a specific SurfaceId.
    ///
    /// The surface starts at 0×0 — its real size arrives with the first
    /// lock-surface configure. Returns false without an active lock or when
    /// the output disconnected.
    pub fn create_lock_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        output: OutputId,
    ) -> bool {
        let Some(lock) = self.active_lock.clone() else {
            log::warn!("Cannot create lock surface without an active session lock");
            return false;
        };
        let Some(wl_output) = self.wl_output_for(output) else {
            log::warn!(
                "Cannot create lock surface: output {:?} is not connected",
                output
            );
            return false;
        };

        let wl_surface = self.compositor_state.create_surface(qh);
        let lock_surface = lock.create_lock_surface(wl_surface.clone(), &wl_output, qh);

        self.surface_lookup.insert(wl_surface.id(), id);
        let surface_state =
            WaylandSurfaceState::new(SurfaceRole::Lock(lock_surface), wl_surface, 0, 0);
        self.surfaces.insert(id, surface_state);

        log::info!("Created lock surface {:?} on output {:?}", id, output);
        true
    }

    /// Drain pending session-lock lifecycle events.
    pub fn take_lock_events(&mut self) -> Vec<LockEvent> {
        std::mem::take(&mut self.lock_events)
    }

    /// Build an xdg_positioner for a popup config at the given size.
    fn build_popup_positioner(
        xdg_shell: &XdgShell,
        config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> Option<XdgPositioner> {
        let positioner = match XdgPositioner::new(xdg_shell) {
            Ok(p) => p,
            Err(e) => {
                log::error!("Failed to create xdg_positioner: {e}");
                return None;
            }
        };
        positioner.set_size(size.0.max(1) as i32, size.1.max(1) as i32);
        let r = config.anchor_rect;
        positioner.set_anchor_rect(
            r.x.floor() as i32,
            r.y.floor() as i32,
            (r.width.ceil() as i32).max(1),
            (r.height.ceil() as i32).max(1),
        );
        positioner.set_anchor(popup_point_to_anchor(config.anchor));
        positioner.set_gravity(popup_point_to_gravity(config.gravity));
        positioner.set_offset(config.offset.0, config.offset.1);
        // Let the compositor keep the popup on screen: flip at the screen
        // edge on either axis, slide where flipping doesn't help.
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::FlipX
                | xdg_positioner::ConstraintAdjustment::FlipY
                | xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::SlideY,
        );
        Some(positioner)
    }

    /// Create an xdg popup anchored to `parent` with a specific SurfaceId.
    ///
    /// `size` is the resolved popup size (config height, or the measured
    /// content height for auto-height popups). The actual size/position
    /// arrive with the popup configure (the compositor may flip/slide it to
    /// stay on screen). Returns false when xdg_wm_base is missing or the
    /// parent can't host popups.
    pub fn create_popup_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        parent: SurfaceId,
        config: &crate::surface::PopupConfig,
        size: (u32, u32),
    ) -> bool {
        let Some(ref xdg_shell) = self.xdg_shell else {
            log::error!("Cannot create popup: compositor lacks xdg_wm_base");
            return false;
        };
        let Some(parent_state) = self.surfaces.get(&parent) else {
            log::warn!("Cannot create popup: parent surface {:?} is gone", parent);
            return false;
        };

        let Some(positioner) = Self::build_popup_positioner(xdg_shell, config, size) else {
            return false;
        };

        let wl_surface = self.compositor_state.create_surface(qh);
        let popup = match &parent_state.role {
            SurfaceRole::Layer(layer_surface) => {
                let popup =
                    match Popup::from_surface(None, &positioner, qh, wl_surface.clone(), xdg_shell)
                    {
                        Ok(popup) => popup,
                        Err(e) => {
                            log::error!("Failed to create xdg popup: {e}");
                            return false;
                        }
                    };
                // Assign the layer surface as the popup parent BEFORE the
                // initial commit (required for parentless xdg popups)
                layer_surface.get_popup(popup.xdg_popup());
                popup
            }
            SurfaceRole::Popup {
                popup: parent_popup,
                ..
            } => {
                // Nested popup (submenu): ordinary xdg parent
                let parent_xdg = parent_popup.xdg_surface().clone();
                match Popup::from_surface(
                    Some(&parent_xdg),
                    &positioner,
                    qh,
                    wl_surface.clone(),
                    xdg_shell,
                ) {
                    Ok(popup) => popup,
                    Err(e) => {
                        log::error!("Failed to create nested xdg popup: {e}");
                        return false;
                    }
                }
            }
            SurfaceRole::Lock(_) => {
                log::warn!("Cannot anchor a popup to a session-lock surface");
                return false;
            }
        };

        // Menu semantics: an input grab dismisses the popup on outside
        // click. Must be requested before mapping, with a recent serial.
        if config.grab
            && let Some(seat) = self.seat_state.seats().next()
        {
            popup
                .xdg_popup()
                .grab(&seat, self.input.latest_input_serial);
        }

        wl_surface.commit();

        self.surface_lookup.insert(wl_surface.id(), id);
        let surface_state = WaylandSurfaceState::new(
            SurfaceRole::Popup {
                popup,
                config: config.clone(),
                parent,
            },
            wl_surface,
            size.0,
            size.1,
        );
        self.surfaces.insert(id, surface_state);

        log::info!(
            "Created popup {:?} on parent {:?} ({}x{}, grab: {})",
            id,
            parent,
            size.0,
            size.1,
            config.grab
        );
        true
    }

    /// Live popup descendants of `root`, deepest first — the order the
    /// protocol demands for teardown (a popup must be destroyed before its
    /// parent, or the compositor raises `not_the_topmost_popup`).
    pub(crate) fn popup_descendants_bottom_up(&self, root: SurfaceId) -> Vec<SurfaceId> {
        let mut out = Vec::new();
        let mut frontier = vec![root];
        while let Some(current) = frontier.pop() {
            for (id, state) in &self.surfaces {
                if let SurfaceRole::Popup { parent, .. } = &state.role
                    && *parent == current
                {
                    out.push(*id);
                    frontier.push(*id);
                }
            }
        }
        out.reverse(); // deepest first
        out
    }

    /// Grabbing popups that would make a new grab on `new_parent` illegal:
    /// xdg-shell requires a new grab to nest under the current grab holder,
    /// so any live grabbing popup that is not `new_parent` itself (or one
    /// of its ancestors) must be destroyed before the new popup is created.
    /// Returned deepest-chain-first, ready for ordered teardown.
    pub(crate) fn conflicting_grab_popups(&self, new_parent: SurfaceId) -> Vec<SurfaceId> {
        // Ancestor chain of the new popup (surfaces a nested grab may sit on)
        let mut ancestors = vec![new_parent];
        let mut current = new_parent;
        while let Some(state) = self.surfaces.get(&current) {
            match &state.role {
                SurfaceRole::Popup { parent, .. } => {
                    ancestors.push(*parent);
                    current = *parent;
                }
                _ => break,
            }
        }

        let mut conflicts: Vec<SurfaceId> = self
            .surfaces
            .iter()
            .filter(|(id, state)| {
                matches!(&state.role, SurfaceRole::Popup { config, .. } if config.grab)
                    && !ancestors.contains(id)
            })
            .map(|(id, _)| *id)
            .collect();
        // Close whole chains children-first: append descendants and dedup
        let mut ordered = Vec::new();
        for id in conflicts.drain(..) {
            for d in self.popup_descendants_bottom_up(id) {
                if !ordered.contains(&d) {
                    ordered.push(d);
                }
            }
            if !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        ordered
    }

    /// For auto-height popups: the width to measure content at.
    /// Returns `None` for non-popup surfaces or fixed-height popups.
    pub(crate) fn popup_auto_width(&self, id: SurfaceId) -> Option<u32> {
        match &self.surfaces.get(&id)?.role {
            SurfaceRole::Popup { config, .. } if config.height.is_none() => Some(config.width),
            _ => None,
        }
    }

    /// Reposition an auto-height popup when its content height changed.
    /// The compositor answers with a new configure carrying the final size.
    pub(crate) fn reposition_popup_if_changed(
        &mut self,
        id: SurfaceId,
        new_height: u32,
        qh: &QueueHandle<Self>,
    ) {
        let _ = qh;
        let Some(ref xdg_shell) = self.xdg_shell else {
            return;
        };
        let Some(surface_state) = self.surfaces.get_mut(&id) else {
            return;
        };
        if surface_state.height == new_height
            || surface_state.pending_popup_height == Some(new_height)
        {
            return;
        }
        let SurfaceRole::Popup { popup, config, .. } = &surface_state.role else {
            return;
        };
        // xdg_popup.reposition needs protocol v3
        if popup.xdg_popup().version() < 3 {
            return;
        }
        let Some(positioner) =
            Self::build_popup_positioner(xdg_shell, config, (config.width, new_height))
        else {
            return;
        };
        self.reposition_token = self.reposition_token.wrapping_add(1);
        popup.reposition(&positioner, self.reposition_token);
        surface_state.pending_popup_height = Some(new_height);
        log::debug!("Popup {:?} repositioning to height {}", id, new_height);
    }

    /// Push a surface's blur region to the compositor if it changed.
    ///
    /// The `set_blur_region` request is double-buffered: with `commit: false`
    /// it rides the buffer commit performed inside the upcoming present, so
    /// region and content change in the same frame. Pass `commit: true` on
    /// paths that skip presenting (e.g. a capability change without repaint).
    ///
    /// Surfaces that never requested blur are left untouched — declaring an
    /// empty region would override compositor-side blur rules (e.g. blur by
    /// namespace). Once a surface has blurred, dropping to zero rects sends
    /// an *empty* region, never NULL: NULL only withdraws our opinion and
    /// lets such a rule blur the whole surface, where an empty region says
    /// "blur exactly nothing".
    pub(crate) fn sync_blur_region(
        &mut self,
        id: SurfaceId,
        rects: Vec<BlurRect>,
        qh: &QueueHandle<Self>,
        commit: bool,
    ) {
        if !self.bg_effect_supports_blur {
            return;
        }
        let Some(manager) = self.bg_effect_manager.clone() else {
            return;
        };
        let Some(surface_state) = self.surfaces.get_mut(&id) else {
            return;
        };

        // Never used blur and still doesn't — don't claim the surface.
        if rects.is_empty()
            && surface_state.blur_region.is_none()
            && surface_state.bg_effect_surface.is_none()
        {
            return;
        }

        if surface_state.blur_region.as_deref() == Some(rects.as_slice()) {
            return;
        }

        // Asking the manager twice for the same surface is a protocol error.
        let effect = surface_state.bg_effect_surface.get_or_insert_with(|| {
            manager.get_background_effect(&surface_state.wl_surface, qh, ())
        });

        let Ok(region) = Region::new(&self.compositor_state) else {
            log::warn!("Failed to create wl_region for blur");
            return;
        };
        for r in &rects {
            region.add(r.x, r.y, r.width, r.height);
        }
        effect.set_blur_region(Some(region.wl_region()));

        log::debug!(
            "Surface {:?} blur region set to {} rect(s)",
            id,
            rects.len()
        );
        surface_state.blur_region = Some(rects);

        if commit {
            surface_state.wl_surface.commit();
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
        let stretch_w =
            config.anchor.contains(Anchor::LEFT) && config.anchor.contains(Anchor::RIGHT);
        let stretch_h =
            config.anchor.contains(Anchor::TOP) && config.anchor.contains(Anchor::BOTTOM);
        if (stretch_w && config.width.is_content()) || (stretch_h && config.height.is_content()) {
            log::warn!(
                "Surface {id:?}: content() sizing on an axis anchored to both                  screen edges is compositor-owned and will be ignored"
            );
        }
        let initial_width = config.width.initial();
        let initial_height = config.height.initial();
        let use_width = if stretch_w { 0 } else { initial_width };
        let use_height = if stretch_h { 0 } else { initial_height };

        layer_surface.set_size(use_width, use_height);
        layer_surface.set_keyboard_interactivity(config.keyboard_interactivity);

        let zone = config.exclusive_zone.resolve(
            config.anchor,
            config.margin,
            initial_width,
            initial_height,
        );
        layer_surface.set_exclusive_zone(zone);

        let (top, right, bottom, left) = config.margin;
        if (top, right, bottom, left) != (0, 0, 0, 0) {
            layer_surface.set_margin(top, right, bottom, left);
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

    /// Helper to modify a surface's layer shell properties and commit.
    /// No-ops (with a warning) on session-lock surfaces, which have none.
    fn with_layer_surface<F>(&mut self, id: SurfaceId, f: F)
    where
        F: FnOnce(&LayerSurface),
    {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            match &surface_state.role {
                SurfaceRole::Layer(layer_surface) => {
                    f(layer_surface);
                    surface_state.wl_surface.commit();
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
    pub fn set_surface_margin(
        &mut self,
        id: SurfaceId,
        top: i32,
        right: i32,
        bottom: i32,
        left: i32,
    ) {
        self.with_layer_surface(id, |ls| ls.set_margin(top, right, bottom, left));
        log::info!(
            "Surface {:?} margin set to top={}, right={}, bottom={}, left={}",
            id,
            top,
            right,
            bottom,
            left
        );
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
            crate::jobs::request_frame();
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

/// Map guido's popup anchor point to the xdg_positioner anchor.
fn popup_point_to_anchor(point: crate::surface::PopupAnchor) -> xdg_positioner::Anchor {
    use crate::surface::PopupAnchor as P;
    match point {
        P::None => xdg_positioner::Anchor::None,
        P::Top => xdg_positioner::Anchor::Top,
        P::Bottom => xdg_positioner::Anchor::Bottom,
        P::Left => xdg_positioner::Anchor::Left,
        P::Right => xdg_positioner::Anchor::Right,
        P::TopLeft => xdg_positioner::Anchor::TopLeft,
        P::BottomLeft => xdg_positioner::Anchor::BottomLeft,
        P::TopRight => xdg_positioner::Anchor::TopRight,
        P::BottomRight => xdg_positioner::Anchor::BottomRight,
    }
}

/// Map guido's popup gravity to the xdg_positioner gravity.
fn popup_point_to_gravity(point: crate::surface::PopupAnchor) -> xdg_positioner::Gravity {
    use crate::surface::PopupAnchor as P;
    match point {
        P::None => xdg_positioner::Gravity::None,
        P::Top => xdg_positioner::Gravity::Top,
        P::Bottom => xdg_positioner::Gravity::Bottom,
        P::Left => xdg_positioner::Gravity::Left,
        P::Right => xdg_positioner::Gravity::Right,
        P::TopLeft => xdg_positioner::Gravity::TopLeft,
        P::BottomLeft => xdg_positioner::Gravity::BottomLeft,
        P::TopRight => xdg_positioner::Gravity::TopRight,
        P::BottomRight => xdg_positioner::Gravity::BottomRight,
    }
}

impl SessionLockHandler for WaylandState {
    fn locked(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _session_lock: SessionLock) {
        log::info!("Session lock granted by compositor");
        self.lock_events.push(LockEvent::Locked);
        crate::jobs::request_frame();
    }

    fn finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _session_lock: SessionLock,
    ) {
        log::info!("Session lock finished (denied or ended)");
        self.active_lock = None;
        self.lock_events.push(LockEvent::Finished);
        crate::jobs::request_frame();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        // Route to the matching surface state, mirroring the layer-shell
        // configure path so the same render machinery applies. sctk has
        // already acked the configure.
        let surface_id = self.surface_lookup.get(&surface.wl_surface().id()).copied();
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            let (width, height) = configure.new_size;
            log::info!(
                "Lock surface {:?} configure: {}x{} (current {}x{})",
                id,
                width,
                height,
                surface_state.width,
                surface_state.height
            );
            if width > 0 {
                surface_state.width = width;
            }
            if height > 0 {
                surface_state.height = height;
            }
            surface_state.configured = true;
            crate::jobs::request_frame();
        }
    }
}

delegate_session_lock!(WaylandState);

impl PopupHandler for WaylandState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        popup: &Popup,
        config: PopupConfigure,
    ) {
        // Route to the matching surface state, like the layer-shell path.
        // sctk has already acked; the compositor may have adjusted size and
        // position to keep the popup on screen.
        let surface_id = self.surface_lookup.get(&popup.wl_surface().id()).copied();
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            log::info!(
                "Popup {:?} configure: {}x{} at {:?}",
                id,
                config.width,
                config.height,
                config.position
            );
            if config.width > 0 {
                surface_state.width = config.width as u32;
            }
            if config.height > 0 {
                surface_state.height = config.height as u32;
            }
            surface_state.pending_popup_height = None;
            surface_state.configured = true;
            crate::jobs::request_frame();
        }
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, popup: &Popup) {
        // Compositor dismissed the popup (outside click on a grab, parent
        // gone). Flip the reactive dismissal signal and route through the
        // normal close command for full teardown.
        if let Some(id) = self.surface_lookup.get(&popup.wl_surface().id()).copied() {
            log::info!("Popup {:?} dismissed by compositor", id);
            crate::surface::mark_popup_dismissed(id);
            crate::surface::push_surface_command(crate::surface::SurfaceCommand::Close(id));
        }
    }
}

// Not delegate_xdg_shell!: that macro (and sctk's decoration dispatches)
// drag in WindowHandler bounds — guido has no toplevel windows. Popups need
// only xdg_wm_base (ping/pong), plus an inert stub for the decoration
// manager that XdgShell::bind insists on binding (the protocol object has
// no events).
smithay_client_toolkit::reexports::client::delegate_dispatch!(WaylandState: [
    smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_wm_base::XdgWmBase: smithay_client_toolkit::globals::GlobalData
] => XdgShell);

impl
    Dispatch<
        smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
        smithay_client_toolkit::globals::GlobalData,
    > for WaylandState
{
    fn event(
        _: &mut Self,
        _: &smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::ZxdgDecorationManagerV1,
        _: smithay_client_toolkit::reexports::protocols::xdg::decoration::zv1::client::zxdg_decoration_manager_v1::Event,
        _: &smithay_client_toolkit::globals::GlobalData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        unreachable!("zxdg_decoration_manager_v1 has no events");
    }
}

delegate_xdg_popup!(WaylandState);

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            let blur = match flags {
                WEnum::Value(c) => c.contains(BgCapability::Blur),
                WEnum::Unknown(_) => false,
            };
            if blur == state.bg_effect_supports_blur {
                return;
            }
            log::info!(
                "Compositor blur capability {}",
                if blur { "available" } else { "lost" }
            );
            state.bg_effect_supports_blur = blur;
            crate::compositor::set_blur_capability(blur);

            // The compositor drops its regions when the capability goes away:
            // forget ours and wake the loop to push them again if it's back.
            for surface_state in state.surfaces.values_mut() {
                surface_state.blur_region = None;
            }
            crate::jobs::request_frame();
        }
    }
}

delegate_noop!(WaylandState: ignore ExtBackgroundEffectSurfaceV1);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(WaylandState);
delegate_layer!(WaylandState);
delegate_registry!(WaylandState);
