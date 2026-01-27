use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
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
    },
    shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    },
};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{
        wl_data_device::WlDataDevice, wl_data_device_manager::DndAction,
        wl_data_source::WlDataSource, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface,
    },
    Connection, EventQueue, Proxy, QueueHandle,
};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape as WpCursorShape;
use wayland_backend::sys::client::ObjectId;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::io::OwnedFd;

use crate::reactive::CursorIcon;
use crate::surface::SurfaceId;
use crate::widgets::{Event, Key, Modifiers, MouseButton, ScrollSource};

/// Pixels per line for discrete scroll (mouse wheel)
const SCROLL_PIXELS_PER_LINE: f32 = 40.0;

/// Per-surface state for multi-surface support.
pub struct WaylandSurfaceState {
    /// The layer surface protocol object
    pub layer_surface: LayerSurface,
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
    /// Pending events for this surface
    pub pending_events: Vec<Event>,
}

impl WaylandSurfaceState {
    /// Create a new surface state.
    pub fn new(
        layer_surface: LayerSurface,
        wl_surface: wl_surface::WlSurface,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            layer_surface,
            wl_surface,
            configured: false,
            width,
            height,
            scale_factor: 1.0,
            scale_factor_received: false,
            first_frame_presented: false,
            pending_events: Vec::new(),
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

    // Legacy single-surface fields (for backward compatibility)
    pub layer_surface: Option<LayerSurface>,
    pub surface: Option<wl_surface::WlSurface>,
    pub configured: bool,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub exit: bool,

    // Initialization tracking for event-driven startup
    pub scale_factor_received: bool,
    pub first_frame_presented: bool,

    // Multi-surface tracking
    /// All surfaces indexed by SurfaceId
    pub surfaces: HashMap<SurfaceId, WaylandSurfaceState>,
    /// Lookup from wl_surface ObjectId to SurfaceId
    pub surface_lookup: HashMap<ObjectId, SurfaceId>,
    /// Which surface currently has pointer focus
    pub current_pointer_surface: Option<SurfaceId>,
    /// Which surface currently has keyboard focus
    pub current_keyboard_surface: Option<SurfaceId>,

    // Pointer state
    pointer: Option<wl_pointer::WlPointer>,
    pointer_x: f32,
    pointer_y: f32,
    pointer_over_surface: bool,
    pointer_enter_serial: u32,

    // Cursor shape
    cursor_shape_manager: Option<CursorShapeManager>,

    // Keyboard state
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,
    keyboard_serial: u32,

    // Clipboard state
    data_device_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    clipboard_content: Option<String>,
    pending_clipboard_read: Option<ReadPipe>,
    clipboard_source: Option<CopyPasteSource>,
    selection_offer: Option<SelectionOffer>,

    // Pending events to be processed by the main loop (legacy single-surface)
    pub pending_events: Vec<Event>,
}

pub fn create_wayland_app() -> (
    Connection,
    EventQueue<WaylandState>,
    WaylandState,
    QueueHandle<WaylandState>,
) {
    let connection = Connection::connect_to_env().expect("Failed to connect to Wayland");
    let (globals, event_queue) =
        registry_queue_init::<WaylandState>(&connection).expect("Failed to initialize registry");
    let qh = event_queue.handle();

    let compositor_state =
        CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("layer_shell not available");
    let output_state = OutputState::new(&globals, &qh);
    let seat_state = SeatState::new(&globals, &qh);

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

    let state = WaylandState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        output_state,
        seat_state,
        layer_shell,
        layer_surface: None,
        surface: None,
        configured: false,
        width: 0,
        height: 0,
        scale_factor: 1.0,
        exit: false,
        scale_factor_received: false,
        first_frame_presented: false,
        surfaces: HashMap::new(),
        surface_lookup: HashMap::new(),
        current_pointer_surface: None,
        current_keyboard_surface: None,
        pointer: None,
        pointer_x: 0.0,
        pointer_y: 0.0,
        pointer_over_surface: false,
        pointer_enter_serial: 0,
        cursor_shape_manager,
        keyboard: None,
        modifiers: Modifiers::default(),
        keyboard_serial: 0,
        data_device_manager,
        data_device: None,
        clipboard_content: None,
        pending_clipboard_read: None,
        clipboard_source: None,
        selection_offer: None,
        pending_events: Vec::new(),
    };

    (connection, event_queue, state, qh)
}

impl WaylandState {
    /// Create a layer surface (legacy single-surface API).
    pub fn create_layer_surface(
        &mut self,
        qh: &QueueHandle<Self>,
        width: u32,
        height: u32,
        anchor: Anchor,
        layer: Layer,
        namespace: &str,
    ) {
        let surface = self.compositor_state.create_surface(qh);
        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            surface.clone(),
            layer,
            Some(namespace.to_string()),
            None,
        );

        layer_surface.set_anchor(anchor);

        // When anchored to both edges on an axis, set that dimension to 0
        // to let the compositor stretch the surface to fill
        let use_width = if anchor.contains(Anchor::LEFT) && anchor.contains(Anchor::RIGHT) {
            0 // Let compositor decide
        } else {
            width
        };
        let use_height = if anchor.contains(Anchor::TOP) && anchor.contains(Anchor::BOTTOM) {
            0 // Let compositor decide
        } else {
            height
        };

        layer_surface.set_size(use_width, use_height);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer_surface.set_exclusive_zone(height as i32);

        surface.commit();

        self.surface = Some(surface);
        self.layer_surface = Some(layer_surface);
        self.width = width;
        self.height = height;
    }

    /// Create a layer surface with a specific SurfaceId (multi-surface API).
    #[allow(clippy::too_many_arguments)]
    pub fn create_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        width: u32,
        height: u32,
        anchor: Anchor,
        layer: Layer,
        namespace: &str,
        exclusive_zone: Option<i32>,
        keyboard_interactivity: KeyboardInteractivity,
    ) {
        let wl_surface = self.compositor_state.create_surface(qh);
        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            wl_surface.clone(),
            layer,
            Some(namespace.to_string()),
            None,
        );

        layer_surface.set_anchor(anchor);

        // When anchored to both edges on an axis, set that dimension to 0
        // to let the compositor stretch the surface to fill
        let use_width = if anchor.contains(Anchor::LEFT) && anchor.contains(Anchor::RIGHT) {
            0 // Let compositor decide
        } else {
            width
        };
        let use_height = if anchor.contains(Anchor::TOP) && anchor.contains(Anchor::BOTTOM) {
            0 // Let compositor decide
        } else {
            height
        };

        layer_surface.set_size(use_width, use_height);
        layer_surface.set_keyboard_interactivity(keyboard_interactivity);

        // Set exclusive zone: None means use height, Some(0) means no exclusive zone
        let zone = exclusive_zone.unwrap_or(height as i32);
        layer_surface.set_exclusive_zone(zone);

        wl_surface.commit();

        // Register in lookup table
        let object_id = wl_surface.id();
        self.surface_lookup.insert(object_id, id);

        // Create and store surface state
        let surface_state = WaylandSurfaceState::new(layer_surface, wl_surface, width, height);
        self.surfaces.insert(id, surface_state);

        log::info!(
            "Created surface {:?} with size {}x{}, anchor {:?}, layer {:?}, keyboard {:?}",
            id,
            width,
            height,
            anchor,
            layer,
            keyboard_interactivity
        );
    }

    /// Destroy a surface by its SurfaceId.
    pub fn destroy_surface(&mut self, id: SurfaceId) {
        if let Some(surface_state) = self.surfaces.remove(&id) {
            // Remove from lookup table
            let object_id = surface_state.wl_surface.id();
            self.surface_lookup.remove(&object_id);

            // Clear pointer/keyboard focus if this surface had it
            if self.current_pointer_surface == Some(id) {
                self.current_pointer_surface = None;
            }
            if self.current_keyboard_surface == Some(id) {
                self.current_keyboard_surface = None;
            }

            // The LayerSurface and WlSurface will be destroyed when dropped
            log::info!("Destroyed surface {:?}", id);
        }
    }

    /// Set the layer for a surface.
    pub fn set_surface_layer(&mut self, id: SurfaceId, layer: Layer) {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state.layer_surface.set_layer(layer);
            surface_state.wl_surface.commit();
            log::info!("Surface {:?} layer set to {:?}", id, layer);
        }
    }

    /// Set the keyboard interactivity for a surface.
    pub fn set_surface_keyboard_interactivity(
        &mut self,
        id: SurfaceId,
        mode: KeyboardInteractivity,
    ) {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state.layer_surface.set_keyboard_interactivity(mode);
            surface_state.wl_surface.commit();
            log::info!("Surface {:?} keyboard interactivity set to {:?}", id, mode);
        }
    }

    /// Set the anchor edges for a surface.
    pub fn set_surface_anchor(&mut self, id: SurfaceId, anchor: Anchor) {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state.layer_surface.set_anchor(anchor);
            surface_state.wl_surface.commit();
            log::info!("Surface {:?} anchor set to {:?}", id, anchor);
        }
    }

    /// Set the size of a surface.
    pub fn set_surface_size(&mut self, id: SurfaceId, width: u32, height: u32) {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state.layer_surface.set_size(width, height);
            surface_state.wl_surface.commit();
            log::info!("Surface {:?} size set to {}x{}", id, width, height);
        }
    }

    /// Set the exclusive zone for a surface.
    pub fn set_surface_exclusive_zone(&mut self, id: SurfaceId, zone: i32) {
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state.layer_surface.set_exclusive_zone(zone);
            surface_state.wl_surface.commit();
            log::info!("Surface {:?} exclusive zone set to {}", id, zone);
        }
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
        if let Some(surface_state) = self.surfaces.get_mut(&id) {
            surface_state
                .layer_surface
                .set_margin(top, right, bottom, left);
            surface_state.wl_surface.commit();
            log::info!(
                "Surface {:?} margin set to top={}, right={}, bottom={}, left={}",
                id,
                top,
                right,
                bottom,
                left
            );
        }
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

    /// Take all pending events (drains the queue)
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
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
        let surface_id = self.surface_lookup.get(&surface.id()).copied();

        if let Some(id) = surface_id {
            if let Some(surface_state) = self.surfaces.get_mut(&id) {
                log::info!("Surface {:?} scale factor changed to: {}", id, new_factor);
                surface_state.scale_factor = new_factor as f32;
                surface_state.scale_factor_received = true;
            }
        } else {
            // Legacy single-surface mode
            log::info!("Scale factor changed to: {}", new_factor);
            self.scale_factor = new_factor as f32;
            self.scale_factor_received = true;
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
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Find which surface this is for
        let surface_id = self.surface_lookup.get(&surface.id()).copied();

        if let Some(id) = surface_id {
            if let Some(surface_state) = self.surfaces.get_mut(&id)
                && !surface_state.first_frame_presented
            {
                log::info!(
                    "Surface {:?} first frame presented by compositor - initialization complete",
                    id
                );
                surface_state.first_frame_presented = true;
            }
        } else {
            // Legacy single-surface mode
            if !self.first_frame_presented {
                log::info!(
                    "First frame presented by compositor - initialization complete, switching to on-demand rendering"
                );
                self.first_frame_presented = true;
            }
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
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for WaylandState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, layer: &LayerSurface) {
        // Find which surface was closed
        let closed_id = self
            .surfaces
            .iter()
            .find(|(_, state)| &state.layer_surface == layer)
            .map(|(id, _)| *id);

        if let Some(id) = closed_id {
            log::info!("Surface {:?} closed by compositor", id);
            self.destroy_surface(id);

            // If no surfaces left, exit
            if self.surfaces.is_empty() {
                self.exit = true;
            }
        } else {
            // Legacy single-surface mode
            self.exit = true;
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
        // Find which surface this configure is for
        let surface_id = self
            .surfaces
            .iter()
            .find(|(_, state)| &state.layer_surface == layer)
            .map(|(id, _)| *id);

        if let Some(id) = surface_id {
            if let Some(surface_state) = self.surfaces.get_mut(&id) {
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
        } else {
            // Legacy single-surface mode
            log::info!(
                "Layer shell configure: requested size {:?}, current {}x{}",
                configure.new_size,
                self.width,
                self.height
            );
            if configure.new_size.0 > 0 {
                self.width = configure.new_size.0;
            }
            if configure.new_size.1 > 0 {
                self.height = configure.new_size.1;
            }
            log::info!("Layer shell using size: {}x{}", self.width, self.height);
            self.configured = true;
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
            let pointer = self
                .seat_state
                .get_pointer(qh, &seat)
                .expect("Failed to get pointer");
            self.pointer = Some(pointer);
        }

        // Handle keyboard capability
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            log::info!("Keyboard capability available, creating keyboard");
            let keyboard = self
                .seat_state
                .get_keyboard(qh, &seat, None)
                .expect("Failed to get keyboard");
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
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
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

            // Check if this event is for our legacy single surface (backward compatibility)
            let is_legacy_surface = self
                .surface
                .as_ref()
                .map(|s| s == &event.surface)
                .unwrap_or(false);

            // Determine which event queue to push to
            let target_events: Option<&mut Vec<Event>> = if let Some(id) = surface_id {
                // Multi-surface mode: push to the specific surface's event queue
                self.surfaces.get_mut(&id).map(|s| &mut s.pending_events)
            } else if is_legacy_surface {
                // Legacy single-surface mode: push to the global pending_events
                Some(&mut self.pending_events)
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
                        if let Some(id) = self.current_pointer_surface {
                            if let Some(surface_state) = self.surfaces.get_mut(&id) {
                                surface_state.pending_events.push(Event::MouseLeave);
                            }
                        } else if is_legacy_surface {
                            self.pending_events.push(Event::MouseLeave);
                        }

                        self.current_pointer_surface = None;
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_x = event.position.0 as f32;
                    self.pointer_y = event.position.1 as f32;
                    if let Some(events) = target_events {
                        events.push(Event::MouseMove {
                            x: self.pointer_x,
                            y: self.pointer_y,
                        });
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
        if let Some(id) = surface_id {
            if let Some(surface_state) = self.surfaces.get_mut(&id) {
                surface_state.pending_events.push(Event::FocusIn);
            }
        } else {
            self.pending_events.push(Event::FocusIn);
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
        if let Some(id) = surface_id {
            if let Some(surface_state) = self.surfaces.get_mut(&id) {
                surface_state.pending_events.push(Event::FocusOut);
            }
        } else {
            self.pending_events.push(Event::FocusOut);
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

        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref()) {
            let key_event = Event::KeyDown {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
                return;
            }
            self.pending_events.push(key_event);
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
        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref()) {
            let key_event = Event::KeyUp {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
                return;
            }
            self.pending_events.push(key_event);
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
        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref()) {
            let key_event = Event::KeyDown {
                key,
                modifiers: self.modifiers,
            };

            // Route to the surface with keyboard focus
            if let Some(id) = self.current_keyboard_surface
                && let Some(surface_state) = self.surfaces.get_mut(&id)
            {
                surface_state.pending_events.push(key_event);
                return;
            }
            self.pending_events.push(key_event);
        }
    }
}

/// Convert XKB keysym to our Key type
fn keysym_to_key(keysym: Keysym, utf8: Option<&str>) -> Option<Key> {
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

    // Fallback: convert keysym directly for printable ASCII characters
    // This is needed for KeyUp events where utf8 may be None
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

    None
}

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

        // Write clipboard content to the file descriptor
        if let Some(ref content) = self.clipboard_content {
            let owned_fd = OwnedFd::from(fd);
            let mut file = File::from(owned_fd);
            if let Err(e) = file.write_all(content.as_bytes()) {
                log::warn!("Failed to write clipboard content: {}", e);
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
delegate_keyboard!(WaylandState);
delegate_data_device!(WaylandState);
delegate_registry!(WaylandState);
