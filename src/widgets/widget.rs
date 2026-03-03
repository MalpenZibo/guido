use crate::layout::{Constraints, Size};
use crate::renderer::PaintContext;
use crate::tree::{Tree, WidgetId};

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
    pub const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    pub const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);
    pub const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    pub const YELLOW: Color = Color::rgb(1.0, 1.0, 0.0);
    pub const CYAN: Color = Color::rgb(0.0, 1.0, 1.0);
    pub const MAGENTA: Color = Color::rgb(1.0, 0.0, 1.0);
    pub const GRAY: Color = Color::rgb(0.5, 0.5, 0.5);

    /// Create a color from 8-bit (0-255) RGB values.
    pub const fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    /// Create a color from 8-bit (0-255) RGBA values.
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Convert to 8-bit RGBA tuple.
    pub fn to_rgba8(self) -> (u8, u8, u8, u8) {
        (
            (self.r * 255.0 + 0.5) as u8,
            (self.g * 255.0 + 0.5) as u8,
            (self.b * 255.0 + 0.5) as u8,
            (self.a * 255.0 + 0.5) as u8,
        )
    }

    /// Blend toward white by `amount` (0.0 = no change, 1.0 = fully white).
    /// Preserves alpha.
    pub fn lighter(self, amount: f32) -> Self {
        Self {
            r: self.r + (1.0 - self.r) * amount,
            g: self.g + (1.0 - self.g) * amount,
            b: self.b + (1.0 - self.b) * amount,
            a: self.a,
        }
    }

    /// Blend toward black by `amount` (0.0 = no change, 1.0 = fully black).
    /// Preserves alpha.
    pub fn darker(self, amount: f32) -> Self {
        Self {
            r: self.r * (1.0 - amount),
            g: self.g * (1.0 - amount),
            b: self.b * (1.0 - amount),
            a: self.a,
        }
    }

    /// Linear interpolate with another color by `t` (0.0 = self, 1.0 = other).
    /// Interpolates all channels including alpha.
    pub fn mix(self, other: Color, t: f32) -> Self {
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Invert RGB channels (1.0 - r/g/b). Preserves alpha.
    pub fn invert(self) -> Self {
        Self {
            r: 1.0 - self.r,
            g: 1.0 - self.g,
            b: 1.0 - self.b,
            a: self.a,
        }
    }

    /// Relative luminance using Rec. 709 coefficients.
    /// Returns 0.0 for black, 1.0 for white.
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// Convert to perceptual grayscale using Rec. 709 weights.
    /// Preserves alpha.
    pub fn grayscale(self) -> Self {
        let l = self.luminance();
        Self {
            r: l,
            g: l,
            b: l,
            a: self.a,
        }
    }

    /// Return a new color with the given alpha value.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha, ..self }
    }

    /// Multiply the alpha channel by a factor.
    pub fn scale_alpha(self, factor: f32) -> Self {
        Self {
            a: self.a * factor,
            ..self
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
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

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// Check if a point is inside this rect with rounded corners.
    /// The corner_radius is clamped to half of the smaller dimension.
    pub fn contains_rounded(&self, x: f32, y: f32, corner_radius: f32) -> bool {
        // First check basic bounds
        if !self.contains(x, y) {
            return false;
        }

        // If no corner radius, we're done
        if corner_radius <= 0.0 {
            return true;
        }

        // Clamp radius to half of smaller dimension
        let max_radius = (self.width.min(self.height) / 2.0).max(0.0);
        let r = corner_radius.min(max_radius);

        // Check if point is in a corner region
        let left = self.x;
        let right = self.x + self.width;
        let top = self.y;
        let bottom = self.y + self.height;

        // Corner circle centers
        let in_left = x < left + r;
        let in_right = x > right - r;
        let in_top = y < top + r;
        let in_bottom = y > bottom - r;

        // If in a corner region, check distance from corner circle center
        if in_left && in_top {
            // Top-left corner
            let cx = left + r;
            let cy = top + r;
            let dx = x - cx;
            let dy = y - cy;
            return dx * dx + dy * dy <= r * r;
        }
        if in_right && in_top {
            // Top-right corner
            let cx = right - r;
            let cy = top + r;
            let dx = x - cx;
            let dy = y - cy;
            return dx * dx + dy * dy <= r * r;
        }
        if in_left && in_bottom {
            // Bottom-left corner
            let cx = left + r;
            let cy = bottom - r;
            let dx = x - cx;
            let dy = y - cy;
            return dx * dx + dy * dy <= r * r;
        }
        if in_right && in_bottom {
            // Bottom-right corner
            let cx = right - r;
            let cy = bottom - r;
            let dx = x - cx;
            let dy = y - cy;
            return dx * dx + dy * dy <= r * r;
        }

        // Not in a corner region, so it's inside
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
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

    /// Override the top padding value.
    pub fn top(mut self, v: f32) -> Self {
        self.top = v;
        self
    }

    /// Override the bottom padding value.
    pub fn bottom(mut self, v: f32) -> Self {
        self.bottom = v;
        self
    }

    /// Override the left padding value.
    pub fn left(mut self, v: f32) -> Self {
        self.left = v;
        self
    }

    /// Override the right padding value.
    pub fn right(mut self, v: f32) -> Self {
        self.right = v;
        self
    }
}

// From conversions for Padding — enables padding(8.0), padding(8), padding([8.0, 16.0]), etc.

impl From<f32> for Padding {
    fn from(v: f32) -> Self {
        Padding::all(v)
    }
}

impl From<u16> for Padding {
    fn from(v: u16) -> Self {
        Padding::all(f32::from(v))
    }
}

impl From<u32> for Padding {
    fn from(v: u32) -> Self {
        Padding::all(v as f32)
    }
}

impl From<i32> for Padding {
    fn from(v: i32) -> Self {
        Padding::all(v as f32)
    }
}

/// `[vertical, horizontal]` — CSS-style 2-value shorthand.
impl From<[f32; 2]> for Padding {
    fn from(v: [f32; 2]) -> Self {
        Padding {
            top: v[0],
            right: v[1],
            bottom: v[0],
            left: v[1],
        }
    }
}

/// `[top, right, bottom, left]` — CSS-style 4-value shorthand.
impl From<[f32; 4]> for Padding {
    fn from(v: [f32; 4]) -> Self {
        Padding {
            top: v[0],
            right: v[1],
            bottom: v[2],
            left: v[3],
        }
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

/// Keyboard modifier state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

/// Named keys for special keyboard keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// Backspace key
    Backspace,
    /// Delete key
    Delete,
    /// Enter/Return key
    Enter,
    /// Tab key
    Tab,
    /// Escape key
    Escape,
    /// Left arrow
    Left,
    /// Right arrow
    Right,
    /// Up arrow
    Up,
    /// Down arrow
    Down,
    /// Home key
    Home,
    /// End key
    End,
    /// Character input (includes A-Z for Ctrl+A shortcuts)
    Char(char),
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
    /// Key pressed
    KeyDown {
        /// The key that was pressed
        key: Key,
        /// Current modifier state
        modifiers: Modifiers,
    },
    /// Key released
    KeyUp {
        /// The key that was released
        key: Key,
        /// Current modifier state
        modifiers: Modifiers,
    },
    /// Widget gained keyboard focus
    FocusIn,
    /// Widget lost keyboard focus
    FocusOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    Ignored,
    Handled,
}

impl Event {
    /// Get the coordinates from this event, if any
    pub fn coords(&self) -> Option<(f32, f32)> {
        match self {
            Event::MouseMove { x, y } => Some((*x, *y)),
            Event::MouseDown { x, y, .. } => Some((*x, *y)),
            Event::MouseUp { x, y, .. } => Some((*x, *y)),
            Event::MouseEnter { x, y } => Some((*x, *y)),
            Event::Scroll { x, y, .. } => Some((*x, *y)),
            Event::MouseLeave
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::FocusIn
            | Event::FocusOut => None,
        }
    }

    /// Create a new event with transformed coordinates
    pub fn with_coords(&self, new_x: f32, new_y: f32) -> Self {
        match self {
            Event::MouseMove { .. } => Event::MouseMove { x: new_x, y: new_y },
            Event::MouseDown { button, .. } => Event::MouseDown {
                x: new_x,
                y: new_y,
                button: *button,
            },
            Event::MouseUp { button, .. } => Event::MouseUp {
                x: new_x,
                y: new_y,
                button: *button,
            },
            Event::MouseEnter { .. } => Event::MouseEnter { x: new_x, y: new_y },
            Event::Scroll {
                delta_x,
                delta_y,
                source,
                ..
            } => Event::Scroll {
                x: new_x,
                y: new_y,
                delta_x: *delta_x,
                delta_y: *delta_y,
                source: *source,
            },
            Event::MouseLeave => Event::MouseLeave,
            // Keyboard/focus events don't have coordinates
            Event::KeyDown { key, modifiers } => Event::KeyDown {
                key: *key,
                modifiers: *modifiers,
            },
            Event::KeyUp { key, modifiers } => Event::KeyUp {
                key: *key,
                modifiers: *modifiers,
            },
            Event::FocusIn => Event::FocusIn,
            Event::FocusOut => Event::FocusOut,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutHints {
    pub fill_width: bool,
    pub fill_height: bool,
}

pub trait Widget {
    /// Advance animations for this widget and children.
    /// Returns true if any animations are still active and need another frame.
    /// Called once per frame before layout.
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let _ = (tree, id);
        false
    }

    /// Reconcile dynamic children. Called from main loop before layout.
    /// Returns true if children changed (requires layout).
    /// Default implementation returns false (no dynamic children).
    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let _ = (tree, id);
        false
    }

    fn layout_hints(&self) -> LayoutHints {
        LayoutHints::default()
    }

    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size;
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext);
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        let _ = (tree, id, event);
        EventResponse::Ignored
    }

    /// Check if this widget has a descendant with the given ID.
    /// Used by containers to check if a child has focus.
    /// Default implementation returns false (leaf widgets have no children).
    fn has_focus_descendant(&self, _tree: &Tree, _id: WidgetId) -> bool {
        false
    }

    /// Register this widget's pending children with the arena.
    ///
    /// Called during widget tree registration to recursively register all
    /// children before the tree is used for layout. Containers should override
    /// this to register their pending children.
    ///
    /// Default implementation does nothing (leaf widgets have no children).
    fn register_children(&mut self, _tree: &mut Tree, _id: WidgetId) {}
}

impl Widget for Box<dyn Widget> {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        (**self).advance_animations(tree, id)
    }
    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        (**self).reconcile_children(tree, id)
    }
    fn layout_hints(&self) -> LayoutHints {
        (**self).layout_hints()
    }
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        (**self).layout(tree, id, constraints)
    }
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        (**self).paint(tree, id, ctx)
    }
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        (**self).event(tree, id, event)
    }
    fn has_focus_descendant(&self, tree: &Tree, id: WidgetId) -> bool {
        (**self).has_focus_descendant(tree, id)
    }
    fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {
        (**self).register_children(tree, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_rgb() {
        let color = Color::rgb(0.5, 0.6, 0.7);
        assert_eq!(color.r, 0.5);
        assert_eq!(color.g, 0.6);
        assert_eq!(color.b, 0.7);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_color_rgba() {
        let color = Color::rgba(0.1, 0.2, 0.3, 0.5);
        assert_eq!(color.r, 0.1);
        assert_eq!(color.g, 0.2);
        assert_eq!(color.b, 0.3);
        assert_eq!(color.a, 0.5);
    }

    #[test]
    fn test_color_from_hex() {
        let color = Color::from_hex(0xFF0000);
        assert_eq!(color.r, 1.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 0.0);
        assert_eq!(color.a, 1.0);

        let color = Color::from_hex(0x00FF00);
        assert_eq!(color.r, 0.0);
        assert_eq!(color.g, 1.0);
        assert_eq!(color.b, 0.0);

        let color = Color::from_hex(0x0000FF);
        assert_eq!(color.r, 0.0);
        assert_eq!(color.g, 0.0);
        assert_eq!(color.b, 1.0);
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(Color::WHITE, Color::rgb(1.0, 1.0, 1.0));
        assert_eq!(Color::BLACK, Color::rgb(0.0, 0.0, 0.0));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0.0, 0.0, 0.0, 0.0));
    }

    #[test]
    fn test_color_default() {
        let color = Color::default();
        assert_eq!(color, Color::TRANSPARENT);
    }

    #[test]
    fn test_color_named_constants() {
        assert_eq!(Color::RED, Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(Color::GREEN, Color::rgb(0.0, 1.0, 0.0));
        assert_eq!(Color::BLUE, Color::rgb(0.0, 0.0, 1.0));
        assert_eq!(Color::YELLOW, Color::rgb(1.0, 1.0, 0.0));
        assert_eq!(Color::CYAN, Color::rgb(0.0, 1.0, 1.0));
        assert_eq!(Color::MAGENTA, Color::rgb(1.0, 0.0, 1.0));
        assert_eq!(Color::GRAY, Color::rgb(0.5, 0.5, 0.5));
    }

    #[test]
    fn test_color_from_rgb8() {
        let color = Color::from_rgb8(255, 128, 0);
        assert!((color.r - 1.0).abs() < 0.01);
        assert!((color.g - 0.502).abs() < 0.01);
        assert!((color.b - 0.0).abs() < 0.01);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn test_color_from_rgba8() {
        let color = Color::from_rgba8(255, 0, 0, 128);
        assert!((color.r - 1.0).abs() < 0.01);
        assert!((color.g - 0.0).abs() < 0.01);
        assert!((color.b - 0.0).abs() < 0.01);
        assert!((color.a - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_color_to_rgba8() {
        let (r, g, b, a) = Color::rgb(1.0, 0.0, 0.5).to_rgba8();
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 128);
        assert_eq!(a, 255);
    }

    #[test]
    fn test_color_rgb8_roundtrip() {
        let original = Color::from_rgb8(100, 200, 50);
        let (r, g, b, a) = original.to_rgba8();
        assert_eq!(r, 100);
        assert_eq!(g, 200);
        assert_eq!(b, 50);
        assert_eq!(a, 255);
    }

    #[test]
    fn test_lighter_no_change() {
        let c = Color::rgb(0.5, 0.5, 0.5);
        assert_eq!(c.lighter(0.0), c);
    }

    #[test]
    fn test_lighter_full() {
        let c = Color::rgba(0.2, 0.4, 0.6, 0.8);
        let result = c.lighter(1.0);
        assert!((result.r - 1.0).abs() < 1e-6);
        assert!((result.g - 1.0).abs() < 1e-6);
        assert!((result.b - 1.0).abs() < 1e-6);
        assert_eq!(result.a, 0.8); // alpha preserved
    }

    #[test]
    fn test_darker_no_change() {
        let c = Color::rgb(0.5, 0.5, 0.5);
        assert_eq!(c.darker(0.0), c);
    }

    #[test]
    fn test_darker_full() {
        let c = Color::rgba(0.2, 0.4, 0.6, 0.8);
        let result = c.darker(1.0);
        assert!((result.r).abs() < 1e-6);
        assert!((result.g).abs() < 1e-6);
        assert!((result.b).abs() < 1e-6);
        assert_eq!(result.a, 0.8); // alpha preserved
    }

    #[test]
    fn test_mix_endpoints() {
        let a = Color::RED;
        let b = Color::BLUE;
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
    }

    #[test]
    fn test_mix_midpoint() {
        let a = Color::rgb(0.0, 0.0, 0.0);
        let b = Color::rgb(1.0, 1.0, 1.0);
        let mid = a.mix(b, 0.5);
        assert!((mid.r - 0.5).abs() < 1e-6);
        assert!((mid.g - 0.5).abs() < 1e-6);
        assert!((mid.b - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_invert_white_is_black() {
        assert_eq!(Color::WHITE.invert(), Color::rgb(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_invert_black_is_white() {
        assert_eq!(Color::BLACK.invert(), Color::rgb(1.0, 1.0, 1.0));
    }

    #[test]
    fn test_invert_roundtrip() {
        let c = Color::rgb(0.2, 0.5, 0.8);
        let roundtrip = c.invert().invert();
        assert!((roundtrip.r - c.r).abs() < 1e-6);
        assert!((roundtrip.g - c.g).abs() < 1e-6);
        assert!((roundtrip.b - c.b).abs() < 1e-6);
    }

    #[test]
    fn test_invert_preserves_alpha() {
        let c = Color::rgba(0.5, 0.5, 0.5, 0.3);
        assert_eq!(c.invert().a, 0.3);
    }

    #[test]
    fn test_luminance_white() {
        assert!((Color::WHITE.luminance() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_luminance_black() {
        assert!(Color::BLACK.luminance().abs() < 1e-6);
    }

    #[test]
    fn test_luminance_green_brightest() {
        // Green has the highest luminance weight (0.7152)
        assert!(Color::GREEN.luminance() > Color::RED.luminance());
        assert!(Color::GREEN.luminance() > Color::BLUE.luminance());
    }

    #[test]
    fn test_grayscale_white() {
        assert_eq!(Color::WHITE.grayscale(), Color::WHITE);
    }

    #[test]
    fn test_grayscale_black() {
        assert_eq!(Color::BLACK.grayscale(), Color::BLACK);
    }

    #[test]
    fn test_grayscale_preserves_alpha() {
        let c = Color::rgba(1.0, 0.0, 0.0, 0.5);
        assert_eq!(c.grayscale().a, 0.5);
    }

    #[test]
    fn test_grayscale_uniform_channels() {
        let g = Color::RED.grayscale();
        assert_eq!(g.r, g.g);
        assert_eq!(g.g, g.b);
    }

    #[test]
    fn test_with_alpha() {
        let c = Color::RED.with_alpha(0.5);
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert_eq!(c.b, 0.0);
        assert_eq!(c.a, 0.5);
    }

    #[test]
    fn test_scale_alpha() {
        let c = Color::rgba(1.0, 0.0, 0.0, 0.8);
        let scaled = c.scale_alpha(0.5);
        assert!((scaled.a - 0.4).abs() < 1e-6);
        assert_eq!(scaled.r, 1.0); // RGB unchanged
    }

    #[test]
    fn test_rect_new() {
        let rect = Rect::new(10.0, 20.0, 100.0, 200.0);
        assert_eq!(rect.x, 10.0);
        assert_eq!(rect.y, 20.0);
        assert_eq!(rect.width, 100.0);
        assert_eq!(rect.height, 200.0);
    }

    #[test]
    fn test_rect_from_size() {
        let size = Size::new(50.0, 75.0);
        let rect = Rect::from_size(size);
        assert_eq!(rect.x, 0.0);
        assert_eq!(rect.y, 0.0);
        assert_eq!(rect.width, 50.0);
        assert_eq!(rect.height, 75.0);
    }

    #[test]
    fn test_rect_offset() {
        let rect = Rect::new(10.0, 20.0, 100.0, 200.0);
        let offset_rect = rect.offset(5.0, 10.0);
        assert_eq!(offset_rect.x, 15.0);
        assert_eq!(offset_rect.y, 30.0);
        assert_eq!(offset_rect.width, 100.0);
        assert_eq!(offset_rect.height, 200.0);
    }

    #[test]
    fn test_rect_inset() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let inset_rect = rect.inset(10.0);
        assert_eq!(inset_rect.x, 10.0);
        assert_eq!(inset_rect.y, 10.0);
        assert_eq!(inset_rect.width, 80.0);
        assert_eq!(inset_rect.height, 80.0);

        // Test that inset doesn't go negative
        let small_rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let over_inset = small_rect.inset(20.0);
        assert_eq!(over_inset.width, 0.0);
        assert_eq!(over_inset.height, 0.0);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);

        // Inside
        assert!(rect.contains(50.0, 40.0));

        // Edges
        assert!(rect.contains(10.0, 20.0)); // Top-left corner (inclusive)
        assert!(!rect.contains(110.0, 70.0)); // Bottom-right corner (exclusive)

        // Outside
        assert!(!rect.contains(5.0, 40.0));
        assert!(!rect.contains(150.0, 40.0));
        assert!(!rect.contains(50.0, 10.0));
        assert!(!rect.contains(50.0, 100.0));
    }

    #[test]
    fn test_padding_all() {
        let padding = Padding::all(10.0);
        assert_eq!(padding.top, 10.0);
        assert_eq!(padding.right, 10.0);
        assert_eq!(padding.bottom, 10.0);
        assert_eq!(padding.left, 10.0);
    }

    #[test]
    fn test_padding_symmetric() {
        let padding = Padding::symmetric(20.0, 30.0);
        assert_eq!(padding.top, 30.0);
        assert_eq!(padding.right, 20.0);
        assert_eq!(padding.bottom, 30.0);
        assert_eq!(padding.left, 20.0);
    }

    #[test]
    fn test_padding_horizontal() {
        let padding = Padding::symmetric(15.0, 10.0);
        assert_eq!(padding.horizontal(), 30.0); // left + right = 15 + 15
    }

    #[test]
    fn test_padding_vertical() {
        let padding = Padding::symmetric(15.0, 10.0);
        assert_eq!(padding.vertical(), 20.0); // top + bottom = 10 + 10
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
        assert_eq!(padding.top, 0.0);
        assert_eq!(padding.right, 0.0);
        assert_eq!(padding.bottom, 0.0);
        assert_eq!(padding.left, 0.0);
    }

    #[test]
    fn test_padding_builder_methods() {
        let padding = Padding::all(8.0).top(20.0).left(0.0);
        assert_eq!(padding.top, 20.0);
        assert_eq!(padding.right, 8.0);
        assert_eq!(padding.bottom, 8.0);
        assert_eq!(padding.left, 0.0);
    }

    #[test]
    fn test_padding_from_f32() {
        let padding = Padding::from(10.0);
        assert_eq!(padding, Padding::all(10.0));
    }

    #[test]
    fn test_padding_from_i32() {
        let padding = Padding::from(10i32);
        assert_eq!(padding, Padding::all(10.0));
    }

    #[test]
    fn test_padding_from_u16() {
        let padding = Padding::from(10u16);
        assert_eq!(padding, Padding::all(10.0));
    }

    #[test]
    fn test_padding_from_u32() {
        let padding = Padding::from(10u32);
        assert_eq!(padding, Padding::all(10.0));
    }

    #[test]
    fn test_padding_from_array_2() {
        let padding = Padding::from([8.0, 16.0]);
        assert_eq!(padding.top, 8.0);
        assert_eq!(padding.right, 16.0);
        assert_eq!(padding.bottom, 8.0);
        assert_eq!(padding.left, 16.0);
    }

    #[test]
    fn test_padding_from_array_4() {
        let padding = Padding::from([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(padding.top, 1.0);
        assert_eq!(padding.right, 2.0);
        assert_eq!(padding.bottom, 3.0);
        assert_eq!(padding.left, 4.0);
    }
}
