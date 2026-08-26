//! Pointer, touch and keyboard: everything the seat hands us.
//!
//! The three devices arrive together with the seat's capabilities and are
//! released together when it loses them, which is why they are kept in one
//! place rather than one struct each.
//!
//! Widgets only understand pointer events, so touch is folded into the same
//! pipeline: the first finger down drives it, and a tap becomes a click.

use std::collections::HashMap;

use smithay_client_toolkit::{
    delegate_keyboard, delegate_pointer, delegate_seat, delegate_touch,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers as WlModifiers, RawModifiers},
        pointer::{
            AxisScroll, PointerEvent, PointerEventKind, PointerHandler,
            cursor_shape::CursorShapeManager,
        },
        touch::TouchHandler,
    },
};

use smithay_client_toolkit::reexports::calloop::LoopHandle;
use smithay_client_toolkit::reexports::client::{
    Connection, Proxy, QueueHandle,
    protocol::{wl_keyboard, wl_pointer, wl_seat, wl_surface, wl_touch},
};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape as WpCursorShape;

use super::wayland::WaylandState;
use crate::reactive::CursorIcon;
use crate::surface::SurfaceId;
use crate::widgets::{Event, Key, Modifiers, MouseButton, ScrollSource};

/// Pixels per line for discrete scroll (mouse wheel)
const SCROLL_PIXELS_PER_LINE: f32 = 40.0;

/// The seat's three devices and the state that tracks them.
pub struct InputState {
    // Pointer
    pub(super) pointer: Option<wl_pointer::WlPointer>,
    pub(super) pointer_x: f32,
    pub(super) pointer_y: f32,
    pub(super) pointer_over_surface: bool,
    pub(super) pointer_enter_serial: u32,

    // Touch
    pub(super) touch: Option<wl_touch::WlTouch>,
    /// Fingers currently down: id → (surface, x, y).
    pub(super) touch_fingers: HashMap<i32, (SurfaceId, f32, f32)>,
    /// The finger driving pointer emulation (the first one down). Widgets
    /// only understand pointer events, so the primary finger synthesizes
    /// MouseMove/MouseDown/MouseUp — a tap becomes a click.
    pub(super) primary_finger: Option<i32>,

    // Cursor shape
    pub(super) cursor_shape_manager: Option<CursorShapeManager>,

    // Keyboard
    pub(super) keyboard: Option<wl_keyboard::WlKeyboard>,
    pub(super) modifiers: Modifiers,
    pub(super) keyboard_serial: u32,
    /// Track raw_code → Key for press/release matching (handles compose sequences)
    pub(super) pressed_keys: HashMap<u32, Key>,
    /// Key repeat runs on a calloop timer owned by the toolkit, armed with the
    /// rate and delay the compositor reports. The keyboard is created inside a
    /// seat capability callback, which has no other route to the loop.
    pub(super) loop_handle: LoopHandle<'static, WaylandState>,

    /// Serial of the most recent input event. Claiming a selection or taking a
    /// popup grab needs one the compositor still considers recent.
    pub(super) latest_input_serial: u32,
}

impl InputState {
    pub(super) fn new(
        cursor_shape_manager: Option<CursorShapeManager>,
        loop_handle: LoopHandle<'static, WaylandState>,
    ) -> Self {
        Self {
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
            loop_handle,
            latest_input_serial: 0,
        }
    }
}

impl WaylandState {
    /// Set the cursor shape
    pub fn set_cursor(&self, cursor: CursorIcon, qh: &QueueHandle<Self>) {
        let Some(ref manager) = self.input.cursor_shape_manager else {
            return;
        };
        let Some(ref pointer) = self.input.pointer else {
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
        device.set_shape(self.input.pointer_enter_serial, shape);
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
        if capability == Capability::Pointer && self.input.pointer.is_none() {
            log::info!("Pointer capability available, creating pointer");
            match self.seat_state.get_pointer(qh, &seat) {
                Ok(pointer) => self.input.pointer = Some(pointer),
                Err(e) => {
                    // A capability race at seat init is not fatal — the app
                    // just runs without pointer input until the seat updates
                    log::warn!("Failed to get pointer: {e}");
                    return;
                }
            }
        }

        // Handle touch capability
        if capability == Capability::Touch && self.input.touch.is_none() {
            log::info!("Touch capability available, creating touch");
            match self.seat_state.get_touch(qh, &seat) {
                Ok(touch) => self.input.touch = Some(touch),
                Err(e) => {
                    log::warn!("Failed to get touch: {e}");
                }
            }
        }

        // Handle keyboard capability
        if capability == Capability::Keyboard && self.input.keyboard.is_none() {
            log::info!("Keyboard capability available, creating keyboard");
            let loop_handle = self.input.loop_handle.clone();
            let keyboard = match self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                loop_handle,
                Box::new(|state: &mut WaylandState, _kbd, event| state.emit_key_repeat(event)),
            ) {
                Ok(keyboard) => keyboard,
                Err(e) => {
                    log::warn!("Failed to get keyboard: {e}");
                    return;
                }
            };
            self.input.keyboard = Some(keyboard);

            // Selections hang off the seat, so they cannot be created with
            // their managers at startup.
            self.selections.attach_devices(qh, &seat);
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
            if let Some(pointer) = self.input.pointer.take() {
                pointer.release();
            }
        }
        if capability == Capability::Keyboard {
            log::info!("Keyboard capability removed");
            if let Some(keyboard) = self.input.keyboard.take() {
                keyboard.release();
            }
        }
        if capability == Capability::Touch {
            log::info!("Touch capability removed");
            if let Some(touch) = self.input.touch.take() {
                touch.release();
            }
            self.input.touch_fingers.clear();
            self.input.primary_finger = None;
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
        serial: u32,
        _time: u32,
        surface: wl_surface::WlSurface,
        id: i32,
        position: (f64, f64),
    ) {
        self.input.latest_input_serial = serial;
        let Some(surface_id) = self.surface_lookup.get(&surface.id()).copied() else {
            return;
        };
        let (x, y) = (position.0 as f32, position.1 as f32);
        self.input.touch_fingers.insert(id, (surface_id, x, y));

        // The first finger down drives pointer emulation: move + press so
        // hover and pressed state layers respond, and a tap becomes a click.
        if self.input.primary_finger.is_none() {
            self.input.primary_finger = Some(id);
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
        let Some((surface_id, x, y)) = self.input.touch_fingers.remove(&id) else {
            return;
        };
        if self.input.primary_finger == Some(id) {
            self.input.primary_finger = None;
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
        let Some(finger) = self.input.touch_fingers.get_mut(&id) else {
            return;
        };
        let (x, y) = (position.0 as f32, position.1 as f32);
        finger.1 = x;
        finger.2 = y;
        let surface_id = finger.0;

        if self.input.primary_finger == Some(id)
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
        if let Some(id) = self.input.primary_finger.take()
            && let Some((surface_id, x, y)) = self.input.touch_fingers.get(&id).copied()
            && let Some(surface_state) = self.surfaces.get_mut(&surface_id)
        {
            surface_state.pending_events.push(Event::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            });
            surface_state.pending_events.push(Event::MouseLeave);
        }
        self.input.touch_fingers.clear();
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
                    self.input.pointer_over_surface = true;
                    self.input.pointer_enter_serial = serial;
                    self.input.latest_input_serial = serial;
                    self.input.pointer_x = event.position.0 as f32;
                    self.input.pointer_y = event.position.1 as f32;

                    // Track which surface has pointer focus
                    self.current_pointer_surface = surface_id;

                    if let Some(events) = target_events {
                        events.push(Event::MouseEnter {
                            x: self.input.pointer_x,
                            y: self.input.pointer_y,
                        });
                        events.push(Event::MouseMove {
                            x: self.input.pointer_x,
                            y: self.input.pointer_y,
                        });
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if self.input.pointer_over_surface {
                        self.input.pointer_over_surface = false;

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
                    self.input.pointer_x = event.position.0 as f32;
                    self.input.pointer_y = event.position.1 as f32;
                    if let Some(events) = target_events {
                        // Coalesce runs of MouseMove: only the latest position
                        // matters for hover state, and every queued move costs
                        // a full event-dispatch walk of the widget tree.
                        if let Some(last @ Event::MouseMove { .. }) = events.last_mut() {
                            *last = Event::MouseMove {
                                x: self.input.pointer_x,
                                y: self.input.pointer_y,
                            };
                        } else {
                            events.push(Event::MouseMove {
                                x: self.input.pointer_x,
                                y: self.input.pointer_y,
                            });
                        }
                    }
                }
                PointerEventKind::Press { button, serial, .. } => {
                    self.input.latest_input_serial = serial;
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button)
                        && let Some(events) = target_events
                    {
                        events.push(Event::MouseDown {
                            x: self.input.pointer_x,
                            y: self.input.pointer_y,
                            button: mouse_button,
                        });
                    }
                }
                PointerEventKind::Release { button, .. } => {
                    if let Some(mouse_button) = wayland_button_to_mouse_button(button)
                        && let Some(events) = target_events
                    {
                        events.push(Event::MouseUp {
                            x: self.input.pointer_x,
                            y: self.input.pointer_y,
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
                    if let Some(events) = target_events {
                        translate_axis(
                            events,
                            source,
                            &horizontal,
                            &vertical,
                            self.input.pointer_x,
                            self.input.pointer_y,
                        );
                    }
                }
            }
        }
    }
}

/// What an axis message asks a widget to do, given where the pointer is.
///
/// Free rather than inline, for the same reason `keysym_to_key` is: a callback
/// that needs a compositor to run is a callback nothing checks, and this is
/// where the protocol's only statement about a gesture *ending* is read.
fn translate_axis(
    events: &mut Vec<Event>,
    source: Option<wl_pointer::AxisSource>,
    horizontal: &AxisScroll,
    vertical: &AxisScroll,
    x: f32,
    y: f32,
) {
    let scroll_source = match source {
        Some(wl_pointer::AxisSource::Wheel) => ScrollSource::Wheel,
        Some(wl_pointer::AxisSource::Finger) => ScrollSource::Finger,
        Some(wl_pointer::AxisSource::Continuous) => ScrollSource::Continuous,
        Some(wl_pointer::AxisSource::WheelTilt) => ScrollSource::Wheel,
        _ => ScrollSource::Wheel,
    };

    // Wheel steps by preference: value120 (wl_pointer v8+, fractional steps
    // from high-resolution wheels) then legacy discrete. Touchpad and finger
    // scroll have neither and report absolute pixels, which pass through —
    // multiplying those by a line height scrolls a page for a fingertip.
    let pixels = |scroll: &AxisScroll| {
        let steps = if scroll.value120 != 0 {
            scroll.value120 as f32 / 120.0
        } else {
            scroll.discrete as f32
        };
        if steps != 0.0 {
            steps * SCROLL_PIXELS_PER_LINE
        } else {
            scroll.absolute as f32
        }
    };

    let delta_x = pixels(horizontal);
    let delta_y = pixels(vertical);

    // An axis message says two things, and a guard on the delta alone dropped
    // the second. `axis_stop` is how the hardware ends a continuous scroll, and
    // it carries no delta — because nothing moved — so it was filtered out one
    // line before anything could use it. It is the only unguessed answer to
    // when a gesture is over.
    if delta_x != 0.0 || delta_y != 0.0 {
        events.push(Event::Scroll {
            x,
            y,
            delta_x,
            delta_y,
            source: scroll_source,
        });
    }
    if horizontal.stop || vertical.stop {
        events.push(Event::ScrollEnd { x, y });
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
        self.input.keyboard_serial = serial;
        self.input.latest_input_serial = serial;

        if let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref(), true) {
            // Store raw_code → Key mapping so release_key can emit the correct Key
            // (e.g., composed 'é' instead of raw 'e' after a compose sequence)
            self.input.pressed_keys.insert(event.raw_code, key);

            let key_event = Event::KeyDown {
                key,
                modifiers: self.input.modifiers,
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
            .input
            .pressed_keys
            .remove(&event.raw_code)
            .or_else(|| keysym_to_key(event.keysym, event.utf8.as_deref(), false));

        if let Some(key) = key {
            let key_event = Event::KeyUp {
                key,
                modifiers: self.input.modifiers,
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
        self.input.modifiers = Modifiers {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            logo: modifiers.logo,
            caps_lock: modifiers.caps_lock,
        };
        // Published as a signal as well: a latched modifier is a state
        // something on screen may need to show with nothing pressed and no key
        // event coming to carry it.
        crate::keyboard::set_keyboard_modifiers(self.input.modifiers);
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        self.emit_key_repeat(event);
    }
}

impl WaylandState {
    /// Deliver a repeated key as an ordinary press.
    ///
    /// Two sources reach here: the toolkit's calloop timer, armed from the
    /// compositor's `repeat_info`, and the protocol's own repeated key state.
    /// Neither is visible to widgets — a held key looks exactly like someone
    /// pressing it very fast.
    fn emit_key_repeat(&mut self, event: KeyEvent) {
        let Some(key) = keysym_to_key(event.keysym, event.utf8.as_deref(), true) else {
            return;
        };
        let key_event = Event::KeyDown {
            key,
            modifiers: self.input.modifiers,
        };

        // Route to the surface with keyboard focus
        if let Some(id) = self.current_keyboard_surface
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            surface_state.pending_events.push(key_event);
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

    // Character input: utf8 carries the composed result, so it wins whenever
    // it holds something printable.
    if let Some(text) = utf8
        && let Some(c) = text.chars().next()
    {
        if !c.is_control() || c == '\n' || c == '\r' || c == '\t' {
            return Some(Key::Char(c));
        }

        // A control character here means a modifier ate the letter: Ctrl+C
        // arrives as U+0003, not as 'c'. The keysym still carries the letter,
        // so fall through to it — otherwise every Ctrl chord is dropped and
        // copy, cut and paste never reach a widget.
        return keysym_char(keysym);
    }

    // No utf8 on a press means a compose sequence is still open: there is
    // nothing to insert yet. On release utf8 is always absent, so the keysym
    // is all there is.
    if !is_press {
        return keysym_char(keysym);
    }

    None
}

/// The character a keysym stands for, ignoring any composition.
fn keysym_char(keysym: Keysym) -> Option<Key> {
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

delegate_seat!(WaylandState);
delegate_pointer!(WaylandState);
delegate_touch!(WaylandState);
delegate_keyboard!(WaylandState);

#[cfg(test)]
mod tests {
    use super::*;

    /// A held modifier replaces the character xkb reports: Ctrl+C arrives as
    /// U+0003, not as 'c'. The keysym still carries the letter, and dropping
    /// the event here means copy, cut, paste, undo and select-all never reach
    /// a widget at all.
    #[test]
    fn a_ctrl_chord_keeps_its_letter() {
        let c = Keysym::new(0x63); // 'c'
        assert_eq!(
            keysym_to_key(c, Some("\u{3}"), true),
            Some(Key::Char('c')),
            "Ctrl+C must arrive as 'c'"
        );
    }

    /// The case the control-character guard exists for: while a compose
    /// sequence is open xkb reports no text, and nothing should be inserted
    /// until it resolves.
    #[test]
    fn an_open_compose_sequence_inserts_nothing() {
        let e = Keysym::new(0x65); // 'e'
        assert_eq!(keysym_to_key(e, None, true), None);
    }

    /// Release events never carry text, so the keysym is all there is —
    /// that is how a composed key is matched back to the press that made it.
    #[test]
    fn a_release_falls_back_to_the_keysym() {
        let e = Keysym::new(0x65);
        assert_eq!(keysym_to_key(e, None, false), Some(Key::Char('e')));
    }

    /// Ordinary typing still goes through the text, not the keysym: that is
    /// what carries an accented or composed character.
    #[test]
    fn plain_typing_uses_the_composed_text() {
        let e = Keysym::new(0x65); // 'e', composed into 'é'
        assert_eq!(keysym_to_key(e, Some("é"), true), Some(Key::Char('é')));
    }
}
