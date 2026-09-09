//! Scroll configuration types for scrollable containers.

use super::widget::Rect;
use crate::clock::{EventInstant, FrameInstant};
use crate::reactive::{IntoSignal, Signal};
use crate::widgets::{Color, Container};

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

/// The measurements the container's own layout needs before anything paints.
///
/// Resolved once per layout pass from the signals on [`Scroll`], and handed to
/// the geometry below as plain numbers — the track and handle rects are
/// arithmetic, and arithmetic should not be reading signals halfway through.
///
/// What is *not* here is appearance. Colour, corners, borders and the hover and
/// pressed states are `Container`'s vocabulary, reached through
/// [`Scroll::track`] and [`Scroll::handle`], because the scrollbar is two
/// containers and always was.
#[derive(Debug, Clone, Copy)]
pub struct ScrollbarMetrics {
    /// Width of the scrollbar track and handle (normal state)
    pub width: f32,
    /// Width of the scrollbar when hovered (expanded state)
    pub hover_width: f32,
    /// Margin from the edge of the container
    pub margin: f32,
    /// Minimum size of the handle (to ensure it's always grabbable)
    pub min_handle_size: f32,
    /// Whether the scrollbar reserves gutter space in layout
    pub reserve_gutter: bool,
}

impl Default for ScrollbarMetrics {
    fn default() -> Self {
        Self {
            width: 6.0,
            hover_width: 10.0,
            margin: 2.0,
            min_handle_size: 20.0,
            reserve_gutter: true,
        }
    }
}

/// A radius far past half the width, which the shader clamps — so a scrollbar
/// stays a pill at whatever width is declared.
fn pill() -> crate::widgets::Corners {
    crate::widgets::Corners::rounded(100.0)
}

/// How a container scrolls, and what its scrollbar looks like.
///
/// One value rather than three setters. `scrollable`, `scrollbar_visibility`
/// and `scrollbar` were separate, and the last two were silent no-ops on a
/// container that never became scrollable — half a scroll configuration that
/// compiled and did nothing. Reached through [`Container::scroll`], there is no
/// way to spell the parts apart from the thing they configure.
///
/// ```ignore
/// container().scroll(Scroll::vertical())
///
/// container().scroll(
///     Scroll::vertical()
///         .width(6.0)
///         .handle(|h| h.background(theme.handle)
///             .when_hovered(|s| s.background(theme.hot))),
/// )
/// ```
///
/// The axis is structural — it decides what kind of widget this is — so it is
/// chosen by constructor and takes no signal, as `layout` and `control` do not.
/// Everything else here is declared and therefore reactive.
pub struct Scroll {
    pub(crate) axis: ScrollAxis,
    pub(crate) visibility: Option<Signal<ScrollbarVisibility>>,
    pub(crate) width: Option<Signal<f32>>,
    pub(crate) hover_width: Option<Signal<f32>>,
    pub(crate) margin: Option<Signal<f32>>,
    pub(crate) min_handle_size: Option<Signal<f32>>,
    pub(crate) reserve_gutter: Option<Signal<bool>>,
    pub(crate) track: Option<Box<dyn Fn(Container) -> Container>>,
    pub(crate) handle: Option<Box<dyn Fn(Container) -> Container>>,
}

impl Scroll {
    fn new(axis: ScrollAxis) -> Self {
        Self {
            axis,
            visibility: None,
            width: None,
            hover_width: None,
            margin: None,
            min_handle_size: None,
            reserve_gutter: None,
            track: None,
            handle: None,
        }
    }

    /// Scroll vertically.
    pub fn vertical() -> Self {
        Self::new(ScrollAxis::Vertical)
    }

    /// Scroll horizontally.
    pub fn horizontal() -> Self {
        Self::new(ScrollAxis::Horizontal)
    }

    /// Scroll on both axes.
    pub fn both() -> Self {
        Self::new(ScrollAxis::Both)
    }

    /// Whether the scrollbar is drawn. Hidden still scrolls.
    pub fn visibility<M>(mut self, visibility: impl IntoSignal<ScrollbarVisibility, M>) -> Self {
        self.visibility = Some(visibility.into_signal());
        self
    }

    /// The width of the track and handle at rest.
    pub fn width<M>(mut self, width: impl IntoSignal<f32, M>) -> Self {
        self.width = Some(width.into_signal());
        self
    }

    /// The width the handle grows to under the pointer.
    ///
    /// Not a `when_hovered(|s| s.width(..))`: the growth is a scale animation on
    /// the handle, so it belongs to the geometry rather than to the styling.
    pub fn hover_width<M>(mut self, width: impl IntoSignal<f32, M>) -> Self {
        self.hover_width = Some(width.into_signal());
        self
    }

    /// The gap between the scrollbar and the container's edges.
    pub fn margin<M>(mut self, margin: impl IntoSignal<f32, M>) -> Self {
        self.margin = Some(margin.into_signal());
        self
    }

    /// How short the handle is allowed to get, so it stays grabbable.
    pub fn min_handle_size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.min_handle_size = Some(size.into_signal());
        self
    }

    /// Whether the scrollbar takes space from the content or floats over it.
    pub fn reserve_gutter<M>(mut self, reserve: impl IntoSignal<bool, M>) -> Self {
        self.reserve_gutter = Some(reserve.into_signal());
        self
    }

    /// Float the scrollbar over the content. Shorthand for
    /// `reserve_gutter(false)`.
    pub fn overlay(self) -> Self {
        self.reserve_gutter(false)
    }

    /// The track as it looks with nothing declared.
    ///
    /// Named rather than buried in the scroll code so an application can read
    /// the values it is overriding, and so [`track`](Self::track) has something
    /// to say when it says the closure composes over a default.
    pub fn default_track() -> Container {
        Container::new()
            .background(Color::rgba(1.0, 1.0, 1.0, 0.05))
            .corners(pill())
    }

    /// The handle as it looks with nothing declared, hover and press included.
    pub fn default_handle() -> Container {
        use crate::widgets::state_layer::{StateStyle, Stateful};
        Container::new()
            .background(Color::rgba(1.0, 1.0, 1.0, 0.3))
            .corners(pill())
            .when_hovered(|s: StateStyle| s.background(Color::rgba(1.0, 1.0, 1.0, 0.5)))
            .when_pressed(|s: StateStyle| s.background(Color::rgba(1.0, 1.0, 1.0, 0.6)).ripple())
    }

    /// Style the track — the groove the handle runs in.
    ///
    /// The closure receives the `Container` the track is, so everything a
    /// container declares is available and reactive by construction. It
    /// receives [`default_track`](Self::default_track), so what it does not set
    /// it keeps — and ignoring the argument (`|_| container()`) is how you get
    /// a track with no default styling at all.
    ///
    /// Its geometry is not yours to set: the scroll code lays both parts out
    /// from the measurements above under tight constraints, so `width`,
    /// `height` and any position set here are overwritten.
    pub fn track(mut self, f: impl Fn(Container) -> Container + 'static) -> Self {
        self.track = Some(Box::new(f));
        self
    }

    /// Style the handle — the part that moves and answers a drag.
    ///
    /// The same contract as [`track`](Self::track), over
    /// [`default_handle`](Self::default_handle), and the same geometry caveat.
    pub fn handle(mut self, f: impl Fn(Container) -> Container + 'static) -> Self {
        self.handle = Some(Box::new(f));
        self
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
    pub last_scroll_time: Option<EventInstant>,
    /// The gesture that produced the velocity is over, so the momentum may run.
    /// Set by an end-of-gesture, cleared by the next sample.
    pub gesture_ended: bool,
    /// When the momentum became due, refreshed by every step it takes. A
    /// momentum that stops being advanced has been abandoned, and the field is
    /// what says how long ago that was.
    pub momentum_since: Option<FrameInstant>,
    /// Samples in the current gesture, so the first timed one seeds the
    /// estimate instead of being smoothed against a velocity from before it.
    pub gesture_samples: u32,
}

/// The unit a momentum's velocity is in: pixels per frame at sixty a second.
///
/// A gesture arrives in pixels per millisecond and `record_gesture_sample`
/// converts; `advance_momentum` converts back, to know how much of a glide a
/// gap between two frames was worth. The two have to agree for "pixels per
/// nominal frame" to mean anything, so there is one of them.
const NOMINAL_FRAME_MS: f32 = 1000.0 / 60.0;

/// How much of a gap between two frames counts as the surface having been
/// drawn, in nominal frames.
///
/// Three of them is fifty milliseconds — a twenty-a-second loop, which is a
/// machine under load rather than a machine that stopped. Below this the glide
/// travels the whole gap, so a 30Hz display and a hitching 45fps loop carry a
/// flick the same distance a 60Hz one does. Above it the surface was not being
/// drawn, and travelling the gap would put the content where it would have been
/// rather than where it was left: 304px of it, which is the jump this exists to
/// stop.
const DRAWN_PACING_FRAMES: f32 = 3.0;

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
    /// A momentum belongs to the gesture that produced it: a finger resting
    /// before it lifts is not throwing anything, so a velocity that old is
    /// spent rather than released.
    ///
    /// How long ago the gesture last moved is computed here rather than passed
    /// in. `last_scroll_time` is this state's own field, and a caller measuring
    /// it with a second clock is how the sample path and the end path came to
    /// disagree about what time it was inside one gesture.
    pub fn end_gesture(&mut self, at: EventInstant) {
        let since_last_sample_ms = self
            .last_scroll_time
            .map(|t| at.duration_since(t).as_secs_f32() * 1000.0);
        /// Longer than this between the last movement and the lift, and the
        /// gesture had already stopped.
        const GESTURE_STALE_MS: f32 = 100.0;

        if since_last_sample_ms.is_none_or(|ms| ms > GESTURE_STALE_MS) {
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
        }
        self.gesture_ended = true;
        self.gesture_samples = 0;
        // The glide begins when the finger leaves, so the first frame decays
        // it by however long it took to arrive. Crossing the clocks here is
        // sound for the reason it was not before: this is where a timeline
        // starts and the gap is decayed continuously, rather than a deadline
        // whose crossing cancelled the motion (#265, #340).
        self.momentum_since = Some(at.as_frame_start());
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

    /// Advance the glide to `now`, and say whether any of it is left.
    ///
    /// The step is how much time has passed, not how many times this was
    /// called. Per call, a friction decays twice as fast on a 120Hz display as
    /// on a 60Hz one — the same flick travelling a different distance depending
    /// on the monitor — and a pause between frames does not slow the glide at
    /// all but freezes it, so any later frame from any source resumes it at the
    /// speed it had. That was measured at 148px, then 304px after a wait of
    /// 400ms, and was guarded by cancelling a motion nothing had advanced for
    /// 200ms: a rule about a number standing in for the elapsed time the motion
    /// should have been using. There is no such rule now (#340).
    ///
    /// It slows first and moves after, so a glide that waited arrives at the
    /// speed the wait left it with. It decays by the whole gap, and moves for
    /// as much of it as the surface
    /// was plausibly being drawn for — [`DRAWN_PACING_FRAMES`]. A gap inside
    /// that is ordinary pacing, including a display slower than sixty a second
    /// and a loop under load, and the glide travels all of it. A gap beyond it
    /// is a surface nobody was drawing, and the part nobody saw is not
    /// travelled: a glide that slowed down out of sight does not have to
    /// reappear where it would have been.
    pub fn advance_momentum(&mut self, now: FrameInstant) -> bool {
        /// What a flick keeps after one nominal frame of friction.
        const FRICTION: f32 = 0.92;
        const VELOCITY_THRESHOLD: f32 = 0.5;

        // Nothing to run, and nothing to keep the loop awake for: a velocity
        // whose gesture has not ended is not a momentum waiting its turn, it
        // belongs to a finger that is still down.
        if !self.should_apply_momentum() {
            return false;
        }

        // Every path that leaves a momentum to run stamps where it began:
        // `end_gesture` at the lift, this function on every frame after. No
        // origin is no glide rather than a glide of some default length.
        let Some(began) = self.momentum_since else {
            return false;
        };
        let frames = now.saturating_duration_since(began).as_secs_f32() * 1000.0 / NOMINAL_FRAME_MS;
        let travelled = frames.min(DRAWN_PACING_FRAMES);
        self.momentum_since = Some(now);

        let mut animating = false;

        // Slowed first, then moved: the glide arrives at the speed the gap
        // left it with, rather than taking its last remembered speed into the
        // frame that finally drew it.
        let decay = FRICTION.powf(frames);

        if self.velocity_x.abs() > VELOCITY_THRESHOLD {
            self.velocity_x *= decay;
            self.offset_x += self.velocity_x * travelled;
            animating = true;
        } else {
            self.velocity_x = 0.0;
        }

        if self.velocity_y.abs() > VELOCITY_THRESHOLD {
            self.velocity_y *= decay;
            self.offset_y += self.velocity_y * travelled;
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
        config: &ScrollbarMetrics,
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
        config: &ScrollbarMetrics,
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
        config: &ScrollbarMetrics,
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
        config: &ScrollbarMetrics,
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
    use std::time::Duration;

    /// Lift the finger `gap_ms` after the last sample, which is what the
    /// production path records in `last_scroll_time` before ending a gesture.
    fn end_gesture_after(state: &mut ScrollState, gap_ms: f32) {
        let at = std::time::Instant::now();
        state.last_scroll_time =
            Some((at - std::time::Duration::from_micros((gap_ms * 1000.0) as u64)).into());
        state.end_gesture(at.into());
    }
    use super::*;

    /// A scroller with room to fling in.
    fn scroller() -> ScrollState {
        ScrollState {
            content_height: 5000.0,
            viewport_height: 400.0,
            ..Default::default()
        }
    }

    /// A scroller with room to fling sideways in.
    fn wide_scroller() -> ScrollState {
        ScrollState {
            content_width: 5000.0,
            viewport_width: 400.0,
            ..Default::default()
        }
    }

    /// A sideways gesture, which is the same thing on the other axis.
    fn sideways_gesture(state: &mut ScrollState, samples: u32, delta: f32, dt_ms: f32) {
        for i in 0..samples {
            let dt = (i > 0).then_some(dt_ms);
            state.record_gesture_sample(delta, 0.0, dt);
        }
        end_gesture_after(state, dt_ms);
    }

    /// The horizontal glide is the vertical one with the other field, and the
    /// two are written out separately — so a flick sideways has to decay the
    /// same way, and until this test nothing here ever flicked sideways at all.
    /// The mutation job named ten survivors on those lines, all of them the x
    /// axis of the arithmetic this change rewrote.
    #[test]
    fn a_sideways_flick_glides_and_decays_like_a_downward_one() {
        let mut sideways = wide_scroller();
        sideways_gesture(&mut sideways, 5, 20.0, 8.0);
        let sideways_speed = sideways.velocity_x;

        let mut downward = scroller();
        gesture(&mut downward, 5, 20.0, 8.0);
        let downward_speed = downward.velocity_y;
        assert!(
            (sideways_speed - downward_speed).abs() < 0.01,
            "the same gesture on either axis leaves the same speed"
        );

        let start = lifted();
        let travelled = {
            let before = sideways.offset_x;
            let mut at = start;
            for _ in 0..30 {
                at = at + A_FRAME;
                if !sideways.advance_momentum(at) {
                    break;
                }
            }
            sideways.offset_x - before
        };
        let down = coast_at(&mut downward, start, A_FRAME, 30);

        assert!(
            travelled > 0.0,
            "a sideways flick has to carry the content sideways"
        );
        assert!(
            (travelled - down).abs() < down * 0.05,
            "and as far as the same flick downward: {travelled}px against {down}px"
        );
        assert!(
            (sideways.velocity_x - downward.velocity_y).abs() < 0.01,
            "leaving the same speed behind it"
        );
    }

    /// A speed exactly at the threshold is not enough to keep an axis going.
    /// Each axis asks separately, so each is set up to be the one at the line
    /// while the other carries the motion — and the answer has to be that the
    /// axis at the line does not move.
    #[test]
    fn an_axis_at_the_threshold_does_not_glide() {
        for sideways in [true, false] {
            let mut state = ScrollState {
                content_width: 5000.0,
                viewport_width: 400.0,
                content_height: 5000.0,
                viewport_height: 400.0,
                gesture_ended: true,
                momentum_since: Some(lifted()),
                ..Default::default()
            };
            // One axis at the line, the other well past it so the glide runs
            // at all.
            let at_the_line = 0.5;
            if sideways {
                state.velocity_x = at_the_line;
                state.velocity_y = 10.0;
            } else {
                state.velocity_x = 10.0;
                state.velocity_y = at_the_line;
            }

            state.advance_momentum(lifted() + A_FRAME);

            let (stalled, moving) = if sideways {
                (state.offset_x, state.offset_y)
            } else {
                (state.offset_y, state.offset_x)
            };
            assert_eq!(
                stalled, 0.0,
                "a speed of exactly {at_the_line} is not a glide (sideways: \
                 {sideways})"
            );
            assert!(
                moving > 0.0,
                "and the other axis carried on (sideways: {sideways})"
            );
        }
    }

    /// Half a frame is half a step, on either axis. Stepping at sixty a second
    /// makes the two indistinguishable — the step is one — so this steps at a
    /// hundred and twenty.
    #[test]
    fn half_a_frame_moves_half_a_step_sideways() {
        let mut state = wide_scroller();
        sideways_gesture(&mut state, 5, 20.0, 8.0);
        let speed = state.velocity_x;

        let start = lifted();
        state.advance_momentum(start + A_FRAME / 2);

        let expected = speed * 0.92f32.powf(0.5) * 0.5;
        assert!(
            (state.offset_x - expected).abs() < expected * 0.05,
            "half a frame of a {speed}px glide moved {}px, against the \
             {expected}px half a step is worth",
            state.offset_x
        );
    }

    /// Play `samples` movements of `delta` pixels `dt_ms` apart, then lift.
    fn gesture(state: &mut ScrollState, samples: u32, delta: f32, dt_ms: f32) {
        for i in 0..samples {
            let dt = (i > 0).then_some(dt_ms);
            state.record_gesture_sample(0.0, delta, dt);
        }
        end_gesture_after(state, dt_ms);
    }

    /// One nominal frame, which is the unit `record_gesture_sample` hands the
    /// velocity over in.
    const A_FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);

    /// The moment the glide starts from, read after the lift has stamped its
    /// own: the gesture helpers end on a real instant, and a synthetic clock
    /// that began before it would run backwards.
    fn lifted() -> FrameInstant {
        FrameInstant::from(std::time::Instant::now())
    }

    /// Step a flick for `frames` nominal frames, `each` apart, and say how far
    /// the content travelled.
    fn coast_at(state: &mut ScrollState, start: FrameInstant, each: Duration, steps: u32) -> f32 {
        let before = state.offset_y;
        let mut at = start;
        for _ in 0..steps {
            at = at + each;
            if !state.advance_momentum(at) {
                break;
            }
        }
        state.offset_y - before
    }

    /// The same flick on a 60Hz display and on a 120Hz one, over the same
    /// half second of wall clock. A friction applied once per frame decays
    /// twice as fast on the faster display, so the same gesture carries the
    /// content a different distance depending on the monitor it is on.
    #[test]
    fn a_flick_travels_the_same_distance_whatever_the_refresh_rate() {
        let mut sixty = scroller();
        gesture(&mut sixty, 5, 20.0, 8.0);
        let at_sixty = coast_at(&mut sixty, lifted(), A_FRAME, 30);

        let mut one_twenty = scroller();
        gesture(&mut one_twenty, 5, 20.0, 8.0);
        let at_one_twenty = coast_at(&mut one_twenty, lifted(), A_FRAME / 2, 60);

        let difference = (at_sixty - at_one_twenty).abs();
        assert!(
            difference < at_sixty * 0.05,
            "the same flick over the same half second travelled {at_sixty}px at \
             60Hz and {at_one_twenty}px at 120Hz"
        );
    }

    /// A pause is not a freezer. Frames stop — the surface is occluded, the
    /// loop is throttled — and the glide has to have slowed by exactly as much
    /// as the pause was long, not resumed at the speed it had.
    #[test]
    fn a_pause_decays_the_flick_by_the_length_of_the_pause() {
        let mut unbroken = scroller();
        gesture(&mut unbroken, 5, 20.0, 8.0);
        coast_at(&mut unbroken, lifted(), A_FRAME, 24);

        let mut paused = scroller();
        gesture(&mut paused, 5, 20.0, 8.0);
        coast_at(&mut paused, lifted(), A_FRAME * 24, 1);

        let difference = (unbroken.velocity_y - paused.velocity_y).abs();
        assert!(
            difference < 0.01,
            "twenty-four frames of glide and one frame covering the same time \
             have to leave the same speed: {} against {}",
            unbroken.velocity_y,
            paused.velocity_y
        );
    }

    /// And it does not teleport. Content nobody drew is content that slowed
    /// down out of sight, not content that has to appear where it would have
    /// been — so a long gap travels the drawn-pacing window and no more.
    #[test]
    fn a_pause_does_not_jump_the_content() {
        let mut state = scroller();
        gesture(&mut state, 5, 20.0, 8.0);
        let first = state.velocity_y;

        let moved = coast_at(&mut state, lifted(), A_FRAME * 24, 1);

        assert!(
            moved <= first * DRAWN_PACING_FRAMES * 1.01,
            "one frame after a four hundred millisecond gap moved {moved}px, \
             which is more than the {}px the drawn part of it was worth",
            first * DRAWN_PACING_FRAMES
        );
    }

    /// The cap has to be wide enough for a loop that is merely slow. A 30Hz
    /// display, or a 60Hz one dropping every other frame, is being drawn — so
    /// a flick has to carry the same distance there as at sixty a second, and
    /// a cap of one nominal frame would have shortened it by half.
    #[test]
    fn a_slow_loop_carries_a_flick_as_far_as_a_fast_one() {
        let mut sixty = scroller();
        gesture(&mut sixty, 5, 20.0, 8.0);
        let at_sixty = coast_at(&mut sixty, lifted(), A_FRAME, 120);

        let mut thirty = scroller();
        gesture(&mut thirty, 5, 20.0, 8.0);
        let at_thirty = coast_at(&mut thirty, lifted(), A_FRAME * 2, 60);

        let difference = (at_sixty - at_thirty).abs();
        assert!(
            difference < at_sixty * 0.05,
            "the same flick carried {at_sixty}px at 60Hz and {at_thirty}px at \
             30Hz"
        );
    }

    /// A gap long enough leaves nothing to run, and nothing says so in
    /// milliseconds: the velocity simply decayed under the threshold.
    #[test]
    fn a_long_enough_pause_ends_the_flick_without_a_rule_about_it() {
        let mut state = scroller();
        gesture(&mut state, 5, 20.0, 8.0);

        let at = lifted() + Duration::from_secs(2);
        state.advance_momentum(at);

        assert!(
            !state.advance_momentum(at + A_FRAME),
            "two seconds of decay is nothing left to animate"
        );
    }

    /// The flick and the frame that carries it are on different clocks: the
    /// lift is stamped when the compositor sent it, the frame when it began.
    /// Between them is the loop's input latency, and `momentum_since` used to
    /// be stamped from the lift and then differenced against the frame — so a
    /// loop 250ms behind read its own lateness as the flick having gone stale
    /// and cancelled it before it moved a pixel (#265).
    #[test]
    fn a_flick_survives_a_loop_that_was_late_delivering_the_lift() {
        let mut state = scroller();
        gesture(&mut state, 5, 20.0, 8.0);
        assert!(
            state.should_apply_momentum(),
            "the gesture left a velocity to fling"
        );

        // The first frame to carry it starts a quarter of a second after the
        // lift — longer than MOMENTUM_STALE_MS, which is the trap.
        let late =
            FrameInstant::from(std::time::Instant::now() + std::time::Duration::from_millis(250));

        assert!(
            state.advance_momentum(late),
            "the flick has to run on the frame that finally arrived, not be \
             expired by how long it took to arrive"
        );
        assert!(
            state.offset_y != 0.0,
            "and it has to have moved the content"
        );
    }

    /// How far the momentum carries the content once the finger has lifted,
    /// at sixty frames a second — which has to be said now that the answer
    /// depends on it.
    fn coast(state: &mut ScrollState) -> f32 {
        coast_at(
            state,
            FrameInstant::from(std::time::Instant::now()),
            A_FRAME,
            600,
        )
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
        end_gesture_after(&mut state, 400.0);

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
            !state.advance_momentum(std::time::Instant::now().into()),
            "and nothing is animating, so the loop is not kept awake for it"
        );
        assert_eq!(coast(&mut state), 0.0);
    }

    /// The unit behind the integration case: a motion that has not been
    /// advanced for long enough is over, whatever velocity is left of it.
    #[test]
    fn a_momentum_abandoned_mid_flight_runs_down_to_nothing() {
        let mut state = scroller();
        gesture(&mut state, 6, 10.0, 8.0);
        assert!(state.should_apply_momentum(), "the flick is due");
        let start = lifted();

        // A frame runs, and then nothing does for four hundred milliseconds.
        state.advance_momentum(start + A_FRAME);
        assert!(state.should_apply_momentum(), "still due between frames");

        let running = state.velocity_y;
        let abandoned = start + A_FRAME + Duration::from_millis(400);
        state.advance_momentum(abandoned);

        assert!(
            state.velocity_y < running * 0.2,
            "four hundred milliseconds of friction leaves a fraction of the \
             speed, not the speed it had: {} against {running}",
            state.velocity_y
        );
        // The frame that covers a gap still moves what it has; it is the one
        // after that finds nothing left.
        let a_second_later = abandoned + Duration::from_secs(1);
        state.advance_momentum(a_second_later);
        assert!(
            !state.advance_momentum(a_second_later + A_FRAME),
            "and a second more of it is nothing left to run"
        );
    }

    /// An ordinary hitch is not an abandonment: a dropped frame or two must not
    /// cut a fling short.
    #[test]
    fn a_dropped_frame_does_not_cut_a_momentum_short() {
        let mut state = scroller();
        gesture(&mut state, 6, 10.0, 8.0);

        // Three frames' worth of hitch, which is an ordinary one.
        let after_the_hitch = lifted() + A_FRAME * 3;
        assert!(
            state.advance_momentum(after_the_hitch),
            "a hitch slows a flick, it does not end it"
        );
        assert!(
            coast_at(&mut state, after_the_hitch, A_FRAME, 120) > 0.0,
            "and there is still a glide to run"
        );
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
        end_gesture_after(&mut state, 8.0);
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

        let config = ScrollbarMetrics::default();

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

        let config = ScrollbarMetrics::default();

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
