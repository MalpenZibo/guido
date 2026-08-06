use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    data_device_manager::{
        data_device::{DataDevice, DataDeviceHandler},
        data_offer::{DataOfferHandler, SelectionOffer},
        data_source::{CopyPasteSource, DataSourceHandler},
        DataDeviceManagerState, ReadPipe,
    },
    delegate_compositor, delegate_data_device, delegate_keyboard, delegate_layer, delegate_output,
    delegate_pointer, delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers as WlModifiers, RawModifiers},
        pointer::{
            cursor_shape::CursorShapeManager, PointerEvent, PointerEventKind, PointerHandler,
        },
        Capability, SeatHandler, SeatState,
        touch::TouchHandler,
    },
    delegate_session_lock, delegate_touch,
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
    },
};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface,
        wl_touch,
    },
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum, delegate_noop,
};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape as WpCursorShape;
use wayland_backend::sys::client::ObjectId;
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::{self, Capability as BgCapability, ExtBackgroundEffectManagerV1},
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::io::OwnedFd;

use crate::blur::BlurRect;
use crate::outputs::{self, OutputId, OutputInfo};
use crate::reactive::CursorIcon;
use crate::surface::SurfaceId;
use crate::widgets::{Event, Key, Modifiers, MouseButton, Rect, ScrollSource};

/// Pixels per line for discrete scroll (mouse wheel)
const SCROLL_PIXELS_PER_LINE: f32 = 40.0;

/// The shell role of a surface: an ordinary layer-shell surface or an
/// ext-session-lock-v1 lock surface. Both share the same widget tree, GPU
/// and input pipeline; only creation, configure, and shell requests differ.
pub enum SurfaceRole {
    Layer(LayerSurface),
    /// Kept alive here — dropping it destroys the protocol object.
    Lock(SessionLockSurface),
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

    // Output tracking
    /// Stable OutputId for each wl_output global. Ids are never reused: a
    /// reconnected monitor gets a fresh id.
    output_ids: HashMap<ObjectId, OutputId>,
    /// Next OutputId to allocate.
    next_output_id: u32,

    // Background effect (blur)
    /// `None` where the protocol is unsupported — the feature simply no-ops.
    bg_effect_manager: Option<ExtBackgroundEffectManagerV1>,
    /// Whether the compositor currently advertises the Blur capability.
    bg_effect_supports_blur: bool,

    // Session lock (ext-session-lock-v1)
    session_lock_state: SessionLockState,
    /// The active lock grant. Written when `start_session_lock` succeeds so a
    /// synchronous double-lock check can reject a second call; cleared by
    /// `finished` or `unlock_session`.
    active_lock: Option<SessionLock>,
    /// Lock lifecycle events, drained by the main loop.
    lock_events: Vec<LockEvent>,

    // Pointer state
    pointer: Option<wl_pointer::WlPointer>,
    pointer_x: f32,
    pointer_y: f32,
    pointer_over_surface: bool,
    pointer_enter_serial: u32,

    // Touch state
    touch: Option<wl_touch::WlTouch>,
    /// Fingers currently down: id → (surface, x, y).
    touch_fingers: HashMap<i32, (SurfaceId, f32, f32)>,
    /// The finger driving pointer emulation (the first one down). Widgets
    /// only understand pointer events, so the primary finger synthesizes
    /// MouseMove/MouseDown/MouseUp — a tap becomes a click.
    primary_finger: Option<i32>,

    // Cursor shape
    cursor_shape_manager: Option<CursorShapeManager>,

    // Keyboard state
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    keyboard_serial: u32,
    /// Track raw_code → Key for press/release matching (handles compose sequences)
    pressed_keys: HashMap<u32, Key>,

    // Clipboard state
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard_content: Option<String>,
    pending_clipboard_read: Option<ReadPipe>,
    clipboard_source: Option<CopyPasteSource>,
    selection_offer: Option<SelectionOffer>,
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
pub fn create_wayland_app() -> Result<
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

    // Initialize data device manager for clipboard support
    let data_device_manager = DataDeviceManagerState::bind(&globals, &qh).ok();
    if data_device_manager.is_none() {
        log::warn!("Data device manager not available - clipboard will not work");
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
        output_ids: HashMap::new(),
        next_output_id: 0,
        bg_effect_manager,
        bg_effect_supports_blur: false,
        session_lock_state,
        active_lock: None,
        lock_events: Vec::new(),
        pointer: None,
        pointer_x: 0.0,
        pointer_y: 0.0,
        pointer_over_surface: false,
        pointer_enter_serial: 0,
        touch: None,
        touch_fingers: HashMap::new(),
        primary_finger: None,
        cursor_shape_manager,
        keyboard: None,
        modifiers: Modifiers::default(),
        keyboard_serial: 0,
        pressed_keys: HashMap::new(),
        data_device_manager,
        data_device: None,
        clipboard_content: None,
        pending_clipboard_read: None,
        clipboard_source: None,
        selection_offer: None,
    };

    Ok((connection, event_queue, state, qh))
}

impl WaylandState {
    /// Get (or allocate) the stable OutputId for a wl_output.
    fn ensure_output_id(&mut self, output: &wl_output::WlOutput) -> OutputId {
        let object_id = output.id();
        if let Some(id) = self.output_ids.get(&object_id) {
            return *id;
        }
        let id = OutputId::from_raw(self.next_output_id);
        self.next_output_id += 1;
        self.output_ids.insert(object_id, id);
        id
    }

    /// Find the wl_output for a stable OutputId, if still connected.
    fn wl_output_for(&self, id: OutputId) -> Option<wl_output::WlOutput> {
        self.output_state
            .outputs()
            .find(|o| self.output_ids.get(&o.id()) == Some(&id))
    }

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

    /// Rebuild the reactive output list from current compositor state.
    fn sync_outputs(&mut self) {
        let wl_outputs: Vec<wl_output::WlOutput> = self.output_state.outputs().collect();
        let mut list: Vec<OutputInfo> = wl_outputs
            .iter()
            .filter_map(|o| {
                let id = self.ensure_output_id(o);
                let info = self.output_state.info(o)?;
                Some(OutputInfo {
                    id,
                    name: info.name,
                    description: info.description,
                    make: info.make,
                    model: info.model,
                    scale_factor: info.scale_factor,
                    logical_size: info.logical_size,
                    logical_position: info.logical_position,
                })
            })
            .collect();
        list.sort_by_key(|o| o.id);
        outputs::sync_outputs(list);
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

        // When anchored to both edges on an axis, set that dimension to 0
        // to let the compositor stretch the surface to fill
        let use_width =
            if config.anchor.contains(Anchor::LEFT) && config.anchor.contains(Anchor::RIGHT) {
                0 // Let compositor decide
            } else {
                config.width
            };
        let use_height =
            if config.anchor.contains(Anchor::TOP) && config.anchor.contains(Anchor::BOTTOM) {
                0 // Let compositor decide
            } else {
                config.height
            };

        layer_surface.set_size(use_width, use_height);
        layer_surface.set_keyboard_interactivity(config.keyboard_interactivity);

        // Set exclusive zone: None means use height, Some(0) means no exclusive zone
        let zone = config.exclusive_zone.unwrap_or(config.height as i32);
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
            config.width,
            config.height,
        );
        self.surfaces.insert(id, surface_state);

        log::info!(
            "Created surface {:?} with size {}x{}, anchor {:?}, layer {:?}, keyboard {:?}",
            id,
            config.width,
            config.height,
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
                SurfaceRole::Lock(_) => {
                    log::warn!(
                        "Ignoring layer-shell property change on lock surface {:?}",
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

    /// Set clipboard content (copy)
    pub fn set_clipboard(&mut self, text: String, qh: &QueueHandle<Self>) {
        if let Some(ref manager) = self.data_device_manager {
            // Create a data source for the clipboard
            let source = manager.create_copy_paste_source(
                qh,
                vec!["text/plain;charset=utf-8", "UTF8_STRING", "TEXT", "STRING"],
            );

            // Store the text to write when compositor requests it
            self.clipboard_content = Some(text);

            // Set selection using the keyboard serial
            if let Some(ref device) = self.data_device {
                source.set_selection(device, self.keyboard_serial);
                self.clipboard_source = Some(source);
            }
        }
    }

    /// Get clipboard content (paste)
    /// Returns the content if available, or None if clipboard is empty
    pub fn get_clipboard(&self) -> Option<String> {
        self.clipboard_content.clone()
    }

    /// Read clipboard content from external selection (from other applications)
    /// This reads from the Wayland selection offer if available
    pub fn read_external_clipboard(&mut self, connection: &Connection) -> Option<String> {
        let offer = self.selection_offer.take()?;

        // Try different mime types in order of preference
        let mime_types = [
            "text/plain;charset=utf-8",
            "UTF8_STRING",
            "text/plain",
            "TEXT",
            "STRING",
        ];

        for mime_type in mime_types {
            // Check if this mime type is offered
            if !offer.with_mime_types(|types| types.iter().any(|t| t == mime_type)) {
                continue;
            }

            // Try to receive data with this mime type
            match offer.receive(mime_type.to_string()) {
                Ok(pipe) => {
                    // Flush the connection to send the receive request to the compositor
                    // The compositor then notifies the source app to write data to the pipe
                    let _ = connection.flush();

                    // Convert to file for reading
                    let fd = OwnedFd::from(pipe);
                    let mut file = File::from(fd);

                    // Use poll() to wait for data with a timeout
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::AsRawFd;
                        let raw_fd = file.as_raw_fd();

                        let mut poll_fd = libc::pollfd {
                            fd: raw_fd,
                            events: libc::POLLIN,
                            revents: 0,
                        };

                        // Wait up to 500ms for data to be available
                        let ret = unsafe { libc::poll(&mut poll_fd, 1, 500) };

                        if ret > 0 && (poll_fd.revents & libc::POLLIN) != 0 {
                            let mut contents = String::new();
                            if file.read_to_string(&mut contents).is_ok() && !contents.is_empty() {
                                self.selection_offer = Some(offer);
                                return Some(contents);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Failed to receive clipboard data as {}: {:?}", mime_type, e);
                }
            }
        }

        // Store back the offer even if we couldn't read
        self.selection_offer = Some(offer);
        None
    }

    /// Check if there's pending clipboard data to read
    pub fn poll_clipboard(&mut self) -> Option<String> {
        if let Some(ref mut pipe) = self.pending_clipboard_read.take() {
            let mut contents = String::new();
            // Read with a small timeout - this is blocking but typically fast
            match pipe.as_fd().try_clone_to_owned() {
                Ok(fd) => {
                    let mut file = std::fs::File::from(fd);
                    if file.read_to_string(&mut contents).is_ok() && !contents.is_empty() {
                        return Some(contents);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to clone clipboard fd: {}", e);
                }
            }
        }
        None
    }

    /// Set the cursor shape
    pub fn set_cursor(&self, cursor: CursorIcon, qh: &QueueHandle<Self>) {
        let Some(ref manager) = self.cursor_shape_manager else {
            return;
        };
        let Some(ref pointer) = self.pointer else {
            return;
        };

        // Convert our CursorIcon to Wayland cursor shape
        let shape = match cursor {
            CursorIcon::Default => WpCursorShape::Default,
            CursorIcon::Text => WpCursorShape::Text,
            CursorIcon::Pointer => WpCursorShape::Pointer,
            CursorIcon::Crosshair => WpCursorShape::Crosshair,
            CursorIcon::Move => WpCursorShape::Move,
            CursorIcon::NotAllowed => WpCursorShape::NotAllowed,
            CursorIcon::Grab => WpCursorShape::Grab,
            CursorIcon::Grabbing => WpCursorShape::Grabbing,
            CursorIcon::ResizeNorth => WpCursorShape::NResize,
            CursorIcon::ResizeSouth => WpCursorShape::SResize,
            CursorIcon::ResizeEast => WpCursorShape::EResize,
            CursorIcon::ResizeWest => WpCursorShape::WResize,
            CursorIcon::ResizeNorthEast => WpCursorShape::NeResize,
            CursorIcon::ResizeNorthWest => WpCursorShape::NwResize,
            CursorIcon::ResizeSouthEast => WpCursorShape::SeResize,
            CursorIcon::ResizeSouthWest => WpCursorShape::SwResize,
            CursorIcon::ColResize => WpCursorShape::ColResize,
            CursorIcon::RowResize => WpCursorShape::RowResize,
            CursorIcon::Wait => WpCursorShape::Wait,
            CursorIcon::Progress => WpCursorShape::Progress,
        };

        // Get cursor shape device and set shape
        let device = manager.get_shape_device(pointer, qh);
        device.set_shape(self.pointer_enter_serial, shape);
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
            && let Some(output_id) = self.output_ids.get(&output.id()).copied()
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

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let id = self.ensure_output_id(&output);
        log::info!(
            "Output {:?} connected: {:?}",
            id,
            self.output_state.info(&output).and_then(|i| i.name)
        );
        self.sync_outputs();
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
        self.sync_outputs();
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if let Some(id) = self.output_ids.remove(&output.id()) {
            log::info!("Output {:?} disconnected", id);
            outputs::output_removed(id);
        }
        self.sync_outputs();
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

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        // Handle pointer capability
        if capability == Capability::Pointer && self.pointer.is_none() {
            log::info!("Pointer capability available, creating pointer");
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.pointer = Some(pointer),
                Err(e) => {
                    // A capability race at seat init is not fatal — the app
                    // just runs without pointer input until the seat updates
                    log::warn!("Failed to get pointer: {e}");
                    return;
                }
            }
        }

        // Handle touch capability
        if capability == Capability::Touch && self.touch.is_none() {
            log::info!("Touch capability available, creating touch");
            match self.seat_state.get_touch(qh, &seat) {
                Ok(touch) => self.touch = Some(touch),
                Err(e) => {
                    log::warn!("Failed to get touch: {e}");
                }
            }
        }

        // Handle keyboard capability
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            log::info!("Keyboard capability available, creating keyboard");
            let keyboard = match self.seat_state.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => keyboard,
                Err(e) => {
                    log::warn!("Failed to get keyboard: {e}");
                    return;
                }
            };
            self.keyboard = Some(keyboard);

            // Create data device for clipboard when we have a seat
            if self.data_device.is_none()
                && let Some(ref manager) = self.data_device_manager
            {
                log::info!("Creating data device for clipboard");
                let data_device = manager.get_data_device(qh, &seat);
                self.data_device = Some(data_device);
            }
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            log::info!("Pointer capability removed");
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            log::info!("Keyboard capability removed");
            if let Some(keyboard) = self.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Touch {
            log::info!("Touch capability removed");
            if let Some(touch) = self.touch.take() {
                touch.release();
            }
            self.touch_fingers.clear();
            self.primary_finger = None;
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl TouchHandler for WaylandState {
    fn down(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _serial: u32,
        _time: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        let Some(surface_id) = self.surface_lookup.get(&surface.id()).copied() else {
            return;
        };
        let (x, y) = (position.0 as f32, position.1 as f32);
        self.touch_fingers.insert(id, (surface_id, x, y));

        // The first finger down drives pointer emulation: move + press so
        // hover and pressed state layers respond, and a tap becomes a click.
        if self.primary_finger.is_none() {
            self.primary_finger = Some(id);
            if let Some(surface_state) = self.surfaces.get_mut(&surface_id) {
                surface_state.pending_events.push(Event::MouseMove { x, y });
                surface_state.pending_events.push(Event::MouseDown {
                    x,
                    y,
                    button: MouseButton::Left,
                });
            }
        }
    }

    fn up(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _serial: u32,
        _time: u32,
        id: i32,
    ) {
        let Some((surface_id, x, y)) = self.touch_fingers.remove(&id) else {
            return;
        };
        if self.primary_finger == Some(id) {
            self.primary_finger = None;
            if let Some(surface_state) = self.surfaces.get_mut(&surface_id) {
                surface_state.pending_events.push(Event::MouseUp {
                    x,
                    y,
                    button: MouseButton::Left,
                });
                // Unlike a real pointer, nothing hovers after lifting the
                // finger — clear hover state.
                surface_state.pending_events.push(Event::MouseLeave);
            }
        }
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _time: u32,
        id: i32,
        position: (f64, f64),
    ) {
        let Some(finger) = self.touch_fingers.get_mut(&id) else {
            return;
        };
        let (x, y) = (position.0 as f32, position.1 as f32);
        finger.1 = x;
        finger.2 = y;
        let surface_id = finger.0;

        if self.primary_finger == Some(id)
            && let Some(surface_state) = self.surfaces.get_mut(&surface_id)
        {
            // Coalesce runs of MouseMove like the pointer path does.
            let events = &mut surface_state.pending_events;
            if let Some(last @ Event::MouseMove { .. }) = events.last_mut() {
                *last = Event::MouseMove { x, y };
            } else {
                events.push(Event::MouseMove { x, y });
            }
        }
    }

    fn shape(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _id: i32,
        _major: f64,
        _minor: f64,
    ) {
    }

    fn orientation(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _touch: &wl_touch::WlTouch,
        _id: i32,
        _orientation: f64,
    ) {
    }

    fn cancel(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _touch: &wl_touch::WlTouch) {
        // The compositor took over the gesture: release the synthesized
        // press and clear hover so no widget is stuck pressed.
        if let Some(id) = self.primary_finger.take()
            && let Some((surface_id, x, y)) = self.touch_fingers.get(&id).copied()
            && let Some(surface_state) = self.surfaces.get_mut(&surface_id)
        {
            surface_state.pending_events.push(Event::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            });
            surface_state.pending_events.push(Event::MouseLeave);
        }
        self.touch_fingers.clear();
    }
}

impl PointerHandler for WaylandState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            // Try to find the surface ID for this event's wl_surface
            let surface_id = self.surface_lookup.get(&event.surface.id()).copied();

            // Get the target event queue for this surface
            let target_events: Option<&mut Vec<Event>> = if let Some(id) = surface_id {
                self.surfaces.get_mut(&id).map(|s| &mut s.pending_events)
            } else if !matches!(event.kind, PointerEventKind::Leave { .. }) {
                // Not our surface and not a leave event, skip
                continue;
            } else {
                None
            };

            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_over_surface = true;
                    self.pointer_enter_serial = serial;
                    self.pointer_x = event.position.0 as f32;
                    self.pointer_y = event.position.1 as f32;

                    // Track which surface has pointer focus
                    self.current_pointer_surface = surface_id;

                    if let Some(events) = target_events {
                        events.push(Event::MouseEnter {
                            x: self.pointer_x,
                            y: self.pointer_y,
                        });
                        events.push(Event::MouseMove {
                            x: self.pointer_x,
                            y: self.pointer_y,
                        });
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_over_surface {
                        self.pointer_over_surface = false;

                        // Send leave event to the surface that had focus
                        if let Some(id) = self.current_pointer_surface
                            && let Some(surface_state) = self.surfaces.get_mut(&id)
                        {
                            surface_state.pending_events.push(Event::MouseLeave);
                        }

                        self.current_pointer_surface = None;
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_x = event.position.0 as f32;
                    self.pointer_y = event.position.1 as f32;
                    if let Some(events) = target_events {
                        // Coalesce runs of MouseMove: only the latest position
                        // matters for hover state, and every queued move costs
                        // a full event-dispatch walk of the widget tree.
                        if let Some(last @ Event::MouseMove { .. }) = events.last_mut() {
                            *last = Event::MouseMove {
                                x: self.pointer_x,
                                y: self.pointer_y,
                            };
                        } else {
                            events.push(Event::MouseMove {
                                x: self.pointer_x,
                                y: self.pointer_y,
                            });
                        }
                    }
                }
                PointerEventKind::Press { button, .. } => {
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button)
                        && let Some(events) = target_events
                    {
                        events.push(Event::MouseDown {
                            x: self.pointer_x,
                            y: self.pointer_y,
                            button: mouse_button,
                        });
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button)
                        && let Some(events) = target_events
                    {
                        events.push(Event::MouseUp {
                            x: self.pointer_x,
                            y: self.pointer_y,
                            button: mouse_button,
                        });
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    source,
                    ..
                } => {
                    // Determine scroll source
                    let scroll_source = match source {
                        Some(wl_pointer::AxisSource::Wheel) => ScrollSource::Wheel,
                        Some(wl_pointer::AxisSource::Finger) => ScrollSource::Finger,
                        Some(wl_pointer::AxisSource::Continuous) => ScrollSource::Continuous,
                        Some(wl_pointer::AxisSource::WheelTilt) => ScrollSource::Wheel,
                        _ => ScrollSource::Wheel,
                    };

                    // Calculate delta in pixels
                    // For mouse wheel: use discrete * pixels_per_line, or fall back to absolute
                    // For touchpad/finger: use absolute (already in pixels)
                    let delta_x = if horizontal.discrete != 0 {
                        horizontal.discrete as f32 * SCROLL_PIXELS_PER_LINE
                    } else {
                        horizontal.absolute as f32
                    };

                    let delta_y = if vertical.discrete != 0 {
                        vertical.discrete as f32 * SCROLL_PIXELS_PER_LINE
                    } else {
                        vertical.absolute as f32
                    };

                    // Only emit scroll event if there's actual scroll delta
                    if (delta_x != 0.0 || delta_y != 0.0)
                        && let Some(events) = target_events
                    {
                        events.push(Event::Scroll {
                            x: self.pointer_x,
                            y: self.pointer_y,
                            delta_x,
                            delta_y,
                            source: scroll_source,
                        });
                    }
                }
            }
        }
    }
}

/// Convert Wayland button code to MouseButton
fn wayland_button_to_mouse_button(button: u32) -> Option<MouseButton> {
    // Linux input event codes (from linux/input-event-codes.h)
    const BTN_LEFT: u32 = 0x110;
    const BTN_RIGHT: u32 = 0x111;
    const BTN_MIDDLE: u32 = 0x112;

    match button {
        BTN_LEFT => Some(MouseButton::Left),
        BTN_RIGHT => Some(MouseButton::Right),
        BTN_MIDDLE => Some(MouseButton::Middle),
        _ => None,
    }
}

impl KeyboardHandler for WaylandState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        log::debug!("Keyboard focus entered");

        // Track which surface has keyboard focus
        let surface_id = self.surface_lookup.get(&surface.id()).copied();
        self.current_keyboard_surface = surface_id;

        // Route event to correct surface
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            surface_state.pending_events.push(Event::FocusIn);
        }
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        log::debug!("Keyboard focus left");

        // Route event to correct surface
        let surface_id = self.surface_lookup.get(&surface.id()).copied();
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            surface_state.pending_events.push(Event::FocusOut);
        }

        self.current_keyboard_surface = None;
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        // Track serial for clipboard operations
        self.keyboard_serial = serial;

        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref(), true) {
            // Store raw_code → Key mapping so release_key can emit the correct Key
            // (e.g., composed 'é' instead of raw 'e' after a compose sequence)
            self.pressed_keys.insert(event.raw_code, key);

            let key_event = Event::KeyDown {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
            }
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        // Use the stored key from press_key if available (handles compose sequences
        // where the composed character differs from the raw keysym on release)
        let key = self
            .pressed_keys
            .remove(&event.raw_code)
            .or_else(|| keysym_to_key(event.keysym, event.utf8.as_deref(), false));

        if let Some(key) = key {
            let key_event = Event::KeyUp {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
            }
        }
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: WlModifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
        self.modifiers = Modifiers {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            logo: modifiers.logo,
        };
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        // Treat key repeat as a new key press
        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref(), true) {
            let key_event = Event::KeyDown {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
            }
        }
    }
}

/// Convert XKB keysym to our Key type
fn keysym_to_key(keysym: Keysym, utf8: Option<&str>, is_press: bool) -> Option<Key> {
    // Named keys first
    match keysym {
        Keysym::BackSpace => return Some(Key::Backspace),
        Keysym::Delete => return Some(Key::Delete),
        Keysym::Return | Keysym::KP_Enter => return Some(Key::Enter),
        Keysym::Tab | Keysym::ISO_Left_Tab => return Some(Key::Tab),
        Keysym::Escape => return Some(Key::Escape),
        Keysym::Left => return Some(Key::Left),
        Keysym::Right => return Some(Key::Right),
        Keysym::Up => return Some(Key::Up),
        Keysym::Down => return Some(Key::Down),
        Keysym::Home => return Some(Key::Home),
        Keysym::End => return Some(Key::End),
        _ => {}
    }

    // Character input - use utf8 if available
    if let Some(text) = utf8
        && let Some(c) = text.chars().next()
    {
        // Only return printable characters or control characters we care about
        if !c.is_control() || c == '\n' || c == '\r' || c == '\t' {
            return Some(Key::Char(c));
        }
    }

    // Fallback: convert keysym directly for release events where utf8 is always None.
    // On press, utf8 = None means a compose sequence is in progress — don't insert anything.
    if !is_press {
        let raw = keysym.raw();

        // Printable ASCII range (space through tilde): 0x20-0x7E
        // XKB keysyms for these characters have the same value as ASCII
        if (0x20..=0x7e).contains(&raw) {
            return Some(Key::Char(char::from_u32(raw)?));
        }

        // Handle keypad numbers (KP_0 through KP_9)
        // XKB_KEY_KP_0 = 0xffb0, XKB_KEY_KP_9 = 0xffb9
        if (0xffb0..=0xffb9).contains(&raw) {
            return Some(Key::Char(char::from_u32(raw - 0xffb0 + 0x30)?)); // Convert to '0'-'9'
        }
    }

    None
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

impl DataDeviceHandler for WaylandState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
        _surface: &wl_surface::WlSurface,
    ) {
        // Drag and drop enter - not used for clipboard
    }

    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _data_device: &WlDataDevice) {
        // Drag and drop leave - not used for clipboard
    }

    fn motion(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
        _x: f64,
        _y: f64,
    ) {
        // Drag and drop motion - not used for clipboard
    }

    fn drop_performed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        // Drag and drop performed - not used for clipboard
    }

    fn selection(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _data_device: &WlDataDevice,
    ) {
        log::debug!("Clipboard selection changed");
        // Store the selection offer for later paste operations
        if let Some(ref device) = self.data_device {
            self.selection_offer = device.data().selection_offer();
        }
    }
}

impl DataOfferHandler for WaylandState {
    fn source_actions(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _actions: DndAction,
    ) {
        // Drag and drop actions - not used for clipboard
    }

    fn selected_action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _offer: &mut smithay_client_toolkit::data_device_manager::data_offer::DragOffer,
        _action: DndAction,
    ) {
        // Drag and drop selected action - not used for clipboard
    }
}

impl DataSourceHandler for WaylandState {
    fn accept_mime(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _mime: Option<String>,
    ) {
        // Mime type accepted notification
    }

    fn send_request(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        mime: String,
        fd: smithay_client_toolkit::data_device_manager::WritePipe,
    ) {
        log::debug!("Clipboard send request for mime type: {}", mime);

        // Write clipboard content on a short-lived thread: a payload larger
        // than the pipe buffer with a slow reader would otherwise block the
        // UI thread indefinitely inside write_all.
        if let Some(ref content) = self.clipboard_content {
            let content = content.clone();
            let owned_fd = OwnedFd::from(fd);
            if let Err(e) = std::thread::Builder::new()
                .name("guido-clipboard-send".into())
                .spawn(move || {
                    let mut file = File::from(owned_fd);
                    if let Err(e) = file.write_all(content.as_bytes()) {
                        log::warn!("Failed to write clipboard content: {}", e);
                    }
                })
            {
                log::warn!("Failed to spawn clipboard writer thread: {e}");
            }
        }
    }

    fn cancelled(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        log::debug!("Clipboard source cancelled");
        self.clipboard_source = None;
    }

    fn dnd_dropped(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _source: &WlDataSource) {
        // Drag and drop completed - not used for clipboard
    }

    fn dnd_finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
    ) {
        // Drag and drop finished - not used for clipboard
    }

    fn action(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _source: &WlDataSource,
        _action: DndAction,
    ) {
        // Action notification - not used for clipboard
    }
}

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_layer!(WaylandState);
delegate_seat!(WaylandState);
delegate_pointer!(WaylandState);
delegate_touch!(WaylandState);
delegate_keyboard!(WaylandState);
delegate_data_device!(WaylandState);
delegate_registry!(WaylandState);
