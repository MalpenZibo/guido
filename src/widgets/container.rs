use std::sync::Arc;

use crate::layout::{Constraints, Size};
use crate::reactive::{IntoMaybeDyn, MaybeDyn};
use crate::renderer::primitives::GradientDir;
use crate::renderer::PaintContext;

use super::widget::{
    Color, Event, EventResponse, MouseButton, Padding, Rect, ScrollSource, Widget,
};

/// Callback for click events
pub type ClickCallback = Arc<dyn Fn() + Send + Sync>;
/// Callback for hover events (bool = is_hovered)
pub type HoverCallback = Arc<dyn Fn(bool) + Send + Sync>;
/// Callback for scroll events (delta_x, delta_y, source)
pub type ScrollCallback = Arc<dyn Fn(f32, f32, ScrollSource) + Send + Sync>;

/// Gradient direction for linear gradients
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientDirection {
    /// Left to right
    Horizontal,
    /// Top to bottom
    Vertical,
    /// Top-left to bottom-right
    Diagonal,
    /// Top-right to bottom-left
    DiagonalReverse,
}

/// Linear gradient definition
#[derive(Debug, Clone)]
pub struct LinearGradient {
    pub start_color: Color,
    pub end_color: Color,
    pub direction: GradientDirection,
}

impl LinearGradient {
    pub fn new(start: Color, end: Color, direction: GradientDirection) -> Self {
        Self {
            start_color: start,
            end_color: end,
            direction,
        }
    }

    pub fn horizontal(start: Color, end: Color) -> Self {
        Self::new(start, end, GradientDirection::Horizontal)
    }

    pub fn vertical(start: Color, end: Color) -> Self {
        Self::new(start, end, GradientDirection::Vertical)
    }
}

/// Border definition
#[derive(Debug, Clone, Copy)]
pub struct Border {
    pub width: f32,
    pub color: Color,
}

impl Border {
    pub fn new(width: f32, color: Color) -> Self {
        Self { width, color }
    }
}

pub struct Container {
    child: Option<Box<dyn Widget>>,
    padding: MaybeDyn<Padding>,
    background: MaybeDyn<Color>,
    gradient: Option<LinearGradient>,
    corner_radius: MaybeDyn<f32>,
    corner_curvature: MaybeDyn<f32>,
    border: Option<Border>,
    min_width: Option<MaybeDyn<f32>>,
    min_height: Option<MaybeDyn<f32>>,
    bounds: Rect,

    // Event callbacks
    on_click: Option<ClickCallback>,
    on_hover: Option<HoverCallback>,
    on_scroll: Option<ScrollCallback>,

    // Internal state for event handling
    is_hovered: bool,
    is_pressed: bool,

    // Ripple effect state
    ripple_enabled: bool,
    ripple_center: Option<(f32, f32)>,
    ripple_progress: f32,
    ripple_color: Color,
    ripple_from_click: bool, // true = click (slower), false = hover (faster)
    ripple_is_exit: bool,    // true = exit ripple (fades out quickly)
    last_mouse_pos: Option<(f32, f32)>, // Track last mouse position for exit ripple
}

impl Container {
    pub fn new() -> Self {
        Self {
            child: None,
            padding: MaybeDyn::Static(Padding::default()),
            background: MaybeDyn::Static(Color::TRANSPARENT),
            gradient: None,
            corner_radius: MaybeDyn::Static(0.0),
            corner_curvature: MaybeDyn::Static(1.0), // Default K=1 (circular/round)
            border: None,
            min_width: None,
            min_height: None,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            on_click: None,
            on_hover: None,
            on_scroll: None,
            is_hovered: false,
            is_pressed: false,
            ripple_enabled: false,
            ripple_center: None,
            ripple_progress: 0.0,
            ripple_color: Color::rgba(1.0, 1.0, 1.0, 0.3),
            ripple_from_click: false,
            ripple_is_exit: false,
            last_mouse_pos: None,
        }
    }

    pub fn child(mut self, widget: impl Widget + 'static) -> Self {
        self.child = Some(Box::new(widget));
        self
    }

    pub fn padding(mut self, value: impl IntoMaybeDyn<f32>) -> Self {
        let value = value.into_maybe_dyn();
        self.padding = MaybeDyn::Dynamic(std::sync::Arc::new(move || Padding::all(value.get())));
        self
    }

    pub fn padding_xy(
        mut self,
        horizontal: impl IntoMaybeDyn<f32>,
        vertical: impl IntoMaybeDyn<f32>,
    ) -> Self {
        let h = horizontal.into_maybe_dyn();
        let v = vertical.into_maybe_dyn();
        self.padding = MaybeDyn::Dynamic(std::sync::Arc::new(move || Padding {
            left: h.get(),
            right: h.get(),
            top: v.get(),
            bottom: v.get(),
        }));
        self
    }

    pub fn background(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.background = color.into_maybe_dyn();
        self
    }

    pub fn corner_radius(mut self, radius: impl IntoMaybeDyn<f32>) -> Self {
        self.corner_radius = radius.into_maybe_dyn();
        self
    }

    /// Set the corner curvature using CSS K-value system
    /// - K = -1.0: scoop (concave inward curves)
    /// - K = 0.0: bevel (straight diagonal cuts)
    /// - K = 1.0: round/circular (default)
    /// - K = 2.0: squircle (iOS-style smooth squares)
    ///
    /// Internally converted to n = 2^K for rendering
    pub fn corner_curvature(mut self, curvature: impl IntoMaybeDyn<f32>) -> Self {
        self.corner_curvature = curvature.into_maybe_dyn();
        self
    }

    /// Convenience: Set squircle/iOS-style corners (K = 2.0 → n = 4.0)
    pub fn squircle(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(2.0);
        self
    }

    /// Convenience: Set concave/scooped corners (K = -1.0 → n = 0.5)
    pub fn scoop(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(-1.0);
        self
    }

    /// Convenience: Set beveled corners (K = 0.0 → n = 1.0)
    pub fn bevel(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(0.0);
        self
    }

    /// Set a border with the given width and color
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border = Some(Border::new(width, color));
        self
    }

    /// Set a linear gradient background (overrides solid background)
    pub fn gradient(mut self, gradient: LinearGradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Convenience: horizontal gradient from start to end color
    pub fn gradient_horizontal(mut self, start: Color, end: Color) -> Self {
        self.gradient = Some(LinearGradient::horizontal(start, end));
        self
    }

    /// Convenience: vertical gradient from start to end color
    pub fn gradient_vertical(mut self, start: Color, end: Color) -> Self {
        self.gradient = Some(LinearGradient::vertical(start, end));
        self
    }

    pub fn min_width(mut self, width: impl IntoMaybeDyn<f32>) -> Self {
        self.min_width = Some(width.into_maybe_dyn());
        self
    }

    pub fn min_height(mut self, height: impl IntoMaybeDyn<f32>) -> Self {
        self.min_height = Some(height.into_maybe_dyn());
        self
    }

    /// Set a callback for click events (mouse button released inside bounds)
    pub fn on_click<F: Fn() + Send + Sync + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Arc::new(callback));
        self
    }

    /// Set a callback for hover events (mouse enter/leave)
    pub fn on_hover<F: Fn(bool) + Send + Sync + 'static>(mut self, callback: F) -> Self {
        self.on_hover = Some(Arc::new(callback));
        self
    }

    /// Set a callback for scroll events
    pub fn on_scroll<F: Fn(f32, f32, ScrollSource) + Send + Sync + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_scroll = Some(Arc::new(callback));
        self
    }

    /// Enable ripple effect on hover
    pub fn ripple(mut self) -> Self {
        self.ripple_enabled = true;
        self
    }

    /// Enable ripple effect with a custom color
    pub fn ripple_with_color(mut self, color: Color) -> Self {
        self.ripple_enabled = true;
        self.ripple_color = color;
        self
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn layout(&mut self, constraints: Constraints) -> Size {
        let padding = self.padding.get();

        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            max_width: (constraints.max_width - padding.horizontal()).max(0.0),
            max_height: (constraints.max_height - padding.vertical()).max(0.0),
        };

        let child_size = if let Some(ref mut child) = self.child {
            child.layout(child_constraints)
        } else {
            Size::zero()
        };

        let mut width = child_size.width + padding.horizontal();
        let mut height = child_size.height + padding.vertical();

        if let Some(ref min_w) = self.min_width {
            width = width.max(min_w.get());
        }
        if let Some(ref min_h) = self.min_height {
            height = height.max(min_h.get());
        }

        let size = Size::new(
            width.max(constraints.min_width).min(constraints.max_width),
            height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        if let Some(ref mut child) = self.child {
            child.set_origin(self.bounds.x + padding.left, self.bounds.y + padding.top);
        }

        // Advance ripple animation each frame (layout is called every frame)
        if self.ripple_enabled && self.ripple_center.is_some() {
            if self.ripple_progress < 1.0 {
                // Exit ripple: very fast (~0.2s), Click ripple: slower (~0.6s), Hover ripple: faster (~0.3s)
                let speed = if self.ripple_is_exit {
                    0.20
                } else if self.ripple_from_click {
                    0.08
                } else {
                    0.12
                };
                self.ripple_progress = (self.ripple_progress + speed).min(1.0);
            } else if self.ripple_from_click || self.ripple_is_exit {
                // Auto-reset click ripples and exit ripples when complete
                self.ripple_center = None;
                self.ripple_progress = 0.0;
                self.ripple_from_click = false;
                self.ripple_is_exit = false;
            }
        }

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let background = self.background.get();
        let corner_radius = self.corner_radius.get();
        let corner_curvature = self.corner_curvature.get();

        // Draw background first (gradient or solid color)
        if let Some(ref gradient) = self.gradient {
            let direction = match gradient.direction {
                GradientDirection::Horizontal => GradientDir::Horizontal,
                GradientDirection::Vertical => GradientDir::Vertical,
                GradientDirection::Diagonal => GradientDir::Diagonal,
                GradientDirection::DiagonalReverse => GradientDir::DiagonalReverse,
            };
            ctx.draw_gradient_rect_with_curvature(
                self.bounds,
                gradient.start_color,
                gradient.end_color,
                direction,
                corner_radius,
                corner_curvature,
            );
        } else if background.a > 0.0 {
            ctx.draw_rounded_rect_with_curvature(
                self.bounds,
                background,
                corner_radius,
                corner_curvature,
            );
        }

        // Draw border frame on top (just the outline, not a filled rect)
        if let Some(ref border) = self.border {
            ctx.draw_border_frame_with_curvature(
                self.bounds,
                border.color,
                corner_radius,
                border.width,
                corner_curvature,
            );
        }

        // Draw child
        if let Some(ref child) = self.child {
            child.paint(ctx);
        }

        // Draw ripple effect on top of everything (including text) using overlay
        if self.ripple_enabled {
            if let Some((cx, cy)) = self.ripple_center {
                // Calculate the maximum radius needed to cover the entire container from the entry point
                let dx1 = cx - self.bounds.x;
                let dx2 = (self.bounds.x + self.bounds.width) - cx;
                let dy1 = cy - self.bounds.y;
                let dy2 = (self.bounds.y + self.bounds.height) - cy;
                let max_radius = (dx1.max(dx2).powi(2) + dy1.max(dy2).powi(2)).sqrt();

                // Animate radius from 0 to max_radius based on progress
                let current_radius = max_radius * self.ripple_progress;

                // Fade out as the ripple expands (exit ripples fade faster)
                let alpha = if self.ripple_is_exit {
                    // Exit ripple: aggressive fade out
                    self.ripple_color.a * (1.0 - self.ripple_progress).powi(2)
                } else {
                    // Normal ripple: gradual fade
                    self.ripple_color.a * (1.0 - self.ripple_progress * 0.5)
                };

                if current_radius > 0.0 && alpha > 0.0 {
                    let ripple_color = Color::rgba(
                        self.ripple_color.r,
                        self.ripple_color.g,
                        self.ripple_color.b,
                        alpha,
                    );
                    // Use overlay so ripple appears on top of text
                    ctx.draw_overlay_circle_clipped_with_curvature(
                        cx,
                        cy,
                        current_radius,
                        ripple_color,
                        self.bounds,
                        corner_radius,
                        corner_curvature,
                    );
                }
            }
        }
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        // First, let children handle the event
        if let Some(ref mut child) = self.child {
            if child.event(event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }

        // Then handle our own event callbacks
        match event {
            Event::MouseEnter { x, y } => {
                if self.bounds.contains(*x, *y) {
                    self.is_hovered = true;
                    self.last_mouse_pos = Some((*x, *y));
                    if let Some(ref callback) = self.on_hover {
                        callback(true);
                    }
                    // Start hover ripple
                    if self.ripple_enabled && self.ripple_center.is_none() {
                        self.ripple_center = Some((*x, *y));
                        self.ripple_progress = 0.0;
                        self.ripple_from_click = false;
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseMove { x, y } => {
                let was_hovered = self.is_hovered;
                self.is_hovered = self.bounds.contains(*x, *y);

                // Track mouse position for exit ripple
                if self.is_hovered {
                    self.last_mouse_pos = Some((*x, *y));
                }

                if was_hovered != self.is_hovered {
                    if let Some(ref callback) = self.on_hover {
                        callback(self.is_hovered);
                    }

                    if self.is_hovered {
                        // Start hover ripple when entering
                        if self.ripple_enabled && self.ripple_center.is_none() {
                            self.ripple_center = Some((*x, *y));
                            self.ripple_progress = 0.0;
                            self.ripple_from_click = false;
                        }
                    } else {
                        // Start exit ripple when leaving (moving to another container)
                        if self.ripple_enabled && !self.ripple_from_click {
                            if let Some((lx, ly)) = self.last_mouse_pos {
                                self.ripple_center = Some((lx, ly));
                                self.ripple_progress = 0.0;
                                self.ripple_is_exit = true;
                            }
                        }
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseDown { x, y, button } => {
                if self.bounds.contains(*x, *y) && *button == MouseButton::Left {
                    self.is_pressed = true;
                    // Start click ripple (overrides hover ripple)
                    if self.ripple_enabled {
                        self.ripple_center = Some((*x, *y));
                        self.ripple_progress = 0.0;
                        self.ripple_from_click = true;
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseUp { x, y, button } => {
                if self.is_pressed && *button == MouseButton::Left {
                    self.is_pressed = false;
                    if self.bounds.contains(*x, *y) {
                        if let Some(ref callback) = self.on_click {
                            callback();
                        }
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseLeave => {
                let was_hovered = self.is_hovered;
                if self.is_hovered {
                    self.is_hovered = false;
                    if let Some(ref callback) = self.on_hover {
                        callback(false);
                    }
                }
                self.is_pressed = false;
                // Start exit ripple only if we were actually hovered
                if was_hovered && self.ripple_enabled && !self.ripple_from_click {
                    if let Some((x, y)) = self.last_mouse_pos {
                        self.ripple_center = Some((x, y));
                        self.ripple_progress = 0.0;
                        self.ripple_is_exit = true;
                    }
                }
            }
            Event::Scroll {
                x,
                y,
                delta_x,
                delta_y,
                source,
            } => {
                if self.bounds.contains(*x, *y) {
                    if let Some(ref callback) = self.on_scroll {
                        callback(*delta_x, *delta_y, *source);
                        return EventResponse::Handled;
                    }
                }
            }
        }

        EventResponse::Ignored
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.bounds.x = x;
        self.bounds.y = y;

        let padding = self.padding.get();
        if let Some(ref mut child) = self.child {
            child.set_origin(x + padding.left, y + padding.top);
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }
}

pub fn container() -> Container {
    Container::new()
}
