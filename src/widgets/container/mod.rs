//! Container widget and related functionality.

mod animations;
mod ripple;
mod scrollable;

pub use animations::{AdvanceResult, AnimationState, get_animated_value};
pub use ripple::RippleState;

use std::borrow::Cow;
use std::rc::Rc;

use crate::advance_anim;
use crate::animation::TransitionConfig;
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job};
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{
    IntoSignal, OptionSignalExt, Signal, create_derived, create_stored, focused_widget,
    with_signal_tracking,
};
use crate::renderer::{GradientDir, PaintContext, Shadow};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::tree::{Tree, WidgetId};
use crate::widget_ref::{WidgetRef, register_widget_ref};

use super::children::ChildrenSource;
use super::into_child::{IntoChild, IntoChildren};
use super::scroll::{
    ScrollAxis, ScrollState, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility,
};
use super::state_layer::{StateStyle, resolve_background};
use super::widget::{
    Color, Event, EventResponse, LayoutHints, MouseButton, Padding, Rect, ScrollSource, Widget,
};

/// Callback for click events
pub type ClickCallback = Rc<dyn Fn()>;
/// Callback for hover events (bool = is_hovered)
pub type HoverCallback = Rc<dyn Fn(bool)>;
/// Callback for scroll events (delta_x, delta_y, source)
pub type ScrollCallback = Rc<dyn Fn(f32, f32, ScrollSource)>;
/// Callback for pointer move events (x, y in container-local coords)
pub type PointerMoveCallback = Rc<dyn Fn(f32, f32)>;
/// Callback for mouse down events (x, y in container-local coords)
pub type MouseDownCallback = Rc<dyn Fn(f32, f32)>;
/// Callback for mouse up events (x, y in container-local coords)
pub type MouseUpCallback = Rc<dyn Fn(f32, f32)>;

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
    pub fn new(width: impl crate::layout::IntoF32, color: Color) -> Self {
        Self {
            width: width.into_f32(),
            color,
        }
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

/// Boxed animation states. Only allocated when `.transition()` or
/// `.animate_*()` is called, saving ~400 bytes per non-animated Container.
#[derive(Default)]
pub(super) struct ContainerAnims {
    pub(super) width: Option<AnimationState<f32>>,
    pub(super) height: Option<AnimationState<f32>>,
    pub(super) background: Option<AnimationState<Color>>,
    pub(super) corner_radius: Option<AnimationState<f32>>,
    pub(super) padding: Option<AnimationState<Padding>>,
    pub(super) border_width: Option<AnimationState<f32>>,
    pub(super) border_color: Option<AnimationState<Color>>,
    pub(super) transform: Option<AnimationState<Transform>>,
}

/// Interaction state (callbacks, hover/press tracking, state styles, ripple).
/// Only allocated when `.on_click()`, `.hover_state()`, `.pressed_state()`, etc. are called.
pub(super) struct InteractionState {
    pub(super) on_click: Option<ClickCallback>,
    pub(super) on_hover: Option<HoverCallback>,
    pub(super) on_scroll: Option<ScrollCallback>,
    pub(super) on_pointer_move: Option<PointerMoveCallback>,
    pub(super) on_mouse_down: Option<MouseDownCallback>,
    pub(super) on_mouse_up: Option<MouseUpCallback>,
    pub(super) is_hovered: bool,
    pub(super) is_pressed: bool,
    pub(super) hover_state: Option<StateStyle>,
    pub(super) pressed_state: Option<StateStyle>,
    pub(super) focused_state: Option<StateStyle>,
    pub(super) ripple: RippleState,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            on_click: None,
            on_hover: None,
            on_scroll: None,
            on_pointer_move: None,
            on_mouse_down: None,
            on_mouse_up: None,
            is_hovered: false,
            is_pressed: false,
            hover_state: None,
            pressed_state: None,
            focused_state: None,
            ripple: RippleState::new(),
        }
    }
}

/// Scroll state and configuration, boxed to avoid bloating Container.
/// Only allocated when `.scrollable()` is called.
pub(super) struct ScrollData {
    pub(super) scrollbar_visibility: ScrollbarVisibility,
    pub(super) scrollbar_config: ScrollbarConfig,
    pub(super) scroll_state: ScrollState,
    pub(super) v_scrollbar_track_id: Option<WidgetId>,
    pub(super) v_scrollbar_handle_id: Option<WidgetId>,
    pub(super) v_scrollbar_scale_anim: Option<AnimationState<f32>>,
    pub(super) h_scrollbar_track_id: Option<WidgetId>,
    pub(super) h_scrollbar_handle_id: Option<WidgetId>,
    pub(super) h_scrollbar_scale_anim: Option<AnimationState<f32>>,
}

impl Default for ScrollData {
    fn default() -> Self {
        Self {
            scrollbar_visibility: ScrollbarVisibility::Always,
            scrollbar_config: ScrollbarConfig::default(),
            scroll_state: ScrollState::default(),
            v_scrollbar_track_id: None,
            v_scrollbar_handle_id: None,
            v_scrollbar_scale_anim: None,
            h_scrollbar_track_id: None,
            h_scrollbar_handle_id: None,
            h_scrollbar_scale_anim: None,
        }
    }
}

pub struct Container {
    // Layout and children
    pub(super) layout: Box<dyn Layout>,
    pub(super) children_source: ChildrenSource,

    // Styling properties
    pub(super) padding: Option<Signal<Padding>>,
    pub(super) background: Option<Signal<Color>>,
    pub(super) gradient: Option<LinearGradient>,
    pub(super) corner_radius: Option<Signal<f32>>,
    pub(super) corner_curvature: Option<Signal<f32>>,
    pub(super) border_width: Option<Signal<f32>>,
    pub(super) border_color: Option<Signal<Color>>,
    pub(super) elevation: Option<Signal<f32>>,
    pub(super) width: Option<Signal<Length>>,
    pub(super) height: Option<Signal<Length>>,
    pub(super) overflow: Overflow,
    pub(super) visible: Option<Signal<bool>>,
    pub(super) transform: Option<Signal<Transform>>,
    pub(super) transform_origin: Option<Signal<TransformOrigin>>,

    // Interaction state (callbacks, hover/press, state styles, ripple)
    // Only allocated when interaction features are used
    pub(super) interaction: Option<Box<InteractionState>>,

    // Widget ref for reactive bounds tracking
    pub(super) widget_ref: Option<WidgetRef>,

    // Animation state (boxed to save ~400 bytes per non-animated container)
    pub(super) anims: Option<Box<ContainerAnims>>,

    // Scroll configuration
    pub(super) scroll_axis: ScrollAxis,
    pub(super) scroll_data: Option<Box<ScrollData>>,
}

impl Container {
    pub fn new() -> Self {
        let children_source = ChildrenSource::default();
        Self {
            layout: Box::new(Flex::column()),
            children_source,
            padding: None,
            background: None,
            gradient: None,
            corner_radius: None,
            corner_curvature: None,
            border_width: None,
            border_color: None,
            elevation: None,
            width: None,
            height: None,
            overflow: Overflow::Visible,
            visible: None,
            transform: None,
            transform_origin: None,
            interaction: None,
            widget_ref: None,
            anims: None,
            scroll_axis: ScrollAxis::None,
            scroll_data: None,
        }
    }

    /// Get scroll data (panics if not scrollable — only call when scroll_axis != None)
    fn scroll(&self) -> &ScrollData {
        self.scroll_data.as_deref().expect("scroll_data not set")
    }

    /// Get mutable scroll data (panics if not scrollable)
    fn scroll_mut(&mut self) -> &mut ScrollData {
        self.scroll_data
            .as_deref_mut()
            .expect("scroll_data not set")
    }

    /// Get or create scroll data
    fn scroll_or_init(&mut self) -> &mut ScrollData {
        self.scroll_data.get_or_insert_with(Box::default)
    }

    /// Get or create animation states box
    fn anims_mut(&mut self) -> &mut ContainerAnims {
        self.anims.get_or_insert_with(Box::default)
    }

    /// Get or create interaction state
    fn interact_mut(&mut self) -> &mut InteractionState {
        self.interaction.get_or_insert_with(Box::default)
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

    /// Set padding in logical pixels.
    ///
    /// Accepts multiple formats via `From` conversions:
    /// - `padding(8.0)` or `padding(8)` — uniform on all sides
    /// - `padding([8.0, 16.0])` — `[vertical, horizontal]` (CSS 2-value shorthand)
    /// - `padding([1.0, 2.0, 3.0, 4.0])` — `[top, right, bottom, left]` (CSS 4-value)
    /// - `padding(Padding::all(8.0).top(20.0))` — builder pattern
    /// - `padding(signal)` or `padding(move || ...)` — reactive
    pub fn padding<M>(mut self, value: impl IntoSignal<Padding, M>) -> Self {
        self.padding = Some(value.into_signal());
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
    pub fn background<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.background = Some(color.into_signal());
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
    pub fn corner_radius<M>(mut self, radius: impl IntoSignal<f32, M>) -> Self {
        self.corner_radius = Some(radius.into_signal());
        self
    }

    /// Set the corner curvature using CSS K-value system
    pub fn corner_curvature<M>(mut self, curvature: impl IntoSignal<f32, M>) -> Self {
        self.corner_curvature = Some(curvature.into_signal());
        self
    }

    /// Convenience: Set squircle/iOS-style corners
    pub fn squircle(mut self) -> Self {
        self.corner_curvature = Some(create_stored(2.0));
        self
    }

    /// Convenience: Set concave/scooped corners
    pub fn scoop(mut self) -> Self {
        self.corner_curvature = Some(create_stored(-1.0));
        self
    }

    /// Convenience: Set beveled corners
    pub fn bevel(mut self) -> Self {
        self.corner_curvature = Some(create_stored(0.0));
        self
    }

    /// Set a border with the given width and color
    pub fn border<M1, M2>(
        mut self,
        width: impl IntoSignal<f32, M1>,
        color: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.border_width = Some(width.into_signal());
        self.border_color = Some(color.into_signal());
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
    pub fn width<M>(mut self, width: impl IntoSignal<Length, M>) -> Self {
        self.width = Some(width.into_signal());
        self
    }

    /// Set the height of the container.
    pub fn height<M>(mut self, height: impl IntoSignal<Length, M>) -> Self {
        self.height = Some(height.into_signal());
        self
    }

    /// Set the overflow behavior for content that exceeds container bounds
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Set visibility of this container.
    ///
    /// When `visible` is false, the container takes up no space in layout,
    /// does not paint, and ignores all events.
    pub fn visible<M>(mut self, visible: impl IntoSignal<bool, M>) -> Self {
        self.visible = Some(visible.into_signal());
        self
    }

    /// Enable scrolling on this container.
    pub fn scrollable(mut self, axis: ScrollAxis) -> Self {
        self.scroll_axis = axis;
        if axis != ScrollAxis::None {
            self.scroll_data = Some(Box::default());
        }
        self
    }

    /// Configure scrollbar visibility.
    pub fn scrollbar_visibility(mut self, visibility: ScrollbarVisibility) -> Self {
        self.scroll_or_init().scrollbar_visibility = visibility;
        self
    }

    /// Customize scrollbar appearance.
    pub fn scrollbar<F>(mut self, f: F) -> Self
    where
        F: FnOnce(ScrollbarBuilder) -> ScrollbarBuilder,
    {
        let builder = f(ScrollbarBuilder::default());
        self.scroll_or_init().scrollbar_config = builder.build();
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_click = Some(Rc::new(callback));
        self
    }

    /// Accept an optional click callback (useful for components)
    pub fn on_click_option(mut self, callback: Option<ClickCallback>) -> Self {
        if callback.is_some() || self.interaction.is_some() {
            self.interact_mut().on_click = callback;
        }
        self
    }

    pub fn on_hover<F: Fn(bool) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_hover = Some(Rc::new(callback));
        self
    }

    pub fn on_scroll<F: Fn(f32, f32, ScrollSource) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_scroll = Some(Rc::new(callback));
        self
    }

    pub fn on_pointer_move<F: Fn(f32, f32) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_pointer_move = Some(Rc::new(callback));
        self
    }

    pub fn on_mouse_down<F: Fn(f32, f32) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_mouse_down = Some(Rc::new(callback));
        self
    }

    pub fn on_mouse_up<F: Fn(f32, f32) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_mouse_up = Some(Rc::new(callback));
        self
    }

    /// Attach a [`WidgetRef`] to track this container's surface-relative bounds.
    pub fn widget_ref(mut self, r: WidgetRef) -> Self {
        self.widget_ref = Some(r);
        self
    }

    pub fn elevation<M>(mut self, level: impl IntoSignal<f32, M>) -> Self {
        self.elevation = Some(level.into_signal());
        self
    }

    /// Set the transform for this container
    pub fn transform<M>(mut self, t: impl IntoSignal<Transform, M>) -> Self {
        self.transform = Some(t.into_signal());
        self
    }

    /// Rotate this container by the given angle in degrees
    pub fn rotate<M>(mut self, degrees: impl IntoSignal<f32, M>) -> Self {
        let degrees = degrees.into_signal();
        let prev = self.transform.signal_or(Transform::IDENTITY);
        self.transform = Some(create_derived(move || {
            prev.get().then(&Transform::rotate_degrees(degrees.get()))
        }));
        self
    }

    /// Scale this container uniformly
    pub fn scale<M>(mut self, s: impl IntoSignal<f32, M>) -> Self {
        let s = s.into_signal();
        let prev = self.transform.signal_or(Transform::IDENTITY);
        self.transform = Some(create_derived(move || {
            prev.get().then(&Transform::scale(s.get()))
        }));
        self
    }

    /// Scale this container non-uniformly
    pub fn scale_xy<M1, M2>(
        mut self,
        sx: impl IntoSignal<f32, M1>,
        sy: impl IntoSignal<f32, M2>,
    ) -> Self {
        let sx = sx.into_signal();
        let sy = sy.into_signal();
        let prev = self.transform.signal_or(Transform::IDENTITY);
        self.transform = Some(create_derived(move || {
            prev.get().then(&Transform::scale_xy(sx.get(), sy.get()))
        }));
        self
    }

    /// Translate (move) this container by the given offset
    pub fn translate<M1, M2>(
        mut self,
        x: impl IntoSignal<f32, M1>,
        y: impl IntoSignal<f32, M2>,
    ) -> Self {
        let x = x.into_signal();
        let y = y.into_signal();
        let prev = self.transform.signal_or(Transform::IDENTITY);
        self.transform = Some(create_derived(move || {
            prev.get().then(&Transform::translate(x.get(), y.get()))
        }));
        self
    }

    /// Set the transform origin (pivot point) for this container.
    pub fn transform_origin<M>(mut self, origin: impl IntoSignal<TransformOrigin, M>) -> Self {
        self.transform_origin = Some(origin.into_signal());
        self
    }

    /// Enable animation for width changes
    pub fn animate_width(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self
            .width
            .as_ref()
            .map(|w| {
                let len = w.get();
                len.exact.or(len.min).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        self.anims_mut().width = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for height changes
    pub fn animate_height(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self
            .height
            .as_ref()
            .map(|h| {
                let len = h.get();
                len.exact.or(len.min).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        self.anims_mut().height = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for background color changes
    pub fn animate_background(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.background.get_or(Color::TRANSPARENT);
        self.anims_mut().background = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for corner radius changes
    pub fn animate_corner_radius(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.corner_radius.get_or(0.0);
        self.anims_mut().corner_radius = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for padding changes
    pub fn animate_padding(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.padding.get_or(Padding::default());
        self.anims_mut().padding = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border width changes
    pub fn animate_border_width(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.border_width.get_or(0.0);
        self.anims_mut().border_width = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border color changes
    pub fn animate_border_color(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.border_color.get_or(Color::TRANSPARENT);
        self.anims_mut().border_color = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for transform changes
    pub fn animate_transform(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.transform.get_or(Transform::IDENTITY);
        self.anims_mut().transform = Some(AnimationState::new(initial, transition));
        self
    }

    /// Set style overrides for the hover state.
    pub fn hover_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.interact_mut().hover_state = Some(f(StateStyle::new()));
        self
    }

    /// Set style overrides for the pressed state.
    pub fn pressed_state<F>(mut self, f: F) -> Self
    where
        F: FnOnce(StateStyle) -> StateStyle,
    {
        self.interact_mut().pressed_state = Some(f(StateStyle::new()));
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
        self.interact_mut().focused_state = Some(f(StateStyle::new()));
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
        // A widget with an active layout-affecting animation is NOT a boundary,
        // because its size changes each frame and the parent must reposition siblings.
        let has_active_layout_anim = self.anims.as_ref().is_some_and(|a| {
            a.width.as_ref().is_some_and(|x| x.is_animating())
                || a.height.as_ref().is_some_and(|x| x.is_animating())
                || a.padding.as_ref().is_some_and(|x| x.is_animating())
        });
        if has_active_layout_anim {
            return false;
        }

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
        let Some(ref ix) = self.interaction else {
            return base;
        };
        if ix.is_pressed
            && let Some(ref state) = ix.pressed_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        // Check focused state
        if ix.focused_state.is_some()
            && self.has_child_focus(tree)
            && let Some(ref state) = ix.focused_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if ix.is_hovered
            && let Some(ref state) = ix.hover_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        base
    }

    /// Get the effective background color target considering state layers.
    fn effective_background_target(&self, tree: &Tree) -> Color {
        let base = self.background.get_or(Color::TRANSPARENT);
        self.resolve_state_value(tree, base, |state| {
            let bg_color = state
                .background
                .as_ref()
                .map(|bg| resolve_background(base, bg));
            match (bg_color, state.alpha) {
                (Some(mut c), Some(a)) => {
                    c.a = a;
                    Some(c)
                }
                (Some(c), None) => Some(c),
                (None, Some(a)) => {
                    let mut c = base;
                    c.a = a;
                    Some(c)
                }
                (None, None) => None,
            }
        })
    }

    /// Get the effective border width target considering state layers.
    fn effective_border_width_target(&self, tree: &Tree) -> f32 {
        let base = self.border_width.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.border_width)
    }

    /// Get the effective border color target considering state layers.
    fn effective_border_color_target(&self, tree: &Tree) -> Color {
        let base = self.border_color.get_or(Color::TRANSPARENT);
        self.resolve_state_value(tree, base, |state| state.border_color)
    }

    /// Get the effective corner radius target considering state layers.
    fn effective_corner_radius_target(&self, tree: &Tree) -> f32 {
        let base = self.corner_radius.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.corner_radius)
    }

    /// Get the effective transform target considering state layers.
    fn effective_transform_target(&self, tree: &Tree) -> Transform {
        let base = self.transform.get_or(Transform::IDENTITY);
        self.resolve_state_value(tree, base, |state| state.transform)
    }

    /// Get the effective elevation considering state layers (not animated).
    fn effective_elevation(&self, tree: &Tree) -> f32 {
        let base = self.elevation.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.elevation)
    }

    /// Get current padding (animated or static)
    fn animated_padding(&self) -> Padding {
        get_animated_value(self.anims.as_ref().and_then(|a| a.padding.as_ref()), || {
            self.padding.get_or(Padding::default())
        })
    }

    /// Get current background color (animated or effective target)
    fn animated_background(&self, tree: &Tree) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.background.as_ref()),
            || self.effective_background_target(tree),
        )
    }

    /// Get current corner radius (animated or effective target)
    fn animated_corner_radius(&self, tree: &Tree) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.corner_radius.as_ref()),
            || self.effective_corner_radius_target(tree),
        )
    }

    /// Get current border width (animated or effective target)
    fn animated_border_width(&self, tree: &Tree) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_width.as_ref()),
            || self.effective_border_width_target(tree),
        )
    }

    /// Get current border color (animated or effective target)
    fn animated_border_color(&self, tree: &Tree) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_color.as_ref()),
            || self.effective_border_color_target(tree),
        )
    }

    /// Get current transform (animated or effective target)
    fn animated_transform(&self, tree: &Tree) -> Transform {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.transform.as_ref()),
            || self.effective_transform_target(tree),
        )
    }

    /// Check if any state layer properties have animations enabled
    fn has_animated_state_properties(&self) -> bool {
        self.anims.as_ref().is_some_and(|a| {
            a.background.is_some()
                || a.corner_radius.is_some()
                || a.border_color.is_some()
                || a.transform.is_some()
        })
    }

    /// Request repaint for state changes (hover/press), with Animation job if needed
    fn request_state_change_repaint(&self, id: WidgetId) {
        if self.has_animated_state_properties() {
            request_job(id, JobRequest::Animation(RequiredJob::Paint));
        } else {
            request_job(id, JobRequest::Paint);
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
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        // Use advance_animations_self for this widget's animations
        let mut any_animating = false;

        #[allow(clippy::unnecessary_unwrap)]
        // Intentional: compute targets with &self before &mut borrow
        if self.anims.is_some() {
            // Compute targets before borrowing anims mutably (&self methods conflict
            // with &mut self.anims). Skipped entirely for the majority of non-animated
            // containers since self.anims is None.
            let padding_target = self.padding.get_or(Padding::default());
            let border_width_target = self.effective_border_width_target(tree);
            let bg_target = self.effective_background_target(tree);
            let corner_radius_target = self.effective_corner_radius_target(tree);
            let border_color_target = self.effective_border_color_target(tree);
            let transform_target = self.effective_transform_target(tree);
            let anims = self.anims.as_mut().unwrap();
            // Layout-affecting animations: width, height, padding
            advance_anim!(anims, width, id, any_animating, layout);
            advance_anim!(anims, height, id, any_animating, layout);
            advance_anim!(anims, padding, padding_target, id, any_animating, layout);

            // Paint-only animations: border_width, background, corner_radius, border_color, transform
            advance_anim!(
                anims,
                border_width,
                border_width_target,
                id,
                any_animating,
                paint
            );
            advance_anim!(anims, background, bg_target, id, any_animating, paint);
            advance_anim!(
                anims,
                corner_radius,
                corner_radius_target,
                id,
                any_animating,
                paint
            );
            advance_anim!(
                anims,
                border_color,
                border_color_target,
                id,
                any_animating,
                paint
            );
            advance_anim!(anims, transform, transform_target, id, any_animating, paint);
        }

        // Advance ripple animation
        if let Some(ref mut ix) = self.interaction
            && ix.ripple.is_active()
            && let Some(ref state) = ix.pressed_state
            && let Some(ref config) = state.ripple
        {
            let ripple_animating = ix.ripple.advance(config);
            if ripple_animating {
                // Ripple is paint-only, request animation continuation with paint
                request_job(id, JobRequest::Animation(RequiredJob::Paint));
            }
            any_animating = any_animating || ripple_animating;
        }

        // Advance kinetic scroll animation
        if let Some(ref mut sd) = self.scroll_data {
            let has_scroll_velocity =
                sd.scroll_state.velocity_x.abs() > 0.5 || sd.scroll_state.velocity_y.abs() > 0.5;
            if has_scroll_velocity {
                let scroll_animating = sd.scroll_state.advance_momentum();
                if scroll_animating {
                    // Kinetic scroll is paint-only, request animation continuation with paint
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }
                any_animating = any_animating || scroll_animating;
            }
        }

        // Update scrollbar handle positions based on current scroll offset
        // (scroll is paint-only, so layout may not run during scrolling)
        if self.scroll_axis != ScrollAxis::None {
            self.update_scrollbar_handle_positions(tree, id);
        }

        // Advance scrollbar scale animations (for hover expansion effect)
        // Must be done here since scroll/hover is paint-only and layout may not run
        if self.advance_scrollbar_scale_animations_internal(id) {
            any_animating = true;
        }

        // Note: No final Animation push needed here - each animation source
        // (advance_anim! macro, ripple, kinetic scroll) handles its own continuation

        any_animating
    }

    fn reconcile_children(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        // Ensure container_id is set before reconciliation
        self.children_source.set_container_id(id);
        self.children_source.reconcile_with_tracking(tree)
    }

    fn register_children(&mut self, tree: &mut Tree, id: WidgetId) {
        // Set container_id for children source
        self.children_source.set_container_id(id);

        // Register pending children
        self.children_source.register_pending(tree, id);
    }

    fn layout_hints(&self) -> LayoutHints {
        if !self.visible.get_or(true) {
            return LayoutHints::default();
        }
        LayoutHints {
            fill_width: self.width.as_ref().map(|w| w.get().fill).unwrap_or(false),
            fill_height: self.height.as_ref().map(|h| h.get().fill).unwrap_or(false),
        }
    }

    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        // Check visibility with signal tracking so changes trigger re-layout
        let is_visible = with_signal_tracking(id, JobType::Layout, || self.visible.get_or(true));
        if !is_visible {
            tree.set_relayout_boundary(id, true);
            let size = Size::zero();
            tree.cache_layout(id, constraints, size);
            tree.clear_needs_layout(id);
            return size;
        }

        // Register this widget's relayout boundary status with the tree
        tree.set_relayout_boundary(id, self.is_relayout_boundary_for(constraints));

        // Ensure scrollbar containers exist if scrolling is enabled
        self.ensure_scrollbar_containers(tree, id);

        // Check if constraints changed compared to cached value in Tree
        let constraints_changed = tree.cached_constraints(id) != Some(constraints);

        // Check if this widget was marked dirty by signal changes or animations
        // (animations call mark_needs_layout directly when their value changes)
        let reactive_changed = tree.needs_layout(id);

        let needs_layout = constraints_changed || reactive_changed;

        if !needs_layout {
            crate::render_stats::record_layout_skipped();
            // Return cached size from Tree
            return tree.cached_size(id).unwrap_or_default();
        }

        crate::render_stats::record_layout_executed_with_reasons(
            crate::render_stats::LayoutReasons {
                constraints_changed,
                reactive_changed,
            },
        );

        // Clear dirty flag since we're doing layout now
        tree.clear_needs_layout(id);

        // Auto-track signal reads for layout properties.
        // Any signals read here (including closures) will register this widget
        // as a Layout subscriber so future changes trigger re-layout.
        let (padding, width_length, height_length) =
            with_signal_tracking(id, JobType::Layout, || {
                (
                    self.animated_padding(),
                    self.width.as_ref().map(|w| w.get()).unwrap_or_default(),
                    self.height.as_ref().map(|h| h.get()).unwrap_or_default(),
                )
            });

        // Calculate dimensions for child layout constraints.
        // When a layout animation is active and the width/height is exact, use
        // the animated current value so children are positioned within the actual
        // visible bounds (e.g., Center alignment tracks the animating size).
        // For non-exact (shrink-to-fit) widths, use the signal value to avoid a
        // circular dependency: animated width → constrains children → target = clamped.
        let child_layout_width =
            if let Some(anim) = self.anims.as_ref().and_then(|a| a.width.as_ref()) {
                if !anim.is_initial() && width_length.exact.is_some() {
                    *anim.current()
                } else {
                    width_length.exact.unwrap_or(constraints.max_width)
                }
            } else if let Some(exact) = width_length.exact {
                exact
            } else {
                let w = constraints.max_width;
                if let Some(max) = width_length.max {
                    w.min(max)
                } else {
                    w
                }
            };

        let child_layout_height =
            if let Some(anim) = self.anims.as_ref().and_then(|a| a.height.as_ref()) {
                if !anim.is_initial() && height_length.exact.is_some() {
                    *anim.current()
                } else {
                    height_length.exact.unwrap_or(constraints.max_height)
                }
            } else if let Some(exact) = height_length.exact {
                exact
            } else {
                let h = constraints.max_height;
                if let Some(max) = height_length.max {
                    h.min(max)
                } else {
                    h
                }
            };

        // Child constraints with padding
        let mut child_max_width = (child_layout_width - padding.horizontal()).max(0.0);
        let mut child_max_height = (child_layout_height - padding.vertical()).max(0.0);

        // Reserve gutter space for scrollbars
        if let Some(ref sd) = self.scroll_data
            && sd.scrollbar_config.reserve_gutter
            && sd.scrollbar_visibility != ScrollbarVisibility::Hidden
        {
            let gutter = sd.scrollbar_config.width + sd.scrollbar_config.margin * 2.0;
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
            // Propagate the effective minimum so layouts like Center/End know
            // how much space they actually have to position children within.
            // Sources of minimum: explicit at_least(min) or parent constraints.
            let effective_min = width_length.min.unwrap_or(0.0).max(constraints.min_width);
            (effective_min - padding.horizontal())
                .max(0.0)
                .min(child_max_width)
        };
        let child_min_height = if height_length.exact.is_some() || height_length.fill {
            child_max_height
        } else {
            let effective_min = height_length.min.unwrap_or(0.0).max(constraints.min_height);
            (effective_min - padding.vertical())
                .max(0.0)
                .min(child_max_height)
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

        // Update scroll state with the viewport dimensions available for children.
        if scroll_axis != ScrollAxis::None {
            let sd = self.scroll_mut();
            sd.scroll_state.content_width = content_size.width + padding.horizontal();
            sd.scroll_state.content_height = content_size.height + padding.vertical();
            sd.scroll_state.viewport_width = child_max_width;
            sd.scroll_state.viewport_height = child_max_height;
            sd.scroll_state.clamp_offsets();
        }

        let content_width = content_size.width + padding.horizontal();
        let content_height = content_size.height + padding.vertical();

        // Update animation targets
        if let Some(ref mut anims) = self.anims {
            if let Some(ref mut anim) = anims.width {
                let effective_target = if let Some(exact) = width_length.exact {
                    exact
                } else {
                    let min_w = width_length.min.unwrap_or(0.0);
                    content_width.max(min_w)
                };
                if anim.is_initial() {
                    // Always mark as initialized on first layout so subsequent
                    // changes animate rather than snap.
                    anim.set_immediate(effective_target);
                } else if (effective_target - *anim.target()).abs() > 0.001 {
                    anim.animate_to(effective_target);
                    // Width affects layout, so use RequiredJob::Layout
                    request_job(id, JobRequest::Animation(RequiredJob::Layout));
                    // Parent must reposition siblings as this child's width changes
                    if let Some(parent_id) = tree.get_parent(id) {
                        request_job(parent_id, JobRequest::Layout);
                    }
                }
            }

            if let Some(ref mut anim) = anims.height {
                let effective_target = if let Some(exact) = height_length.exact {
                    exact
                } else {
                    let min_h = height_length.min.unwrap_or(0.0);
                    content_height.max(min_h)
                };
                if anim.is_initial() {
                    anim.set_immediate(effective_target);
                } else if (effective_target - *anim.target()).abs() > 0.001 {
                    anim.animate_to(effective_target);
                    // Height affects layout, so use RequiredJob::Layout
                    request_job(id, JobRequest::Animation(RequiredJob::Layout));
                    // Parent must reposition siblings as this child's height changes
                    if let Some(parent_id) = tree.get_parent(id) {
                        request_job(parent_id, JobRequest::Layout);
                    }
                }
            }
        }

        // Initialize paint animations on first layout (set_immediate so they start
        // from the correct signal value rather than a stale construction-time value)
        // Compute targets first to avoid borrow conflicts with &mut anim + &self
        let bg_init = self
            .anims
            .as_ref()
            .and_then(|a| a.background.as_ref())
            .is_some_and(|a| a.is_initial());
        let cr_init = self
            .anims
            .as_ref()
            .and_then(|a| a.corner_radius.as_ref())
            .is_some_and(|a| a.is_initial());
        let bc_init = self
            .anims
            .as_ref()
            .and_then(|a| a.border_color.as_ref())
            .is_some_and(|a| a.is_initial());
        let tf_init = self
            .anims
            .as_ref()
            .and_then(|a| a.transform.as_ref())
            .is_some_and(|a| a.is_initial());
        if bg_init || cr_init || bc_init || tf_init {
            let bg_target = if bg_init {
                Some(self.effective_background_target(tree))
            } else {
                None
            };
            let cr_target = if cr_init {
                Some(self.effective_corner_radius_target(tree))
            } else {
                None
            };
            let bc_target = if bc_init {
                Some(self.effective_border_color_target(tree))
            } else {
                None
            };
            let tf_target = if tf_init {
                Some(self.effective_transform_target(tree))
            } else {
                None
            };
            if let Some(ref mut anims) = self.anims {
                if let (Some(anim), Some(target)) = (&mut anims.background, bg_target) {
                    anim.set_immediate(target);
                }
                if let (Some(anim), Some(target)) = (&mut anims.corner_radius, cr_target) {
                    anim.set_immediate(target);
                }
                if let (Some(anim), Some(target)) = (&mut anims.border_color, bc_target) {
                    anim.set_immediate(target);
                }
                if let (Some(anim), Some(target)) = (&mut anims.transform, tf_target) {
                    anim.set_immediate(target);
                }
            }
        }

        // Determine shrink behavior
        let width_animating = self
            .anims
            .as_ref()
            .and_then(|a| a.width.as_ref())
            .is_some_and(|a| a.is_animating());
        let height_animating = self
            .anims
            .as_ref()
            .and_then(|a| a.height.as_ref())
            .is_some_and(|a| a.is_animating());
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
        let mut width = if let Some(anim) = self.anims.as_ref().and_then(|a| a.width.as_ref()) {
            if allow_shrink_width {
                *anim.current()
            } else {
                content_width.max(*anim.current())
            }
        } else if let Some(exact) = width_length.exact {
            exact
        } else if width_length.fill {
            constraints.max_width
        } else {
            content_width
        };

        // Apply Length min/max as post-adjustments (works with all cases including fill)
        if let Some(min) = width_length.min {
            width = width.max(min);
        }
        if let Some(max) = width_length.max {
            width = width.min(max);
        }

        let mut height = if let Some(anim) = self.anims.as_ref().and_then(|a| a.height.as_ref()) {
            if allow_shrink_height {
                *anim.current()
            } else {
                content_height.max(*anim.current())
            }
        } else if let Some(exact) = height_length.exact {
            exact
        } else if height_length.fill {
            constraints.max_height
        } else {
            content_height
        };

        // Apply Length min/max as post-adjustments (works with all cases including fill)
        if let Some(min) = height_length.min {
            height = height.max(min);
        }
        if let Some(max) = height_length.max {
            height = height.min(max);
        }

        let has_width_anim = self.anims.as_ref().is_some_and(|a| a.width.is_some());
        let has_height_anim = self.anims.as_ref().is_some_and(|a| a.height.is_some());
        if !allow_shrink_width && !has_width_anim && !has_exact_width {
            width = width.max(content_width);
        }
        if !allow_shrink_height && !has_height_anim && !has_exact_height {
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

        // Layout scrollbar containers after size is determined
        // Note: cache_layout is called at the end which stores size in Tree
        self.layout_scrollbar_containers(tree, id, size);

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // Register widget ref so update_widget_refs() can refresh bounds
        if let Some(ref wr) = self.widget_ref {
            register_widget_ref(id, wr.rw_signal());
        }

        size
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        if !self.visible.get_or(true) {
            return EventResponse::Ignored;
        }

        // Get bounds from Tree (single source of truth)
        let bounds = tree.get_bounds(id).unwrap_or_default();

        let transform = self.animated_transform(tree);
        let transform_origin = self.transform_origin.get_or(TransformOrigin::CENTER);
        let corner_radius = self.animated_corner_radius(tree);

        // Transform event coordinates to local space
        let local_event: Cow<'_, Event> = if !transform.is_identity() {
            if let Some((x, y)) = event.coords() {
                let (origin_x, origin_y) = transform_origin.resolve(bounds);
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
        if let Some(response) = self.handle_scrollbar_event(tree, id, bounds, &local_event) {
            return response;
        }

        // Pre-dispatch: update hover state and fire pointer move callback
        // before children get the event. This ensures parent hover tracking
        // works even when a child container handles the MouseMove/MouseEnter.
        let has_animated = self.has_animated_state_properties();
        if let Some(ref mut ix) = self.interaction {
            let request_repaint = |id: WidgetId| {
                if has_animated {
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                } else {
                    request_job(id, JobRequest::Paint);
                }
            };
            match local_event.as_ref() {
                Event::MouseEnter { x, y } => {
                    if bounds.contains_rounded(*x, *y, corner_radius) && !ix.is_hovered {
                        ix.is_hovered = true;
                        if ix.hover_state.is_some() {
                            request_repaint(id);
                        }
                        if let Some(ref callback) = ix.on_hover {
                            callback(true);
                        }
                    }
                }
                Event::MouseMove { x, y } => {
                    if let Some(ref callback) = ix.on_pointer_move
                        && (bounds.contains_rounded(*x, *y, corner_radius) || ix.is_pressed)
                    {
                        callback(*x - bounds.x, *y - bounds.y);
                    }

                    let was_hovered = ix.is_hovered;
                    ix.is_hovered = bounds.contains_rounded(*x, *y, corner_radius);

                    if was_hovered != ix.is_hovered {
                        if ix.hover_state.is_some() {
                            request_repaint(id);
                        }
                        if let Some(ref callback) = ix.on_hover {
                            callback(ix.is_hovered);
                        }
                    }
                }
                _ => {}
            }
        }

        // Transform event coordinates to local space (relative to container origin)
        // Children are positioned in local coordinates, so events must be too
        let child_event: Cow<'_, Event> = if let Some((x, y)) = local_event.coords() {
            // Convert from global/window coordinates to local (relative to container)
            let local_x = x - bounds.x;
            let local_y = y - bounds.y;

            // For scrollable containers, also add scroll offset
            let (child_x, child_y) = if self.scroll_axis != ScrollAxis::None {
                let sd = self.scroll();
                (
                    local_x + sd.scroll_state.offset_x,
                    local_y + sd.scroll_state.offset_y,
                )
            } else {
                (local_x, local_y)
            };
            Cow::Owned(local_event.with_coords(child_x, child_y))
        } else {
            local_event.clone()
        };

        // When overflow is hidden, children outside the container's bounds are
        // clipped and invisible. Skip dispatching pointer events to them so that
        // invisible children (e.g. inside a 0-height collapsed submenu) cannot
        // steal clicks from siblings positioned below.
        let skip_child_dispatch = (self.overflow == Overflow::Hidden
            || self.scroll_axis != ScrollAxis::None)
            && local_event
                .coords()
                .is_some_and(|(x, y)| !bounds.contains(x, y));

        // Let children handle first (layout already reconciled)
        if !skip_child_dispatch {
            for &child_id in self.children_source.get() {
                if let Some(response) = tree.with_widget_mut(child_id, |child, child_id, tree| {
                    child.event(tree, child_id, &child_event)
                }) && response == EventResponse::Handled
                {
                    return EventResponse::Handled;
                }
            }
        }

        // Handle our own events
        match local_event.as_ref() {
            // Hover tracking already handled in pre-dispatch above.
            // Don't return Handled — hover changes should not prevent
            // sibling containers from tracking their own hover state.
            Event::MouseEnter { .. } | Event::MouseMove { .. } => {}
            Event::MouseDown { x, y, button } => {
                if bounds.contains_rounded(*x, *y, corner_radius)
                    && *button == MouseButton::Left
                    && let Some(ref mut ix) = self.interaction
                {
                    let was_pressed = ix.is_pressed;
                    ix.is_pressed = true;

                    // Start ripple animation if configured
                    let has_ripple = ix
                        .pressed_state
                        .as_ref()
                        .is_some_and(|s| s.ripple.is_some());
                    if has_ripple {
                        // Convert screen coords to local coords accounting for transform
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        let (local_x, local_y) = if !transform.is_identity() {
                            let (origin_x, origin_y) = transform_origin.resolve(bounds);
                            let screen_transform = transform.center_at(origin_x, origin_y);
                            let (inv_x, inv_y) = screen_transform
                                .inverse()
                                .transform_point(screen_x, screen_y);
                            (inv_x - bounds.x, inv_y - bounds.y)
                        } else {
                            (screen_x - bounds.x, screen_y - bounds.y)
                        };
                        ix.ripple.start(local_x, local_y);
                        // Ripple animation needs Animation + Paint
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if !was_pressed && ix.pressed_state.is_some() {
                        self.request_state_change_repaint(id);
                    }
                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_down
                    {
                        callback(*x - bounds.x, *y - bounds.y);
                        return EventResponse::Handled;
                    }
                    if let Some(ref ix) = self.interaction
                        && (ix.on_click.is_some() || ix.on_mouse_up.is_some())
                    {
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseUp { x, y, button } => {
                if let Some(ref mut ix) = self.interaction
                    && ix.is_pressed
                    && *button == MouseButton::Left
                {
                    let was_pressed = ix.is_pressed;
                    ix.is_pressed = false;

                    // Start ripple fade animation
                    if ix.ripple.is_active() {
                        // Convert screen coords to local coords accounting for transform
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        let (local_x, local_y) = if !transform.is_identity() {
                            let (origin_x, origin_y) = transform_origin.resolve(bounds);
                            let screen_transform = transform.center_at(origin_x, origin_y);
                            let (inv_x, inv_y) = screen_transform
                                .inverse()
                                .transform_point(screen_x, screen_y);
                            (inv_x - bounds.x, inv_y - bounds.y)
                        } else {
                            (screen_x - bounds.x, screen_y - bounds.y)
                        };
                        ix.ripple.start_fade(local_x, local_y);
                        // Ripple fade animation needs Animation + Paint
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if was_pressed && ix.pressed_state.is_some() {
                        self.request_state_change_repaint(id);
                    }
                    let mut handled = false;
                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_up
                    {
                        callback(*x - bounds.x, *y - bounds.y);
                        handled = true;
                    }
                    if let Some(ref ix) = self.interaction
                        && bounds.contains_rounded(*x, *y, corner_radius)
                        && let Some(ref callback) = ix.on_click
                    {
                        callback();
                        return EventResponse::Handled;
                    }
                    if handled {
                        return EventResponse::Handled;
                    }
                }
            }
            Event::MouseLeave => {
                if let Some(ref mut ix) = self.interaction {
                    let was_hovered = ix.is_hovered;
                    let was_pressed = ix.is_pressed;
                    if ix.is_hovered {
                        ix.is_hovered = false;
                        if let Some(ref callback) = ix.on_hover {
                            callback(false);
                        }
                    }
                    ix.is_pressed = false;

                    // Start ripple fade to center
                    if ix.ripple.is_active() {
                        ix.ripple.start_fade_to_center(bounds.width, bounds.height);
                        // Ripple fade animation needs Animation + Paint
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if (was_hovered && ix.hover_state.is_some())
                        || (was_pressed && ix.pressed_state.is_some())
                    {
                        self.request_state_change_repaint(id);
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
                if bounds.contains_rounded(*x, *y, corner_radius) {
                    if self.scroll_axis != ScrollAxis::None {
                        let consumed = self.apply_scroll(*delta_x, *delta_y, *source);
                        if consumed {
                            // Kinetic scrolling needs Animation + Paint if has velocity
                            let sd = self.scroll();
                            let has_velocity = sd.scroll_state.velocity_x.abs() > 0.5
                                || sd.scroll_state.velocity_y.abs() > 0.5;
                            if has_velocity {
                                request_job(id, JobRequest::Animation(RequiredJob::Paint));
                            } else {
                                request_job(id, JobRequest::Paint);
                            }
                            return EventResponse::Handled;
                        }
                    }

                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_scroll
                    {
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

    fn has_focus_descendant(&self, tree: &Tree, focused_id: WidgetId) -> bool {
        if !self.visible.get_or(true) {
            return false;
        }
        self.widget_has_focus(tree, focused_id)
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        let is_visible = with_signal_tracking(id, JobType::Paint, || self.visible.get_or(true));
        if !is_visible {
            return;
        }

        // Get bounds from Tree (single source of truth)
        let bounds = tree.get_bounds(id).unwrap_or_default();

        // Auto-track signal reads for paint properties.
        // Any signals read here (including closures) will register this widget
        // as a Paint subscriber so future changes trigger repaint.
        let (
            background,
            corner_radius,
            corner_curvature,
            elevation_level,
            user_transform,
            transform_origin,
            border_width,
            border_color,
        ) = with_signal_tracking(id, JobType::Paint, || {
            (
                self.animated_background(tree),
                self.animated_corner_radius(tree),
                self.corner_curvature.get_or(1.0),
                self.effective_elevation(tree),
                self.animated_transform(tree),
                self.transform_origin.get_or(TransformOrigin::CENTER),
                self.animated_border_width(tree),
                self.animated_border_color(tree),
            )
        });

        // When animations exist, also track raw signal reads for Animation jobs.
        // This ensures signal changes trigger advance_animations() to update targets.
        // (The animated_* methods above may read from the animation cache instead of the signal,
        // so we do a second pass reading raw signals for Animation subscriber registration.)
        if self.has_animated_state_properties() {
            with_signal_tracking(id, JobType::Animation, || {
                if let Some(s) = &self.background {
                    let _ = s.get();
                }
                if let Some(s) = &self.corner_radius {
                    let _ = s.get();
                }
                if let Some(s) = &self.border_width {
                    let _ = s.get();
                }
                if let Some(s) = &self.border_color {
                    let _ = s.get();
                }
                if let Some(s) = &self.transform {
                    let _ = s.get();
                }
            });
        }

        let shadow = elevation_to_shadow(elevation_level);

        // LOCAL bounds (0,0 is widget origin) - all drawing uses these
        let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
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

        // Draw border using LOCAL coordinates (values captured above in with_signal_tracking)
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

        // Set clip region for scrollable or overflow:hidden containers
        // This clips all children to the container bounds
        if is_scrollable || self.overflow == Overflow::Hidden {
            ctx.set_clip(local_bounds, corner_radius, corner_curvature);
        }

        // Determine the effective cull rect for children.
        // For scrollable containers: viewport mapped to layout space (before scroll transform).
        // For non-scrollable containers: inherited from parent via PaintContext.
        let effective_cull_rect = if is_scrollable {
            let sd = self.scroll();
            Some(Rect::new(
                sd.scroll_state.offset_x,
                sd.scroll_state.offset_y,
                local_bounds.width,
                local_bounds.height,
            ))
        } else {
            ctx.cull_rect()
        };

        // Skip painting children when the container has zero area — nothing
        // can be visible and attempting to render (especially text) wastes
        // atlas space and GPU work.
        if bounds.width < 0.5 || bounds.height < 0.5 {
            return;
        }

        // Draw children - each gets its own node with position transform
        for &child_id in self.children_source.get() {
            // Get child bounds from Tree - these are in LOCAL coordinates (relative to parent)
            let child_bounds = tree
                .get_bounds(child_id)
                .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            // Child's LOCAL bounds (0,0 origin with its own width/height)
            let child_local = Rect::new(0.0, 0.0, child_bounds.width, child_bounds.height);
            // Child offset is directly from child bounds (already in local coordinates)
            let child_offset_x = child_bounds.x;
            let child_offset_y = child_bounds.y;

            // Child's position transform (may include scroll offset)
            let child_position = if is_scrollable {
                let sd = self.scroll();
                Transform::translate(
                    child_offset_x - sd.scroll_state.offset_x,
                    child_offset_y - sd.scroll_state.offset_y,
                )
            } else {
                Transform::translate(child_offset_x, child_offset_y)
            };

            // Cull clean off-screen children using the effective viewport
            if let Some(ref cull_rect) = effective_cull_rect
                && !tree.needs_paint(child_id)
            {
                let child_rect = Rect::new(
                    child_offset_x,
                    child_offset_y,
                    child_bounds.width,
                    child_bounds.height,
                );
                if !cull_rect.intersects(&child_rect) {
                    crate::render_stats::record_paint_child_culled();
                    continue;
                }
            }

            // Try cached paint for clean children.
            // For scrollable containers, skip cached paint for direct children so their
            // paint method runs and can cull grandchildren using the propagated cull_rect.
            if !is_scrollable
                && !tree.needs_paint(child_id)
                && let Some(cached) = tree.cached_paint(child_id)
            {
                let mut reused = cached.clone();
                // Decompose: extract user transform, recompose with new position
                let user_part = cached
                    .parent_position
                    .inverse()
                    .then(&cached.local_transform);
                reused.local_transform = child_position.then(&user_part);
                reused.parent_position = child_position;
                reused.bounds = child_local;
                reused.repainted = false;
                ctx.add_child_node(reused);
                crate::render_stats::record_paint_child_cached();
                continue;
            }

            // Full paint (child is dirty, no cache, or scrollable container child)
            let mut child_ctx = ctx.add_child(child_id.as_u64(), child_local);
            child_ctx.set_transform(child_position);

            // Propagate cull_rect to child (transformed into child's local space)
            if let Some(ref cull_rect) = effective_cull_rect {
                child_ctx.set_cull_rect(Rect::new(
                    cull_rect.x - child_offset_x,
                    cull_rect.y - child_offset_y,
                    cull_rect.width,
                    cull_rect.height,
                ));
            }

            // Paint child via tree
            tree.with_widget(child_id, |child| {
                child.paint(tree, child_id, &mut child_ctx)
            });
            crate::render_stats::record_paint_child_painted();
        }

        // Draw scrollbar containers
        if is_scrollable {
            self.paint_scrollbar_containers(tree, id, ctx);
        }

        // Draw ripple effect as overlay (ripple.center is already in local coordinates)
        if let Some(ref ix) = self.interaction
            && let Some((local_cx, local_cy)) = ix.ripple.center
            && let Some(ref pressed_state) = ix.pressed_state
            && let Some(ref ripple_config) = pressed_state.ripple
            && ix.ripple.opacity > 0.0
        {
            // Set overlay clip to container bounds with rounded corners
            // This clips the ripple without affecting children
            ctx.set_overlay_clip(local_bounds, corner_radius, corner_curvature);

            let max_dist_x = local_cx.max(bounds.width - local_cx);
            let max_dist_y = local_cy.max(bounds.height - local_cy);
            let max_radius = (max_dist_x * max_dist_x + max_dist_y * max_dist_y).sqrt();
            let current_radius = max_radius * ix.ripple.progress;

            let ripple_color = Color::rgba(
                ripple_config.color.r,
                ripple_config.color.g,
                ripple_config.color.b,
                ripple_config.color.a * ix.ripple.opacity,
            );

            ctx.draw_overlay_circle(local_cx, local_cy, current_radius, ripple_color);
        }
    }
}

pub fn container() -> Container {
    Container::new()
}
