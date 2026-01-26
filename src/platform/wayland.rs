use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers as WlModifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    },
};
use wayland_backend::sys::client::ObjectId;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, EventQueue, Proxy, QueueHandle,
};

use crate::widgets::{Event, Key, Modifiers, MouseButton, ScrollSource};

/// Pixels per line for discrete scroll (mouse wheel)
const SCROLL_PIXELS_PER_LINE: f32 = 40.0;

pub struct WaylandState {
    pub registry_state: RegistryState,
    pub compositor_state: CompositorState,
    pub output_state: OutputState,
    pub seat_state: SeatState,
    pub layer_shell: LayerShell,
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

    // Pointer state
    pointer: Option<wl_pointer::WlPointer>,
    pointer_x: f32,
    pointer_y: f32,
    pointer_over_surface: bool,

    // Keyboard state
    keyboard: Option<wl_keyboard::WlKeyboard>,
    modifiers: Modifiers,

    // Pending events to be processed by the main loop
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
        pointer: None,
        pointer_x: 0.0,
        pointer_y: 0.0,
        pointer_over_surface: false,
        keyboard: None,
        modifiers: Modifiers::default(),
        pending_events: Vec::new(),
    };

    (connection, event_queue, state, qh)
}

impl WaylandState {
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

    /// Take all pending events (drains the queue)
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.pending_events)
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
        log::info!("Scale factor changed to: {}", new_factor);
        self.scale_factor = new_factor as f32;
        self.scale_factor_received = true;
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
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if !self.first_frame_presented {
            log::info!("First frame presented by compositor - initialization complete, switching to on-demand rendering");
            self.first_frame_presented = true;
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
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
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
            // Check if this event is for our surface
            let is_our_surface = self
                .surface
                .as_ref()
                .map(|s| s == &event.surface)
                .unwrap_or(false);

            if !is_our_surface && !matches!(event.kind, PointerEventKind::Leave { .. }) {
                continue;
            }

            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.pointer_over_surface = true;
                    self.pointer_x = event.position.0 as f32;
                    self.pointer_y = event.position.1 as f32;
                    self.pending_events.push(Event::MouseEnter {
                        x: self.pointer_x,
                        y: self.pointer_y,
                    });
                    self.pending_events.push(Event::MouseMove {
                        x: self.pointer_x,
                        y: self.pointer_y,
                    });
                }
                PointerEventKind::Leave { .. } => {
                    if self.pointer_over_surface {
                        self.pointer_over_surface = false;
                        self.pending_events.push(Event::MouseLeave);
                    }
                }
                PointerEventKind::Motion { .. } => {
                    self.pointer_x = event.position.0 as f32;
                    self.pointer_y = event.position.1 as f32;
                    self.pending_events.push(Event::MouseMove {
                        x: self.pointer_x,
                        y: self.pointer_y,
                    });
                }
                PointerEventKind::Press { button, .. } => {
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button) {
                        self.pending_events.push(Event::MouseDown {
                            x: self.pointer_x,
                            y: self.pointer_y,
                            button: mouse_button,
                        });
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button) {
                        self.pending_events.push(Event::MouseUp {
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
                        Some(wayland_client::protocol::wl_pointer::AxisSource::Wheel) => {
                            ScrollSource::Wheel
                        }
                        Some(wayland_client::protocol::wl_pointer::AxisSource::Finger) => {
                            ScrollSource::Finger
                        }
                        Some(wayland_client::protocol::wl_pointer::AxisSource::Continuous) => {
                            ScrollSource::Continuous
                        }
                        Some(wayland_client::protocol::wl_pointer::AxisSource::WheelTilt) => {
                            ScrollSource::Wheel
                        }
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
                    if delta_x != 0.0 || delta_y != 0.0 {
                        self.pending_events.push(Event::Scroll {
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
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        log::debug!("Keyboard focus entered");
        self.pending_events.push(Event::FocusIn);
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        log::debug!("Keyboard focus left");
        self.pending_events.push(Event::FocusOut);
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref()) {
            self.pending_events.push(Event::KeyDown {
                key,
                modifiers: self.modifiers,
            });
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
            self.pending_events.push(Event::KeyUp {
                key,
                modifiers: self.modifiers,
            });
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
            self.pending_events.push(Event::KeyDown {
                key,
                modifiers: self.modifiers,
            });
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

    // Character input - use utf8 if available, or convert keysym to char
    if let Some(text) = utf8 {
        if let Some(c) = text.chars().next() {
            // Only return printable characters or control characters we care about
            if !c.is_control() || c == '\n' || c == '\r' || c == '\t' {
                return Some(Key::Char(c));
            }
        }
    }

    // Fallback: convert keysym directly for A-Z (for Ctrl+A style shortcuts)
    let raw = keysym.raw();
    if (0x61..=0x7a).contains(&raw) {
        // lowercase a-z
        return Some(Key::Char(char::from_u32(raw)?));
    }
    if (0x41..=0x5a).contains(&raw) {
        // uppercase A-Z
        return Some(Key::Char(char::from_u32(raw + 32)?)); // convert to lowercase
    }

    None
}

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(WaylandState);
delegate_output!(WaylandState);
delegate_layer!(WaylandState);
delegate_seat!(WaylandState);
delegate_pointer!(WaylandState);
delegate_keyboard!(WaylandState);
delegate_registry!(WaylandState);
