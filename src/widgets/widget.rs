use crate::layout::{Constraints, Size};
use crate::reactive::{ChangeFlags, WidgetId};
use crate::renderer::PaintContext;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        }
    }

    pub fn offset(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..*self
        }
    }

    pub fn inset(&self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            width: (self.width - amount * 2.0).max(0.0),
            height: (self.height - amount * 2.0).max(0.0),
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Padding {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Padding {
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Scroll source - discrete (mouse wheel) or smooth (touchpad/touchscreen)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollSource {
    /// Mouse wheel - discrete steps (converted to pixels)
    Wheel,
    /// Touchpad/touchscreen - smooth pixel-based scrolling
    Finger,
    /// Continuous scrolling (e.g., kinetic/momentum)
    Continuous,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Mouse/pointer moved
    MouseMove { x: f32, y: f32 },
    /// Mouse button pressed
    MouseDown { x: f32, y: f32, button: MouseButton },
    /// Mouse button released
    MouseUp { x: f32, y: f32, button: MouseButton },
    /// Mouse/pointer entered the surface (with entry coordinates)
    MouseEnter { x: f32, y: f32 },
    /// Mouse/pointer left the surface
    MouseLeave,
    /// Scroll event (wheel, touchpad, or touchscreen)
    Scroll {
        /// X position of the pointer
        x: f32,
        /// Y position of the pointer
        y: f32,
        /// Horizontal scroll delta in pixels (positive = right)
        delta_x: f32,
        /// Vertical scroll delta in pixels (positive = down)
        delta_y: f32,
        /// Source of the scroll event
        source: ScrollSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    Ignored,
    Handled,
}

pub trait Widget {
    fn layout(&mut self, constraints: Constraints) -> Size;
    fn paint(&self, ctx: &mut PaintContext);
    fn event(&mut self, event: &Event) -> EventResponse {
        let _ = event;
        EventResponse::Ignored
    }
    fn set_origin(&mut self, x: f32, y: f32);

    /// Get the widget's bounding rectangle (for hit testing)
    fn bounds(&self) -> Rect;

    /// Get the widget's unique identifier
    fn id(&self) -> WidgetId;

    /// Mark this widget as needing layout and/or paint
    fn mark_dirty(&mut self, flags: ChangeFlags);

    /// Check if this widget needs layout
    fn needs_layout(&self) -> bool;

    /// Check if this widget needs paint
    fn needs_paint(&self) -> bool;

    /// Clear dirty flags after processing
    fn clear_dirty(&mut self);
}
