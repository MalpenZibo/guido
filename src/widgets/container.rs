use std::sync::Arc;
use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition};
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{request_animation_frame, ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::primitives::{GradientDir, Shadow};
use crate::renderer::PaintContext;
use crate::transform::Transform;

use super::children::ChildrenSource;
use super::into_child::{IntoChild, IntoChildren};
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

// Ripple animation speeds
const RIPPLE_ENTER_SPEED: f32 = 0.12;
const RIPPLE_EXIT_SPEED: f32 = 0.20;
const CLICK_RIPPLE_EXPAND_SPEED: f32 = 0.08;
const CLICK_RIPPLE_REVERSE_SPEED: f32 = 0.15;

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

    // Ripple effect state
    ripple_enabled: bool,
    ripple_center: Option<(f32, f32)>,
    ripple_progress: f32,
    ripple_color: Color,
    ripple_from_click: bool,
    ripple_is_exit: bool,
    last_mouse_pos: Option<(f32, f32)>,

    // Click ripple effect
    click_ripple_center: Option<(f32, f32)>,
    click_ripple_progress: f32,
    click_ripple_reversing: bool,
    click_ripple_release_pos: Option<(f32, f32)>,

    // Animation state
    width_anim: Option<AnimationState<f32>>,
    height_anim: Option<AnimationState<f32>>,
    background_anim: Option<AnimationState<Color>>,
    corner_radius_anim: Option<AnimationState<f32>>,
    padding_anim: Option<AnimationState<Padding>>,
    border_width_anim: Option<AnimationState<f32>>,
    border_color_anim: Option<AnimationState<Color>>,
    transform_anim: Option<AnimationState<Transform>>,
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

    pub fn ripple(mut self) -> Self {
        self.ripple_enabled = true;
        self
    }

    pub fn ripple_with_color(mut self, color: Color) -> Self {
        self.ripple_enabled = true;
        self.ripple_color = color;
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
    pub fn rotate(mut self, degrees: impl IntoMaybeDyn<f32>) -> Self {
        let degrees = degrees.into_maybe_dyn();
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            Transform::rotate_degrees(degrees.get())
        }));
        self
    }

    /// Scale this container uniformly
    /// Scaling is applied around the center of the widget bounds
    pub fn scale(mut self, s: impl IntoMaybeDyn<f32>) -> Self {
        let s = s.into_maybe_dyn();
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || Transform::scale(s.get())));
        self
    }

    /// Scale this container non-uniformly
    pub fn scale_xy(mut self, sx: impl IntoMaybeDyn<f32>, sy: impl IntoMaybeDyn<f32>) -> Self {
        let sx = sx.into_maybe_dyn();
        let sy = sy.into_maybe_dyn();
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            Transform::scale_xy(sx.get(), sy.get())
        }));
        self
    }

    /// Translate (move) this container by the given offset
    pub fn translate(mut self, x: impl IntoMaybeDyn<f32>, y: impl IntoMaybeDyn<f32>) -> Self {
        let x = x.into_maybe_dyn();
        let y = y.into_maybe_dyn();
        self.transform = MaybeDyn::Dynamic(std::sync::Arc::new(move || {
            Transform::translate(x.get(), y.get())
        }));
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

    /// Get current background color (animated or static)
    fn animated_background(&self) -> Color {
        self.background_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.background.get())
    }

    /// Get current corner radius (animated or static)
    fn animated_corner_radius(&self) -> f32 {
        self.corner_radius_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.corner_radius.get())
    }

    /// Get current border width (animated or static)
    fn animated_border_width(&self) -> f32 {
        self.border_width_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.border_width.get())
    }

    /// Get current border color (animated or static)
    fn animated_border_color(&self) -> Color {
        self.border_color_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.border_color.get())
    }

    /// Get current transform (animated or static)
    fn animated_transform(&self) -> Transform {
        self.transform_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or_else(|| self.transform.get())
    }

    /// Advance animation state (ripple effects and property animations)
    fn advance_animations(&mut self) {
        let mut any_animating = false;

        // Advance property animations
        // Note: width/height animation targets are set in layout() after we know content size
        advance_anim!(self, width_anim, any_animating);
        advance_anim!(self, height_anim, any_animating);
        advance_anim!(self, background_anim, self.background.get(), any_animating);
        advance_anim!(
            self,
            corner_radius_anim,
            self.corner_radius.get(),
            any_animating
        );
        advance_anim!(self, padding_anim, self.padding.get(), any_animating);
        advance_anim!(
            self,
            border_width_anim,
            self.border_width.get(),
            any_animating
        );
        advance_anim!(
            self,
            border_color_anim,
            self.border_color.get(),
            any_animating
        );
        advance_anim!(self, transform_anim, self.transform.get(), any_animating);

        // Request next frame if any property animations are running
        if any_animating {
            request_animation_frame();
        }

        // Advance hover ripple animation
        if self.ripple_enabled && self.ripple_center.is_some() {
            if self.ripple_progress < 1.0 {
                request_animation_frame();
                let speed = if self.ripple_is_exit {
                    RIPPLE_EXIT_SPEED
                } else {
                    RIPPLE_ENTER_SPEED
                };
                self.ripple_progress = (self.ripple_progress + speed).min(1.0);
            } else if self.ripple_is_exit {
                request_animation_frame();
                self.ripple_center = None;
                self.ripple_progress = 0.0;
                self.ripple_is_exit = false;
            }
        }

        // Advance click ripple animation
        if self.ripple_enabled && self.click_ripple_center.is_some() {
            if self.click_ripple_reversing {
                request_animation_frame();
                if self.click_ripple_progress > 0.0 {
                    self.click_ripple_progress =
                        (self.click_ripple_progress - CLICK_RIPPLE_REVERSE_SPEED).max(0.0);
                } else {
                    self.click_ripple_center = None;
                    self.click_ripple_progress = 0.0;
                    self.click_ripple_reversing = false;
                    self.click_ripple_release_pos = None;
                }
            } else if self.click_ripple_progress < 1.0 {
                request_animation_frame();
                self.click_ripple_progress =
                    (self.click_ripple_progress + CLICK_RIPPLE_EXPAND_SPEED).min(1.0);
            }
        }
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

        let child_needs_layout = self.any_child_needs_layout();
        let has_ripple_animations =
            self.ripple_center.is_some() || self.click_ripple_center.is_some();

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
        if self.needs_layout()
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
            || has_ripple_animations // Ripples need layout for animation frame requests
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
        let child_constraints = Constraints {
            min_width: 0.0,
            min_height: 0.0,
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
        let elevation_level = self.elevation.get();
        let shadow = elevation_to_shadow(elevation_level);
        let transform = self.animated_transform();

        // Push transform if not identity
        // Note: Centering around widget bounds is done in to_vertices() in NDC space
        let has_transform = !transform.is_identity();
        if has_transform {
            ctx.push_transform(transform);
        }

        // Draw background
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

        // Draw ripple effects
        if self.ripple_enabled {
            // Hover ripple
            if let Some((cx, cy)) = self.ripple_center {
                let dx1 = cx - self.bounds.x;
                let dx2 = (self.bounds.x + self.bounds.width) - cx;
                let dy1 = cy - self.bounds.y;
                let dy2 = (self.bounds.y + self.bounds.height) - cy;
                let max_radius = (dx1.max(dx2).powi(2) + dy1.max(dy2).powi(2)).sqrt();
                let current_radius = max_radius * self.ripple_progress;

                let alpha = if self.ripple_is_exit {
                    self.ripple_color.a * (1.0 - self.ripple_progress).powi(2)
                } else {
                    self.ripple_color.a * (1.0 - self.ripple_progress * 0.5)
                };

                if current_radius > 0.0 && alpha > 0.0 {
                    let ripple_color = Color::rgba(
                        self.ripple_color.r,
                        self.ripple_color.g,
                        self.ripple_color.b,
                        alpha,
                    );
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

            // Click ripple
            if let Some((cx, cy)) = self.click_ripple_center {
                let (center_x, center_y) = if self.click_ripple_reversing {
                    if let Some((rx, ry)) = self.click_ripple_release_pos {
                        let t = self.click_ripple_progress;
                        (cx + (rx - cx) * (1.0 - t), cy + (ry - cy) * (1.0 - t))
                    } else {
                        (cx, cy)
                    }
                } else {
                    (cx, cy)
                };

                let dx1 = center_x - self.bounds.x;
                let dx2 = (self.bounds.x + self.bounds.width) - center_x;
                let dy1 = center_y - self.bounds.y;
                let dy2 = (self.bounds.y + self.bounds.height) - center_y;
                let max_radius = (dx1.max(dx2).powi(2) + dy1.max(dy2).powi(2)).sqrt();
                let current_radius = max_radius * self.click_ripple_progress;

                let alpha = self.ripple_color.a * (1.0 - self.click_ripple_progress * 0.5);

                if current_radius > 0.0 && alpha > 0.0 {
                    let ripple_color = Color::rgba(
                        self.ripple_color.r,
                        self.ripple_color.g,
                        self.ripple_color.b,
                        alpha,
                    );
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

        // Pop transform if we pushed one
        if has_transform {
            ctx.pop_transform();
        }
    }

    fn event(&mut self, event: &Event) -> EventResponse {
        // Let children handle first
        for child in self.children_source.reconcile_and_get_mut() {
            if child.event(event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }

        // Handle our own events (same as before)
        match event {
            Event::MouseEnter { x, y } => {
                if self.bounds.contains(*x, *y) {
                    self.is_hovered = true;
                    self.last_mouse_pos = Some((*x, *y));
                    if let Some(ref callback) = self.on_hover {
                        callback(true);
                    }
                    if self.ripple_enabled && self.ripple_center.is_none() {
                        self.ripple_center = Some((*x, *y));
                        self.ripple_progress = 0.0;
                        self.ripple_from_click = false;
                        request_animation_frame();
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseMove { x, y } => {
                let was_hovered = self.is_hovered;
                self.is_hovered = self.bounds.contains(*x, *y);

                if self.is_hovered {
                    self.last_mouse_pos = Some((*x, *y));
                }

                if was_hovered != self.is_hovered {
                    if let Some(ref callback) = self.on_hover {
                        callback(self.is_hovered);
                    }

                    if self.is_hovered {
                        if self.ripple_enabled && self.ripple_center.is_none() {
                            self.ripple_center = Some((*x, *y));
                            self.ripple_progress = 0.0;
                            self.ripple_from_click = false;
                            request_animation_frame();
                        }
                    } else if self.ripple_enabled && !self.ripple_from_click {
                        if let Some((lx, ly)) = self.last_mouse_pos {
                            self.ripple_center = Some((lx, ly));
                            self.ripple_progress = 0.0;
                            self.ripple_is_exit = true;
                            request_animation_frame();
                        }
                    }
                    return EventResponse::Handled;
                }
            }
            Event::MouseDown { x, y, button } => {
                if self.bounds.contains(*x, *y) && *button == MouseButton::Left {
                    self.is_pressed = true;
                    if self.ripple_enabled {
                        self.click_ripple_center = Some((*x, *y));
                        self.click_ripple_progress = 0.0;
                        self.click_ripple_reversing = false;
                        self.click_ripple_release_pos = None;
                        request_animation_frame();
                    }
                    // Only consume the event if we have a click handler
                    if self.on_click.is_some() || self.ripple_enabled {
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseUp { x, y, button } => {
                if self.is_pressed && *button == MouseButton::Left {
                    self.is_pressed = false;
                    if self.ripple_enabled && self.click_ripple_center.is_some() {
                        self.click_ripple_reversing = true;
                        self.click_ripple_release_pos = Some((*x, *y));
                        request_animation_frame();
                    }
                    if self.bounds.contains(*x, *y) {
                        if let Some(ref callback) = self.on_click {
                            callback();
                            return EventResponse::Handled;
                        }
                        // If we have ripple but no click handler, still mark as handled
                        if self.ripple_enabled {
                            return EventResponse::Handled;
                        }
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
                if was_hovered && self.ripple_enabled && !self.ripple_from_click {
                    if let Some((x, y)) = self.last_mouse_pos {
                        self.ripple_center = Some((x, y));
                        self.ripple_progress = 0.0;
                        self.ripple_is_exit = true;
                        request_animation_frame();
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

        // Re-layout children with current bounds minus padding
        let children = self.children_source.reconcile_and_get_mut();
        if !children.is_empty() {
            let child_max_width = (self.bounds.width - padding.horizontal()).max(0.0);
            let child_max_height = (self.bounds.height - padding.vertical()).max(0.0);

            let child_constraints = Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: child_max_width,
                max_height: child_max_height,
            };

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
        // Use reconcile_and_get_mut to ensure children are available
        for child in self.children_source.reconcile_and_get_mut() {
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
        for child in self.children_source.reconcile_and_get_mut() {
            child.clear_dirty();
        }
    }
}

pub fn container() -> Container {
    Container::new()
}
