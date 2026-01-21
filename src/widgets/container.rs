use std::sync::Arc;

use crate::layout::{Constraints, Size};
use crate::reactive::{request_animation_frame, ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::primitives::{GradientDir, Shadow};
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
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,
    child: Option<Box<dyn Widget>>,
    padding: MaybeDyn<Padding>,
    background: MaybeDyn<Color>,
    gradient: Option<LinearGradient>,
    corner_radius: MaybeDyn<f32>,
    corner_curvature: MaybeDyn<f32>,
    border: Option<Border>,
    elevation: MaybeDyn<f32>,
    min_width: Option<MaybeDyn<f32>>,
    min_height: Option<MaybeDyn<f32>>,
    bounds: Rect,

    // Cached values for change detection (Phase 3)
    cached_padding: Padding,
    cached_background: Color,
    cached_corner_radius: f32,
    cached_corner_curvature: f32,
    cached_elevation: f32,

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

    // Click ripple effect (separate from hover ripple)
    click_ripple_center: Option<(f32, f32)>,
    click_ripple_progress: f32,
    click_ripple_reversing: bool, // true when animating backwards on release
    click_ripple_release_pos: Option<(f32, f32)>, // position where mouse was released
}

impl Container {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            child: None,
            padding: MaybeDyn::Static(Padding::default()),
            background: MaybeDyn::Static(Color::TRANSPARENT),
            gradient: None,
            corner_radius: MaybeDyn::Static(0.0),
            corner_curvature: MaybeDyn::Static(1.0), // Default K=1 (circular/round)
            border: None,
            elevation: MaybeDyn::Static(0.0),
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
            click_ripple_center: None,
            click_ripple_progress: 0.0,
            click_ripple_reversing: false,
            click_ripple_release_pos: None,
            // Initialize cached values
            cached_padding: Padding::default(),
            cached_background: Color::TRANSPARENT,
            cached_corner_radius: 0.0,
            cached_corner_curvature: 1.0,
            cached_elevation: 0.0,
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

    /// Set the elevation level (0 = no shadow, higher = more elevated)
    /// Automatically computes shadow offset, blur, and color based on the level
    pub fn elevation(mut self, level: impl IntoMaybeDyn<f32>) -> Self {
        self.elevation = level.into_maybe_dyn();
        self
    }

    /// Advance animation state (ripple effects)
    fn advance_animations(&mut self) {
        // Advance hover ripple animation
        if self.ripple_enabled && self.ripple_center.is_some() {
            if self.ripple_progress < 1.0 {
                // Animation in progress, request next frame
                request_animation_frame();

                // Exit ripple: very fast (~0.2s), Hover ripple: faster (~0.3s)
                let speed = if self.ripple_is_exit {
                    0.20
                } else {
                    0.12
                };
                self.ripple_progress = (self.ripple_progress + speed).min(1.0);
            } else if self.ripple_is_exit {
                // Exit ripple complete, still need one more frame to fade out
                request_animation_frame();

                // Auto-reset exit ripples when complete
                self.ripple_center = None;
                self.ripple_progress = 0.0;
                self.ripple_is_exit = false;
            }
            // Hover ripples persist at 100% progress while hovering (no animation needed)
        }

        // Advance click ripple animation
        if self.ripple_enabled && self.click_ripple_center.is_some() {
            if self.click_ripple_reversing {
                // Animation in progress (reversing), request next frame
                request_animation_frame();

                // Reverse animation: contract back to 0
                if self.click_ripple_progress > 0.0 {
                    // Reverse faster than expand for snappy feel (~0.3s)
                    let speed = 0.15;
                    self.click_ripple_progress = (self.click_ripple_progress - speed).max(0.0);
                } else {
                    // Animation complete, reset everything
                    self.click_ripple_center = None;
                    self.click_ripple_progress = 0.0;
                    self.click_ripple_reversing = false;
                    self.click_ripple_release_pos = None;
                }
            } else if self.click_ripple_progress < 1.0 {
                // Animation in progress (expanding), request next frame
                request_animation_frame();

                // Forward animation: expand to 100%
                let speed = 0.08;
                self.click_ripple_progress = (self.click_ripple_progress + speed).min(1.0);
            }
            // Click ripple stays at 100% until MouseUp triggers reverse (no animation needed)
        }
    }
}

/// Convert elevation level to shadow parameters
/// Returns: Shadow struct with offset, blur, spread, and color
/// Based on Material Design elevation specifications
fn elevation_to_shadow(level: f32) -> Shadow {
    if level <= 0.0 {
        return Shadow::none();
    }

    // Material Design elevation mapping (CSS-like values)
    // Offset and blur scale with elevation, but stay reasonable
    let (offset_y, blur, alpha) = match level as i32 {
        1 => (1.0, 3.0, 0.12),
        2 => (2.0, 4.0, 0.16),
        3 => (3.0, 6.0, 0.19),
        4 => (4.0, 8.0, 0.20),
        5 => (6.0, 10.0, 0.22),
        _ => {
            // For levels > 5, scale gradually
            let offset = (level * 1.2).min(12.0);
            let blur = (level * 2.0).min(24.0);
            let alpha = (0.12 + level * 0.02).min(0.25);
            (offset, blur, alpha)
        }
    };

    Shadow::new((0.0, offset_y), blur, 0.0, Color::rgba(0.0, 0.0, 0.0, alpha))
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Always advance animations first (before early return)
        self.advance_animations();

        // Phase 3: Check which properties actually changed
        let padding = self.padding.get();
        let background = self.background.get();
        let corner_radius = self.corner_radius.get();
        let corner_curvature = self.corner_curvature.get();
        let elevation = self.elevation.get();

        // Detect layout-affecting changes
        let padding_changed = padding.top != self.cached_padding.top
            || padding.right != self.cached_padding.right
            || padding.bottom != self.cached_padding.bottom
            || padding.left != self.cached_padding.left;

        // Detect paint-only changes
        let visual_changed = background != self.cached_background
            || corner_radius != self.cached_corner_radius
            || corner_curvature != self.cached_corner_curvature
            || elevation != self.cached_elevation;

        let child_needs_layout = self.child.as_ref().map_or(false, |c| c.needs_layout());
        let has_animations = self.ripple_center.is_some() || self.click_ripple_center.is_some();

        // If only visual properties changed (no layout changes), downgrade to paint-only
        if self.needs_layout() && !padding_changed && !child_needs_layout && visual_changed {
            self.dirty_flags = ChangeFlags::NEEDS_PAINT;
        }

        // Only do layout if: layout properties changed, child needs it, or animating
        let needs_layout = self.needs_layout() || padding_changed || child_needs_layout || has_animations;

        if !needs_layout {
            // No layout needed, but update cached values if visual properties changed
            if visual_changed {
                self.cached_background = background;
                self.cached_corner_radius = corner_radius;
                self.cached_corner_curvature = corner_curvature;
                self.cached_elevation = elevation;
            }
            // Return cached size
            return Size::new(self.bounds.width, self.bounds.height);
        }

        // Update all cached values
        self.cached_padding = padding;
        self.cached_background = background;
        self.cached_corner_radius = corner_radius;
        self.cached_corner_curvature = corner_curvature;
        self.cached_elevation = elevation;

        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            max_width: (constraints.max_width - padding.horizontal()).max(0.0),
            max_height: (constraints.max_height - padding.vertical()).max(0.0),
        };

        let child_size = if let Some(ref mut child) = self.child {
            // Only layout child if it needs it
            if child.needs_layout() {
                child.layout(child_constraints)
            } else {
                let bounds = child.bounds();
                Size::new(bounds.width, bounds.height)
            }
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

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Paint is always called - selective rendering is handled at main loop level
        let background = self.background.get();
        let corner_radius = self.corner_radius.get();
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.elevation.get();
        let shadow = elevation_to_shadow(elevation_level);

        // Draw background first (gradient or solid color)
        if let Some(ref gradient) = self.gradient {
            let direction = match gradient.direction {
                GradientDirection::Horizontal => GradientDir::Horizontal,
                GradientDirection::Vertical => GradientDir::Vertical,
                GradientDirection::Diagonal => GradientDir::Diagonal,
                GradientDirection::DiagonalReverse => GradientDir::DiagonalReverse,
            };
            // Note: Gradients don't currently support shadows, draw without shadow
            ctx.draw_gradient_rect_with_curvature(
                self.bounds,
                gradient.start_color,
                gradient.end_color,
                direction,
                corner_radius,
                corner_curvature,
            );
        } else if background.a > 0.0 {
            // Draw with or without shadow based on elevation
            if elevation_level > 0.0 {
                ctx.draw_rounded_rect_with_shadow_and_curvature(
                    self.bounds,
                    background,
                    corner_radius,
                    corner_curvature,
                    shadow,
                );
            } else {
                ctx.draw_rounded_rect_with_curvature(
                    self.bounds,
                    background,
                    corner_radius,
                    corner_curvature,
                );
            }
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

        // Draw ripple effects on top of everything (including text) using overlay
        if self.ripple_enabled {
            // First draw hover ripple (if active)
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
                    // Hover ripple: gradual fade
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

            // Then draw click ripple on top (if active)
            if let Some((cx, cy)) = self.click_ripple_center {
                // Interpolate center position during reverse animation
                let (center_x, center_y) = if self.click_ripple_reversing {
                    if let Some((rx, ry)) = self.click_ripple_release_pos {
                        // As progress goes from 1.0 to 0.0, move from click position to release position
                        // reverse_progress: 0.0 = at release point, 1.0 = at click point
                        let t = self.click_ripple_progress; // progress as interpolation factor
                        (
                            cx + (rx - cx) * (1.0 - t),
                            cy + (ry - cy) * (1.0 - t),
                        )
                    } else {
                        (cx, cy)
                    }
                } else {
                    (cx, cy)
                };

                // Calculate the maximum radius needed to cover the entire container from the current center
                let dx1 = center_x - self.bounds.x;
                let dx2 = (self.bounds.x + self.bounds.width) - center_x;
                let dy1 = center_y - self.bounds.y;
                let dy2 = (self.bounds.y + self.bounds.height) - center_y;
                let max_radius = (dx1.max(dx2).powi(2) + dy1.max(dy2).powi(2)).sqrt();

                // Animate radius from 0 to max_radius based on progress
                let current_radius = max_radius * self.click_ripple_progress;

                // Click ripple: gradual fade (similar to hover but slightly stronger)
                let alpha = self.ripple_color.a * (1.0 - self.click_ripple_progress * 0.5);

                if current_radius > 0.0 && alpha > 0.0 {
                    let ripple_color = Color::rgba(
                        self.ripple_color.r,
                        self.ripple_color.g,
                        self.ripple_color.b,
                        alpha,
                    );
                    // Use overlay so ripple appears on top of text
                    ctx.draw_overlay_circle_clipped_with_curvature(
                        center_x,
                        center_y,
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
                    // Start click ripple (separate from hover ripple)
                    if self.ripple_enabled {
                        self.click_ripple_center = Some((*x, *y));
                        self.click_ripple_progress = 0.0;
                        self.click_ripple_reversing = false;
                        self.click_ripple_release_pos = None;
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseUp { x, y, button } => {
                if self.is_pressed && *button == MouseButton::Left {
                    self.is_pressed = false;
                    // Start reverse animation for click ripple when mouse button is released
                    if self.ripple_enabled && self.click_ripple_center.is_some() {
                        self.click_ripple_reversing = true;
                        self.click_ripple_release_pos = Some((*x, *y));
                    }
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

    fn id(&self) -> WidgetId {
        self.widget_id
    }

    fn mark_dirty(&mut self, flags: ChangeFlags) {
        self.dirty_flags |= flags;
    }

    fn needs_layout(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_LAYOUT)
    }

    fn needs_paint(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_PAINT)
    }

    fn clear_dirty(&mut self) {
        self.dirty_flags = ChangeFlags::empty();
        // Also clear child dirty flags
        if let Some(ref mut child) = self.child {
            child.clear_dirty();
        }
    }
}

pub fn container() -> Container {
    Container::new()
}
