use std::borrow::Cow;
use std::sync::Arc;
use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition};
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{request_animation_frame, ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::primitives::{GradientDir, Shadow};
use crate::renderer::PaintContext;
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;

use super::children::ChildrenSource;
use super::into_child::{IntoChild, IntoChildren};
use super::state_layer::{resolve_background, StateStyle};
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

impl From<GradientDirection> for GradientDir {
    fn from(direction: GradientDirection) -> Self {
        match direction {
            GradientDirection::Horizontal => GradientDir::Horizontal,
            GradientDirection::Vertical => GradientDir::Vertical,
            GradientDirection::Diagonal => GradientDir::Diagonal,
            GradientDirection::DiagonalReverse => GradientDir::DiagonalReverse,
        }
    }
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

/// Overflow behavior for container content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Content is not clipped and may overflow the container bounds
    #[default]
    Visible,
    /// Content is clipped to the container bounds
    Hidden,
}

/// Animation state for animatable properties
struct AnimationState<T: Animatable> {
    /// Current interpolated value
    current: T,
    /// Target value from MaybeDyn
    target: T,
    /// Value when animation started
    start: T,
    /// Progress from 0.0 to 1.0 (or beyond for overshoot)
    progress: f32,
    /// Time when animation started
    start_time: Instant,
    /// Transition configuration
    transition: Transition,
    /// Spring state (for spring timing functions)
    spring_state: Option<SpringState>,
    /// Whether the animation has been initialized with its first real value
    initialized: bool,
}

impl<T: Animatable> AnimationState<T> {
    fn new(initial_value: T, transition: Transition) -> Self {
        let spring_state = if matches!(
            transition.timing,
            crate::animation::TimingFunction::Spring(_)
        ) {
            Some(SpringState::new())
        } else {
            None
        };
        Self {
            current: initial_value.clone(),
            target: initial_value.clone(),
            start: initial_value,
            progress: 1.0, // Start completed
            start_time: Instant::now(),
            transition,
            spring_state,
            initialized: false, // Not yet initialized with real content-based value
        }
    }

    /// Start animating to a new target value
    fn animate_to(&mut self, new_target: T) {
        // Don't restart if we're already animating to this target
        if new_target == self.target {
            return;
        }

        self.start = self.current.clone();
        self.target = new_target.clone();
        self.progress = 0.0;
        self.start_time = Instant::now();
        // Reset spring state for new animation
        if self.spring_state.is_some() {
            self.spring_state = Some(SpringState::new());
        }
    }

    /// Advance the animation and return the current value
    fn advance(&mut self) -> T {
        if self.progress >= 1.0 && self.spring_state.is_none() {
            return self.current.clone();
        }

        let elapsed = self.start_time.elapsed().as_secs_f32() * 1000.0; // Convert to ms
        let adjusted_elapsed = (elapsed - self.transition.delay_ms).max(0.0);

        if adjusted_elapsed <= 0.0 {
            // Still in delay period
            return self.current.clone();
        }

        // Calculate eased value based on timing function type
        let eased_t = if let Some(ref mut spring_state) = self.spring_state {
            // For spring animations: use real elapsed time in seconds (not normalized)
            // This allows the spring to continue oscillating until it naturally settles
            let elapsed_secs = adjusted_elapsed / 1000.0;
            if let crate::animation::TimingFunction::Spring(ref config) = self.transition.timing {
                spring_state.step(elapsed_secs, config)
            } else {
                // Fallback: shouldn't happen, but use normalized time
                adjusted_elapsed / self.transition.duration_ms
            }
        } else {
            // For non-spring animations: use normalized time 0..1
            let t = (adjusted_elapsed / self.transition.duration_ms).min(1.0);
            self.transition.timing.evaluate(t)
        };

        // Interpolate
        self.current = T::lerp(&self.start, &self.target, eased_t);

        // Update progress
        if let Some(ref state) = self.spring_state {
            // For spring animations, only mark complete when spring has settled
            if state.is_settled(0.01) {
                self.progress = 1.0;
            } else {
                // Keep progress < 1.0 to continue animating
                self.progress = 0.5;
            }
        } else {
            // For non-spring animations, use time-based progress
            let t = (adjusted_elapsed / self.transition.duration_ms).min(1.0);
            self.progress = t;
        }

        self.current.clone()
    }

    /// Check if animation is still running
    fn is_animating(&self) -> bool {
        self.progress < 1.0 || (self.spring_state.is_some() && self.progress < 0.99)
    }

    /// Get current value
    fn current(&self) -> &T {
        &self.current
    }

    /// Get target value
    fn target(&self) -> &T {
        &self.target
    }

    /// Set value immediately without animation (for initialization)
    fn set_immediate(&mut self, value: T) {
        self.current = value.clone();
        self.target = value.clone();
        self.start = value;
        self.progress = 1.0;
        self.initialized = true;
    }

    /// Check if animation has never been initialized (first layout)
    fn is_initial(&self) -> bool {
        !self.initialized
    }
}

/// Macro to advance an animation field, optionally updating its target first
macro_rules! advance_anim {
    // Simple advance (no target update)
    ($self:expr, $anim:ident, $any_animating:expr) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                anim.advance();
                $any_animating = true;
            }
        }
    };
    // With target update
    ($self:expr, $anim:ident, $target_expr:expr, $any_animating:expr) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                anim.advance();
                $any_animating = true;
            }
        }
    };
}

pub struct Container {
    widget_id: WidgetId,
    dirty_flags: ChangeFlags,

    // Layout and children
    layout: Box<dyn Layout>,
    children_source: ChildrenSource,

    // Styling properties
    padding: MaybeDyn<Padding>,
    background: MaybeDyn<Color>,
    gradient: Option<LinearGradient>,
    corner_radius: MaybeDyn<f32>,
    corner_curvature: MaybeDyn<f32>,
    border_width: MaybeDyn<f32>,
    border_color: MaybeDyn<Color>,
    elevation: MaybeDyn<f32>,
    width: Option<MaybeDyn<Length>>,
    height: Option<MaybeDyn<Length>>,
    overflow: Overflow,
    bounds: Rect,
    transform: MaybeDyn<Transform>,
    transform_origin: MaybeDyn<TransformOrigin>,

    // Cached values for change detection
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

    // Animation state
    width_anim: Option<AnimationState<f32>>,
    height_anim: Option<AnimationState<f32>>,
    background_anim: Option<AnimationState<Color>>,
    corner_radius_anim: Option<AnimationState<f32>>,
    padding_anim: Option<AnimationState<Padding>>,
    border_width_anim: Option<AnimationState<f32>>,
    border_color_anim: Option<AnimationState<Color>>,
    transform_anim: Option<AnimationState<Transform>>,

    // State layer styles (hover/pressed overrides)
    hover_state: Option<StateStyle>,
    pressed_state: Option<StateStyle>,

    // Ripple animation state
    /// Center point of the ripple in local container coordinates (start position)
    ripple_center: Option<(f32, f32)>,
    /// Exit center point where ripple contracts toward (release position)
    ripple_exit_center: Option<(f32, f32)>,
    /// Current ripple expansion progress (0.0 = start, 1.0 = fully expanded)
    ripple_progress: f32,
    /// Current ripple opacity (1.0 = visible, 0.0 = faded out)
    ripple_opacity: f32,
    /// Whether the ripple is currently fading out (mouse released)
    ripple_fading: bool,
    /// Time when ripple animation started (for smooth animation)
    ripple_start_time: Option<std::time::Instant>,
    /// Time when ripple fade/contraction started
    ripple_fade_start_time: Option<std::time::Instant>,
    /// Progress at which fading started (for smooth contraction)
    ripple_fade_start_progress: f32,
}

impl Container {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
            dirty_flags: ChangeFlags::NEEDS_LAYOUT | ChangeFlags::NEEDS_PAINT,
            layout: Box::new(Flex::column()),
            children_source: ChildrenSource::default(),
            padding: MaybeDyn::Static(Padding::default()),
            background: MaybeDyn::Static(Color::TRANSPARENT),
            gradient: None,
            corner_radius: MaybeDyn::Static(0.0),
            corner_curvature: MaybeDyn::Static(1.0),
            border_width: MaybeDyn::Static(0.0),
            border_color: MaybeDyn::Static(Color::TRANSPARENT),
            elevation: MaybeDyn::Static(0.0),
            width: None,
            height: None,
            overflow: Overflow::Visible,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            transform: MaybeDyn::Static(Transform::IDENTITY),
            transform_origin: MaybeDyn::Static(TransformOrigin::CENTER),
            on_click: None,
            on_hover: None,
            on_scroll: None,
            is_hovered: false,
            is_pressed: false,
            cached_padding: Padding::default(),
            cached_background: Color::TRANSPARENT,
            cached_corner_radius: 0.0,
            cached_corner_curvature: 1.0,
            cached_elevation: 0.0,
            width_anim: None,
            height_anim: None,
            background_anim: None,
            corner_radius_anim: None,
            padding_anim: None,
            border_width_anim: None,
            border_color_anim: None,
            transform_anim: None,
            hover_state: None,
            pressed_state: None,
            ripple_center: None,
            ripple_exit_center: None,
            ripple_progress: 0.0,
            ripple_opacity: 0.0,
            ripple_fading: false,
            ripple_start_time: None,
            ripple_fade_start_time: None,
            ripple_fade_start_progress: 0.0,
        }
    }

    /// Set the layout strategy for this container
    pub fn layout(mut self, layout: impl Layout + 'static) -> Self {
        self.layout = Box::new(layout);
        self
    }

    /// Add a single child (static or dynamic)
    ///
    /// Accepts both:
    /// - Static widgets: `container().child(text("Hello"))`
    /// - Dynamic closures: `container().child(move || Some(text("Hello")))`
    pub fn child<M>(mut self, child: impl IntoChild<M>) -> Self {
        child.add_to_container(&mut self.children_source);
        self
    }

    /// Add a child if Some (static mode)
    pub fn maybe_child(mut self, widget: Option<impl Widget + 'static>) -> Self {
        if let Some(w) = widget {
            self = self.child(w);
        }
        self
    }

    /// Add multiple children (static or dynamic)
    ///
    /// Accepts both:
    /// - Static: `container().children([widget1, widget2])`
    /// - Dynamic: `container().children(move || items.iter().map(|i| (key, widget)))`
    pub fn children<M>(mut self, children: impl IntoChildren<M>) -> Self {
        children.add_to_container(&mut self.children_source);
        self
    }

    /// Transfer children from another ChildrenSource (useful for components)
    pub fn children_source(mut self, source: ChildrenSource) -> Self {
        self.children_source = source;
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
    pub fn corner_curvature(mut self, curvature: impl IntoMaybeDyn<f32>) -> Self {
        self.corner_curvature = curvature.into_maybe_dyn();
        self
    }

    /// Convenience: Set squircle/iOS-style corners
    pub fn squircle(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(2.0);
        self
    }

    /// Convenience: Set concave/scooped corners
    pub fn scoop(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(-1.0);
        self
    }

    /// Convenience: Set beveled corners
    pub fn bevel(mut self) -> Self {
        self.corner_curvature = MaybeDyn::Static(0.0);
        self
    }

    /// Set a border with the given width and color
    pub fn border(
        mut self,
        width: impl IntoMaybeDyn<f32>,
        color: impl IntoMaybeDyn<Color>,
    ) -> Self {
        self.border_width = width.into_maybe_dyn();
        self.border_color = color.into_maybe_dyn();
        self
    }

    /// Set a linear gradient background
    pub fn gradient(mut self, gradient: LinearGradient) -> Self {
        self.gradient = Some(gradient);
        self
    }

    /// Convenience: horizontal gradient
    pub fn gradient_horizontal(mut self, start: Color, end: Color) -> Self {
        self.gradient = Some(LinearGradient::horizontal(start, end));
        self
    }

    /// Convenience: vertical gradient
    pub fn gradient_vertical(mut self, start: Color, end: Color) -> Self {
        self.gradient = Some(LinearGradient::vertical(start, end));
        self
    }

    /// Set the width of the container.
    ///
    /// Accepts:
    /// - Exact size: `width(200.0)`
    /// - Minimum: `width(at_least(100.0))`
    /// - Maximum: `width(at_most(400.0))`
    /// - Range: `width(at_least(50.0).at_most(400.0))`
    /// - Reactive: `width(move || if expanded.get() { 600.0 } else { 50.0 })`
    pub fn width(mut self, width: impl IntoMaybeDyn<Length>) -> Self {
        self.width = Some(width.into_maybe_dyn());
        self
    }

    /// Set the height of the container.
    ///
    /// Accepts:
    /// - Exact size: `height(100.0)`
    /// - Minimum: `height(at_least(50.0))`
    /// - Maximum: `height(at_most(200.0))`
    /// - Range: `height(at_least(25.0).at_most(200.0))`
    /// - Reactive: `height(move || if expanded.get() { 300.0 } else { 50.0 })`
    pub fn height(mut self, height: impl IntoMaybeDyn<Length>) -> Self {
        self.height = Some(height.into_maybe_dyn());
        self
    }

    /// Set the overflow behavior for content that exceeds container bounds
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    pub fn on_click<F: Fn() + Send + Sync + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Arc::new(callback));
        self
    }

    /// Accept an optional click callback (useful for components)
    pub fn on_click_option(mut self, callback: Option<ClickCallback>) -> Self {
        self.on_click = callback;
        self
    }

    pub fn on_hover<F: Fn(bool) + Send + Sync + 'static>(mut self, callback: F) -> Self {
        self.on_hover = Some(Arc::new(callback));
        self
    }

    pub fn on_scroll<F: Fn(f32, f32, ScrollSource) + Send + Sync + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_scroll = Some(Arc::new(callback));
        self
    }

    pub fn elevation(mut self, level: impl IntoMaybeDyn<f32>) -> Self {
        self.elevation = level.into_maybe_dyn();
        self
    }

    /// Set the transform for this container
    /// Transforms are applied around the center of the widget bounds
    pub fn transform(mut self, t: impl IntoMaybeDyn<Transform>) -> Self {
        self.transform = t.into_maybe_dyn();
        self
    }

    /// Rotate this container by the given angle in degrees
    /// Rotation is applied around the center of the widget bounds
    /// Note: Rotation may appear stretched on non-square aspect ratios
    /// Multiple transform calls are composed (e.g., `.rotate(30).scale(1.5)` applies both)
    pub fn rotate(mut self, degrees: impl IntoMaybeDyn<f32>) -> Self {
        let degrees = degrees.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::rotate_degrees(degrees.get()))
        }));
        self
    }

    /// Scale this container uniformly
    /// Scaling is applied around the center of the widget bounds
    /// Multiple transform calls are composed (e.g., `.rotate(30).scale(1.5)` applies both)
    pub fn scale(mut self, s: impl IntoMaybeDyn<f32>) -> Self {
        let s = s.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            prev_transform.get().then(&Transform::scale(s.get()))
        }));
        self
    }

    /// Scale this container non-uniformly
    /// Multiple transform calls are composed (e.g., `.rotate(30).scale_xy(1.5, 2.0)` applies both)
    pub fn scale_xy(mut self, sx: impl IntoMaybeDyn<f32>, sy: impl IntoMaybeDyn<f32>) -> Self {
        let sx = sx.into_maybe_dyn();
        let sy = sy.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::scale_xy(sx.get(), sy.get()))
        }));
        self
    }

    /// Translate (move) this container by the given offset
    /// Multiple transform calls are composed (e.g., `.rotate(30).translate(10, 20)` applies both)
    pub fn translate(mut self, x: impl IntoMaybeDyn<f32>, y: impl IntoMaybeDyn<f32>) -> Self {
        let x = x.into_maybe_dyn();
        let y = y.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::translate(x.get(), y.get()))
        }));
        self
    }

    /// Set the transform origin (pivot point) for this container.
    ///
    /// The transform origin specifies the point around which rotations and scales are applied.
    /// By default, transforms are centered on the widget (50%, 50%).
    ///
    /// # Example
    /// ```ignore
    /// // Rotate around the top-left corner
    /// container()
    ///     .rotate(45.0)
    ///     .transform_origin(TransformOrigin::TOP_LEFT)
    ///
    /// // Scale from the bottom-right corner
    /// container()
    ///     .scale(1.5)
    ///     .transform_origin(TransformOrigin::BOTTOM_RIGHT)
    ///
    /// // Reactive transform origin
    /// container()
    ///     .rotate(30.0)
    ///     .transform_origin(move || if condition.get() {
    ///         TransformOrigin::CENTER
    ///     } else {
    ///         TransformOrigin::TOP_LEFT
    ///     })
    /// ```
    pub fn transform_origin(mut self, origin: impl IntoMaybeDyn<TransformOrigin>) -> Self {
        self.transform_origin = origin.into_maybe_dyn();
        self
    }

    /// Enable animation for width changes
    pub fn animate_width(mut self, transition: Transition) -> Self {
        // Initialize with current width (exact or min) or 0
        let initial = self
            .width
            .as_ref()
            .map(|w| {
                let len = w.get();
                len.exact.or(len.min).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        self.width_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for height changes
    pub fn animate_height(mut self, transition: Transition) -> Self {
        // Initialize with current height (exact or min) or 0
        let initial = self
            .height
            .as_ref()
            .map(|h| {
                let len = h.get();
                len.exact.or(len.min).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        self.height_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for background color changes
    pub fn animate_background(mut self, transition: Transition) -> Self {
        let initial = self.background.get();
        self.background_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for corner radius changes
    pub fn animate_corner_radius(mut self, transition: Transition) -> Self {
        let initial = self.corner_radius.get();
        self.corner_radius_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for padding changes
    pub fn animate_padding(mut self, transition: Transition) -> Self {
        let initial = self.padding.get();
        self.padding_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border width changes
    pub fn animate_border_width(mut self, transition: Transition) -> Self {
        let initial = self.border_width.get();
        self.border_width_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border color changes
    pub fn animate_border_color(mut self, transition: Transition) -> Self {
        let initial = self.border_color.get();
        self.border_color_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for transform changes
    pub fn animate_transform(mut self, transition: Transition) -> Self {
        let initial = self.transform.get();
        self.transform_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Set style overrides for the hover state.
    ///
    /// Properties set in the hover state will override the base properties
    /// when the container is hovered.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgb(0.2, 0.2, 0.3))
    ///     .hover_state(|s| s.lighter(0.1))
    ///     .child(text("Hover me"))
    /// ```
    pub fn hover_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.hover_state = Some(f(StateStyle::new()));
        self
    }

    /// Set style overrides for the pressed state.
    ///
    /// Properties set in the pressed state will override the base properties
    /// (and hover state properties) when the container is pressed.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgb(0.2, 0.2, 0.3))
    ///     .pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
    ///     .child(text("Click me"))
    /// ```
    pub fn pressed_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.pressed_state = Some(f(StateStyle::new()));
        self
    }

    /// Get the effective background color target considering state layers.
    ///
    /// Priority: pressed_state > hover_state > base background
    fn effective_background_target(&self) -> Color {
        let base = self.background.get();

        // Pressed state takes precedence
        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(ref bg) = state.background {
                    return resolve_background(base, bg);
                }
            }
        }

        // Then hover state
        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(ref bg) = state.background {
                    return resolve_background(base, bg);
                }
            }
        }

        base
    }

    /// Get the effective border width target considering state layers.
    fn effective_border_width_target(&self) -> f32 {
        let base = self.border_width.get();

        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(width) = state.border_width {
                    return width;
                }
            }
        }

        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(width) = state.border_width {
                    return width;
                }
            }
        }

        base
    }

    /// Get the effective border color target considering state layers.
    fn effective_border_color_target(&self) -> Color {
        let base = self.border_color.get();

        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(color) = state.border_color {
                    return color;
                }
            }
        }

        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(color) = state.border_color {
                    return color;
                }
            }
        }

        base
    }

    /// Get the effective corner radius target considering state layers.
    fn effective_corner_radius_target(&self) -> f32 {
        let base = self.corner_radius.get();

        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(radius) = state.corner_radius {
                    return radius;
                }
            }
        }

        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(radius) = state.corner_radius {
                    return radius;
                }
            }
        }

        base
    }

    /// Get the effective transform target considering state layers.
    fn effective_transform_target(&self) -> Transform {
        let base = self.transform.get();

        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(transform) = state.transform {
                    return transform;
                }
            }
        }

        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(transform) = state.transform {
                    return transform;
                }
            }
        }

        base
    }

    /// Get the effective elevation considering state layers (not animated).
    fn effective_elevation(&self) -> f32 {
        let base = self.elevation.get();

        if self.is_pressed {
            if let Some(ref state) = self.pressed_state {
                if let Some(elevation) = state.elevation {
                    return elevation;
                }
            }
        }

        if self.is_hovered {
            if let Some(ref state) = self.hover_state {
                if let Some(elevation) = state.elevation {
                    return elevation;
                }
            }
        }

        base
    }

    /// Check if any child widget needs layout
    fn any_child_needs_layout(&self) -> bool {
        // If children haven't been reconciled yet, we need layout
        if self.children_source.needs_reconcile() {
            return true;
        }
        self.children_source
            .get()
            .iter()
            .any(|child| child.needs_layout())
    }

    /// Get current padding (animated or static)
    fn animated_padding(&self) -> Padding {
        self.padding_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.padding.get())
    }

    /// Get current background color (animated or effective target)
    fn animated_background(&self) -> Color {
        self.background_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.effective_background_target())
    }

    /// Get current corner radius (animated or effective target)
    fn animated_corner_radius(&self) -> f32 {
        self.corner_radius_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.effective_corner_radius_target())
    }

    /// Get current border width (animated or effective target)
    fn animated_border_width(&self) -> f32 {
        self.border_width_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.effective_border_width_target())
    }

    /// Get current border color (animated or effective target)
    fn animated_border_color(&self) -> Color {
        self.border_color_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.effective_border_color_target())
    }

    /// Get current transform (animated or effective target)
    fn animated_transform(&self) -> Transform {
        self.transform_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.effective_transform_target())
    }

    /// Calculate constraints for children based on container dimensions and padding
    fn calc_child_constraints(&self) -> Constraints {
        let padding = self.padding.get();
        let child_max_width = (self.bounds.width - padding.horizontal()).max(0.0);
        let child_max_height = (self.bounds.height - padding.vertical()).max(0.0);

        // When container has explicit dimensions, pass them as min constraints
        // so layouts can use the full space for centering/alignment
        let width_length = self.width.as_ref().map(|w| w.get()).unwrap_or_default();
        let height_length = self.height.as_ref().map(|h| h.get()).unwrap_or_default();
        let child_min_width = if width_length.exact.is_some() {
            child_max_width
        } else {
            0.0
        };
        let child_min_height = if height_length.exact.is_some() {
            child_max_height
        } else {
            0.0
        };

        Constraints {
            min_width: child_min_width,
            min_height: child_min_height,
            max_width: child_max_width,
            max_height: child_max_height,
        }
    }

    /// Advance animation state (property animations)
    fn advance_animations(&mut self) {
        let mut any_animating = false;

        // Advance property animations
        // Note: width/height animation targets are set in layout() after we know content size
        advance_anim!(self, width_anim, any_animating);
        advance_anim!(self, height_anim, any_animating);

        // Use effective targets that consider state layers (hover/pressed overrides)
        let bg_target = self.effective_background_target();
        advance_anim!(self, background_anim, bg_target, any_animating);

        let corner_radius_target = self.effective_corner_radius_target();
        advance_anim!(
            self,
            corner_radius_anim,
            corner_radius_target,
            any_animating
        );

        advance_anim!(self, padding_anim, self.padding.get(), any_animating);

        let border_width_target = self.effective_border_width_target();
        advance_anim!(self, border_width_anim, border_width_target, any_animating);

        let border_color_target = self.effective_border_color_target();
        advance_anim!(self, border_color_anim, border_color_target, any_animating);

        let transform_target = self.effective_transform_target();
        advance_anim!(self, transform_anim, transform_target, any_animating);

        // Advance ripple animation
        if self.ripple_center.is_some() {
            let ripple_animating = self.advance_ripple();
            any_animating = any_animating || ripple_animating;
        }

        // Request next frame if any property animations are running
        if any_animating {
            request_animation_frame();
        }
    }

    /// Advance ripple animation, returns true if still animating
    fn advance_ripple(&mut self) -> bool {
        // Get ripple config from pressed_state
        let ripple_config = match &self.pressed_state {
            Some(state) => match &state.ripple {
                Some(config) => config.clone(),
                None => return false,
            },
            None => return false,
        };

        let Some(start_time) = self.ripple_start_time else {
            return false;
        };

        let elapsed = start_time.elapsed().as_secs_f32();

        // Expansion animation (0.4 seconds base, modified by expand_speed)
        let expand_duration = 0.4 / ripple_config.expand_speed;

        if self.ripple_fading {
            // Reverse animation: contract toward exit point
            let Some(fade_start) = self.ripple_fade_start_time else {
                return false;
            };
            let fade_elapsed = fade_start.elapsed().as_secs_f32();
            let fade_duration = 0.3 / ripple_config.fade_speed;

            // Calculate contraction progress (0 = just started fading, 1 = fully contracted)
            let contraction_t = (fade_elapsed / fade_duration).min(1.0);
            // Use ease-in curve for contraction (accelerates as it shrinks)
            let eased_t = contraction_t * contraction_t;

            // Shrink the ripple from its current progress back to 0
            self.ripple_progress = self.ripple_fade_start_progress * (1.0 - eased_t);

            // Interpolate center from start toward exit point
            if let (Some((start_x, start_y)), Some((exit_x, exit_y))) =
                (self.ripple_center, self.ripple_exit_center)
            {
                // The effective center moves toward the exit point as it contracts
                let current_x = start_x + (exit_x - start_x) * eased_t;
                let current_y = start_y + (exit_y - start_y) * eased_t;
                self.ripple_center = Some((current_x, current_y));
            }

            // Fade opacity as well for smooth disappearance
            self.ripple_opacity = (1.0 - eased_t).max(0.0);

            // Clear ripple when fully contracted
            if contraction_t >= 1.0 {
                self.ripple_center = None;
                self.ripple_exit_center = None;
                self.ripple_start_time = None;
                self.ripple_fade_start_time = None;
                self.ripple_fading = false;
                self.ripple_fade_start_progress = 0.0;
                return false;
            }
        } else {
            // Expansion animation
            if self.ripple_progress < 1.0 {
                self.ripple_progress = (elapsed / expand_duration).min(1.0);
                // Use ease-out curve for expansion
                self.ripple_progress = 1.0 - (1.0 - self.ripple_progress).powi(3);
            }
        }

        // Still animating if expanding or fading
        self.ripple_progress < 1.0 || self.ripple_fading
    }
}

/// Convert elevation level to shadow parameters
fn elevation_to_shadow(level: f32) -> Shadow {
    if level <= 0.0 {
        return Shadow::none();
    }

    let (offset_y, blur, alpha) = match level as i32 {
        1 => (1.0, 3.0, 0.12),
        2 => (2.0, 4.0, 0.16),
        3 => (3.0, 6.0, 0.19),
        4 => (4.0, 8.0, 0.20),
        5 => (6.0, 10.0, 0.22),
        _ => {
            let offset = (level * 1.2).min(12.0);
            let blur = (level * 2.0).min(24.0);
            let alpha = (0.12 + level * 0.02).min(0.25);
            (offset, blur, alpha)
        }
    };

    Shadow::new(
        (0.0, offset_y),
        blur,
        0.0,
        Color::rgba(0.0, 0.0, 0.0, alpha),
    )
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Container {
    fn layout(&mut self, constraints: Constraints) -> Size {
        // Always advance animations first
        self.advance_animations();

        // Get current property values (use animated values if available)
        let padding = self.animated_padding();
        let background = self.animated_background();
        let corner_radius = self.animated_corner_radius();
        let corner_curvature = self.corner_curvature.get();
        let elevation = self.effective_elevation();

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

        let child_needs_layout = self.any_child_needs_layout();

        // Size animations require full layout recalculation (for child constraints)
        let has_size_animations = self.width_anim.as_ref().is_some_and(|a| a.is_animating())
            || self.height_anim.as_ref().is_some_and(|a| a.is_animating());

        // Layout-affecting animations (affect child positioning or bounds)
        let has_layout_animations = self.padding_anim.as_ref().is_some_and(|a| a.is_animating())
            || self
                .border_width_anim
                .as_ref()
                .is_some_and(|a| a.is_animating());

        // Paint-only animations (visual only, no layout impact)
        let has_paint_animations = self
            .background_anim
            .as_ref()
            .is_some_and(|a| a.is_animating())
            || self
                .corner_radius_anim
                .as_ref()
                .is_some_and(|a| a.is_animating())
            || self
                .border_color_anim
                .as_ref()
                .is_some_and(|a| a.is_animating());

        // Downgrade to paint-only if only visuals changed (but not during layout-affecting animations)
        // Don't downgrade on first layout (when bounds are uninitialized)
        let bounds_initialized = self.bounds.width > 0.0 || self.bounds.height > 0.0;
        if self.needs_layout()
            && bounds_initialized
            && !padding_changed
            && !child_needs_layout
            && !has_size_animations
            && !has_layout_animations
            && visual_changed
        {
            self.dirty_flags = ChangeFlags::NEEDS_PAINT;
        }

        // Check if we need layout - paint-only animations don't require layout
        let needs_layout = self.needs_layout()
            || padding_changed
            || child_needs_layout
            || has_size_animations
            || has_layout_animations;

        // Request animation frame for paint-only animations without triggering layout
        if has_paint_animations && !needs_layout {
            request_animation_frame();
        }

        if !needs_layout {
            if visual_changed {
                self.cached_background = background;
                self.cached_corner_radius = corner_radius;
                self.cached_corner_curvature = corner_curvature;
                self.cached_elevation = elevation;
            }
            return Size::new(self.bounds.width, self.bounds.height);
        }

        // Update all cached values
        self.cached_padding = padding;
        self.cached_background = background;
        self.cached_corner_radius = corner_radius;
        self.cached_corner_curvature = corner_curvature;
        self.cached_elevation = elevation;

        // Reconcile dynamic children if needed
        let children = self.children_source.reconcile_and_get_mut();

        // During size animations, force ALL descendants to re-layout with new constraints.
        // This ensures nested children (like text inside inner containers) don't use
        // stale cached layouts from before animation.
        if has_size_animations {
            for child in children.iter_mut() {
                child.mark_dirty_recursive(ChangeFlags::NEEDS_LAYOUT);
            }
        }

        // Get width/height Length values
        let width_length = self.width.as_ref().map(|w| w.get()).unwrap_or_default();
        let height_length = self.height.as_ref().map(|h| h.get()).unwrap_or_default();

        // Calculate current container dimensions (use animated value if animating)
        // For width: use animation current value, but on initial state use constraints
        // so children can determine their natural size before animation target is set
        let current_width = if let Some(ref anim) = self.width_anim {
            if anim.is_initial() {
                // On initial layout, use constraint max so content can size naturally
                constraints.max_width
            } else {
                *anim.current()
            }
        } else if let Some(exact) = width_length.exact {
            exact
        } else {
            constraints.max_width
        };

        // Same for height
        let current_height = if let Some(ref anim) = self.height_anim {
            if anim.is_initial() {
                constraints.max_height
            } else {
                *anim.current()
            }
        } else if let Some(exact) = height_length.exact {
            exact
        } else {
            constraints.max_height
        };

        // Calculate undershoot (how much smaller than target we are during bounce-back)
        // This allows us to "eat" padding during undershoot to give children more space,
        // preventing unnecessary text re-wrapping during the bounce-back phase of spring animation
        let width_undershoot = if let Some(ref anim) = self.width_anim {
            let target = *anim.target();
            let current = *anim.current();
            // Undershoot = target - current, but only when current < target
            (target - current).max(0.0)
        } else {
            0.0
        };

        let height_undershoot = if let Some(ref anim) = self.height_anim {
            let target = *anim.target();
            let current = *anim.current();
            (target - current).max(0.0)
        } else {
            0.0
        };

        // Child constraints: current container size minus effective padding
        // During undershoot, we reduce padding to give children more space
        let effective_h_padding = (padding.horizontal() - width_undershoot).max(0.0);
        let effective_v_padding = (padding.vertical() - height_undershoot).max(0.0);
        let child_max_width = (current_width - effective_h_padding).max(0.0);
        let child_max_height = (current_height - effective_v_padding).max(0.0);

        // Calculate constraints for children (accounting for padding)
        // When container has explicit dimensions, pass them as min constraints
        // so layouts can use the full space for centering/alignment
        let child_min_width = if width_length.exact.is_some() {
            child_max_width
        } else {
            0.0
        };
        let child_min_height = if height_length.exact.is_some() {
            child_max_height
        } else {
            0.0
        };

        let child_constraints = Constraints {
            min_width: child_min_width,
            min_height: child_min_height,
            max_width: child_max_width,
            max_height: child_max_height,
        };

        // Use the layout strategy to position children
        let content_size = if !children.is_empty() {
            self.layout.layout(
                children,
                child_constraints,
                (self.bounds.x + padding.left, self.bounds.y + padding.top),
            )
        } else {
            Size::zero()
        };

        // Calculate container size including padding
        let content_width = content_size.width + padding.horizontal();
        let content_height = content_size.height + padding.vertical();

        // Update animation targets based on the effective target size.
        // Priority: exact > max(content_size, min_size)
        if let Some(ref mut anim) = self.width_anim {
            let effective_target = if let Some(exact) = width_length.exact {
                exact // Exact width takes precedence
            } else {
                let min_w = width_length.min.unwrap_or(0.0);
                content_width.max(min_w)
            };
            if (effective_target - *anim.target()).abs() > 0.001 {
                // On first layout, snap immediately to avoid startup animation
                if anim.is_initial() {
                    anim.set_immediate(effective_target);
                } else {
                    anim.animate_to(effective_target);
                    // Request frame since we just started/changed animation
                    request_animation_frame();
                }
            }
        }

        if let Some(ref mut anim) = self.height_anim {
            let effective_target = if let Some(exact) = height_length.exact {
                exact // Exact height takes precedence
            } else {
                let min_h = height_length.min.unwrap_or(0.0);
                content_height.max(min_h)
            };
            if (effective_target - *anim.target()).abs() > 0.001 {
                // On first layout, snap immediately to avoid startup animation
                if anim.is_initial() {
                    anim.set_immediate(effective_target);
                } else {
                    anim.animate_to(effective_target);
                    // Request frame since we just started/changed animation
                    request_animation_frame();
                }
            }
        }

        // Determine if we should allow shrinking below content size.
        // This happens when:
        // 1. overflow is Hidden (always clip content), OR
        // 2. A size animation is currently running (temporary clip during animation), OR
        // 3. Exact width/height is set (user wants that exact size, content clips)
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let has_exact_width = width_length.exact.is_some();
        let has_exact_height = height_length.exact.is_some();
        let allow_shrink_width =
            self.overflow == Overflow::Hidden || width_animating || has_exact_width;
        let allow_shrink_height =
            self.overflow == Overflow::Hidden || height_animating || has_exact_height;

        // Calculate final width
        // Priority: animation > exact > min constraint > content_width
        let mut width = if let Some(ref anim) = self.width_anim {
            if allow_shrink_width {
                *anim.current() // Use animated value directly, can shrink below content
            } else {
                content_width.max(*anim.current())
            }
        } else if let Some(exact) = width_length.exact {
            exact // Exact width takes precedence over content
        } else if let Some(min) = width_length.min {
            content_width.max(min)
        } else {
            content_width
        };

        // Apply max constraint from Length
        if let Some(max) = width_length.max {
            width = width.min(max);
        }

        // Calculate final height
        // Priority: animation > exact > min constraint > content_height
        let mut height = if let Some(ref anim) = self.height_anim {
            if allow_shrink_height {
                *anim.current() // Use animated value directly, can shrink below content
            } else {
                content_height.max(*anim.current())
            }
        } else if let Some(exact) = height_length.exact {
            exact // Exact height takes precedence over content
        } else if let Some(min) = height_length.min {
            content_height.max(min)
        } else {
            content_height
        };

        // Apply max constraint from Length
        if let Some(max) = height_length.max {
            height = height.min(max);
        }

        // Ensure minimum content size when not allowing shrink (but only if not using exact dimensions)
        if !allow_shrink_width && self.width_anim.is_none() && !has_exact_width {
            width = width.max(content_width);
        }
        if !allow_shrink_height && self.height_anim.is_none() && !has_exact_height {
            height = height.max(content_height);
        }

        let size = Size::new(
            width.max(constraints.min_width).min(constraints.max_width),
            height
                .max(constraints.min_height)
                .min(constraints.max_height),
        );

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        // Use animated values if available
        let background = self.animated_background();
        let corner_radius = self.animated_corner_radius();
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.effective_elevation();
        let shadow = elevation_to_shadow(elevation_level);
        let transform = self.animated_transform();
        let transform_origin = self.transform_origin.get();

        // Push transform if not identity
        let has_transform = !transform.is_identity();
        if has_transform {
            if transform_origin.is_center() {
                // Default behavior: let the primitives auto-center around their bounds
                ctx.push_transform(transform);
            } else {
                // Custom origin: pass the origin point to let primitives center in NDC space
                let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
                ctx.push_transform_with_origin(transform, origin_x, origin_y);
            }
        }

        // Draw background
        if let Some(ref gradient) = self.gradient {
            ctx.draw_gradient_rect_with_curvature(
                self.bounds,
                gradient.start_color,
                gradient.end_color,
                gradient.direction.into(),
                corner_radius,
                corner_curvature,
            );
        } else if background.a > 0.0 {
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

        // Draw border
        let border_width = self.animated_border_width();
        let border_color = self.animated_border_color();

        if border_width > 0.0 {
            ctx.draw_border_frame_with_curvature(
                self.bounds,
                border_color,
                corner_radius,
                border_width,
                corner_curvature,
            );
        }

        // Determine if we need to clip children
        // Clip when: overflow is Hidden OR a size animation is running OR exact size is set
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let has_exact_size = self.width.as_ref().is_some_and(|w| w.get().exact.is_some())
            || self
                .height
                .as_ref()
                .is_some_and(|h| h.get().exact.is_some());
        let should_clip = self.overflow == Overflow::Hidden
            || width_animating
            || height_animating
            || has_exact_size;

        // Push clip if needed
        if should_clip {
            ctx.push_clip(self.bounds, corner_radius, corner_curvature);
        }

        // Draw children
        for child in self.children_source.get() {
            child.paint(ctx);
        }

        // Pop clip if we pushed one
        if should_clip {
            ctx.pop_clip();
        }

        // Pop transform BEFORE drawing ripple so ripple uses screen coordinates
        if has_transform {
            ctx.pop_transform();
        }

        // Draw ripple effect as overlay (rendered after children/text, without transform)
        // Ripple uses absolute screen coordinates so it appears where user clicked
        if let Some((screen_cx, screen_cy)) = self.ripple_center {
            if let Some(ref pressed_state) = self.pressed_state {
                if let Some(ref ripple_config) = pressed_state.ripple {
                    if self.ripple_opacity > 0.0 {
                        // Calculate maximum radius to cover entire container
                        // Use distance from click point to farthest corner of bounds
                        let local_cx = screen_cx - self.bounds.x;
                        let local_cy = screen_cy - self.bounds.y;
                        let max_dist_x = local_cx.max(self.bounds.width - local_cx);
                        let max_dist_y = local_cy.max(self.bounds.height - local_cy);
                        let max_radius = (max_dist_x * max_dist_x + max_dist_y * max_dist_y).sqrt();

                        // Current radius based on progress
                        let current_radius = max_radius * self.ripple_progress;

                        // Ripple color with opacity
                        let ripple_color = Color::rgba(
                            ripple_config.color.r,
                            ripple_config.color.g,
                            ripple_config.color.b,
                            ripple_config.color.a * self.ripple_opacity,
                        );

                        // For transformed containers, we need to clip to the transformed bounds
                        // Transform the clip region to match the visual container position
                        let (clip_bounds, clip_transform) = if has_transform {
                            // Apply transform to get visual bounds for clipping
                            (self.bounds, Some((transform, transform_origin)))
                        } else {
                            (self.bounds, None)
                        };

                        // Draw ripple circle clipped to container bounds
                        ctx.draw_overlay_circle_clipped_with_transform(
                            screen_cx,
                            screen_cy,
                            current_radius,
                            ripple_color,
                            clip_bounds,
                            corner_radius,
                            corner_curvature,
                            clip_transform,
                        );
                    }
                }
            }
        }
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        // Get transform and corner radius for hit testing
        let transform = self.animated_transform();
        let transform_origin = self.transform_origin.get();
        let corner_radius = self.animated_corner_radius();

        // Transform the event coordinates from screen space to this container's local space
        // Use Cow to avoid cloning when no transformation is needed
        let local_event: Cow<'_, Event> = if !transform.is_identity() {
            if let Some((x, y)) = event.coords() {
                // Compute the centered transform (as used in rendering)
                let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
                let centered_transform = transform.center_at(origin_x, origin_y);
                // Inverse to go from screen space back to local space
                let inverse = centered_transform.inverse();

                // Only apply Y-flip compensation for rotation.
                // Rotation is applied in NDC space (Y-up) but hit testing is in
                // screen space (Y-down), which inverts the rotation direction.
                // Translation and scale don't need this compensation.
                let final_inverse = if transform.has_rotation() {
                    let y_flip = Transform::scale_xy(1.0, -1.0).center_at(origin_x, origin_y);
                    y_flip.then(&inverse).then(&y_flip)
                } else {
                    inverse
                };

                let (local_x, local_y) = final_inverse.transform_point(x, y);
                Cow::Owned(event.with_coords(local_x, local_y))
            } else {
                Cow::Borrowed(event)
            }
        } else {
            Cow::Borrowed(event)
        };

        // Let children handle first (with transformed coordinates for nested transforms)
        for child in self.children_source.reconcile_and_get_mut() {
            if child.event(&local_event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }

        // Handle our own events using transformed coordinates
        match local_event.as_ref() {
            Event::MouseEnter { x, y } => {
                if self.bounds.contains_rounded(*x, *y, corner_radius) {
                    let was_hovered = self.is_hovered;
                    self.is_hovered = true;
                    // Request repaint if state layer is defined and state changed
                    if !was_hovered && self.hover_state.is_some() {
                        request_animation_frame();
                    }
                    if let Some(ref callback) = self.on_hover {
                        callback(true);
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseMove { x, y } => {
                let was_hovered = self.is_hovered;
                self.is_hovered = self.bounds.contains_rounded(*x, *y, corner_radius);

                if was_hovered != self.is_hovered {
                    // Request repaint if state layer is defined
                    if self.hover_state.is_some() {
                        request_animation_frame();
                    }
                    if let Some(ref callback) = self.on_hover {
                        callback(self.is_hovered);
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseDown { x, y, button } => {
                if self.bounds.contains_rounded(*x, *y, corner_radius)
                    && *button == MouseButton::Left
                {
                    let was_pressed = self.is_pressed;
                    self.is_pressed = true;

                    // Start ripple animation if configured
                    let has_ripple = self
                        .pressed_state
                        .as_ref()
                        .is_some_and(|s| s.ripple.is_some());
                    if has_ripple {
                        // Store click position in SCREEN coordinates (absolute)
                        // We use original event coords, not transformed ones, so ripple
                        // appears exactly where user clicked regardless of container transform
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        self.ripple_center = Some((screen_x, screen_y));
                        self.ripple_progress = 0.0;
                        self.ripple_opacity = 1.0;
                        self.ripple_fading = false;
                        self.ripple_start_time = Some(std::time::Instant::now());
                        request_animation_frame();
                    }

                    // Request repaint if state layer is defined and state changed
                    if !was_pressed && self.pressed_state.is_some() {
                        request_animation_frame();
                    }
                    // Only consume the event if we have a click handler
                    if self.on_click.is_some() {
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseUp { x, y, button } => {
                if self.is_pressed && *button == MouseButton::Left {
                    let was_pressed = self.is_pressed;
                    self.is_pressed = false;

                    // Start ripple reverse animation if ripple is active
                    if self.ripple_center.is_some() && self.ripple_opacity > 0.0 {
                        // Store the release position in SCREEN coordinates (absolute)
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        self.ripple_exit_center = Some((screen_x, screen_y));
                        self.ripple_fading = true;
                        self.ripple_fade_start_time = Some(std::time::Instant::now());
                        self.ripple_fade_start_progress = self.ripple_progress;
                        request_animation_frame();
                    }

                    // Request repaint if state layer is defined and state changed
                    if was_pressed && self.pressed_state.is_some() {
                        request_animation_frame();
                    }
                    if self.bounds.contains_rounded(*x, *y, corner_radius) {
                        if let Some(ref callback) = self.on_click {
                            callback();
                            return EventResponse::Handled;
                        }
                    }
                }
            }
            Event::MouseLeave => {
                let was_hovered = self.is_hovered;
                let was_pressed = self.is_pressed;
                if self.is_hovered {
                    self.is_hovered = false;
                    if let Some(ref callback) = self.on_hover {
                        callback(false);
                    }
                }
                self.is_pressed = false;

                // Start ripple reverse animation if ripple is active
                if self.ripple_center.is_some() && self.ripple_opacity > 0.0 {
                    // For MouseLeave, use the center of the container as exit point
                    // (we don't have the exact leave position)
                    self.ripple_exit_center =
                        Some((self.bounds.width / 2.0, self.bounds.height / 2.0));
                    self.ripple_fading = true;
                    self.ripple_fade_start_time = Some(std::time::Instant::now());
                    self.ripple_fade_start_progress = self.ripple_progress;
                    request_animation_frame();
                }

                // Request repaint if state layer is defined and state changed
                if (was_hovered && self.hover_state.is_some())
                    || (was_pressed && self.pressed_state.is_some())
                {
                    request_animation_frame();
                }
            }
            Event::Scroll {
                x,
                y,
                delta_x,
                delta_y,
                source,
            } => {
                if self.bounds.contains_rounded(*x, *y, corner_radius) {
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

        // Calculate constraints before borrowing children
        let child_constraints = self.calc_child_constraints();
        let padding = self.padding.get();

        // Re-layout children with current bounds minus padding
        // The layout will position children relative to the new origin
        let children = self.children_source.reconcile_and_get_mut();
        if !children.is_empty() {
            self.layout.layout(
                children,
                child_constraints,
                (x + padding.left, y + padding.top),
            );
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

    fn mark_dirty_recursive(&mut self, flags: ChangeFlags) {
        self.dirty_flags |= flags;
        // Use get_mut since we're operating on existing children (reconciliation already done)
        for child in self.children_source.get_mut() {
            child.mark_dirty_recursive(flags);
        }
    }

    fn needs_layout(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_LAYOUT)
    }

    fn needs_paint(&self) -> bool {
        self.dirty_flags.contains(ChangeFlags::NEEDS_PAINT)
    }

    fn clear_dirty(&mut self) {
        self.dirty_flags = ChangeFlags::empty();
        // Use get_mut since we're operating on existing children (reconciliation already done)
        for child in self.children_source.get_mut() {
            child.clear_dirty();
        }
    }
}

pub fn container() -> Container {
    Container::new()
}
