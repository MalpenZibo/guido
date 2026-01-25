//! Scroll configuration types for scrollable containers.

use super::widget::{Color, Rect};

/// Axis for scrollbar calculations (vertical or horizontal)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAxis {
    Vertical,
    Horizontal,
}

/// Axis along which scrolling is enabled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollAxis {
    /// No scrolling (default)
    #[default]
    None,
    /// Vertical scrolling only
    Vertical,
    /// Horizontal scrolling only
    Horizontal,
    /// Bidirectional scrolling
    Both,
}

impl ScrollAxis {
    /// Returns true if vertical scrolling is enabled
    pub fn allows_vertical(&self) -> bool {
        matches!(self, ScrollAxis::Vertical | ScrollAxis::Both)
    }

    /// Returns true if horizontal scrolling is enabled
    pub fn allows_horizontal(&self) -> bool {
        matches!(self, ScrollAxis::Horizontal | ScrollAxis::Both)
    }
}

/// When to show the scrollbar
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbarVisibility {
    /// Always show scrollbar when content overflows
    #[default]
    Always,
    /// Never show scrollbar (content still scrollable)
    Hidden,
}

/// Configuration for scrollbar appearance
#[derive(Debug, Clone)]
pub struct ScrollbarConfig {
    /// Width of the scrollbar track and handle (normal state)
    pub width: f32,
    /// Width of the scrollbar when hovered (expanded state)
    pub hover_width: f32,
    /// Margin from the edge of the container
    pub margin: f32,
    /// Color of the scrollbar track
    pub track_color: Color,
    /// Corner radius of the track
    pub track_corner_radius: f32,
    /// Corner curvature of the track (K-value: 0=bevel, 1=circular, 2=squircle)
    pub track_corner_curvature: f32,
    /// Color of the scrollbar handle
    pub handle_color: Color,
    /// Corner radius of the handle
    pub handle_corner_radius: f32,
    /// Corner curvature of the handle (K-value: 0=bevel, 1=circular, 2=squircle)
    pub handle_corner_curvature: f32,
    /// Color of the handle when hovered
    pub handle_hover_color: Color,
    /// Color of the handle when pressed/dragged
    pub handle_pressed_color: Color,
    /// Minimum size of the handle (to ensure it's always grabbable)
    pub min_handle_size: f32,
    /// Whether scrollbar reserves gutter space in layout
    pub reserve_gutter: bool,
}

impl Default for ScrollbarConfig {
    fn default() -> Self {
        Self {
            width: 6.0,
            hover_width: 10.0,
            margin: 2.0,
            track_color: Color::rgba(1.0, 1.0, 1.0, 0.05),
            track_corner_radius: 100.0, // Large value to ensure pill shape (clamped to half width)
            track_corner_curvature: 1.0, // Circular corners (standard)
            handle_color: Color::rgba(1.0, 1.0, 1.0, 0.3),
            handle_corner_radius: 100.0, // Large value to ensure pill shape (clamped to half width)
            handle_corner_curvature: 1.0, // Circular corners (standard)
            handle_hover_color: Color::rgba(1.0, 1.0, 1.0, 0.5),
            handle_pressed_color: Color::rgba(1.0, 1.0, 1.0, 0.6),
            min_handle_size: 20.0,
            reserve_gutter: true,
        }
    }
}

/// Builder for customizing scrollbar appearance
#[derive(Default)]
pub struct ScrollbarBuilder {
    config: ScrollbarConfig,
}

impl ScrollbarBuilder {
    /// Create a new scrollbar builder with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the width of the scrollbar (normal state)
    pub fn width(mut self, width: f32) -> Self {
        self.config.width = width;
        self
    }

    /// Set the width of the scrollbar when hovered (expanded state)
    pub fn hover_width(mut self, width: f32) -> Self {
        self.config.hover_width = width;
        self
    }

    /// Set the margin from the container edge
    pub fn margin(mut self, margin: f32) -> Self {
        self.config.margin = margin;
        self
    }

    /// Set the track color
    pub fn track_color(mut self, color: Color) -> Self {
        self.config.track_color = color;
        self
    }

    /// Set the track corner radius
    pub fn track_corner_radius(mut self, radius: f32) -> Self {
        self.config.track_corner_radius = radius;
        self
    }

    /// Set the track corner curvature (K-value)
    /// - 0.0 = bevel (diagonal cut)
    /// - 1.0 = circular (standard, default)
    /// - 2.0 = squircle (iOS-style smooth)
    pub fn track_corner_curvature(mut self, curvature: f32) -> Self {
        self.config.track_corner_curvature = curvature;
        self
    }

    /// Set the track to use squircle corners (K=2, iOS-style)
    pub fn track_squircle(mut self) -> Self {
        self.config.track_corner_curvature = 2.0;
        self
    }

    /// Set the handle color
    pub fn handle_color(mut self, color: Color) -> Self {
        self.config.handle_color = color;
        self
    }

    /// Set the handle corner radius
    pub fn handle_corner_radius(mut self, radius: f32) -> Self {
        self.config.handle_corner_radius = radius;
        self
    }

    /// Set the handle corner curvature (K-value)
    /// - 0.0 = bevel (diagonal cut)
    /// - 1.0 = circular (standard, default)
    /// - 2.0 = squircle (iOS-style smooth)
    pub fn handle_corner_curvature(mut self, curvature: f32) -> Self {
        self.config.handle_corner_curvature = curvature;
        self
    }

    /// Set the handle to use squircle corners (K=2, iOS-style)
    pub fn handle_squircle(mut self) -> Self {
        self.config.handle_corner_curvature = 2.0;
        self
    }

    /// Set both track and handle to use squircle corners (K=2, iOS-style)
    pub fn squircle(mut self) -> Self {
        self.config.track_corner_curvature = 2.0;
        self.config.handle_corner_curvature = 2.0;
        self
    }

    /// Set the handle hover color
    pub fn handle_hover_color(mut self, color: Color) -> Self {
        self.config.handle_hover_color = color;
        self
    }

    /// Set the handle pressed/dragged color
    pub fn handle_pressed_color(mut self, color: Color) -> Self {
        self.config.handle_pressed_color = color;
        self
    }

    /// Set the minimum handle size
    pub fn min_handle_size(mut self, size: f32) -> Self {
        self.config.min_handle_size = size;
        self
    }

    /// Set whether scrollbar reserves gutter space in layout
    /// When true (default), content area is reduced to make room for scrollbar
    /// When false, scrollbar overlays the content
    pub fn reserve_gutter(mut self, reserve: bool) -> Self {
        self.config.reserve_gutter = reserve;
        self
    }

    /// Make the scrollbar overlay content (no gutter space reserved)
    pub fn overlay(mut self) -> Self {
        self.config.reserve_gutter = false;
        self
    }

    /// Build the scrollbar configuration
    pub fn build(self) -> ScrollbarConfig {
        self.config
    }
}

/// Internal scroll state for a container
#[derive(Debug, Default)]
pub(crate) struct ScrollState {
    /// Current scroll offset in X direction
    pub offset_x: f32,
    /// Current scroll offset in Y direction
    pub offset_y: f32,
    /// Size of the content (computed during layout)
    pub content_width: f32,
    pub content_height: f32,
    /// Viewport size (container inner size)
    pub viewport_width: f32,
    pub viewport_height: f32,
    /// Scrollbar interaction state
    pub scrollbar_hovered: bool,
    pub scrollbar_track_hovered: bool, // Mouse is over the track area (for expansion)
    pub scrollbar_dragging: bool,
    pub scrollbar_drag_start_y: f32,
    pub scrollbar_drag_start_offset: f32,
    /// Horizontal scrollbar state (for Both axis)
    pub h_scrollbar_hovered: bool,
    pub h_scrollbar_track_hovered: bool, // Mouse is over the track area (for expansion)
    pub h_scrollbar_dragging: bool,
    pub h_scrollbar_drag_start_x: f32,
    pub h_scrollbar_drag_start_offset: f32,
    /// Velocity for kinetic/momentum scrolling
    pub velocity_x: f32,
    pub velocity_y: f32,
    /// Timestamp of last scroll event (for detecting when scrolling stops)
    pub last_scroll_time: Option<std::time::Instant>,
}

impl ScrollState {
    /// Get the maximum scroll offset in X direction
    pub fn max_scroll_x(&self) -> f32 {
        (self.content_width - self.viewport_width).max(0.0)
    }

    /// Get the maximum scroll offset in Y direction
    pub fn max_scroll_y(&self) -> f32 {
        (self.content_height - self.viewport_height).max(0.0)
    }

    /// Check if content overflows vertically
    pub fn needs_vertical_scrollbar(&self) -> bool {
        self.content_height > self.viewport_height
    }

    /// Check if content overflows horizontally
    pub fn needs_horizontal_scrollbar(&self) -> bool {
        self.content_width > self.viewport_width
    }

    /// Clamp scroll offsets to valid range
    pub fn clamp_offsets(&mut self) {
        self.offset_x = self.offset_x.clamp(0.0, self.max_scroll_x());
        self.offset_y = self.offset_y.clamp(0.0, self.max_scroll_y());
    }

    /// Check if momentum scrolling should be active (user stopped scrolling but has velocity)
    pub fn should_apply_momentum(&self) -> bool {
        const VELOCITY_THRESHOLD: f32 = 0.5;
        const SCROLL_TIMEOUT_MS: u128 = 50; // Wait 50ms after last scroll event

        // Only apply momentum if we have velocity AND enough time has passed since last scroll
        let has_velocity = self.velocity_x.abs() > VELOCITY_THRESHOLD
            || self.velocity_y.abs() > VELOCITY_THRESHOLD;

        let scroll_stopped = self
            .last_scroll_time
            .map(|t| t.elapsed().as_millis() > SCROLL_TIMEOUT_MS)
            .unwrap_or(true);

        has_velocity && scroll_stopped
    }

    /// Advance kinetic scrolling animation, returns true if still animating
    pub fn advance_momentum(&mut self) -> bool {
        const FRICTION: f32 = 0.92;
        const VELOCITY_THRESHOLD: f32 = 0.5;

        // Don't apply momentum while actively scrolling
        if !self.should_apply_momentum() {
            // Still animating if we have velocity (waiting for timeout)
            return self.velocity_x.abs() > VELOCITY_THRESHOLD
                || self.velocity_y.abs() > VELOCITY_THRESHOLD;
        }

        let mut animating = false;

        // Apply velocity to offset
        if self.velocity_x.abs() > VELOCITY_THRESHOLD {
            self.offset_x += self.velocity_x;
            self.velocity_x *= FRICTION;
            animating = true;
        } else {
            self.velocity_x = 0.0;
        }

        if self.velocity_y.abs() > VELOCITY_THRESHOLD {
            self.offset_y += self.velocity_y;
            self.velocity_y *= FRICTION;
            animating = true;
        } else {
            self.velocity_y = 0.0;
        }

        // Clamp to bounds
        let max_x = self.max_scroll_x();
        let max_y = self.max_scroll_y();
        self.offset_x = self.offset_x.clamp(0.0, max_x);
        self.offset_y = self.offset_y.clamp(0.0, max_y);

        // Stop velocity at edges
        if self.offset_x == 0.0 || self.offset_x == max_x {
            self.velocity_x = 0.0;
        }
        if self.offset_y == 0.0 || self.offset_y == max_y {
            self.velocity_y = 0.0;
        }

        animating
    }

    /// Get scrollbar track rectangle for the given axis
    pub fn scrollbar_track_rect(
        &self,
        axis: ScrollbarAxis,
        bounds: Rect,
        config: &ScrollbarConfig,
        needs_other_scrollbar: bool,
    ) -> Rect {
        let margin = config.margin;
        let width = config.width;

        match axis {
            ScrollbarAxis::Vertical => Rect::new(
                bounds.x + bounds.width - width - margin,
                bounds.y + margin,
                width,
                bounds.height - margin * 2.0,
            ),
            ScrollbarAxis::Horizontal => {
                let right_padding = if needs_other_scrollbar {
                    config.hover_width + margin
                } else {
                    margin
                };
                Rect::new(
                    bounds.x + margin,
                    bounds.y + bounds.height - width - margin,
                    bounds.width - margin - right_padding,
                    width,
                )
            }
        }
    }

    /// Get scrollbar hit test area for the given axis (uses hover_width for easier targeting)
    pub fn scrollbar_hit_area(
        &self,
        axis: ScrollbarAxis,
        bounds: Rect,
        config: &ScrollbarConfig,
        needs_other_scrollbar: bool,
    ) -> Rect {
        let margin = config.margin;

        match axis {
            ScrollbarAxis::Vertical => Rect::new(
                bounds.x + bounds.width - config.hover_width - margin,
                bounds.y + margin,
                config.hover_width,
                bounds.height - margin * 2.0,
            ),
            ScrollbarAxis::Horizontal => {
                let right_padding = if needs_other_scrollbar {
                    config.hover_width + margin
                } else {
                    margin
                };
                Rect::new(
                    bounds.x + margin,
                    bounds.y + bounds.height - config.hover_width - margin,
                    bounds.width - margin - right_padding,
                    config.hover_width,
                )
            }
        }
    }

    /// Calculate scrollbar handle size for the given axis
    pub fn scrollbar_handle_size(
        &self,
        axis: ScrollbarAxis,
        track_size: f32,
        config: &ScrollbarConfig,
    ) -> f32 {
        let (viewport, content) = match axis {
            ScrollbarAxis::Vertical => (self.viewport_height, self.content_height),
            ScrollbarAxis::Horizontal => (self.viewport_width, self.content_width),
        };

        if content <= viewport || content == 0.0 {
            return 0.0;
        }

        let ratio = viewport / content;
        (track_size * ratio).max(config.min_handle_size)
    }

    /// Calculate scrollbar handle offset for the given axis
    pub fn scrollbar_handle_offset(
        &self,
        axis: ScrollbarAxis,
        track_size: f32,
        handle_size: f32,
    ) -> f32 {
        let (offset, max_scroll) = match axis {
            ScrollbarAxis::Vertical => (self.offset_y, self.max_scroll_y()),
            ScrollbarAxis::Horizontal => (self.offset_x, self.max_scroll_x()),
        };

        if max_scroll <= 0.0 {
            return 0.0;
        }

        let available_travel = track_size - handle_size;
        (offset / max_scroll) * available_travel
    }

    /// Get scrollbar handle rectangle for the given axis
    pub fn scrollbar_handle_rect(
        &self,
        axis: ScrollbarAxis,
        bounds: Rect,
        config: &ScrollbarConfig,
        needs_other_scrollbar: bool,
    ) -> Rect {
        let track = self.scrollbar_track_rect(axis, bounds, config, needs_other_scrollbar);
        let track_size = match axis {
            ScrollbarAxis::Vertical => track.height,
            ScrollbarAxis::Horizontal => track.width,
        };
        let handle_size = self.scrollbar_handle_size(axis, track_size, config);
        let handle_offset = self.scrollbar_handle_offset(axis, track_size, handle_size);

        match axis {
            ScrollbarAxis::Vertical => {
                Rect::new(track.x, track.y + handle_offset, track.width, handle_size)
            }
            ScrollbarAxis::Horizontal => Rect::new(
                track.x.max(track.x + handle_offset),
                track.y,
                handle_size,
                track.height,
            ),
        }
    }

    /// Check if scrollbar for given axis is hovered (track area)
    pub fn is_track_hovered(&self, axis: ScrollbarAxis) -> bool {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_track_hovered,
            ScrollbarAxis::Horizontal => self.h_scrollbar_track_hovered,
        }
    }

    /// Check if scrollbar handle for given axis is hovered
    pub fn is_handle_hovered(&self, axis: ScrollbarAxis) -> bool {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_hovered,
            ScrollbarAxis::Horizontal => self.h_scrollbar_hovered,
        }
    }

    /// Check if scrollbar for given axis is being dragged
    pub fn is_dragging(&self, axis: ScrollbarAxis) -> bool {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_dragging,
            ScrollbarAxis::Horizontal => self.h_scrollbar_dragging,
        }
    }

    /// Set track hover state for given axis
    pub fn set_track_hovered(&mut self, axis: ScrollbarAxis, hovered: bool) {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_track_hovered = hovered,
            ScrollbarAxis::Horizontal => self.h_scrollbar_track_hovered = hovered,
        }
    }

    /// Set handle hover state for given axis
    pub fn set_handle_hovered(&mut self, axis: ScrollbarAxis, hovered: bool) {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_hovered = hovered,
            ScrollbarAxis::Horizontal => self.h_scrollbar_hovered = hovered,
        }
    }

    /// Set dragging state for given axis
    pub fn set_dragging(&mut self, axis: ScrollbarAxis, dragging: bool) {
        match axis {
            ScrollbarAxis::Vertical => self.scrollbar_dragging = dragging,
            ScrollbarAxis::Horizontal => self.h_scrollbar_dragging = dragging,
        }
    }

    /// Set drag start position for given axis
    pub fn set_drag_start(&mut self, axis: ScrollbarAxis, pos: f32, offset: f32) {
        match axis {
            ScrollbarAxis::Vertical => {
                self.scrollbar_drag_start_y = pos;
                self.scrollbar_drag_start_offset = offset;
            }
            ScrollbarAxis::Horizontal => {
                self.h_scrollbar_drag_start_x = pos;
                self.h_scrollbar_drag_start_offset = offset;
            }
        }
    }

    /// Get drag start position for given axis
    pub fn drag_start(&self, axis: ScrollbarAxis) -> (f32, f32) {
        match axis {
            ScrollbarAxis::Vertical => (
                self.scrollbar_drag_start_y,
                self.scrollbar_drag_start_offset,
            ),
            ScrollbarAxis::Horizontal => (
                self.h_scrollbar_drag_start_x,
                self.h_scrollbar_drag_start_offset,
            ),
        }
    }

    /// Set scroll offset for given axis
    pub fn set_offset(&mut self, axis: ScrollbarAxis, offset: f32) {
        match axis {
            ScrollbarAxis::Vertical => self.offset_y = offset,
            ScrollbarAxis::Horizontal => self.offset_x = offset,
        }
    }

    /// Get max scroll for given axis
    pub fn max_scroll(&self, axis: ScrollbarAxis) -> f32 {
        match axis {
            ScrollbarAxis::Vertical => self.max_scroll_y(),
            ScrollbarAxis::Horizontal => self.max_scroll_x(),
        }
    }
}
