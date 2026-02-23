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
