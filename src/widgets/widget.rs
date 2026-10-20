use std::rc::Rc;

use crate::layout::{Axis, Constraints, Size};
use crate::reactive::{Callback, IntoHandler};
use crate::renderer::PaintContext;
use crate::tree::{LayoutCtx, Tree, WidgetId};

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

    /// Linear interpolate with another color by `t` (0.0 = self, 1.0 = other),
    /// with alpha premultiplied, as CSS Color 4 interpolates: the colour
    /// channels are weighted by their alpha, so a mix with `TRANSPARENT` keeps
    /// the other colour's hue rather than passing through black. Between two
    /// opaque colours it is the plain channel-by-channel mix.
    ///
    /// The result is clamped as CSS clamps an interpolated colour — alpha into
    /// `[0, 1]`, then the channels — which is what holds a spring or an easing
    /// overshooting past a fade inside the colours that can be drawn. Where the mixed alpha is zero or below there is nothing
    /// to divide back by, and the colour channels are mixed as they are.
    pub fn mix(self, other: Color, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        let a = f(self.a, other.a);
        let (from, to, unweight) = if a > 0.0 {
            (self.premultiplied(), other.premultiplied(), 1.0 / a)
        } else {
            let straight = |c: Self| [c.r, c.g, c.b, c.a];
            (straight(self), straight(other), 1.0)
        };
        let channel = |i: usize| (f(from[i], to[i]) * unweight).clamp(0.0, 1.0);
        Self {
            r: channel(0),
            g: channel(1),
            b: channel(2),
            a: a.clamp(0.0, 1.0),
        }
    }

    /// The colour channels multiplied by alpha, and the alpha: the space
    /// [`mix`](Self::mix) is a straight line in.
    pub(crate) fn premultiplied(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
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

/// Where a pointer is.
///
/// Its own type rather than a pair of floats because the interesting value is
/// [`Option<Point>`]: a pointer event that has descended into a subtree
/// collapsed to nothing has no position, and there is no *number* that means
/// nowhere. See [`Event::coords`].
/// No `Default`, deliberately. The origin is a place like any other, and this
/// whole type exists so that "nowhere" is not spelled as a coordinate — a
/// `Option<Point>::unwrap_or_default()` would put the pointer at the top-left
/// corner, which is the snapped-to-zero failure #227 records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Offset by a vector — how a container rebases a point into child space.
    #[inline]
    pub fn offset(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy)
    }
    /// The coordinate along `axis` — a point's [`Rect::span`].
    #[inline]
    pub(crate) fn along(self, axis: crate::layout::Axis) -> f32 {
        match axis {
            crate::layout::Axis::Horizontal => self.x,
            crate::layout::Axis::Vertical => self.y,
        }
    }
}

/// `(x, y)`, as a path is usually written.
impl From<(f32, f32)> for Point {
    #[inline]
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

/// The superellipse "length" of a vector — the shader's `superellipse_length`.
///
/// `n = 1` is the diamond a bevel cuts, `n = 2` the circle of an ordinary
/// rounded corner, and everything between is a squircle on its way.
///
/// The general case divides through by the larger component before raising
/// it, exactly as the shaders do. Taken straight, `|x|^n` leaves `f32` a
/// little above `k = 4` at an ordinary radius, and the sum is then `inf`
/// whatever the other component holds — so `inf - r` is `inf` and every
/// point the corner box reaches reads as outside, along the sides as much as
/// around the corners. A 120×120 with `Corners::superellipse(40.0, 6.0)`
/// took clicks on 2304 of its 14,400 pixels: the inner square, and nothing
/// else. Scaled, the larger term is exactly 1 and the smaller at most 1; as
/// `n` grows the smaller underflows and this tends to `max(|x|, |y|)`, the
/// square corner a superellipse approaches.
fn superellipse_length(x: f32, y: f32, n: f32) -> f32 {
    if (n - 1.0).abs() < 0.01 {
        x.abs() + y.abs()
    } else if (n - 2.0).abs() < 0.01 {
        x.hypot(y)
    } else {
        let (ax, ay) = (x.abs(), y.abs());
        let m = ax.max(ay);
        if m <= 0.0 {
            return 0.0;
        }
        let t = ax.min(ay) / m;
        m * (1.0 + t.powf(n)).powf(1.0 / n)
    }
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

    /// The interval this rect covers on one axis, near edge first.
    ///
    /// What a search along an axis compares against, and what deciding
    /// whether two rects follow one another along it comes down to.
    pub(crate) fn span(&self, axis: Axis) -> (f32, f32) {
        match axis {
            Axis::Horizontal => (self.x, self.x + self.width),
            Axis::Vertical => (self.y, self.y + self.height),
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

    /// Grow this rect by `amount` on every side.
    ///
    /// What "how far past its bounds does this widget paint" turns into when
    /// something has to test against it: a shadow's falloff, or the distance a
    /// transform can carry the box.
    pub fn outset(&self, amount: f32) -> Self {
        self.inset(-amount)
    }

    /// Grow this rect by each of `outsets` on its own side.
    pub fn outset_by(&self, outsets: Outsets) -> Self {
        Self {
            x: self.x - outsets.left,
            y: self.y - outsets.top,
            width: self.width + outsets.left + outsets.right,
            height: self.height + outsets.top + outsets.bottom,
        }
    }

    /// How far this rect stands outside `inner` on each side, and zero on a
    /// side where it does not.
    ///
    /// One question about two rects, asked in three places — how far a shadow
    /// and a transform carry a widget's painting past its own box, how far a
    /// child's painting carries past its parent's, and how far a damage rect
    /// has to grow to cover either. Two copies of it would be two copies of
    /// which rect is the outer one.
    pub fn outset_beyond(&self, inner: Rect) -> Outsets {
        Outsets {
            left: inner.x - self.x,
            top: inner.y - self.y,
            right: (self.x + self.width) - (inner.x + inner.width),
            bottom: (self.y + self.height) - (inner.y + inner.height),
        }
        .floored()
    }

    /// The area two rects share, or `None` where they share none — touching
    /// edges included, which share no area.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
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

    /// Whether a point that may not exist is inside this rect.
    ///
    /// An event with no position is inside nothing — see [`Event::coords`] for
    /// the two ways it comes to have none. Every hit test asks this rather
    /// than unwrapping first, so the rule is written once instead of being
    /// taught to each widget separately.
    #[inline]
    pub fn contains_at(&self, at: Option<Point>) -> bool {
        at.is_some_and(|at| self.contains(at.x, at.y))
    }

    /// [`contains_at`](Self::contains_at), narrowed by the corner shape.
    #[inline]
    pub fn contains_shape_at(&self, at: Option<Point>, corners: crate::widgets::Corners) -> bool {
        at.is_some_and(|at| self.contains_shape(at.x, at.y, corners))
    }

    /// Whether a point is inside this rect once its corners are taken off.
    ///
    /// The same signed-distance function the shader draws with, evaluated on
    /// the CPU: a bevelled corner is a straight cut and a scooped one curves
    /// inward, so a circular test would answer for a shape nobody drew. What
    /// is drawn and what is clicked come from one formula, which is the only
    /// way they stay in step when the formula changes.
    /// The rectangle is tested half-open, as [`contains`](Self::contains) does:
    /// the far edge belongs to whatever comes next, so two boxes side by side
    /// in a row do not both answer for the column between them. The shape only
    /// takes more away.
    pub fn contains_shape(&self, x: f32, y: f32, corners: crate::widgets::Corners) -> bool {
        self.contains(x, y) && self.shape_distance(x, y, corners) <= 0.0
    }

    /// Distance from the point to the shape's edge: negative inside, positive
    /// outside. Mirrors `rounded_rect_sdf` in `shader.wgsl`.
    pub(crate) fn shape_distance(&self, x: f32, y: f32, corners: crate::widgets::Corners) -> f32 {
        let (cx, cy) = (self.x + self.width * 0.5, self.y + self.height * 0.5);
        let (hx, hy) = (self.width * 0.5, self.height * 0.5);
        let (rx, ry) = (x - cx, y - cy);

        // The quadrant the point falls in chooses its radius.
        let radii = corners.radii;
        let radius = match (rx < 0.0, ry < 0.0) {
            (true, true) => radii.top_left,
            (false, true) => radii.top_right,
            (false, false) => radii.bottom_right,
            (true, false) => radii.bottom_left,
        };
        let r = radius.min(hx.min(hy));

        let (px, py) = (rx.abs(), ry.abs());
        if r <= 0.0 {
            return (px - hx).max(py - hy);
        }

        // Scooped corners curve inward: the box minus a circle at the corner.
        if corners.curvature < 0.0 {
            let box_sdf = (px - hx).max(py - hy);
            let circle_sdf = ((px - hx).hypot(py - hy)) - r;
            return box_sdf.max(-circle_sdf);
        }

        // Convex corners: bevel, round, squircle and everything between.
        let n = 2f32.powf(corners.curvature);
        let (qx, qy) = (px - hx + r, py - hy + r);
        let (mx, my) = (qx.max(0.0), qy.max(0.0));
        let inside = qx.max(qy).min(0.0);
        inside + superellipse_length(mx, my, n) - r
    }
}

/// How far something reaches past a box, on each of its four sides.
///
/// What a widget paints outside its own bounds is rarely the same on every
/// side: a shadow offset downward falls below the box and barely above it, and
/// a box sliding up is drawn above where it was laid out and nowhere else. One
/// number for all four widened every side by the furthest of them, so a
/// 200 pixel vertical slide damaged 200 pixels either side of it as well.
/// Chromium keeps a box's ink overflow as four outsets for the same reason
/// (`InkOverflow`), and GTK gives a shadow's extents per side.
///
/// A plain number is the same on all four sides, which is what a widget whose
/// reach has no direction — a glyph's stroke — publishes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Outsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Outsets {
    /// Nothing past any side.
    pub const ZERO: Self = Self::all(0.0);

    /// The same reach on every side.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// The furthest of the four.
    pub fn widest(self) -> f32 {
        self.left.max(self.top).max(self.right).max(self.bottom)
    }

    /// Each side the further of the two.
    pub fn max(self, other: Self) -> Self {
        self.zip(other, f32::max)
    }

    /// Each side the nearer of the two.
    pub fn min(self, other: Self) -> Self {
        self.zip(other, f32::min)
    }

    /// Whether this reaches at least as far as `other` on every side.
    pub fn covers(self, other: Self) -> bool {
        self.left >= other.left
            && self.top >= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }

    /// No side below zero — `clamp` rather than `max`, so a `NaN` stays one.
    pub(crate) fn floored(self) -> Self {
        self.map(|side| side.clamp(0.0, f32::INFINITY))
    }

    /// The four sides, in the order the fields are declared.
    pub(crate) fn sides(self) -> [f32; 4] {
        [self.left, self.top, self.right, self.bottom]
    }

    pub(crate) fn map(self, f: impl Fn(f32) -> f32) -> Self {
        Self {
            left: f(self.left),
            top: f(self.top),
            right: f(self.right),
            bottom: f(self.bottom),
        }
    }

    fn zip(self, other: Self, f: impl Fn(f32, f32) -> f32) -> Self {
        Self {
            left: f(self.left, other.left),
            top: f(self.top, other.top),
            right: f(self.right, other.right),
            bottom: f(self.bottom, other.bottom),
        }
    }
}

impl From<f32> for Outsets {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl std::ops::Add for Outsets {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.zip(other, |a, b| a + b)
    }
}

impl std::ops::Sub for Outsets {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.zip(other, |a, b| a - b)
    }
}

impl std::ops::Mul<f32> for Outsets {
    type Output = Self;

    fn mul(self, k: f32) -> Self {
        self.map(|side| side * k)
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

    /// How much width the padding takes: `left + right`.
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// How much height the padding takes: `top + bottom`.
    pub fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }

    /// Override the top padding value.
    pub fn with_top(mut self, v: f32) -> Self {
        self.top = v;
        self
    }

    /// Override the bottom padding value.
    pub fn with_bottom(mut self, v: f32) -> Self {
        self.bottom = v;
        self
    }

    /// Override the left padding value.
    pub fn with_left(mut self, v: f32) -> Self {
        self.left = v;
        self
    }

    /// Override the right padding value.
    pub fn with_right(mut self, v: f32) -> Self {
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

/// Bare float literals default to f64; accepting them avoids the deprecated
/// f32 inference fallback.
impl From<f64> for Padding {
    fn from(v: f64) -> Self {
        Padding::all(v as f32)
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

/// `[vertical, horizontal]` — CSS-style 2-value shorthand (integer).
impl From<[i32; 2]> for Padding {
    fn from(v: [i32; 2]) -> Self {
        Padding {
            top: v[0] as f32,
            right: v[1] as f32,
            bottom: v[0] as f32,
            left: v[1] as f32,
        }
    }
}

/// `[top, right, bottom, left]` — CSS-style 4-value shorthand (integer).
impl From<[i32; 4]> for Padding {
    fn from(v: [i32; 4]) -> Self {
        Padding {
            top: v[0] as f32,
            right: v[1] as f32,
            bottom: v[2] as f32,
            left: v[3] as f32,
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

impl ScrollSource {
    /// Whether this source scrolls as a gesture that can be measured for a
    /// speed, rather than in discrete steps. A wheel is not one: its steps have
    /// no duration to divide by, and nothing to carry on afterwards.
    ///
    /// Being a gesture is not the same as having a reported end. The protocol
    /// guarantees a `wl_pointer.axis_stop` only for
    /// [`Finger`](ScrollSource::Finger); a sequence from
    /// [`Continuous`](ScrollSource::Continuous) is to be treated as
    /// unterminated by default. So a continuous source builds a velocity like a
    /// touchpad and, on hardware that never terminates, simply never spends it.
    pub fn is_continuous(self) -> bool {
        matches!(self, ScrollSource::Finger | ScrollSource::Continuous)
    }
}

/// What was moved or pressed: a mouse, or a finger on the glass.
///
/// Widgets understand one pointer, and touch is folded into it
/// (`src/platform/input.rs`) so that every widget can alias the two in a
/// single arm — which is what almost all of them want. This is the one
/// distinction the fold cannot make for them: a finger dragging the content of
/// a scroller scrolls it, and a mouse doing the same must keep selecting text
/// rather than taking drag-select away inside every scrollable.
///
/// One field on the events that already exist, not a parallel touch event
/// family. [`Event::Scroll`] carries a [`ScrollSource`] on the same principle,
/// and for the same reason: the handling is shared, so only the source differs.
///
/// No `Default`. Every event is built by something that knows which device it
/// came from — the constructors below, and the fold in `input.rs` — and a
/// default here would be a policy saying an unknown pointer is a mouse, which
/// is how a finger comes to be one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    /// A mouse, a trackball, a touchpad — anything that hovers before it
    /// presses.
    Mouse,
    /// A finger on a touchscreen, folded into the pointer pipeline.
    Finger,
}

/// Keyboard modifier state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
    /// Caps lock, which is latched rather than held: it stays on with nothing
    /// pressed. To say so on screen, read
    /// [`keyboard_modifiers()`](crate::keyboard::keyboard_modifiers) instead of
    /// the copy a key event carries — that one describes the keystroke it came
    /// with, and the compositor reports the latch only *after* the key that
    /// toggled it.
    pub caps_lock: bool,
}

impl Modifiers {
    /// Whether Ctrl, Alt or Super is held: the key is a shortcut, and what a
    /// key does by default — Tab moving the focus, Page Down paging — is not
    /// what it asks for.
    pub(crate) fn holds_a_shortcut(&self) -> bool {
        self.ctrl || self.alt || self.logo
    }
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
    /// Page Up, from the main block or the keypad
    PageUp,
    /// Page Down, from the main block or the keypad
    PageDown,
    /// A function key: `F(1)` is F1, up to `F(12)`
    F(u8),
    /// Character input (includes A-Z for Ctrl+A shortcuts)
    Char(char),
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Mouse/pointer moved
    MouseMove {
        at: Option<Point>,
        /// What is moving — see [`PointerKind`].
        pointer: PointerKind,
    },
    /// Mouse button pressed
    MouseDown {
        at: Option<Point>,
        button: MouseButton,
        /// What is pressing — see [`PointerKind`].
        pointer: PointerKind,
        /// Which press of a run of them this is: 1 for a single click, 2 for
        /// the second of a double click, 3 for a triple. The platform counts a
        /// press as the next of a run when it is the same button, within
        /// 400 ms and 5 px of the press before it, with no drag between; any
        /// other press starts a new run at 1. A finger always presses 1.
        count: u32,
    },
    /// Mouse button released
    MouseUp {
        at: Option<Point>,
        button: MouseButton,
    },
    /// Mouse/pointer entered the surface (with entry coordinates)
    MouseEnter { at: Option<Point> },
    /// Mouse/pointer left the surface
    MouseLeave,
    /// Scroll event (wheel, touchpad, or touchscreen)
    Scroll {
        /// Where the pointer is, if it is anywhere
        at: Option<Point>,
        /// Horizontal scroll delta in pixels (positive = right)
        delta_x: f32,
        /// Vertical scroll delta in pixels (positive = down)
        delta_y: f32,
        /// Source of the scroll event
        source: ScrollSource,
    },
    /// A continuous scroll gesture ended: the finger lifted.
    ///
    /// Sent for the sources that have an end — a touchpad or a touchscreen say
    /// so with `wl_pointer.axis_stop`, a wheel has nothing to say. It carries
    /// no delta, because nothing moved; what it carries is the one unguessed
    /// answer to when a momentum may begin.
    ScrollEnd {
        /// Where the pointer is, if it is anywhere
        at: Option<Point>,
    },
    /// Key pressed, or held long enough to repeat
    KeyDown {
        /// The key that was pressed
        key: Key,
        /// Current modifier state
        modifiers: Modifiers,
        /// Whether a held key produced this rather than a press. A repeat is
        /// delivered like any key-down — text editing wants every one — and
        /// something that acts once per press ignores the ones that say so.
        repeat: bool,
    },
    /// Key released
    KeyUp {
        /// The key that was released
        key: Key,
        /// Current modifier state
        modifiers: Modifiers,
    },
    /// The surface has the keyboard again, and the widget it had focused when
    /// the focus went to another surface has the focus back. Sent down the
    /// focus path, as a key is.
    FocusIn,
    /// The surface lost the keyboard. The focus stays where it was — a text
    /// input keeps it and draws no caret — and keys reach it again after the
    /// next `FocusIn`.
    FocusOut,
    /// The text a paste this widget asked for brought. Sent to that widget
    /// alone, on a later iteration than the one that asked.
    Pasted(crate::reactive::PastedText),
}

/// What a widget answers for an event routed to it.
///
/// Siblings are asked from the one drawn on top to the one drawn first, and
/// the first that answers anything but `Ignored` stops the walk: the siblings
/// beneath it are not asked (#636). Its ancestors still are, whatever it said,
/// and they decide by the same three answers whether the event goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    /// The point is not on this widget: the sibling beneath it is asked.
    Ignored,
    /// The point is on this widget and nothing here took the event. The
    /// siblings beneath are covered and are not asked; the ancestors still
    /// handle it, and a press nobody took still releases the focus.
    Hit,
    /// Taken: nothing else handles it.
    Handled,
}

impl Event {
    /// A pointer move at a place.
    ///
    /// The constructors exist for the positioned case only, and deliberately:
    /// it is the overwhelmingly common one, and the absent case has to stay
    /// spelled out. `Event::MouseMove { at: None }` says what it is; a
    /// `mouse_move_nowhere()` beside `mouse_move(x, y)` would make the two
    /// look like a pair of ordinary alternatives, which is exactly what the
    /// `Option` exists to prevent.
    pub fn mouse_move(x: f32, y: f32) -> Self {
        Event::MouseMove {
            at: Some(Point::new(x, y)),
            pointer: PointerKind::Mouse,
        }
    }

    /// A button pressed at a place, as the first press of a run. See
    /// [`mouse_move`](Self::mouse_move).
    pub fn mouse_down(x: f32, y: f32, button: MouseButton) -> Self {
        Event::MouseDown {
            at: Some(Point::new(x, y)),
            button,
            pointer: PointerKind::Mouse,
            count: 1,
        }
    }

    /// A finger moved to a place, as the fold synthesizes it. See
    /// [`mouse_move`](Self::mouse_move).
    pub fn finger_move(x: f32, y: f32) -> Self {
        Event::MouseMove {
            at: Some(Point::new(x, y)),
            pointer: PointerKind::Finger,
        }
    }

    /// A finger landed at a place, as the fold synthesizes it.
    ///
    /// No button to name: the primary finger presses
    /// [`Left`](MouseButton::Left) and nothing else ever does.
    pub fn finger_down(x: f32, y: f32) -> Self {
        Event::MouseDown {
            at: Some(Point::new(x, y)),
            button: MouseButton::Left,
            pointer: PointerKind::Finger,
            count: 1,
        }
    }

    /// A button released at a place. See [`mouse_move`](Self::mouse_move).
    pub fn mouse_up(x: f32, y: f32, button: MouseButton) -> Self {
        Event::MouseUp {
            at: Some(Point::new(x, y)),
            button,
        }
    }

    /// The pointer arriving at a place. See [`mouse_move`](Self::mouse_move).
    pub fn mouse_enter(x: f32, y: f32) -> Self {
        Event::MouseEnter {
            at: Some(Point::new(x, y)),
        }
    }

    /// A scroll at a place. See [`mouse_move`](Self::mouse_move).
    pub fn scroll(x: f32, y: f32, delta_x: f32, delta_y: f32, source: ScrollSource) -> Self {
        Event::Scroll {
            at: Some(Point::new(x, y)),
            delta_x,
            delta_y,
            source,
        }
    }

    /// A scroll gesture ending at a place. See [`mouse_move`](Self::mouse_move).
    pub fn scroll_end(x: f32, y: f32) -> Self {
        Event::ScrollEnd {
            at: Some(Point::new(x, y)),
        }
    }

    /// Where this event happened, if it happened anywhere.
    ///
    /// `None` for two different reasons, and a consumer wants the same thing
    /// from both: a keyboard or focus event never had a position, and a
    /// pointer event that descended into a subtree collapsed to nothing has
    /// lost the one it had. Either way there is no point to test against
    /// bounds, so nothing is under the pointer here.
    #[inline]
    pub fn coords(&self) -> Option<Point> {
        match self {
            Event::MouseMove { at, .. }
            | Event::MouseDown { at, .. }
            | Event::MouseUp { at, .. }
            | Event::MouseEnter { at }
            | Event::Scroll { at, .. }
            | Event::ScrollEnd { at } => *at,
            Event::MouseLeave
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::FocusIn
            | Event::FocusOut
            | Event::Pasted(_) => None,
        }
    }

    /// Whether it goes where the keyboard focus is: a key, or the surface
    /// gaining or losing the keyboard. The pointer's events go where the
    /// pointer is, and a paste to the widget that asked for it.
    pub(crate) fn follows_the_focus(&self) -> bool {
        match self {
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::FocusIn | Event::FocusOut => true,
            Event::MouseMove { .. }
            | Event::MouseDown { .. }
            | Event::MouseUp { .. }
            | Event::MouseEnter { .. }
            | Event::MouseLeave
            | Event::Scroll { .. }
            | Event::ScrollEnd { .. }
            | Event::Pasted(_) => false,
        }
    }

    /// The same event somewhere else, or nowhere.
    ///
    /// `None` is what a container passes down when its own transform cannot
    /// be undone. The event still travels — a press given up and a hover
    /// cleared are things only a delivered event can do — it simply arrives
    /// without a position, so every bounds test below answers no.
    pub fn with_coords(&self, at: Option<Point>) -> Self {
        match self {
            Event::MouseMove { pointer, .. } => Event::MouseMove {
                at,
                pointer: *pointer,
            },
            Event::MouseDown {
                button,
                pointer,
                count,
                ..
            } => Event::MouseDown {
                at,
                button: *button,
                pointer: *pointer,
                count: *count,
            },
            Event::MouseUp { button, .. } => Event::MouseUp {
                at,
                button: *button,
            },
            Event::MouseEnter { .. } => Event::MouseEnter { at },
            Event::Scroll {
                delta_x,
                delta_y,
                source,
                ..
            } => Event::Scroll {
                at,
                delta_x: *delta_x,
                delta_y: *delta_y,
                source: *source,
            },
            Event::ScrollEnd { .. } => Event::ScrollEnd { at },
            Event::MouseLeave => Event::MouseLeave,
            // Keyboard, focus and paste events never had one to replace.
            Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::FocusIn
            | Event::FocusOut
            | Event::Pasted(_) => self.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutHints {
    pub fill_width: bool,
    pub fill_height: bool,
    /// Hidden, as CSS's `display: none`: it takes no space, so a parent that
    /// sizes itself from its children leaves it out.
    pub hidden: bool,
    /// On its way out: removed from a dynamic children list and playing its
    /// exit, as [`Widget::is_exiting`] says. A
    /// [`ZStack`](crate::layout::ZStack) is not held open by it.
    pub leaving: bool,
    /// Where it sits in its parent, where it says so itself, as CSS's
    /// `align-self`. A [`ZStack`](crate::layout::ZStack) reads it in place of
    /// its own alignment.
    pub align_self: Option<crate::layout::Alignment>,
}

/// Where a widget draws itself and its children, against where it was laid
/// out: what [`Widget::placement`] answers.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The widget's own transform, about its pivot, in its own coordinates:
    /// its laid-out box is `(0, 0, width, height)` before it.
    pub own: crate::transform::Transform,
    /// How far it moves its children against where they were laid out — a
    /// scroll offset, which carries the content the other way.
    pub scroll: (f32, f32),
}

impl Placement {
    /// Drawn where it was laid out, with its children where they were.
    pub const NONE: Self = Self {
        own: crate::transform::Transform::IDENTITY,
        scroll: (0.0, 0.0),
    };
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

    /// Begin leaving: this widget has been removed from a dynamic children
    /// list, and plays whatever exit it declared before it is disposed.
    ///
    /// Returns whether it declared one. `false` — the default, and every
    /// widget that declares none — is torn down in the same pass, as a removed
    /// child always was. `true` keeps it in the tree, inert, until
    /// [`is_exiting`](Self::is_exiting) says the exit has settled.
    ///
    /// Only the removed widget itself is asked: an exit declared further down
    /// plays when *that* widget is the one removed.
    fn begin_exit(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let _ = (tree, id);
        false
    }

    /// Whether an exit begun by [`begin_exit`](Self::begin_exit) is still
    /// playing. Once it answers `false` the widget is disposed.
    fn is_exiting(&self) -> bool {
        false
    }

    /// Stop leaving: the key this widget was built for came back while its
    /// exit played, so it stays, and whatever the exit moved travels back to
    /// its declared value from where it is.
    fn cancel_exit(&mut self, tree: &mut Tree, id: WidgetId) {
        let _ = (tree, id);
    }

    /// The reactive scope this widget owns, if it owns one — a row of a
    /// dynamic list does. A leaving subtree pauses the effects and timers in
    /// it, and a reclaimed one resumes them.
    #[doc(hidden)]
    fn owned_scope(&self) -> Option<crate::reactive::__internal::OwnerId> {
        None
    }

    /// Publish how far this widget's paint will land outside its own bounds,
    /// before anything decides whether to paint it.
    ///
    /// Called by the framework through `tree::refresh_paint_reach`, which opens
    /// this widget's own `JobType::Paint` scope around it — so a signal read
    /// here belongs to this widget and a write to it repaints this widget,
    /// without reflowing anything. There is nothing to remember: read plainly.
    ///
    /// Called from the Paint job, and again from `Tree::layout_widget` after
    /// every layout — either can be the last pass to run. The Paint job is the
    /// frame's one chance to ask: a parent narrows its children to the visible rect at the top of its
    /// own paint, so by the time a child paints, the decision about it has been
    /// taken. A transform is the reason this exists — where a widget draws is
    /// not where it was laid out, and a parent culling by laid-out bounds drops
    /// a widget that has moved into view.
    ///
    /// Layout is the wrong place to answer it. A transform is a paint property
    /// and changing one must not reflow anything, so the answer is refreshed
    /// here, on the same signal write that schedules the repaint, in the pass
    /// that runs before paint. Blink resolves its transforms in a PrePaint walk
    /// between layout and paint for the same reason.
    ///
    /// Report it with [`Tree::set_own_paint_reach`]. The default reports
    /// nothing, which is right for a widget that draws inside its own box.
    ///
    /// Which pass to publish from follows what the reach depends on. A reach
    /// that moves with a layout-tracked signal cannot change without a layout,
    /// so `Text` and `TextInput` publish their stroke and shadow reach from
    /// `layout` and never implement this. A reach that moves with a paint-only
    /// one — a transform — has no layout to publish from, and this is where it
    /// goes. One channel, two schedules.
    fn refresh_paint_bounds(&self, tree: &mut Tree, id: WidgetId) {
        let _ = (tree, id);
    }

    /// Where this widget draws itself and its children, against where it was
    /// laid out: its own transform and the scroll offset it moves its children
    /// by. A widget that paints with a transform or scrolls answers it, the
    /// same numbers it paints and hit-tests with; the rest draw where they
    /// were laid out, which is the default.
    ///
    /// Asked only for the widgets a [`WidgetRef`](crate::widget_ref::WidgetRef)
    /// points at and their ancestors, whenever a frame may have moved them, so
    /// that [`WidgetRef::rect`](crate::widget_ref::WidgetRef::rect) is where
    /// the widget is drawn.
    #[doc(hidden)]
    fn placement(&self, tree: &Tree, id: WidgetId) -> Placement {
        let _ = (tree, id);
        Placement::NONE
    }

    /// What a parent laying this widget out asks of it before it does. A
    /// widget that plays an exit is leaving whatever else it answers.
    fn layout_hints(&self) -> LayoutHints {
        LayoutHints {
            leaving: self.is_exiting(),
            ..LayoutHints::default()
        }
    }

    /// Measure this widget, and place its children.
    ///
    /// Called by the framework through `Tree::layout_widget` or
    /// `LayoutCtx::layout_child`, which is what decides *whether* it runs, what
    /// its reads are attributed to, which pass it is in, and what becomes of
    /// what it answers. A widget computes a size and places its children; it
    /// does not open a tracking scope, write a skip check, or cache anything.
    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size;
    /// Draw this widget.
    ///
    /// Called by the framework through `Tree::paint_widget` or
    /// `PaintContext::paint_child`, which is what attributes the reads made
    /// here to *this* widget, decides whether it runs at all, and places what
    /// it draws. A widget draws; it does not open a tracking scope, test a
    /// cull rect or reach for a cache.
    ///
    /// `ctx.id()` is which widget this is and `ctx.tree()` is the tree it is
    /// in, for the bounds and the children a drawing needs.
    fn paint(&self, ctx: &mut PaintContext);
    /// Handle an event routed to this widget.
    ///
    /// Not every event reaches every widget. One from the pointer reaches the
    /// widgets that can be under its point, and those it owes one — the widget
    /// it just left, the one a press landed on. A key reaches the widget with
    /// the focus and the containers on the way to it, then those listening
    /// with `on_key_down` or `on_key_up`. A container hands an event to its
    /// children through `Tree::event_targets`; a widget that holds children of
    /// its own and offers them events by hand steps outside that routing.
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        let _ = (tree, id, event);
        EventResponse::Ignored
    }

    /// Register this widget's pending children with the arena.
    ///
    /// Called during widget tree registration to recursively register all
    /// children before the tree is used for layout. Containers should override
    /// this to register their pending children.
    ///
    /// Default implementation does nothing (leaf widgets have no children).
    fn register_children(&mut self, _tree: &mut Tree, _id: WidgetId) {}

    /// Type-erase this widget into a boxed trait object.
    ///
    /// Useful when returning different widget types from conditional branches:
    /// ```no_run
    /// # use guido::prelude::*;
    /// # let condition = true;
    /// # let (widget_a, widget_b) = (text("a"), text("b"));
    /// let _child = if condition {
    ///     widget_a.into_any()
    /// } else {
    ///     widget_b.into_any()
    /// };
    /// ```
    fn into_any(self) -> AnyWidget
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

#[cfg(test)]
mod rect_tests {
    use super::*;

    /// `outset_beyond` is what every cull test and every damage rect is built
    /// on, and it was only ever reached through something else.
    #[test]
    fn a_rect_reports_how_far_it_stands_outside_another() {
        let inner = Rect::new(0.0, 0.0, 100.0, 100.0);
        let side = |left, top, right, bottom| Outsets {
            left,
            top,
            right,
            bottom,
        };

        assert_eq!(inner.outset_beyond(inner), Outsets::ZERO, "itself");
        assert_eq!(
            inner.inset(10.0).outset_beyond(inner),
            Outsets::ZERO,
            "wholly inside stands outside by nothing"
        );

        // Each side on its own, so a sign or a term swapped in one of the four
        // cannot hide behind the others — and a box moved one way stands
        // outside on that side alone.
        let moved = |dx, dy| inner.offset(dx, dy).outset_beyond(inner);
        assert_eq!(moved(-30.0, 0.0), side(30.0, 0.0, 0.0, 0.0), "left");
        assert_eq!(moved(0.0, -30.0), side(0.0, 30.0, 0.0, 0.0), "top");
        assert_eq!(moved(30.0, 0.0), side(0.0, 0.0, 30.0, 0.0), "right");
        assert_eq!(moved(0.0, 30.0), side(0.0, 0.0, 0.0, 30.0), "bottom");

        assert_eq!(
            inner.outset(7.0).outset_beyond(inner),
            Outsets::all(7.0),
            "all four"
        );
        assert_eq!(
            Rect::new(-5.0, -40.0, 110.0, 180.0).outset_beyond(inner),
            side(5.0, 40.0, 5.0, 40.0),
            "each its own"
        );
    }

    /// Growing by what a rect stands outside another gives back the outer
    /// rect, side by side.
    #[test]
    fn growing_by_the_outsets_beyond_a_rect_reaches_the_outer_one() {
        let inner = Rect::new(10.0, 20.0, 100.0, 50.0);
        let outer = Rect::new(4.0, -10.0, 130.0, 80.0);
        assert_eq!(inner.outset_by(outer.outset_beyond(inner)), outer);
        assert_eq!(inner.outset_by(Outsets::all(3.0)), inner.outset(3.0));
    }
}

/// A type-erased widget. Shorthand for `Box<dyn Widget>`.
pub type AnyWidget = Box<dyn Widget>;

impl Widget for Box<dyn Widget> {
    /// Already erased: boxing it again would put a second pointer between
    /// the tree and the widget, for every call it ever makes.
    fn into_any(self) -> AnyWidget {
        self
    }

    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        (**self).advance_animations(tree, id)
    }
    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        (**self).reconcile_children(tree, id)
    }
    fn begin_exit(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        (**self).begin_exit(tree, id)
    }
    fn is_exiting(&self) -> bool {
        (**self).is_exiting()
    }
    fn cancel_exit(&mut self, tree: &mut Tree, id: WidgetId) {
        (**self).cancel_exit(tree, id)
    }
    fn owned_scope(&self) -> Option<crate::reactive::__internal::OwnerId> {
        (**self).owned_scope()
    }
    fn refresh_paint_bounds(&self, tree: &mut Tree, id: WidgetId) {
        (**self).refresh_paint_bounds(tree, id)
    }
    fn placement(&self, tree: &Tree, id: WidgetId) -> Placement {
        (**self).placement(tree, id)
    }
    fn layout_hints(&self) -> LayoutHints {
        (**self).layout_hints()
    }
    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        (**self).layout(ctx, constraints)
    }
    fn paint(&self, ctx: &mut PaintContext) {
        (**self).paint(ctx)
    }
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        (**self).event(tree, id, event)
    }
    fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {
        (**self).register_children(tree, id)
    }
}

#[doc(hidden)]
pub struct WidgetHandler;

/// A closure returning any widget is a render prop's handler too: the setter
/// boxes what it returns, so `fn(P) -> impl Widget` takes a closure that
/// builds a `Container` as readily as one that builds a component — as
/// Leptos erases to `AnyView` and Dioxus to `Element`.
macro_rules! erases_to_any_widget {
    ($(($($arg:ident),*)),* $(,)?) => {$(
        impl<$($arg: 'static,)* W: Widget + 'static, F: Fn($($arg),*) -> W + 'static>
            IntoHandler<($($arg,)*), WidgetHandler, AnyWidget> for F
        {
            #[allow(non_snake_case)]
            fn into_handler(self) -> Option<Rc<dyn Fn(($($arg,)*)) -> AnyWidget>> {
                Some(Rc::new(move |($($arg,)*)| self($($arg),*).into_any()))
            }

            fn into_callback_prop(self) -> Option<Callback<($($arg,)*), AnyWidget>> {
                self.into_handler().map(Callback::from_rc)
            }
        }
    )*};
}

erases_to_any_widget!((), (A), (A, B), (A, B, C));

#[cfg(test)]
mod tests {
    use crate::widgets::Corners;

    /// A widget already erased stays in its one box: a render prop's
    /// `AnyWidget` handed to `.child` is not boxed again.
    #[test]
    fn an_erased_widget_is_not_boxed_twice() {
        let erased: super::AnyWidget = crate::widgets::container().into_any();
        let at = &*erased as *const dyn super::Widget as *const ();
        let again = erased.into_any();
        assert_eq!(&*again as *const dyn super::Widget as *const (), at);
    }

    /// Two rects share an area only where they overlap on both axes: edges
    /// that touch share none, and neither does an overlap along one axis
    /// with a gap along the other.
    #[test]
    fn an_intersection_is_an_area_both_rects_hold() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);

        let inside = a.intersection(&Rect::new(5.0, 2.0, 10.0, 4.0));
        assert_eq!(
            inside.map(|r| (r.x, r.y, r.width, r.height)),
            Some((5.0, 2.0, 5.0, 4.0))
        );
        assert!(
            a.intersection(&Rect::new(10.0, 0.0, 5.0, 10.0)).is_none(),
            "touching on the right"
        );
        assert!(
            a.intersection(&Rect::new(0.0, 10.0, 10.0, 5.0)).is_none(),
            "touching below"
        );
        assert!(
            a.intersection(&Rect::new(2.0, 20.0, 4.0, 4.0)).is_none(),
            "overlapping across, apart down"
        );
        assert!(
            a.intersection(&Rect::new(20.0, 2.0, 4.0, 4.0)).is_none(),
            "overlapping down, apart across"
        );
    }

    /// The shape that is drawn is the shape that answers a click: a corner
    /// rounded only at the top must let a point through at the bottom, where
    /// the box is square.
    #[test]
    fn a_click_asks_the_corner_it_landed_in() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        let radii = Corners::rounded([20.0, 0.0]); // top pair rounded

        assert!(
            !r.contains_shape(2.0, 2.0, radii),
            "the top-left corner is cut away"
        );
        assert!(
            !r.contains_shape(98.0, 2.0, radii),
            "and so is the top-right"
        );
        assert!(
            r.contains_shape(2.0, 98.0, radii),
            "the bottom-left is square, so the same offset is inside"
        );
        assert!(
            r.contains_shape(98.0, 98.0, radii),
            "and the bottom-right too"
        );
    }

    /// The picture that found this: a bevelled box lit up while the pointer
    /// was well outside the drawn octagon, because the test was circular
    /// whatever the shape.
    ///
    /// A bevel cuts the corner with a chord, an ordinary radius with an arc,
    /// and the arc bulges toward the corner — so between the two lies a
    /// wedge that one keeps and the other does not. That wedge is what a
    /// circular test got wrong, and it is what this pins.
    #[test]
    fn a_bevel_is_clicked_as_a_straight_cut() {
        let r = Rect::new(0.0, 0.0, 200.0, 200.0);
        // Corner radius 80: the chord runs from (0, 80) to (80, 0), and the
        // arc of the same radius passes about 23 units from the corner.
        // (30, 30) sits between them.
        assert!(
            !r.contains_shape(30.0, 30.0, Corners::bevel(80.0)),
            "the chord has already cut this away"
        );
        assert!(
            r.contains_shape(30.0, 30.0, Corners::rounded(80.0)),
            "while the arc of the same radius still keeps it"
        );

        // Both agree well past the cut, and well inside it.
        assert!(!r.contains_shape(5.0, 5.0, Corners::bevel(80.0)));
        assert!(r.contains_shape(100.0, 100.0, Corners::bevel(80.0)));
    }

    /// The far edge belongs to the next box along, corners or no corners —
    /// otherwise two containers in a row both answer for the column they
    /// share.
    #[test]
    fn the_far_edge_is_not_inside() {
        let r = Rect::new(0.0, 0.0, 100.0, 40.0);
        for corners in [Corners::SQUARE, Corners::rounded(8.0)] {
            assert!(
                !r.contains_shape(100.0, 20.0, corners),
                "the right edge is out"
            );
            assert!(
                !r.contains_shape(50.0, 40.0, corners),
                "the bottom edge is out"
            );
            assert!(r.contains_shape(0.0, 20.0, corners), "the left edge is in");
            assert!(
                r.contains_shape(99.0, 20.0, corners),
                "and so is the pixel before"
            );
        }
    }

    /// A scoop curves *inward*, so the corner is hollow: a point that a
    /// rounded corner would keep is outside a scooped one.
    #[test]
    fn a_scoop_is_clicked_as_a_hollow() {
        let r = Rect::new(0.0, 0.0, 200.0, 200.0);
        assert!(
            !r.contains_shape(10.0, 10.0, Corners::scoop(80.0)),
            "the hollow is not part of the box"
        );
        assert!(r.contains_shape(100.0, 100.0, Corners::scoop(80.0)));
    }

    /// A uniform radius still behaves as it always did.
    #[test]
    fn a_uniform_radius_cuts_all_four_corners() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        let radii = Corners::rounded(20.0);
        for (x, y) in [(2.0, 2.0), (98.0, 2.0), (2.0, 98.0), (98.0, 98.0)] {
            assert!(!r.contains_shape(x, y, radii), "({x}, {y}) is cut away");
        }
        assert!(r.contains_shape(50.0, 50.0, radii), "the middle is not");
    }

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

    /// The two-value shorthand is CSS's: vertical first, then horizontal.
    #[test]
    fn test_padding_two_value_shorthand() {
        let padding = Padding::from([30.0, 20.0]);
        assert_eq!(padding.top, 30.0);
        assert_eq!(padding.right, 20.0);
        assert_eq!(padding.bottom, 30.0);
        assert_eq!(padding.left, 20.0);
    }

    #[test]
    fn test_padding_totals_are_sums() {
        let padding = Padding::from([10.0, 15.0]);
        assert_eq!(padding.horizontal_total(), 30.0);
        assert_eq!(padding.vertical_total(), 20.0);
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
        let padding = Padding::all(8.0).with_top(20.0).with_left(0.0);
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
