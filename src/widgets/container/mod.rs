//! Container widget and related functionality.

mod animations;
mod ripple;
mod scrollable;

pub use animations::{AdvanceResult, AnimationState, get_animated_value};
pub use ripple::RippleState;

use std::borrow::Cow;
use std::sync::Arc;

use crate::advance_anim;
use crate::animation::Transition;
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{
    IntoMaybeDyn, MaybeDyn, WidgetId, finish_layout_tracking, focused_widget,
    get_needs_layout_flag, register_relayout_boundary, request_animation_frame,
    set_needs_layout_flag, set_widget_parent, start_layout_tracking,
};
use crate::renderer::PaintContext;
use crate::renderer::primitives::{GradientDir, Shadow};
#[cfg(feature = "renderer_v2")]
use crate::renderer_v2::PaintContextV2;
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;

use super::children::ChildrenSource;
use super::into_child::{IntoChild, IntoChildren};
use super::scroll::{
    ScrollAxis, ScrollState, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility,
};
use super::state_layer::{StateStyle, resolve_background};
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

pub struct Container {
    pub(super) widget_id: WidgetId,

    // Layout and children
    pub(super) layout: Box<dyn Layout>,
    pub(super) children_source: ChildrenSource,

    // Styling properties
    pub(super) padding: MaybeDyn<Padding>,
    pub(super) background: MaybeDyn<Color>,
    pub(super) gradient: Option<LinearGradient>,
    pub(super) corner_radius: MaybeDyn<f32>,
    pub(super) corner_curvature: MaybeDyn<f32>,
    pub(super) border_width: MaybeDyn<f32>,
    pub(super) border_color: MaybeDyn<Color>,
    pub(super) elevation: MaybeDyn<f32>,
    pub(super) width: Option<MaybeDyn<Length>>,
    pub(super) height: Option<MaybeDyn<Length>>,
    pub(super) overflow: Overflow,
    pub(super) bounds: Rect,
    pub(super) transform: MaybeDyn<Transform>,
    pub(super) transform_origin: MaybeDyn<TransformOrigin>,

    // Event callbacks
    pub(super) on_click: Option<ClickCallback>,
    pub(super) on_hover: Option<HoverCallback>,
    pub(super) on_scroll: Option<ScrollCallback>,

    // Internal state for event handling
    pub(super) is_hovered: bool,
    pub(super) is_pressed: bool,

    // Animation state
    pub(super) width_anim: Option<AnimationState<f32>>,
    pub(super) height_anim: Option<AnimationState<f32>>,
    pub(super) background_anim: Option<AnimationState<Color>>,
    pub(super) corner_radius_anim: Option<AnimationState<f32>>,
    pub(super) padding_anim: Option<AnimationState<Padding>>,
    pub(super) border_width_anim: Option<AnimationState<f32>>,
    pub(super) border_color_anim: Option<AnimationState<Color>>,
    pub(super) transform_anim: Option<AnimationState<Transform>>,

    // State layer styles (hover/pressed/focused overrides)
    pub(super) hover_state: Option<StateStyle>,
    pub(super) pressed_state: Option<StateStyle>,
    pub(super) focused_state: Option<StateStyle>,

    // Scroll configuration
    pub(super) scroll_axis: ScrollAxis,
    pub(super) scrollbar_visibility: ScrollbarVisibility,
    pub(super) scrollbar_config: ScrollbarConfig,
    pub(super) scroll_state: ScrollState,

    // Vertical scrollbar containers
    pub(super) v_scrollbar_track: Option<Box<Container>>,
    pub(super) v_scrollbar_handle: Option<Box<Container>>,
    pub(super) v_scrollbar_scale_anim: Option<AnimationState<f32>>,

    // Horizontal scrollbar containers
    pub(super) h_scrollbar_track: Option<Box<Container>>,
    pub(super) h_scrollbar_handle: Option<Box<Container>>,
    pub(super) h_scrollbar_scale_anim: Option<AnimationState<f32>>,

    // Ripple animation state
    pub(super) ripple: RippleState,

    // Constraint caching for layout skip optimization
    pub(super) last_constraints: Option<Constraints>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            widget_id: WidgetId::next(),
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
            focused_state: None,
            scroll_axis: ScrollAxis::None,
            scrollbar_visibility: ScrollbarVisibility::Always,
            scrollbar_config: ScrollbarConfig::default(),
            scroll_state: ScrollState::default(),
            v_scrollbar_track: None,
            v_scrollbar_handle: None,
            v_scrollbar_scale_anim: None,
            h_scrollbar_track: None,
            h_scrollbar_handle: None,
            h_scrollbar_scale_anim: None,
            ripple: RippleState::new(),
            last_constraints: None,
        }
    }

    /// Set the layout strategy for this container
    pub fn layout(mut self, layout: impl Layout + 'static) -> Self {
        self.layout = Box::new(layout);
        self
    }

    /// Add a single child (static or dynamic)
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
        self.padding = MaybeDyn::Dynamic(Arc::new(move || Padding::all(value.get())));
        self
    }

    pub fn padding_xy(
        mut self,
        horizontal: impl IntoMaybeDyn<f32>,
        vertical: impl IntoMaybeDyn<f32>,
    ) -> Self {
        let h = horizontal.into_maybe_dyn();
        let v = vertical.into_maybe_dyn();
        self.padding = MaybeDyn::Dynamic(Arc::new(move || Padding {
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
    pub fn width(mut self, width: impl IntoMaybeDyn<Length>) -> Self {
        self.width = Some(width.into_maybe_dyn());
        self
    }

    /// Set the height of the container.
    pub fn height(mut self, height: impl IntoMaybeDyn<Length>) -> Self {
        self.height = Some(height.into_maybe_dyn());
        self
    }

    /// Set the overflow behavior for content that exceeds container bounds
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Enable scrolling on this container.
    pub fn scrollable(mut self, axis: ScrollAxis) -> Self {
        self.scroll_axis = axis;
        self
    }

    /// Configure scrollbar visibility.
    pub fn scrollbar_visibility(mut self, visibility: ScrollbarVisibility) -> Self {
        self.scrollbar_visibility = visibility;
        self
    }

    /// Customize scrollbar appearance.
    pub fn scrollbar<F>(mut self, f: F) -> Self
    where
        F: FnOnce(ScrollbarBuilder) -> ScrollbarBuilder,
    {
        let builder = f(ScrollbarBuilder::default());
        self.scrollbar_config = builder.build();
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
    pub fn transform(mut self, t: impl IntoMaybeDyn<Transform>) -> Self {
        self.transform = t.into_maybe_dyn();
        self
    }

    /// Rotate this container by the given angle in degrees
    pub fn rotate(mut self, degrees: impl IntoMaybeDyn<f32>) -> Self {
        let degrees = degrees.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::rotate_degrees(degrees.get()))
        }));
        self
    }

    /// Scale this container uniformly
    pub fn scale(mut self, s: impl IntoMaybeDyn<f32>) -> Self {
        let s = s.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(Arc::new(move || {
            prev_transform.get().then(&Transform::scale(s.get()))
        }));
        self
    }

    /// Scale this container non-uniformly
    pub fn scale_xy(mut self, sx: impl IntoMaybeDyn<f32>, sy: impl IntoMaybeDyn<f32>) -> Self {
        let sx = sx.into_maybe_dyn();
        let sy = sy.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::scale_xy(sx.get(), sy.get()))
        }));
        self
    }

    /// Translate (move) this container by the given offset
    pub fn translate(mut self, x: impl IntoMaybeDyn<f32>, y: impl IntoMaybeDyn<f32>) -> Self {
        let x = x.into_maybe_dyn();
        let y = y.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic(Arc::new(move || {
            prev_transform
                .get()
                .then(&Transform::translate(x.get(), y.get()))
        }));
        self
    }

    /// Set the transform origin (pivot point) for this container.
    pub fn transform_origin(mut self, origin: impl IntoMaybeDyn<TransformOrigin>) -> Self {
        self.transform_origin = origin.into_maybe_dyn();
        self
    }

    /// Set the transform on an existing container (non-builder pattern)
    pub(crate) fn set_transform(&mut self, t: Transform) {
        self.transform = MaybeDyn::Static(t);
    }

    /// Set the transform origin on an existing container (non-builder pattern)
    pub(crate) fn set_transform_origin(&mut self, origin: TransformOrigin) {
        self.transform_origin = MaybeDyn::Static(origin);
    }

    /// Enable animation for width changes
    pub fn animate_width(mut self, transition: Transition) -> Self {
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
    pub fn hover_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.hover_state = Some(f(StateStyle::new()));
        self
    }

    /// Set style overrides for the pressed state.
    pub fn pressed_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.pressed_state = Some(f(StateStyle::new()));
        self
    }

    /// Set style overrides for when any child widget has focus.
    ///
    /// This is useful for styling input containers when their child text input is focused.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    ///     .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
    ///     .child(text_input(value))
    /// ```
    pub fn focused_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.focused_state = Some(f(StateStyle::new()));
        self
    }

    /// Check if any child widget has focus
    fn has_child_focus(&self) -> bool {
        if let Some(focused_id) = focused_widget() {
            return self.widget_has_focus(focused_id);
        }
        false
    }

    /// Recursively check if this widget or any child matches the focused widget ID
    fn widget_has_focus(&self, focused_id: WidgetId) -> bool {
        for child in self.children_source.get() {
            if child.id() == focused_id {
                return true;
            }
            // Recursively check nested containers/widgets
            if child.has_focus_descendant(focused_id) {
                return true;
            }
        }
        false
    }

    // State layer resolution helper
    // Priority: pressed > focused > hovered
    fn resolve_state_value<T: Clone>(
        &self,
        base: T,
        extractor: impl Fn(&StateStyle) -> Option<T>,
    ) -> T {
        if self.is_pressed
            && let Some(ref state) = self.pressed_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        // Check focused state
        if self.focused_state.is_some()
            && self.has_child_focus()
            && let Some(ref state) = self.focused_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if self.is_hovered
            && let Some(ref state) = self.hover_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        base
    }

    /// Get the effective background color target considering state layers.
    fn effective_background_target(&self) -> Color {
        let base = self.background.get();
        self.resolve_state_value(base, |state| {
            state
                .background
                .as_ref()
                .map(|bg| resolve_background(base, bg))
        })
    }

    /// Get the effective border width target considering state layers.
    fn effective_border_width_target(&self) -> f32 {
        let base = self.border_width.get();
        self.resolve_state_value(base, |state| state.border_width)
    }

    /// Get the effective border color target considering state layers.
    fn effective_border_color_target(&self) -> Color {
        let base = self.border_color.get();
        self.resolve_state_value(base, |state| state.border_color)
    }

    /// Get the effective corner radius target considering state layers.
    fn effective_corner_radius_target(&self) -> f32 {
        let base = self.corner_radius.get();
        self.resolve_state_value(base, |state| state.corner_radius)
    }

    /// Get the effective transform target considering state layers.
    fn effective_transform_target(&self) -> Transform {
        let base = self.transform.get();
        self.resolve_state_value(base, |state| state.transform)
    }

    /// Get the effective elevation considering state layers (not animated).
    fn effective_elevation(&self) -> f32 {
        let base = self.elevation.get();
        self.resolve_state_value(base, |state| state.elevation)
    }

    /// Get current padding (animated or static)
    fn animated_padding(&self) -> Padding {
        get_animated_value(&self.padding_anim, || self.padding.get())
    }

    /// Get current background color (animated or effective target)
    fn animated_background(&self) -> Color {
        get_animated_value(&self.background_anim, || self.effective_background_target())
    }

    /// Get current corner radius (animated or effective target)
    fn animated_corner_radius(&self) -> f32 {
        get_animated_value(&self.corner_radius_anim, || {
            self.effective_corner_radius_target()
        })
    }

    /// Get current border width (animated or effective target)
    fn animated_border_width(&self) -> f32 {
        get_animated_value(&self.border_width_anim, || {
            self.effective_border_width_target()
        })
    }

    /// Get current border color (animated or effective target)
    fn animated_border_color(&self) -> Color {
        get_animated_value(&self.border_color_anim, || {
            self.effective_border_color_target()
        })
    }

    /// Get current transform (animated or effective target)
    fn animated_transform(&self) -> Transform {
        get_animated_value(&self.transform_anim, || self.effective_transform_target())
    }

    /// Calculate constraints for children based on container dimensions and padding
    fn calc_child_constraints(&self) -> Constraints {
        let padding = self.padding.get();
        let child_max_width = (self.bounds.width - padding.horizontal()).max(0.0);
        let child_max_height = (self.bounds.height - padding.vertical()).max(0.0);

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
    fn advance_animations(&mut self) -> bool {
        let mut any_animating = false;

        // Layout-affecting animations: width, height, padding, border_width
        advance_anim!(self, width_anim, any_animating, layout);
        advance_anim!(self, height_anim, any_animating, layout);
        advance_anim!(
            self,
            padding_anim,
            self.padding.get(),
            any_animating,
            layout
        );

        let border_width_target = self.effective_border_width_target();
        advance_anim!(
            self,
            border_width_anim,
            border_width_target,
            any_animating,
            layout
        );

        // Paint-only animations: background, corner_radius, border_color, transform
        let bg_target = self.effective_background_target();
        advance_anim!(self, background_anim, bg_target, any_animating, paint);

        let corner_radius_target = self.effective_corner_radius_target();
        advance_anim!(
            self,
            corner_radius_anim,
            corner_radius_target,
            any_animating,
            paint
        );

        let border_color_target = self.effective_border_color_target();
        advance_anim!(
            self,
            border_color_anim,
            border_color_target,
            any_animating,
            paint
        );

        let transform_target = self.effective_transform_target();
        advance_anim!(self, transform_anim, transform_target, any_animating, paint);

        // Advance ripple animation
        if self.ripple.is_active()
            && let Some(ref state) = self.pressed_state
            && let Some(ref config) = state.ripple
        {
            let ripple_animating = self.ripple.advance(config);
            any_animating = any_animating || ripple_animating;
        }

        // Advance kinetic scroll animation
        let has_scroll_velocity =
            self.scroll_state.velocity_x.abs() > 0.5 || self.scroll_state.velocity_y.abs() > 0.5;
        if has_scroll_velocity {
            let scroll_animating = self.scroll_state.advance_momentum();
            any_animating = any_animating || scroll_animating;
        }

        // Recurse to children
        for child in self.children_source.get_mut() {
            if child.advance_animations() {
                any_animating = true;
            }
        }

        // Advance scrollbar animations
        if let Some(ref mut track) = self.v_scrollbar_track
            && track.advance_animations()
        {
            any_animating = true;
        }
        if let Some(ref mut handle) = self.v_scrollbar_handle
            && handle.advance_animations()
        {
            any_animating = true;
        }
        if let Some(ref mut track) = self.h_scrollbar_track
            && track.advance_animations()
        {
            any_animating = true;
        }
        if let Some(ref mut handle) = self.h_scrollbar_handle
            && handle.advance_animations()
        {
            any_animating = true;
        }

        // Update scrollbar handle positions based on current scroll offset
        // (scroll is paint-only, so layout may not run during scrolling)
        if self.scroll_axis != ScrollAxis::None {
            self.update_scrollbar_handle_positions();
        }

        // Advance scrollbar scale animations (for hover expansion effect)
        // Must be done here since scroll/hover is paint-only and layout may not run
        if self.advance_scrollbar_scale_animations() {
            any_animating = true;
        }

        any_animating
    }

    fn layout(&mut self, constraints: Constraints) -> Size {
        // Start layout tracking for dependency registration
        start_layout_tracking(self.widget_id);

        // Register this widget's relayout boundary status
        register_relayout_boundary(self.widget_id, self.is_relayout_boundary());

        // Ensure scrollbar containers exist if scrolling is enabled
        self.ensure_scrollbar_containers();

        let constraints_changed = self.last_constraints != Some(constraints);

        // Check if this widget was marked dirty by signal changes or animations
        // (animations call mark_needs_layout directly when their value changes)
        let reactive_changed = get_needs_layout_flag(self.widget_id);

        let needs_layout = constraints_changed || reactive_changed;

        if !needs_layout {
            crate::layout_stats::record_layout_skipped();
            finish_layout_tracking();
            return Size::new(self.bounds.width, self.bounds.height);
        }

        crate::layout_stats::record_layout_executed_with_reasons(
            crate::layout_stats::LayoutReasons {
                constraints_changed,
                reactive_changed,
            },
        );

        self.last_constraints = Some(constraints);

        // Clear dirty flag since we're doing layout now
        set_needs_layout_flag(self.widget_id, false);

        // Get current animated padding for layout calculations
        let padding = self.animated_padding();

        // Get width/height Length values
        let width_length = self.width.as_ref().map(|w| w.get()).unwrap_or_default();
        let height_length = self.height.as_ref().map(|h| h.get()).unwrap_or_default();

        // Calculate current container dimensions
        let current_width = if let Some(ref anim) = self.width_anim {
            if anim.is_initial() {
                constraints.max_width
            } else {
                *anim.current()
            }
        } else if let Some(exact) = width_length.exact {
            exact
        } else {
            constraints.max_width
        };

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

        // Calculate undershoot for spring animations
        let width_undershoot = if let Some(ref anim) = self.width_anim {
            (*anim.target() - *anim.current()).max(0.0)
        } else {
            0.0
        };

        let height_undershoot = if let Some(ref anim) = self.height_anim {
            (*anim.target() - *anim.current()).max(0.0)
        } else {
            0.0
        };

        // Child constraints with effective padding
        let effective_h_padding = (padding.horizontal() - width_undershoot).max(0.0);
        let effective_v_padding = (padding.vertical() - height_undershoot).max(0.0);
        let mut child_max_width = (current_width - effective_h_padding).max(0.0);
        let mut child_max_height = (current_height - effective_v_padding).max(0.0);

        // Reserve gutter space for scrollbars
        if self.scrollbar_config.reserve_gutter
            && self.scrollbar_visibility != ScrollbarVisibility::Hidden
        {
            let gutter = self.scrollbar_config.width + self.scrollbar_config.margin * 2.0;
            if self.scroll_axis.allows_vertical() {
                child_max_width = (child_max_width - gutter).max(0.0);
            }
            if self.scroll_axis.allows_horizontal() {
                child_max_height = (child_max_height - gutter).max(0.0);
            }
        }

        let child_min_width = if width_length.exact.is_some() || width_length.fill {
            child_max_width
        } else {
            0.0
        };
        let child_min_height = if height_length.exact.is_some() || height_length.fill {
            child_max_height
        } else {
            0.0
        };

        // For scrollable containers, use unbounded constraints in scroll direction
        let scroll_axis = self.scroll_axis;

        let child_constraints = match scroll_axis {
            ScrollAxis::Vertical => Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: child_max_width,
                max_height: f32::INFINITY,
            },
            ScrollAxis::Horizontal => Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: f32::INFINITY,
                max_height: child_max_height,
            },
            ScrollAxis::Both => Constraints {
                min_width: 0.0,
                min_height: 0.0,
                max_width: f32::INFINITY,
                max_height: f32::INFINITY,
            },
            ScrollAxis::None => Constraints {
                min_width: child_min_width,
                min_height: child_min_height,
                max_width: child_max_width,
                max_height: child_max_height,
            },
        };

        // Children are positioned at their "unscrolled" positions.
        // Scroll offset is applied as a transform in paint().
        let (child_origin_x, child_origin_y) =
            (self.bounds.x + padding.left, self.bounds.y + padding.top);

        // Reconcile and layout children
        let children = self.children_source.reconcile_and_get_mut();

        // Update parent tracking for all children
        for child in children.iter() {
            set_widget_parent(child.id(), self.widget_id);
        }

        let content_size = if !children.is_empty() {
            self.layout.layout(
                children,
                child_constraints,
                (child_origin_x, child_origin_y),
            )
        } else {
            Size::zero()
        };

        // Update scroll state
        if scroll_axis != ScrollAxis::None {
            self.scroll_state.content_width = content_size.width + padding.horizontal();
            self.scroll_state.content_height = content_size.height + padding.vertical();
            self.scroll_state.viewport_width = child_max_width;
            self.scroll_state.viewport_height = child_max_height;
            self.scroll_state.clamp_offsets();
        }

        let content_width = content_size.width + padding.horizontal();
        let content_height = content_size.height + padding.vertical();

        // Update animation targets
        if let Some(ref mut anim) = self.width_anim {
            let effective_target = if let Some(exact) = width_length.exact {
                exact
            } else {
                let min_w = width_length.min.unwrap_or(0.0);
                content_width.max(min_w)
            };
            if (effective_target - *anim.target()).abs() > 0.001 {
                if anim.is_initial() {
                    anim.set_immediate(effective_target);
                } else {
                    anim.animate_to(effective_target);
                    request_animation_frame();
                }
            }
        }

        if let Some(ref mut anim) = self.height_anim {
            let effective_target = if let Some(exact) = height_length.exact {
                exact
            } else {
                let min_h = height_length.min.unwrap_or(0.0);
                content_height.max(min_h)
            };
            if (effective_target - *anim.target()).abs() > 0.001 {
                if anim.is_initial() {
                    anim.set_immediate(effective_target);
                } else {
                    anim.animate_to(effective_target);
                    request_animation_frame();
                }
            }
        }

        // Determine shrink behavior
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let has_exact_width = width_length.exact.is_some();
        let has_exact_height = height_length.exact.is_some();
        let allow_shrink_width = self.overflow == Overflow::Hidden
            || width_animating
            || has_exact_width
            || self.scroll_axis.allows_horizontal();
        let allow_shrink_height = self.overflow == Overflow::Hidden
            || height_animating
            || has_exact_height
            || self.scroll_axis.allows_vertical();

        // Calculate final dimensions
        let mut width = if let Some(ref anim) = self.width_anim {
            if allow_shrink_width {
                *anim.current()
            } else {
                content_width.max(*anim.current())
            }
        } else if let Some(exact) = width_length.exact {
            exact
        } else if width_length.fill {
            constraints.max_width
        } else if let Some(min) = width_length.min {
            content_width.max(min)
        } else {
            content_width
        };

        if let Some(max) = width_length.max {
            width = width.min(max);
        }

        let mut height = if let Some(ref anim) = self.height_anim {
            if allow_shrink_height {
                *anim.current()
            } else {
                content_height.max(*anim.current())
            }
        } else if let Some(exact) = height_length.exact {
            exact
        } else if height_length.fill {
            constraints.max_height
        } else if let Some(min) = height_length.min {
            content_height.max(min)
        } else {
            content_height
        };

        if let Some(max) = height_length.max {
            height = height.min(max);
        }

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

        // Layout scrollbar containers after bounds are set
        self.layout_scrollbar_containers();

        // Finish layout tracking
        finish_layout_tracking();

        size
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let background = self.animated_background();
        let corner_radius = self.animated_corner_radius();
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.effective_elevation();
        let shadow = elevation_to_shadow(elevation_level);
        let transform = self.animated_transform();
        let transform_origin = self.transform_origin.get();

        // Push transform if not identity
        // Always resolve the transform origin so child elements (like text) know where to center
        let has_transform = !transform.is_identity();
        if has_transform {
            let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
            ctx.push_transform_with_origin(transform, origin_x, origin_y);
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
        let width_animating = self.width_anim.as_ref().is_some_and(|a| a.is_animating());
        let height_animating = self.height_anim.as_ref().is_some_and(|a| a.is_animating());
        let has_exact_size = self.width.as_ref().is_some_and(|w| w.get().exact.is_some())
            || self
                .height
                .as_ref()
                .is_some_and(|h| h.get().exact.is_some());
        let is_scrollable = self.scroll_axis != ScrollAxis::None;
        let should_clip = self.overflow == Overflow::Hidden
            || width_animating
            || height_animating
            || has_exact_size
            || is_scrollable;

        // Push clip first (in screen space), then scroll transform
        // The text renderer adjusts both text position and clip bounds by the transform,
        // so they stay in the same coordinate space for culling checks
        if should_clip {
            ctx.push_clip(self.bounds, corner_radius, corner_curvature);
        }

        // Apply scroll offset as a transform (paint-only, doesn't affect layout)
        if is_scrollable {
            let scroll_transform =
                Transform::translate(-self.scroll_state.offset_x, -self.scroll_state.offset_y);
            ctx.push_transform(scroll_transform);
        }

        // Draw children
        for child in self.children_source.get() {
            child.paint(ctx);
        }

        // Pop scroll transform first (reverse order of push)
        if is_scrollable {
            ctx.pop_transform();
        }

        // Pop clip
        if should_clip {
            ctx.pop_clip();
        }

        // Draw scrollbar containers
        if is_scrollable {
            self.paint_scrollbar_containers(ctx);
        }

        // Pop transform BEFORE drawing ripple
        if has_transform {
            ctx.pop_transform();
        }

        // Draw ripple effect as overlay
        if let Some((screen_cx, screen_cy)) = self.ripple.center
            && let Some(ref pressed_state) = self.pressed_state
            && let Some(ref ripple_config) = pressed_state.ripple
            && self.ripple.opacity > 0.0
        {
            let local_cx = screen_cx - self.bounds.x;
            let local_cy = screen_cy - self.bounds.y;
            let max_dist_x = local_cx.max(self.bounds.width - local_cx);
            let max_dist_y = local_cy.max(self.bounds.height - local_cy);
            let max_radius = (max_dist_x * max_dist_x + max_dist_y * max_dist_y).sqrt();
            let current_radius = max_radius * self.ripple.progress;

            let ripple_color = Color::rgba(
                ripple_config.color.r,
                ripple_config.color.g,
                ripple_config.color.b,
                ripple_config.color.a * self.ripple.opacity,
            );

            let (clip_bounds, clip_transform) = if has_transform {
                (self.bounds, Some((transform, transform_origin)))
            } else {
                (self.bounds, None)
            };

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

    fn event(&mut self, event: &Event) -> EventResponse {
        let transform = self.animated_transform();
        let transform_origin = self.transform_origin.get();
        let corner_radius = self.animated_corner_radius();

        // Transform event coordinates to local space
        let local_event: Cow<'_, Event> = if !transform.is_identity() {
            if let Some((x, y)) = event.coords() {
                let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
                // Transform is in screen space, simply center and invert
                let screen_space_transform = transform.center_at(origin_x, origin_y);
                let (local_x, local_y) = screen_space_transform.inverse().transform_point(x, y);
                Cow::Owned(event.with_coords(local_x, local_y))
            } else {
                Cow::Borrowed(event)
            }
        } else {
            Cow::Borrowed(event)
        };

        // Handle scrollbar events first
        if let Some(response) = self.handle_scrollbar_event(&local_event) {
            return response;
        }

        // For scrollable containers, offset event coordinates for children
        let child_event: Cow<'_, Event> = if self.scroll_axis != ScrollAxis::None {
            if let Some((x, y)) = local_event.coords() {
                let scrolled_x = x + self.scroll_state.offset_x;
                let scrolled_y = y + self.scroll_state.offset_y;
                Cow::Owned(local_event.with_coords(scrolled_x, scrolled_y))
            } else {
                local_event.clone()
            }
        } else {
            local_event.clone()
        };

        // Let children handle first (layout already reconciled)
        for child in self.children_source.get_mut() {
            if child.event(&child_event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }

        // Handle our own events
        match local_event.as_ref() {
            Event::MouseEnter { x, y } => {
                if self.bounds.contains_rounded(*x, *y, corner_radius) {
                    let was_hovered = self.is_hovered;
                    self.is_hovered = true;
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
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        self.ripple.start(screen_x, screen_y);
                        request_animation_frame();
                    }

                    if !was_pressed && self.pressed_state.is_some() {
                        request_animation_frame();
                    }
                    if self.on_click.is_some() {
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseUp { x, y, button } => {
                if self.is_pressed && *button == MouseButton::Left {
                    let was_pressed = self.is_pressed;
                    self.is_pressed = false;

                    // Start ripple fade animation
                    if self.ripple.is_active() {
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        self.ripple.start_fade(screen_x, screen_y);
                        request_animation_frame();
                    }

                    if was_pressed && self.pressed_state.is_some() {
                        request_animation_frame();
                    }
                    if self.bounds.contains_rounded(*x, *y, corner_radius)
                        && let Some(ref callback) = self.on_click
                    {
                        callback();
                        return EventResponse::Handled;
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

                // Start ripple fade to center
                if self.ripple.is_active() {
                    self.ripple
                        .start_fade_to_center(self.bounds.width, self.bounds.height);
                    request_animation_frame();
                }

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
                    if self.scroll_axis != ScrollAxis::None {
                        let consumed = self.apply_scroll(*delta_x, *delta_y, *source);
                        if consumed {
                            request_animation_frame();
                            return EventResponse::Handled;
                        }
                    }

                    if let Some(ref callback) = self.on_scroll {
                        callback(*delta_x, *delta_y, *source);
                        return EventResponse::Handled;
                    }
                }
            }
            // Keyboard and focus events are handled by focused widgets, not containers
            Event::KeyDown { .. } | Event::KeyUp { .. } | Event::FocusIn | Event::FocusOut => {}
        }

        EventResponse::Ignored
    }

    fn set_origin(&mut self, x: f32, y: f32) {
        self.bounds.x = x;
        self.bounds.y = y;

        let child_constraints = self.calc_child_constraints();
        let padding = self.padding.get();

        // Children are positioned at their "unscrolled" positions.
        // Scroll offset is applied as a transform in paint().
        let (child_origin_x, child_origin_y) = (x + padding.left, y + padding.top);

        // Layout already reconciled children, just get them
        let children = self.children_source.get_mut();
        if !children.is_empty() {
            self.layout.layout(
                children,
                child_constraints,
                (child_origin_x, child_origin_y),
            );
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn id(&self) -> WidgetId {
        self.widget_id
    }

    fn has_focus_descendant(&self, id: WidgetId) -> bool {
        self.widget_has_focus(id)
    }

    fn is_relayout_boundary(&self) -> bool {
        // Widget is a boundary if it has fixed width AND height
        let has_fixed_width = self.width.as_ref().is_some_and(|w| w.get().exact.is_some());
        let has_fixed_height = self
            .height
            .as_ref()
            .is_some_and(|h| h.get().exact.is_some());
        has_fixed_width && has_fixed_height
    }

    #[cfg(feature = "renderer_v2")]
    fn paint_v2(&self, ctx: &mut PaintContextV2) {
        let background = self.animated_background();
        let corner_radius = self.animated_corner_radius();
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.effective_elevation();
        let shadow = elevation_to_shadow(elevation_level);
        let transform = self.animated_transform();
        let transform_origin = self.transform_origin.get();

        // Set node properties
        ctx.set_bounds(self.bounds);
        if !transform.is_identity() {
            ctx.set_transform_with_origin(transform, transform_origin);
        }

        // Draw background
        if let Some(ref gradient) = self.gradient {
            ctx.draw_gradient_rect(
                self.bounds,
                crate::renderer::primitives::Gradient {
                    start_color: gradient.start_color,
                    end_color: gradient.end_color,
                    direction: gradient.direction.into(),
                },
                corner_radius,
                corner_curvature,
            );
        } else if background.a > 0.0 {
            if elevation_level > 0.0 {
                ctx.draw_rounded_rect_with_shadow(
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
        let is_scrollable = self.scroll_axis != ScrollAxis::None;

        // TODO: Clipping temporarily disabled in V2 renderer - will be re-implemented in a future PR

        // Draw children - each gets its own node
        // TODO: Handle scroll offset in children's transforms
        for child in self.children_source.get() {
            let child_bounds = child.bounds();
            let mut child_ctx = ctx.add_child(0, child_bounds); // Use 0 as placeholder ID

            // Apply scroll offset as child transform if scrollable
            if is_scrollable {
                let scroll_transform =
                    Transform::translate(-self.scroll_state.offset_x, -self.scroll_state.offset_y);
                child_ctx.set_transform(scroll_transform);
            }

            child.paint_v2(&mut child_ctx);
        }

        // TODO: Draw scrollbar containers when V2 is ready

        // Draw ripple effect as overlay
        if let Some((screen_cx, screen_cy)) = self.ripple.center
            && let Some(ref pressed_state) = self.pressed_state
            && let Some(ref ripple_config) = pressed_state.ripple
            && self.ripple.opacity > 0.0
        {
            let local_cx = screen_cx - self.bounds.x;
            let local_cy = screen_cy - self.bounds.y;
            let max_dist_x = local_cx.max(self.bounds.width - local_cx);
            let max_dist_y = local_cy.max(self.bounds.height - local_cy);
            let max_radius = (max_dist_x * max_dist_x + max_dist_y * max_dist_y).sqrt();
            let current_radius = max_radius * self.ripple.progress;

            let ripple_color = Color::rgba(
                ripple_config.color.r,
                ripple_config.color.g,
                ripple_config.color.b,
                ripple_config.color.a * self.ripple.opacity,
            );

            ctx.draw_overlay_circle(local_cx, local_cy, current_radius, ripple_color);
        }
    }
}

pub fn container() -> Container {
    Container::new()
}
