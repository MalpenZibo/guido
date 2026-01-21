use std::sync::Arc;
use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transform, Transition};
use crate::layout::{Constraints, Flex, Layout, Size};
use crate::reactive::{request_animation_frame, ChangeFlags, IntoMaybeDyn, MaybeDyn, WidgetId};
use crate::renderer::primitives::{GradientDir, Shadow};
use crate::renderer::PaintContext;

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
        }
    }

    /// Start animating to a new target value
    fn animate_to(&mut self, new_target: T) {
        // Don't restart if we're already animating to this target
        // Use custom comparison to avoid floating point precision issues
        if self.targets_equal(&new_target, &self.target) {
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

    /// Compare targets with appropriate precision for the type
    fn targets_equal(&self, a: &T, b: &T) -> bool {
        a == b
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
            self.transition.timing.evaluate(t, None)
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

    /// Get start value (value when animation began)
    fn start(&self) -> &T {
        &self.start
    }
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
    border: Option<Border>,
    elevation: MaybeDyn<f32>,
    min_width: Option<MaybeDyn<f32>>,
    min_height: Option<MaybeDyn<f32>>,
    overflow: Overflow,
    bounds: Rect,

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
    transform_anim: Option<AnimationState<Transform>>,

    // Transform property
    transform: MaybeDyn<Transform>,
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
            border: None,
            elevation: MaybeDyn::Static(0.0),
            min_width: None,
            min_height: None,
            overflow: Overflow::Visible,
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
            transform_anim: None,
            transform: MaybeDyn::Static(Transform::default()),
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
    pub fn border(mut self, width: f32, color: Color) -> Self {
        self.border = Some(Border::new(width, color));
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

    pub fn min_width(mut self, width: impl IntoMaybeDyn<f32>) -> Self {
        self.min_width = Some(width.into_maybe_dyn());
        self
    }

    pub fn min_height(mut self, height: impl IntoMaybeDyn<f32>) -> Self {
        self.min_height = Some(height.into_maybe_dyn());
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

    /// Set the transform (translate, scale, rotate) - does not trigger layout
    pub fn transform(mut self, transform: impl IntoMaybeDyn<Transform>) -> Self {
        self.transform = transform.into_maybe_dyn();
        self
    }

    /// Enable animation for width changes
    pub fn animate_width(mut self, transition: Transition) -> Self {
        // Initialize with current min_width or 0
        let initial = self.min_width.as_ref().map(|w| w.get()).unwrap_or(0.0);
        self.width_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for height changes
    pub fn animate_height(mut self, transition: Transition) -> Self {
        // Initialize with current min_height or 0
        let initial = self.min_height.as_ref().map(|h| h.get()).unwrap_or(0.0);
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

    /// Enable animation for transform changes
    pub fn animate_transform(mut self, transition: Transition) -> Self {
        let initial = self.transform.get();
        self.transform_anim = Some(AnimationState::new(initial, transition));
        self
    }

    /// Check if any child widget needs layout
    fn any_child_needs_layout(&self) -> bool {
        self.children_source
            .get()
            .iter()
            .any(|child| child.needs_layout())
    }

    /// Advance animation state (ripple effects and property animations)
    fn advance_animations(&mut self) {
        let mut any_animating = false;

        // Advance property animations
        if let Some(ref mut anim) = self.width_anim {
            if let Some(ref min_w) = self.min_width {
                let target = min_w.get();
                // Use epsilon comparison for f32 to avoid floating point precision issues
                if (target - *anim.target()).abs() > 0.001 {
                    anim.animate_to(target);
                }
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        if let Some(ref mut anim) = self.height_anim {
            if let Some(ref min_h) = self.min_height {
                let target = min_h.get();
                // Use epsilon comparison for f32 to avoid floating point precision issues
                if (target - *anim.target()).abs() > 0.001 {
                    anim.animate_to(target);
                }
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        if let Some(ref mut anim) = self.background_anim {
            let target = self.background.get();
            if target != *anim.target() {
                anim.animate_to(target);
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        if let Some(ref mut anim) = self.corner_radius_anim {
            let target = self.corner_radius.get();
            // Use epsilon comparison for f32 to avoid floating point precision issues
            if (target - *anim.target()).abs() > 0.001 {
                anim.animate_to(target);
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        if let Some(ref mut anim) = self.padding_anim {
            let target = self.padding.get();
            if target != *anim.target() {
                anim.animate_to(target);
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        if let Some(ref mut anim) = self.transform_anim {
            let target = self.transform.get();
            if target != *anim.target() {
                anim.animate_to(target);
            }
            if anim.is_animating() {
                anim.advance();
                any_animating = true;
            }
        }

        // Request next frame if any property animations are running
        if any_animating {
            request_animation_frame();
        }

        // Advance hover ripple animation
        if self.ripple_enabled && self.ripple_center.is_some() {
            if self.ripple_progress < 1.0 {
                request_animation_frame();
                let speed = if self.ripple_is_exit { 0.20 } else { 0.12 };
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
                    let speed = 0.15;
                    self.click_ripple_progress = (self.click_ripple_progress - speed).max(0.0);
                } else {
                    self.click_ripple_center = None;
                    self.click_ripple_progress = 0.0;
                    self.click_ripple_reversing = false;
                    self.click_ripple_release_pos = None;
                }
            } else if self.click_ripple_progress < 1.0 {
                request_animation_frame();
                let speed = 0.08;
                self.click_ripple_progress = (self.click_ripple_progress + speed).min(1.0);
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
        let padding = if let Some(ref anim) = self.padding_anim {
            *anim.current()
        } else {
            self.padding.get()
        };
        let background = if let Some(ref anim) = self.background_anim {
            *anim.current()
        } else {
            self.background.get()
        };
        let corner_radius = if let Some(ref anim) = self.corner_radius_anim {
            *anim.current()
        } else {
            self.corner_radius.get()
        };
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
        let has_animations = has_ripple_animations || has_size_animations;

        // Downgrade to paint-only if only visuals changed (but not during size animations)
        if self.needs_layout()
            && !padding_changed
            && !child_needs_layout
            && !has_size_animations
            && visual_changed
        {
            self.dirty_flags = ChangeFlags::NEEDS_PAINT;
        }

        // Check if we need layout
        let needs_layout =
            self.needs_layout() || padding_changed || child_needs_layout || has_animations;

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

        // During size animations, use the LARGER of (start, target) for child constraints.
        // This prevents children (like text) from re-wrapping during collapse animations.
        // Children maintain their layout at the larger size and get clipped instead.
        let child_max_width = if let Some(ref anim) = self.width_anim {
            if anim.is_animating() {
                // Use the larger of start/target so children don't re-wrap during collapse
                let start = *anim.start();
                let target = *anim.target();
                let anim_max = start.max(target);
                // Use the larger of animation bounds and parent constraints
                let base = constraints.max_width.max(anim_max);
                (base - padding.horizontal()).max(0.0)
            } else {
                (constraints.max_width - padding.horizontal()).max(0.0)
            }
        } else {
            (constraints.max_width - padding.horizontal()).max(0.0)
        };

        let child_max_height = if let Some(ref anim) = self.height_anim {
            if anim.is_animating() {
                // Use the larger of start/target so children don't re-wrap during collapse
                let start = *anim.start();
                let target = *anim.target();
                let anim_max = start.max(target);
                // Use the larger of animation bounds and parent constraints
                let base = constraints.max_height.max(anim_max);
                (base - padding.vertical()).max(0.0)
            } else {
                (constraints.max_height - padding.vertical()).max(0.0)
            }
        } else {
            (constraints.max_height - padding.vertical()).max(0.0)
        };

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

        // Determine if we should allow shrinking below content size.
        // This happens when:
        // 1. overflow is Hidden (always clip content), OR
        // 2. A size animation is currently running (temporary clip during animation)
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let allow_shrink_width = self.overflow == Overflow::Hidden || width_animating;
        let allow_shrink_height = self.overflow == Overflow::Hidden || height_animating;

        // Calculate final width
        let mut width = if let Some(ref anim) = self.width_anim {
            if allow_shrink_width {
                *anim.current() // Use animated value directly, can shrink below content
            } else {
                content_width.max(*anim.current())
            }
        } else if let Some(ref min_w) = self.min_width {
            content_width.max(min_w.get())
        } else {
            content_width
        };

        // Calculate final height
        let mut height = if let Some(ref anim) = self.height_anim {
            if allow_shrink_height {
                *anim.current() // Use animated value directly, can shrink below content
            } else {
                content_height.max(*anim.current())
            }
        } else if let Some(ref min_h) = self.min_height {
            content_height.max(min_h.get())
        } else {
            content_height
        };

        // Ensure minimum content size when not allowing shrink
        if !allow_shrink_width && self.width_anim.is_none() {
            width = width.max(content_width);
        }
        if !allow_shrink_height && self.height_anim.is_none() {
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
        let background = if let Some(ref anim) = self.background_anim {
            *anim.current()
        } else {
            self.background.get()
        };
        let corner_radius = if let Some(ref anim) = self.corner_radius_anim {
            *anim.current()
        } else {
            self.corner_radius.get()
        };
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.elevation.get();
        let shadow = elevation_to_shadow(elevation_level);

        // Apply transform if available
        let transform = if let Some(ref anim) = self.transform_anim {
            *anim.current()
        } else {
            self.transform.get()
        };
        let has_transform = transform != Transform::default();

        if has_transform {
            ctx.push_transform(transform, self.bounds);
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
        if let Some(ref border) = self.border {
            ctx.draw_border_frame_with_curvature(
                self.bounds,
                border.color,
                corner_radius,
                border.width,
                corner_curvature,
            );
        }

        // Determine if we need to clip children
        // Clip when: overflow is Hidden OR a size animation is running
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let should_clip = self.overflow == Overflow::Hidden || width_animating || height_animating;

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

        // Pop transform if applied
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

        // Re-layout children with their current constraints
        let children = self.children_source.reconcile_and_get_mut();
        if !children.is_empty() {
            // During size animations, use the LARGER of (start, target) for child constraints.
            // This prevents children (like text) from re-wrapping during collapse animations.
            let child_max_width = if let Some(ref anim) = self.width_anim {
                if anim.is_animating() {
                    let start = *anim.start();
                    let target = *anim.target();
                    let anim_max = start.max(target);
                    (anim_max - padding.horizontal()).max(0.0)
                } else {
                    (self.bounds.width - padding.horizontal()).max(0.0)
                }
            } else {
                (self.bounds.width - padding.horizontal()).max(0.0)
            };

            let child_max_height = if let Some(ref anim) = self.height_anim {
                if anim.is_animating() {
                    let start = *anim.start();
                    let target = *anim.target();
                    let anim_max = start.max(target);
                    (anim_max - padding.vertical()).max(0.0)
                } else {
                    (self.bounds.height - padding.vertical()).max(0.0)
                }
            } else {
                (self.bounds.height - padding.vertical()).max(0.0)
            };

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
