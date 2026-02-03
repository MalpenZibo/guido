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
use crate::jobs::{JobType, push_job};
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{IntoMaybeDyn, MaybeDyn, focused_widget, register_layout_signal};
use crate::renderer::{GradientDir, PaintContext, Shadow};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::tree::{Tree, WidgetId};

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
        let widget_id = WidgetId::next();
        let mut children_source = ChildrenSource::default();
        children_source.set_container_id(widget_id);
        Self {
            widget_id,
            layout: Box::new(Flex::column()),
            children_source,
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

    /// Set uniform padding on all sides in logical pixels.
    ///
    /// Accepts static values or reactive signals/closures.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Static padding
    /// container().padding(8.0)
    ///
    /// // Reactive padding
    /// let padding = create_signal(8.0);
    /// container().padding(padding)
    /// ```
    pub fn padding(mut self, value: impl IntoMaybeDyn<f32>) -> Self {
        let value = value.into_maybe_dyn();
        // Register layout dependency if value is from a signal
        if let Some(signal_id) = value.signal_id() {
            register_layout_signal(self.widget_id, signal_id);
        }
        self.padding = MaybeDyn::Dynamic {
            getter: Arc::new(move || Padding::all(value.get())),
            signal_id: None, // Compound MaybeDyn, original signal already registered
        };
        self
    }

    /// Set separate horizontal and vertical padding in logical pixels.
    ///
    /// Horizontal padding applies to left and right, vertical to top and bottom.
    /// Accepts static values or reactive signals/closures.
    ///
    /// # Example
    ///
    /// ```ignore
    /// container().padding_xy(16.0, 8.0)  // 16px horizontal, 8px vertical
    /// ```
    pub fn padding_xy(
        mut self,
        horizontal: impl IntoMaybeDyn<f32>,
        vertical: impl IntoMaybeDyn<f32>,
    ) -> Self {
        let h = horizontal.into_maybe_dyn();
        let v = vertical.into_maybe_dyn();
        // Register layout dependencies if values are from signals
        if let Some(signal_id) = h.signal_id() {
            register_layout_signal(self.widget_id, signal_id);
        }
        if let Some(signal_id) = v.signal_id() {
            register_layout_signal(self.widget_id, signal_id);
        }
        self.padding = MaybeDyn::Dynamic {
            getter: Arc::new(move || Padding {
                left: h.get(),
                right: h.get(),
                top: v.get(),
                bottom: v.get(),
            }),
            signal_id: None, // Compound MaybeDyn, original signals already registered
        };
        self
    }

    /// Set the background fill color.
    ///
    /// Supports RGBA transparency. Use [`Color::TRANSPARENT`] for no background.
    /// Accepts static values or reactive signals/closures.
    ///
    /// # Example
    ///
    /// ```ignore
    /// container().background(Color::rgb(0.2, 0.2, 0.3))
    /// container().background(Color::rgba(0.0, 0.0, 0.0, 0.5))  // 50% transparent black
    /// ```
    pub fn background(mut self, color: impl IntoMaybeDyn<Color>) -> Self {
        self.background = color.into_maybe_dyn();
        self
    }

    /// Set the corner radius in logical pixels.
    ///
    /// Combined with [`corner_curvature()`](Self::corner_curvature) to control corner shape.
    /// Default curvature is 1.0 (circular). Use [`squircle()`](Self::squircle),
    /// [`bevel()`](Self::bevel), or [`scoop()`](Self::scoop) for preset shapes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// container().corner_radius(8.0)                    // Standard rounded corners
    /// container().corner_radius(12.0).squircle()        // iOS-style smooth corners
    /// ```
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
        let maybe_dyn = width.into_maybe_dyn();
        // Register layout dependency if value is from a signal
        if let Some(signal_id) = maybe_dyn.signal_id() {
            register_layout_signal(self.widget_id, signal_id);
        }
        self.width = Some(maybe_dyn);
        self
    }

    /// Set the height of the container.
    pub fn height(mut self, height: impl IntoMaybeDyn<Length>) -> Self {
        let maybe_dyn = height.into_maybe_dyn();
        // Register layout dependency if value is from a signal
        if let Some(signal_id) = maybe_dyn.signal_id() {
            register_layout_signal(self.widget_id, signal_id);
        }
        self.height = Some(maybe_dyn);
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
        self.transform = MaybeDyn::Dynamic {
            getter: Arc::new(move || {
                prev_transform
                    .get()
                    .then(&Transform::rotate_degrees(degrees.get()))
            }),
            signal_id: None,
        };
        self
    }

    /// Scale this container uniformly
    pub fn scale(mut self, s: impl IntoMaybeDyn<f32>) -> Self {
        let s = s.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic {
            getter: Arc::new(move || prev_transform.get().then(&Transform::scale(s.get()))),
            signal_id: None,
        };
        self
    }

    /// Scale this container non-uniformly
    pub fn scale_xy(mut self, sx: impl IntoMaybeDyn<f32>, sy: impl IntoMaybeDyn<f32>) -> Self {
        let sx = sx.into_maybe_dyn();
        let sy = sy.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic {
            getter: Arc::new(move || {
                prev_transform
                    .get()
                    .then(&Transform::scale_xy(sx.get(), sy.get()))
            }),
            signal_id: None,
        };
        self
    }

    /// Translate (move) this container by the given offset
    pub fn translate(mut self, x: impl IntoMaybeDyn<f32>, y: impl IntoMaybeDyn<f32>) -> Self {
        let x = x.into_maybe_dyn();
        let y = y.into_maybe_dyn();
        let prev_transform =
            std::mem::replace(&mut self.transform, MaybeDyn::Static(Transform::IDENTITY));
        self.transform = MaybeDyn::Dynamic {
            getter: Arc::new(move || {
                prev_transform
                    .get()
                    .then(&Transform::translate(x.get(), y.get()))
            }),
            signal_id: None,
        };
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
    fn has_child_focus(&self, tree: &Tree) -> bool {
        if let Some(focused_id) = focused_widget() {
            return self.widget_has_focus(tree, focused_id);
        }
        false
    }

    /// Recursively check if this widget or any child matches the focused widget ID
    fn widget_has_focus(&self, tree: &Tree, focused_id: WidgetId) -> bool {
        for &child_id in self.children_source.get() {
            if child_id == focused_id {
                return true;
            }
            // Recursively check nested containers/widgets
            if tree.with_widget(child_id, |child| {
                child.has_focus_descendant(tree, focused_id)
            }) == Some(true)
            {
                return true;
            }
        }
        false
    }

    /// Check if this widget is a relayout boundary given constraints.
    /// A widget is a boundary if its size doesn't depend on children.
    fn is_relayout_boundary_for(&self, constraints: Constraints) -> bool {
        // Widget is a boundary if its size doesn't depend on children.
        // This happens when:
        // 1. It has explicit fixed width AND height
        // 2. OR the parent passes tight constraints (min == max)
        let has_fixed_width = self.width.as_ref().is_some_and(|w| w.get().exact.is_some());
        let has_fixed_height = self
            .height
            .as_ref()
            .is_some_and(|h| h.get().exact.is_some());
        let tight_width = constraints.min_width == constraints.max_width;
        let tight_height = constraints.min_height == constraints.max_height;
        (has_fixed_width || tight_width) && (has_fixed_height || tight_height)
    }

    // State layer resolution helper
    // Priority: pressed > focused > hovered
    fn resolve_state_value<T: Clone>(
        &self,
        tree: &Tree,
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
            && self.has_child_focus(tree)
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
    fn effective_background_target(&self, tree: &Tree) -> Color {
        let base = self.background.get();
        self.resolve_state_value(tree, base, |state| {
            state
                .background
                .as_ref()
                .map(|bg| resolve_background(base, bg))
        })
    }

    /// Get the effective border width target considering state layers.
    fn effective_border_width_target(&self, tree: &Tree) -> f32 {
        let base = self.border_width.get();
        self.resolve_state_value(tree, base, |state| state.border_width)
    }

    /// Get the effective border color target considering state layers.
    fn effective_border_color_target(&self, tree: &Tree) -> Color {
        let base = self.border_color.get();
        self.resolve_state_value(tree, base, |state| state.border_color)
    }

    /// Get the effective corner radius target considering state layers.
    fn effective_corner_radius_target(&self, tree: &Tree) -> f32 {
        let base = self.corner_radius.get();
        self.resolve_state_value(tree, base, |state| state.corner_radius)
    }

    /// Get the effective transform target considering state layers.
    fn effective_transform_target(&self, tree: &Tree) -> Transform {
        let base = self.transform.get();
        self.resolve_state_value(tree, base, |state| state.transform)
    }

    /// Get the effective elevation considering state layers (not animated).
    fn effective_elevation(&self, tree: &Tree) -> f32 {
        let base = self.elevation.get();
        self.resolve_state_value(tree, base, |state| state.elevation)
    }

    /// Get current padding (animated or static)
    fn animated_padding(&self) -> Padding {
        get_animated_value(&self.padding_anim, || self.padding.get())
    }

    /// Get current background color (animated or effective target)
    fn animated_background(&self, tree: &Tree) -> Color {
        get_animated_value(&self.background_anim, || {
            self.effective_background_target(tree)
        })
    }

    /// Get current corner radius (animated or effective target)
    fn animated_corner_radius(&self, tree: &Tree) -> f32 {
        get_animated_value(&self.corner_radius_anim, || {
            self.effective_corner_radius_target(tree)
        })
    }

    /// Get current border width (animated or effective target)
    fn animated_border_width(&self, tree: &Tree) -> f32 {
        get_animated_value(&self.border_width_anim, || {
            self.effective_border_width_target(tree)
        })
    }

    /// Get current border color (animated or effective target)
    fn animated_border_color(&self, tree: &Tree) -> Color {
        get_animated_value(&self.border_color_anim, || {
            self.effective_border_color_target(tree)
        })
    }

    /// Get current transform (animated or effective target)
    fn animated_transform(&self, tree: &Tree) -> Transform {
        get_animated_value(&self.transform_anim, || {
            self.effective_transform_target(tree)
        })
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
    fn advance_animations(&mut self, tree: &Tree) -> bool {
        // Use advance_animations_self for this widget's animations
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

        let border_width_target = self.effective_border_width_target(tree);
        advance_anim!(
            self,
            border_width_anim,
            border_width_target,
            any_animating,
            layout
        );

        // Paint-only animations: background, corner_radius, border_color, transform
        let bg_target = self.effective_background_target(tree);
        advance_anim!(self, background_anim, bg_target, any_animating, paint);

        let corner_radius_target = self.effective_corner_radius_target(tree);
        advance_anim!(
            self,
            corner_radius_anim,
            corner_radius_target,
            any_animating,
            paint
        );

        let border_color_target = self.effective_border_color_target(tree);
        advance_anim!(
            self,
            border_color_anim,
            border_color_target,
            any_animating,
            paint
        );

        let transform_target = self.effective_transform_target(tree);
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
            if scroll_animating {
                push_job(self.widget_id, JobType::Paint);
            }
            any_animating = any_animating || scroll_animating;
        }

        // Advance scrollbar animations (these are owned by this container, not tree children)
        if let Some(ref mut track) = self.v_scrollbar_track
            && track.advance_animations(tree)
        {
            any_animating = true;
        }
        if let Some(ref mut handle) = self.v_scrollbar_handle
            && handle.advance_animations(tree)
        {
            any_animating = true;
        }
        if let Some(ref mut track) = self.h_scrollbar_track
            && track.advance_animations(tree)
        {
            any_animating = true;
        }
        if let Some(ref mut handle) = self.h_scrollbar_handle
            && handle.advance_animations(tree)
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

        // If still animating, push Animation job for next frame
        if any_animating {
            push_job(self.widget_id, JobType::Animation);
        }

        any_animating
    }

    fn reconcile_children(&mut self, tree: &mut Tree) -> bool {
        self.children_source.reconcile_with_tracking(tree)
    }

    fn register_children(&mut self, tree: &mut Tree) {
        self.children_source.register_pending(tree, self.widget_id);
    }

    fn layout(&mut self, tree: &mut Tree, constraints: Constraints) -> Size {
        // Register this widget's relayout boundary status with the tree
        tree.set_relayout_boundary(self.widget_id, self.is_relayout_boundary_for(constraints));

        // Ensure scrollbar containers exist if scrolling is enabled
        self.ensure_scrollbar_containers();

        let constraints_changed = self.last_constraints != Some(constraints);

        // Check if this widget was marked dirty by signal changes or animations
        // (animations call mark_needs_layout directly when their value changes)
        let reactive_changed = tree.is_dirty(self.widget_id);

        let needs_layout = constraints_changed || reactive_changed;

        if !needs_layout {
            crate::layout_stats::record_layout_skipped();
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
        tree.clear_dirty(self.widget_id);

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

        // Children are positioned in LOCAL coordinates (relative to container's 0,0).
        // The container's absolute position is handled by the parent via transforms.
        // Scroll offset is applied as a transform in paint().
        let (child_origin_x, child_origin_y) = (padding.left, padding.top);

        // Reconcile and get children IDs
        let children = self.children_source.reconcile_and_get(tree);

        // Update parent tracking for all children in the tree
        for &child_id in children.iter() {
            tree.set_parent(child_id, self.widget_id);
        }

        let content_size = if !children.is_empty() {
            self.layout.layout(
                tree,
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
                    push_job(self.widget_id, JobType::Animation);
                    push_job(self.widget_id, JobType::Paint);
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
                    push_job(self.widget_id, JobType::Animation);
                    push_job(self.widget_id, JobType::Paint);
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

        // When explicit dimensions are set, respect them over min constraints
        // This allows children with .width(60) to stay 60px even when parent uses Stretch
        let final_width = if width_length.exact.is_some() {
            // Explicit width: only apply max constraint, not min
            width.min(constraints.max_width)
        } else {
            width.max(constraints.min_width).min(constraints.max_width)
        };
        let final_height = if height_length.exact.is_some() {
            // Explicit height: only apply max constraint, not min
            height.min(constraints.max_height)
        } else {
            height
                .max(constraints.min_height)
                .min(constraints.max_height)
        };
        let size = Size::new(final_width, final_height);

        self.bounds.width = size.width;
        self.bounds.height = size.height;

        // Layout scrollbar containers after bounds are set
        self.layout_scrollbar_containers(tree);

        // Cache constraints and size for partial layout
        tree.cache_layout(self.widget_id, constraints, size);

        size
    }

    fn event(&mut self, tree: &Tree, event: &Event) -> EventResponse {
        let transform = self.animated_transform(tree);
        let transform_origin = self.transform_origin.get();
        let corner_radius = self.animated_corner_radius(tree);

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
        if let Some(response) = self.handle_scrollbar_event(tree, &local_event) {
            return response;
        }

        // Transform event coordinates to local space (relative to container origin)
        // Children are positioned in local coordinates, so events must be too
        let child_event: Cow<'_, Event> = if let Some((x, y)) = local_event.coords() {
            // Convert from global/window coordinates to local (relative to container)
            let local_x = x - self.bounds.x;
            let local_y = y - self.bounds.y;

            // For scrollable containers, also add scroll offset
            let (child_x, child_y) = if self.scroll_axis != ScrollAxis::None {
                (
                    local_x + self.scroll_state.offset_x,
                    local_y + self.scroll_state.offset_y,
                )
            } else {
                (local_x, local_y)
            };
            Cow::Owned(local_event.with_coords(child_x, child_y))
        } else {
            local_event.clone()
        };

        // Let children handle first (layout already reconciled)
        for &child_id in self.children_source.get() {
            if let Some(response) =
                tree.with_widget_mut(child_id, |child| child.event(tree, &child_event))
                && response == EventResponse::Handled
            {
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
                        // Push Animation job if there are animated properties
                        if self.background_anim.is_some()
                            || self.corner_radius_anim.is_some()
                            || self.border_color_anim.is_some()
                            || self.transform_anim.is_some()
                        {
                            push_job(self.widget_id, JobType::Animation);
                        }
                        push_job(self.widget_id, JobType::Paint);
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
                        // Push Animation job if there are animated properties
                        if self.background_anim.is_some()
                            || self.corner_radius_anim.is_some()
                            || self.border_color_anim.is_some()
                            || self.transform_anim.is_some()
                        {
                            push_job(self.widget_id, JobType::Animation);
                        }
                        push_job(self.widget_id, JobType::Paint);
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
                        // Convert screen coords to local coords accounting for transform
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        let (local_x, local_y) = if !transform.is_identity() {
                            let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
                            let screen_transform = transform.center_at(origin_x, origin_y);
                            let (inv_x, inv_y) = screen_transform
                                .inverse()
                                .transform_point(screen_x, screen_y);
                            (inv_x - self.bounds.x, inv_y - self.bounds.y)
                        } else {
                            (screen_x - self.bounds.x, screen_y - self.bounds.y)
                        };
                        self.ripple.start(local_x, local_y);
                        // Push Animation job for ripple
                        push_job(self.widget_id, JobType::Animation);
                        push_job(self.widget_id, JobType::Paint);
                    }

                    if !was_pressed && self.pressed_state.is_some() {
                        // Push Animation job if there are animated properties
                        if self.background_anim.is_some()
                            || self.corner_radius_anim.is_some()
                            || self.border_color_anim.is_some()
                            || self.transform_anim.is_some()
                        {
                            push_job(self.widget_id, JobType::Animation);
                        }
                        push_job(self.widget_id, JobType::Paint);
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
                        // Convert screen coords to local coords accounting for transform
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        let (local_x, local_y) = if !transform.is_identity() {
                            let (origin_x, origin_y) = transform_origin.resolve(self.bounds);
                            let screen_transform = transform.center_at(origin_x, origin_y);
                            let (inv_x, inv_y) = screen_transform
                                .inverse()
                                .transform_point(screen_x, screen_y);
                            (inv_x - self.bounds.x, inv_y - self.bounds.y)
                        } else {
                            (screen_x - self.bounds.x, screen_y - self.bounds.y)
                        };
                        self.ripple.start_fade(local_x, local_y);
                        // Push Animation job for ripple fade
                        push_job(self.widget_id, JobType::Animation);
                        push_job(self.widget_id, JobType::Paint);
                    }

                    if was_pressed && self.pressed_state.is_some() {
                        // Push Animation job if there are animated properties
                        if self.background_anim.is_some()
                            || self.corner_radius_anim.is_some()
                            || self.border_color_anim.is_some()
                            || self.transform_anim.is_some()
                        {
                            push_job(self.widget_id, JobType::Animation);
                        }
                        push_job(self.widget_id, JobType::Paint);
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
                    // Push Animation job for ripple fade
                    push_job(self.widget_id, JobType::Animation);
                    push_job(self.widget_id, JobType::Paint);
                }

                if (was_hovered && self.hover_state.is_some())
                    || (was_pressed && self.pressed_state.is_some())
                {
                    // Push Animation job if there are animated properties
                    if self.background_anim.is_some()
                        || self.corner_radius_anim.is_some()
                        || self.border_color_anim.is_some()
                        || self.transform_anim.is_some()
                    {
                        push_job(self.widget_id, JobType::Animation);
                    }
                    push_job(self.widget_id, JobType::Paint);
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
                            // Push Animation job for kinetic scrolling if has velocity
                            let has_velocity = self.scroll_state.velocity_x.abs() > 0.5
                                || self.scroll_state.velocity_y.abs() > 0.5;
                            if has_velocity {
                                push_job(self.widget_id, JobType::Animation);
                            }
                            push_job(self.widget_id, JobType::Paint);
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
        // Calculate delta from previous position
        let dx = x - self.bounds.x;
        let dy = y - self.bounds.y;
        self.bounds.x = x;
        self.bounds.y = y;

        // If position changed, offset all children by the delta
        // This avoids needing tree access - children already have their relative
        // positions set during layout, we just need to translate them
        if (dx.abs() > f32::EPSILON || dy.abs() > f32::EPSILON) && !self.children_source.is_empty()
        {
            // Children positions need to be updated but we don't have tree access.
            // The layout engine already positioned children relative to the container
            // during the layout pass. Paint handles the final positioning via transforms.
            // No action needed here - paint() uses self.bounds to compute child offsets.
        }

        // Update scrollbar container positions now that we have the correct origin.
        // Scrollbar layout calculates positions using self.bounds, but bounds.x/y are
        // only set correctly after set_origin is called by the parent.
        if self.scroll_axis != ScrollAxis::None {
            self.update_scrollbar_origins();
        }
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn id(&self) -> WidgetId {
        self.widget_id
    }

    fn has_focus_descendant(&self, tree: &Tree, id: WidgetId) -> bool {
        self.widget_has_focus(tree, id)
    }

    fn paint(&self, tree: &Tree, ctx: &mut PaintContext) {
        let background = self.animated_background(tree);
        let corner_radius = self.animated_corner_radius(tree);
        let corner_curvature = self.corner_curvature.get();
        let elevation_level = self.effective_elevation(tree);
        let shadow = elevation_to_shadow(elevation_level);
        let user_transform = self.animated_transform(tree);
        let transform_origin = self.transform_origin.get();

        // LOCAL bounds (0,0 is widget origin) - all drawing uses these
        let local_bounds = Rect::new(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.set_bounds(local_bounds);

        // Apply user transform (rotation, scale, user-specified translate)
        // Position is handled by the parent via set_transform before calling paint
        // We COMPOSE our user transform with the existing position transform
        if !user_transform.is_identity() {
            ctx.apply_transform_with_origin(user_transform, transform_origin);
        }

        // Draw background using LOCAL coordinates
        if let Some(ref gradient) = self.gradient {
            ctx.draw_gradient_rect(
                local_bounds,
                crate::renderer::Gradient {
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
                    local_bounds,
                    background,
                    corner_radius,
                    corner_curvature,
                    shadow,
                );
            } else {
                ctx.draw_rounded_rect_with_curvature(
                    local_bounds,
                    background,
                    corner_radius,
                    corner_curvature,
                );
            }
        }

        // Draw border using LOCAL coordinates
        let border_width = self.animated_border_width(tree);
        let border_color = self.animated_border_color(tree);

        if border_width > 0.0 {
            ctx.draw_border_frame_with_curvature(
                local_bounds,
                border_color,
                corner_radius,
                border_width,
                corner_curvature,
            );
        }

        // Determine if we need to clip children
        let is_scrollable = self.scroll_axis != ScrollAxis::None;

        // Set clip region for scrollable containers
        // This clips all children to the container bounds
        if is_scrollable {
            // Clip to container bounds (local coordinates)
            ctx.set_clip(local_bounds, corner_radius, corner_curvature);
        }

        // Draw children - each gets its own node with position transform
        for &child_id in self.children_source.get() {
            // Get child bounds from tree - these are in LOCAL coordinates (relative to parent)
            let child_bounds = tree
                .with_widget(child_id, |child| child.bounds())
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            // Child's LOCAL bounds (0,0 origin with its own width/height)
            let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);
            // Child offset is directly from child bounds (already in local coordinates)
            let child_offset_x = child_bounds.x;
            let child_offset_y = child_bounds.y;

            let mut child_ctx = ctx.add_child(child_id.as_u64(), child_local);

            // Child's position transform (may include scroll offset)
            let child_position = if is_scrollable {
                Transform::translate(
                    child_offset_x - self.scroll_state.offset_x,
                    child_offset_y - self.scroll_state.offset_y,
                )
            } else {
                Transform::translate(child_offset_x, child_offset_y)
            };
            child_ctx.set_transform(child_position);

            // Paint child via tree
            tree.with_widget(child_id, |child| child.paint(tree, &mut child_ctx));
        }

        // Draw scrollbar containers
        if is_scrollable {
            self.paint_scrollbar_containers(tree, ctx);
        }

        // Draw ripple effect as overlay (ripple.center is already in local coordinates)
        if let Some((local_cx, local_cy)) = self.ripple.center
            && let Some(ref pressed_state) = self.pressed_state
            && let Some(ref ripple_config) = pressed_state.ripple
            && self.ripple.opacity > 0.0
        {
            // Set overlay clip to container bounds with rounded corners
            // This clips the ripple without affecting children
            ctx.set_overlay_clip(local_bounds, corner_radius, corner_curvature);

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
