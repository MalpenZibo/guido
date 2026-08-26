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
    /// Momentum velocity, in pixels per frame — the unit `advance_momentum`
    /// adds. Built from a speed, not from the last delta.
    pub velocity_x: f32,
    pub velocity_y: f32,
    /// Timestamp of the last gesture sample, for the interval between samples
    /// and for how old the gesture is when it ends.
    pub last_scroll_time: Option<std::time::Instant>,
    /// The gesture that produced the velocity is over, so the momentum may run.
    /// Set by an end-of-gesture, cleared by the next sample.
    pub gesture_ended: bool,
    /// When the momentum became due, refreshed by every step it takes. A
    /// momentum that stops being advanced has been abandoned, and the field is
    /// what says how long ago that was.
    pub momentum_since: Option<std::time::Instant>,
    /// Samples in the current gesture, so the first timed one seeds the
    /// estimate instead of being smoothed against a velocity from before it.
    pub gesture_samples: u32,
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

    /// Record one sample of a continuous gesture: `delta` pixels moved,
    /// `dt_ms` since the previous sample of the same gesture.
    ///
    /// The velocity is distance over time. Storing the raw delta made 3px in
    /// 8ms and 3px in 60ms — speeds seven times apart — the same number, so
    /// what the momentum continued with bore little relation to how fast the
    /// finger was going.
    ///
    /// `dt_ms` is `None` for the first sample of a gesture: a distance with no
    /// time is not a speed, and guessing one from a single sample is how the
    /// old behaviour started. That sample moves the content and contributes no
    /// momentum.
    pub fn record_gesture_sample(&mut self, delta_x: f32, delta_y: f32, dt_ms: Option<f32>) {
        /// Two events can arrive within the same millisecond; the speed that
        /// implies is unbounded and meaningless.
        const MIN_SAMPLE_MS: f32 = 1.0;
        /// A momentum step is in pixels per frame, a gesture in pixels per
        /// millisecond. This is what turns one into the other.
        const NOMINAL_FRAME_MS: f32 = 1000.0 / 60.0;
        /// Weight of the newest sample. Smoothed, because the last sample of a
        /// gesture is the one most likely to be a stray.
        const SMOOTHING: f32 = 0.6;
        /// However fast the hardware claims the finger went.
        const MAX_STEP: f32 = 120.0;

        // A new sample means the finger is still down, whatever a previous
        // end-of-gesture said.
        self.gesture_ended = false;
        self.momentum_since = None;

        let Some(dt_ms) = dt_ms else {
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
            self.gesture_samples = 1;
            return;
        };

        let dt_ms = dt_ms.max(MIN_SAMPLE_MS);
        let step = |delta: f32| ((delta / dt_ms) * NOMINAL_FRAME_MS).clamp(-MAX_STEP, MAX_STEP);
        let (step_x, step_y) = (step(delta_x), step(delta_y));

        if self.gesture_samples <= 1 {
            self.velocity_x = step_x;
            self.velocity_y = step_y;
        } else {
            self.velocity_x = self.velocity_x * (1.0 - SMOOTHING) + step_x * SMOOTHING;
            self.velocity_y = self.velocity_y * (1.0 - SMOOTHING) + step_y * SMOOTHING;
        }
        self.gesture_samples = self.gesture_samples.saturating_add(1);
    }

    /// The gesture ended — the finger lifted.
    ///
    /// `since_last_sample_ms` is how long ago the gesture last moved. A
    /// momentum belongs to the gesture that produced it: a finger resting
    /// before it lifts is not throwing anything, so a velocity that old is
    /// spent rather than released.
    pub fn end_gesture(&mut self, since_last_sample_ms: Option<f32>) {
        /// Longer than this between the last movement and the lift, and the
        /// gesture had already stopped.
        const GESTURE_STALE_MS: f32 = 100.0;

        if since_last_sample_ms.is_none_or(|ms| ms > GESTURE_STALE_MS) {
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
        }
        self.gesture_ended = true;
        self.gesture_samples = 0;
        self.momentum_since = Some(std::time::Instant::now());
    }

    /// Discard a momentum that stopped being advanced `idle_ms` ago.
    ///
    /// A momentum belongs to a moment as well as to a gesture. Gating it on the
    /// gesture having ended stopped a velocity being flung while the finger was
    /// still down, but a flag that is set and stays set has the same fault the
    /// timeout had: one that was left half-run — the loop went idle, nothing
    /// asked for a frame — was still due, and the next animation frame from any
    /// source picked it up where it stopped. Measured at 148px, then 304px
    /// after a wait of 400ms.
    ///
    /// This is not a guess about the user, which is what the timeout was. It is
    /// a statement about this loop: nothing has advanced this motion for long
    /// enough that it is not the same motion any more.
    pub fn expire_stale_momentum(&mut self, idle_ms: f32) {
        /// Six dropped frames at 60fps. Long enough that an ordinary hitch does
        /// not cut a fling short, short enough that nobody reads the resumption
        /// as a continuation.
        const MOMENTUM_STALE_MS: f32 = 200.0;

        if idle_ms > MOMENTUM_STALE_MS {
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
            self.gesture_ended = false;
            self.momentum_since = None;
        }
    }

    /// How long since the momentum was last advanced, if one is due.
    pub fn momentum_idle_ms(&self) -> Option<f32> {
        self.momentum_since
            .map(|t| t.elapsed().as_secs_f32() * 1000.0)
    }

    /// Whether the momentum may run: the gesture is over and it left a speed.
    ///
    /// No clock. The end of a gesture used to be guessed from a 50ms gap since
    /// the last sample, and a slow scroll is made of gaps longer than that — so
    /// the guess fired *inside* the gesture and flung the list while the finger
    /// was still down. It also meant a momentum could become due with nothing
    /// scheduled to run it, and then be resumed by the next animation frame
    /// from any source, however much later.
    pub fn should_apply_momentum(&self) -> bool {
        const VELOCITY_THRESHOLD: f32 = 0.5;

        self.gesture_ended
            && (self.velocity_x.abs() > VELOCITY_THRESHOLD
                || self.velocity_y.abs() > VELOCITY_THRESHOLD)
    }

    /// Advance kinetic scrolling animation, returns true if still animating
    pub fn advance_momentum(&mut self) -> bool {
        const FRICTION: f32 = 0.92;
        const VELOCITY_THRESHOLD: f32 = 0.5;

        // A motion nobody has advanced for long enough is not this motion any
        // more, whatever velocity is left of it.
        if let Some(idle_ms) = self.momentum_idle_ms() {
            self.expire_stale_momentum(idle_ms);
        }

        // Nothing to run, and nothing to keep the loop awake for: a velocity
        // whose gesture has not ended is not a momentum waiting its turn, it
        // belongs to a finger that is still down.
        if !self.should_apply_momentum() {
            return false;
        }

        self.momentum_since = Some(std::time::Instant::now());

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroller with room to fling in.
    fn scroller() -> ScrollState {
        ScrollState {
            content_height: 5000.0,
            viewport_height: 400.0,
            ..Default::default()
        }
    }

    /// Play `samples` movements of `delta` pixels `dt_ms` apart, then lift.
    fn gesture(state: &mut ScrollState, samples: u32, delta: f32, dt_ms: f32) {
        for i in 0..samples {
            let dt = (i > 0).then_some(dt_ms);
            state.record_gesture_sample(0.0, delta, dt);
        }
        state.end_gesture(Some(dt_ms));
    }

    /// How far the momentum carries the content once the finger has lifted.
    fn coast(state: &mut ScrollState) -> f32 {
        let start = state.offset_y;
        for _ in 0..600 {
            if !state.advance_momentum() {
                break;
            }
        }
        state.offset_y - start
    }

    /// The point of measuring a speed instead of keeping the last delta: two
    /// gestures covering the same distance in different times are different
    /// speeds, and have to fling differently. Storing the raw delta made them
    /// the same number.
    #[test]
    fn a_gesture_that_covered_its_distance_faster_flings_further() {
        let mut quick = scroller();
        gesture(&mut quick, 6, 10.0, 8.0);
        let quick_coast = coast(&mut quick);

        let mut slow = scroller();
        gesture(&mut slow, 6, 10.0, 60.0);
        let slow_coast = coast(&mut slow);

        assert!(
            quick_coast > slow_coast * 2.0,
            "same 50px of gesture, {}x apart in time, coasted {quick_coast} and {slow_coast}",
            60.0 / 8.0
        );
    }

    /// A finger that has come to rest before it lifts is not throwing
    /// anything. The velocity belongs to a gesture that had already stopped.
    #[test]
    fn a_finger_resting_before_it_lifts_does_not_fling() {
        let mut state = scroller();
        for i in 0..6 {
            state.record_gesture_sample(0.0, 10.0, (i > 0).then_some(8.0));
        }
        assert!(state.velocity_y.abs() > 0.5, "the gesture built a speed");

        // ... and then the finger sat still for a while before lifting.
        state.end_gesture(Some(400.0));

        assert!(!state.should_apply_momentum());
        assert_eq!(coast(&mut state), 0.0);
    }

    /// While the finger is down there is no momentum to run, whatever speed
    /// the samples have built up: the list goes where the finger puts it.
    #[test]
    fn a_gesture_in_progress_has_no_momentum_to_apply() {
        let mut state = scroller();
        for i in 0..6 {
            state.record_gesture_sample(0.0, 10.0, (i > 0).then_some(8.0));
        }

        assert!(state.velocity_y.abs() > 0.5, "the gesture built a speed");
        assert!(!state.should_apply_momentum(), "but it is not due");
        assert!(
            !state.advance_momentum(),
            "and nothing is animating, so the loop is not kept awake for it"
        );
        assert_eq!(coast(&mut state), 0.0);
    }

    /// The unit behind the integration case: a motion that has not been
    /// advanced for long enough is over, whatever velocity is left of it.
    #[test]
    fn a_momentum_abandoned_mid_flight_expires() {
        let mut state = scroller();
        gesture(&mut state, 6, 10.0, 8.0);
        assert!(state.should_apply_momentum(), "the flick is due");

        // A few frames run, and then nothing does.
        state.advance_momentum();
        assert!(state.should_apply_momentum(), "still due between frames");

        state.expire_stale_momentum(400.0);

        assert!(!state.should_apply_momentum());
        assert_eq!(state.velocity_y, 0.0);
        assert_eq!(coast(&mut state), 0.0);
    }

    /// An ordinary hitch is not an abandonment: a dropped frame or two must not
    /// cut a fling short.
    #[test]
    fn a_dropped_frame_does_not_expire_a_momentum() {
        let mut state = scroller();
        gesture(&mut state, 6, 10.0, 8.0);

        state.expire_stale_momentum(50.0);

        assert!(state.should_apply_momentum());
        assert!(coast(&mut state) > 0.0);
    }

    /// A sample landing after the gesture ended is the next gesture starting,
    /// and it cancels the momentum rather than being flung along with it.
    #[test]
    fn a_new_sample_takes_the_gesture_back() {
        let mut state = scroller();
        gesture(&mut state, 6, 10.0, 8.0);
        assert!(state.should_apply_momentum());

        state.record_gesture_sample(0.0, 4.0, Some(8.0));
        assert!(!state.should_apply_momentum());
    }

    /// One sample is a distance with no time. Guessing a speed from it is what
    /// the old behaviour did, and it is what a slow gesture kept triggering.
    #[test]
    fn the_first_sample_of_a_gesture_is_not_a_speed() {
        let mut state = scroller();
        state.record_gesture_sample(0.0, 30.0, None);

        assert_eq!(state.velocity_y, 0.0);
        state.end_gesture(Some(8.0));
        assert!(!state.should_apply_momentum());
    }

    #[test]
    fn test_scroll_axis_allows_vertical() {
        assert!(!ScrollAxis::None.allows_vertical());
        assert!(ScrollAxis::Vertical.allows_vertical());
        assert!(!ScrollAxis::Horizontal.allows_vertical());
        assert!(ScrollAxis::Both.allows_vertical());
    }

    #[test]
    fn test_scroll_axis_allows_horizontal() {
        assert!(!ScrollAxis::None.allows_horizontal());
        assert!(!ScrollAxis::Vertical.allows_horizontal());
        assert!(ScrollAxis::Horizontal.allows_horizontal());
        assert!(ScrollAxis::Both.allows_horizontal());
    }

    #[test]
    fn test_scroll_state_max_scroll() {
        let state = ScrollState {
            content_width: 500.0,
            content_height: 800.0,
            viewport_width: 300.0,
            viewport_height: 400.0,
            ..Default::default()
        };

        assert_eq!(state.max_scroll_x(), 200.0);
        assert_eq!(state.max_scroll_y(), 400.0);
        assert_eq!(state.max_scroll(ScrollbarAxis::Horizontal), 200.0);
        assert_eq!(state.max_scroll(ScrollbarAxis::Vertical), 400.0);
    }

    #[test]
    fn test_scroll_state_max_scroll_no_overflow() {
        let state = ScrollState {
            content_width: 200.0,
            content_height: 300.0,
            viewport_width: 300.0,
            viewport_height: 400.0,
            ..Default::default()
        };

        assert_eq!(state.max_scroll_x(), 0.0);
        assert_eq!(state.max_scroll_y(), 0.0);
    }

    #[test]
    fn test_scroll_state_needs_scrollbar() {
        let state = ScrollState {
            content_width: 500.0,
            content_height: 800.0,
            viewport_width: 300.0,
            viewport_height: 400.0,
            ..Default::default()
        };

        assert!(state.needs_horizontal_scrollbar());
        assert!(state.needs_vertical_scrollbar());
    }

    #[test]
    fn test_scroll_state_needs_scrollbar_no_overflow() {
        let state = ScrollState {
            content_width: 200.0,
            content_height: 300.0,
            viewport_width: 300.0,
            viewport_height: 400.0,
            ..Default::default()
        };

        assert!(!state.needs_horizontal_scrollbar());
        assert!(!state.needs_vertical_scrollbar());
    }

    #[test]
    fn test_scroll_state_clamp_offsets() {
        let mut state = ScrollState {
            content_width: 500.0,
            content_height: 800.0,
            viewport_width: 300.0,
            viewport_height: 400.0,
            offset_x: 300.0, // Over max
            offset_y: -50.0, // Under min
            ..Default::default()
        };

        state.clamp_offsets();

        assert_eq!(state.offset_x, 200.0); // Clamped to max
        assert_eq!(state.offset_y, 0.0); // Clamped to min
    }

    #[test]
    fn test_scroll_state_set_offset_by_axis() {
        let mut state = ScrollState::default();

        state.set_offset(ScrollbarAxis::Vertical, 100.0);
        state.set_offset(ScrollbarAxis::Horizontal, 50.0);

        assert_eq!(state.offset_y, 100.0);
        assert_eq!(state.offset_x, 50.0);
    }

    #[test]
    fn test_scroll_state_hover_states() {
        let mut state = ScrollState::default();

        assert!(!state.is_track_hovered(ScrollbarAxis::Vertical));
        assert!(!state.is_handle_hovered(ScrollbarAxis::Vertical));

        state.set_track_hovered(ScrollbarAxis::Vertical, true);
        state.set_handle_hovered(ScrollbarAxis::Vertical, true);

        assert!(state.is_track_hovered(ScrollbarAxis::Vertical));
        assert!(state.is_handle_hovered(ScrollbarAxis::Vertical));

        // Horizontal should still be false
        assert!(!state.is_track_hovered(ScrollbarAxis::Horizontal));
        assert!(!state.is_handle_hovered(ScrollbarAxis::Horizontal));
    }

    #[test]
    fn test_scroll_state_dragging() {
        let mut state = ScrollState::default();

        assert!(!state.is_dragging(ScrollbarAxis::Vertical));
        assert!(!state.is_dragging(ScrollbarAxis::Horizontal));

        state.set_dragging(ScrollbarAxis::Vertical, true);
        assert!(state.is_dragging(ScrollbarAxis::Vertical));
        assert!(!state.is_dragging(ScrollbarAxis::Horizontal));

        state.set_dragging(ScrollbarAxis::Horizontal, true);
        assert!(state.is_dragging(ScrollbarAxis::Horizontal));
    }

    #[test]
    fn test_scroll_state_drag_start() {
        let mut state = ScrollState::default();

        state.set_drag_start(ScrollbarAxis::Vertical, 100.0, 50.0);
        let (pos, offset) = state.drag_start(ScrollbarAxis::Vertical);
        assert_eq!(pos, 100.0);
        assert_eq!(offset, 50.0);

        state.set_drag_start(ScrollbarAxis::Horizontal, 200.0, 75.0);
        let (pos, offset) = state.drag_start(ScrollbarAxis::Horizontal);
        assert_eq!(pos, 200.0);
        assert_eq!(offset, 75.0);
    }

    #[test]
    fn test_scrollbar_handle_size() {
        let state = ScrollState {
            viewport_height: 400.0,
            content_height: 800.0,
            viewport_width: 300.0,
            content_width: 600.0,
            ..Default::default()
        };

        let config = ScrollbarConfig::default();

        // Vertical: viewport/content = 0.5, so handle should be 50% of track
        let v_handle = state.scrollbar_handle_size(ScrollbarAxis::Vertical, 400.0, &config);
        assert_eq!(v_handle, 200.0);

        // Horizontal: viewport/content = 0.5, so handle should be 50% of track
        let h_handle = state.scrollbar_handle_size(ScrollbarAxis::Horizontal, 300.0, &config);
        assert_eq!(h_handle, 150.0);
    }

    #[test]
    fn test_scrollbar_handle_size_min() {
        let state = ScrollState {
            viewport_height: 100.0,
            content_height: 10000.0, // Very large content
            ..Default::default()
        };

        let config = ScrollbarConfig::default();

        // Handle should be at least min_handle_size
        let handle = state.scrollbar_handle_size(ScrollbarAxis::Vertical, 400.0, &config);
        assert_eq!(handle, config.min_handle_size);
    }

    #[test]
    fn test_scrollbar_handle_offset() {
        let state = ScrollState {
            viewport_height: 400.0,
            content_height: 800.0,
            offset_y: 200.0, // 50% scrolled
            ..Default::default()
        };

        // With 400px track and 200px handle, available travel is 200px
        // At 50% scroll, offset should be 100px
        let offset = state.scrollbar_handle_offset(ScrollbarAxis::Vertical, 400.0, 200.0);
        assert_eq!(offset, 100.0);
    }
}
