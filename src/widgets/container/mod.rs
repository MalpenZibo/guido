//! Container widget and related functionality.

mod anim_bridge;
mod animations;
mod box_model;
mod interaction;
mod ripple;
mod scrollable;
mod style;

#[cfg(test)]
mod characterization;

pub(crate) use animations::with_measure_final;
pub use animations::{AdvanceResult, AnimationState, get_animated_value};
use interaction::{HitContext, untransform_point};
pub use ripple::{MAX_LIVE_RIPPLES, Ripple, RippleState};
use style::Decoration;

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use crate::advance_anim;
use crate::animation::TransitionConfig;
use crate::backdrop::{BackdropBlur, BackdropSources};
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job};
use crate::layout::{Constraints, Flex, Layout, Length, Size};
use crate::reactive::{
    IntoSignal, OptionSignalExt, OwnerId, RwSignal, Signal, create_derived, create_signal,
    create_stored, dispose_owner_now, focus_path, with_owner, with_signal_tracking,
};
use crate::renderer::{GradientDir, PaintContext, Shadow};
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::tree::{Tree, WidgetId};
use crate::widget_ref::{WidgetRef, register_widget_ref};

use super::children::ChildrenSource;
use super::control::Control;
use super::font::{FontFamily, FontWeight};
use super::input_style::InputStyle;
use super::into_child::{IntoChild, IntoChildren};
use super::paint_children::{ChildPaintOptions, paint_children};
use super::scroll::{
    ScrollAxis, ScrollState, ScrollbarBuilder, ScrollbarConfig, ScrollbarVisibility,
};
use super::state_layer::{RippleConfig, StateStyle, StateWhen, Stateful, resolve_background};
use super::text_style::{TextShadow, TextStroke, TextStyle};
use super::widget::{
    Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding, Rect,
    ScrollSource, Widget,
};

/// Callback for click events
pub type ClickCallback = Rc<dyn Fn()>;

#[doc(hidden)]
pub struct FnHandler;
#[doc(hidden)]
pub struct CallbackHandler;
#[doc(hidden)]
pub struct OptionHandler;

/// What a click handler can be written as.
///
/// A closure is the ordinary case. A [`Callback`](crate::reactive::Callback)
/// and an `Option<Callback>` are the shapes a `#[component]` callback prop
/// arrives in, so a component can forward its own prop straight through
/// without a second method name for it.
///
/// The marker parameter is the same trick [`IntoSignal`] uses: it keeps the
/// three impls from overlapping.
pub trait IntoClickHandler<Marker = FnHandler> {
    fn into_click_handler(self) -> Option<ClickCallback>;
}

impl<F: Fn() + 'static> IntoClickHandler<FnHandler> for F {
    fn into_click_handler(self) -> Option<ClickCallback> {
        Some(Rc::new(self))
    }
}

impl IntoClickHandler<CallbackHandler> for crate::reactive::Callback {
    fn into_click_handler(self) -> Option<ClickCallback> {
        Some(Rc::new(move || self.run()))
    }
}

impl IntoClickHandler<OptionHandler> for Option<crate::reactive::Callback> {
    fn into_click_handler(self) -> Option<ClickCallback> {
        self.map(|cb| Rc::new(move || cb.run()) as ClickCallback)
    }
}

/// Callback for a key press: the key and the modifiers held with it.
pub type KeyCallback = Rc<dyn Fn(Key, Modifiers)>;
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
#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub(super) elevation: Option<AnimationState<f32>>,
    pub(super) padding: Option<AnimationState<Padding>>,
    pub(super) border_width: Option<AnimationState<f32>>,
    pub(super) border_color: Option<AnimationState<Color>>,
    pub(super) transform: Option<AnimationState<Transform>>,
    pub(super) text_color: Option<AnimationState<Color>>,
}

bitflags::bitflags! {
    /// What the pointer is doing to this container.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub(crate) struct InteractionFlags: u8 {
        const HOVERED = 1;
        const PRESSED = 2;
    }
}

/// Interaction state (callbacks, hover/press tracking, state styles, ripple).
/// Only allocated when `.on_click()`, `.when_hovered()`, `.when_pressed()`, etc. are called.
pub(super) struct InteractionState {
    pub(super) on_click: Option<ClickCallback>,
    pub(super) on_right_click: Option<ClickCallback>,
    pub(super) on_middle_click: Option<ClickCallback>,
    pub(super) on_key_down: Option<KeyCallback>,
    pub(super) on_hover: Option<HoverCallback>,
    pub(super) on_scroll: Option<ScrollCallback>,
    pub(super) on_pointer_move: Option<PointerMoveCallback>,
    pub(super) on_mouse_down: Option<MouseDownCallback>,
    pub(super) on_mouse_up: Option<MouseUpCallback>,
    /// Hover and press, behind a signal so that *resolving a state layer
    /// subscribes to them*.
    ///
    /// This is the whole point. The container's own paint subscribes, as
    /// before — but so does any descendant text whose colour resolves through
    /// this container, and that is what a plain `bool` plus a hand-written
    /// `request_job(id, Paint)` could never do: it marked one widget, and the
    /// text's cached paint node was reused with the old colour still in it.
    ///
    /// Read with `get_untracked` from event handling, where the value drives
    /// *behaviour* (drag capture, ripple) and a subscription would be noise;
    /// with `get` from style resolution, where the subscription is the point.
    pub(super) flags: RwSignal<InteractionFlags>,
    /// State layers in declaration order. Resolution walks it backwards, so
    /// the last one declared wins wherever two of them speak about the same
    /// property — CSS's rule at equal specificity, and the only one that lets
    /// a caller decide that an error outranks the focus.
    pub(super) states: Vec<(StateWhen, StateStyle)>,
    pub(super) ripple: RippleState,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            on_click: None,
            on_right_click: None,
            on_middle_click: None,
            on_key_down: None,
            on_hover: None,
            on_scroll: None,
            on_pointer_move: None,
            on_mouse_down: None,
            on_mouse_up: None,
            flags: create_signal(InteractionFlags::empty()),
            states: Vec::new(),
            ripple: RippleState::new(),
        }
    }
}

impl InteractionState {
    pub(super) fn is_hovered(&self) -> bool {
        self.flags
            .get_untracked()
            .contains(InteractionFlags::HOVERED)
    }

    pub(super) fn is_pressed(&self) -> bool {
        self.flags
            .get_untracked()
            .contains(InteractionFlags::PRESSED)
    }

    /// Set a flag, writing only on a real change so an unchanged pointer move
    /// does not wake every subscriber.
    pub(super) fn set_flag(&self, flag: InteractionFlags, on: bool) {
        let current = self.flags.get_untracked();
        let next = if on { current | flag } else { current - flag };
        if next != current {
            self.flags.set(next);
        }
    }

    /// Whether any state layer is declared at all.
    ///
    /// Gates the subscription in style resolution: a container with only an
    /// `on_click` has nothing to resolve, and must not start repainting on
    /// every hover just because the flags became reactive.
    pub(super) fn has_any_state(&self) -> bool {
        !self.states.is_empty()
    }

    /// Whether a layer with this trigger is declared, without reading any
    /// signal — the gate event handling uses before asking for a repaint.
    pub(super) fn declares(&self, when: impl Fn(&StateWhen) -> bool) -> bool {
        self.states.iter().any(|(w, _)| when(w))
    }

    /// The ripple a pressed layer declares, if one does.
    ///
    /// By value: the caller advances `self.ripple` while holding it, and a
    /// borrow of the whole `InteractionState` would stand in the way of a
    /// borrow of one of its fields.
    pub(super) fn ripple_config(&self) -> Option<RippleConfig> {
        self.states
            .iter()
            .rev()
            .find(|(when, s)| matches!(when, StateWhen::Pressed) && s.ripple.is_some())
            .and_then(|(_, s)| s.ripple.clone())
    }

    /// Whether the layer is active right now. Reading this is what subscribes
    /// the caller to the state, which is why it is not asked for layers that
    /// declare nothing about the property being resolved.
    pub(super) fn is_active(&self, id: WidgetId, when: &StateWhen) -> bool {
        match when {
            StateWhen::Hovered => self.flags.get().contains(InteractionFlags::HOVERED),
            StateWhen::Pressed => self.flags.get().contains(InteractionFlags::PRESSED),
            StateWhen::Focused => Container::has_child_focus(id),
            StateWhen::When(condition) => condition.get(),
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
    pub(super) gradient: Option<Signal<LinearGradient>>,
    pub(super) corner_radius: Option<Signal<f32>>,
    pub(super) corner_radii: Option<Signal<crate::renderer::CornerRadii>>,
    pub(super) corner_curvature: Option<Signal<f32>>,
    pub(super) border_width: Option<Signal<f32>>,
    pub(super) border_color: Option<Signal<Color>>,
    pub(super) elevation: Option<Signal<f32>>,
    pub(super) width: Option<Signal<Length>>,
    pub(super) height: Option<Signal<Length>>,
    pub(super) overflow: Option<Signal<Overflow>>,
    /// What `overflow` resolved to in the last layout or paint.
    ///
    /// Event dispatch needs it — a clipped child must not answer for a click
    /// outside the bounds — but it needs the value the *drawn* frame used, and
    /// it needs it without running application code: a closure-backed signal
    /// recomputes on every read, and this one would be read for every container
    /// on the pointer's path, on every coalesced MouseMove. The tracked reads
    /// that do subscribe write here on their way past.
    pub(super) overflow_resolved: Cell<Overflow>,
    pub(super) visible: Option<Signal<bool>>,
    pub(super) transform: Option<Signal<Transform>>,
    pub(super) transform_origin: Option<Signal<TransformOrigin>>,

    // Interaction state (callbacks, hover/press, state styles, ripple)
    // Only allocated when interaction features are used
    pub(super) interaction: Option<Box<InteractionState>>,

    // Widget ref for reactive bounds tracking
    pub(super) widget_ref: Option<WidgetRef>,

    // Backdrop blur: this surface's own content, the compositor's, or both.
    pub(super) backdrop_blur: Option<Signal<BackdropBlur>>,

    // How text inside this container looks. Boxed: most containers hold no
    // text and pay one pointer for the whole feature.
    pub(super) text: Option<Box<TextStyle>>,

    /// What this container declares for the inputs below it. Separate from
    /// `text` because a text cannot draw any of it.
    pub(super) input: Option<Box<InputStyle>>,

    /// Declared with `control()`. A container is an interaction unit for other
    /// reasons too — see `is_control` — so this is only the explicit half.
    pub(super) declared_control: bool,

    // Owns the derived published in place of the text colour when a state
    // layer declares one. Created at registration, so it belongs to no user
    // scope and has to be torn down by hand when the container goes.
    pub(super) text_owner: Option<OwnerId>,

    // The in-flight value of an animated text colour, `None` while nothing is
    // animating. Every other animated property is consumed by the paint of the
    // container that owns it; this one is drawn by a *different* widget, so
    // the value has to leave through a signal — a write per frame, which is
    // what a per-frame repaint of the text costs under any design.
    pub(super) animated_text: Option<RwSignal<Option<Color>>>,

    // The base the published derived falls back to: this container's own
    // declaration, or the nearest ancestor's, resolved once at registration.
    //
    // The animation has to aim at the same value the derived would fold, or it
    // departs from a colour the text was never showing — see
    // `effective_text_color_target`.
    pub(super) text_base: Option<Signal<Color>>,

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
            corner_radii: None,
            corner_curvature: None,
            border_width: None,
            border_color: None,
            elevation: None,
            width: None,
            height: None,
            overflow: None,
            overflow_resolved: Cell::new(Overflow::Visible),
            visible: None,
            transform: None,
            transform_origin: None,
            interaction: None,
            widget_ref: None,
            backdrop_blur: None,
            text: None,
            input: None,
            declared_control: false,
            text_owner: None,
            animated_text: None,
            text_base: None,
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

    /// Add a single child: a widget value, or [`dynamic(data, build)`](crate::widgets::dynamic)
    /// for reactive content.
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
    /// - `padding(Padding::all(8.0).with_top(20.0))` — builder pattern
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

    // -----------------------------------------------------------------------
    // Text
    //
    // Text widgets carry content, not style; how they look is declared here.
    // Each property is inherited by descendants until a nearer container
    // overrides it — see [`TextStyle`](crate::widgets::TextStyle).
    // -----------------------------------------------------------------------

    fn text_mut(&mut self) -> &mut TextStyle {
        self.text.get_or_insert_with(Box::default)
    }

    fn input_mut(&mut self) -> &mut InputStyle {
        self.input.get_or_insert_with(Box::default)
    }

    /// Set the colour of text in this container and its descendants.
    ///
    /// Named `text_color` rather than `color` because `color` on a box reads
    /// as its fill — that one is [`background`](Self::background).
    ///
    /// ```ignore
    /// container().text_color(theme.text).child(text("Hello"))
    /// ```
    pub fn text_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.text_mut().color = Some(color.into_signal());
        self
    }

    /// Set the font size of text in this container and its descendants, in
    /// logical pixels.
    pub fn font_size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.text_mut().font_size = Some(size.into_signal());
        self
    }

    /// Set the font family of text in this container and its descendants.
    ///
    /// ```ignore
    /// container().font_family(FontFamily::Name("Inter".into()))
    /// ```
    pub fn font_family<M>(mut self, family: impl IntoSignal<FontFamily, M>) -> Self {
        self.text_mut().font_family = Some(family.into_signal());
        self
    }

    /// Set the font weight of text in this container and its descendants, on
    /// the CSS 100-900 scale.
    pub fn font_weight<M>(mut self, weight: impl IntoSignal<FontWeight, M>) -> Self {
        self.text_mut().font_weight = Some(weight.into_signal());
        self
    }

    /// Shorthand for [`font_weight(FontWeight::BOLD)`](Self::font_weight).
    pub fn bold(self) -> Self {
        self.font_weight(FontWeight::BOLD)
    }

    /// Shorthand for [`font_family(FontFamily::Monospace)`](Self::font_family).
    pub fn mono(self) -> Self {
        self.font_family(FontFamily::Monospace)
    }

    /// Draw a contour around the glyphs, under the fill.
    ///
    /// Named `text_stroke` and not `text_outline` because CSS `outline` is the
    /// contour of a *box* — this is the one CSS spells `-webkit-text-stroke`.
    ///
    /// ```ignore
    /// container().text_color(Color::WHITE).text_stroke(1.5, Color::BLACK)
    /// ```
    pub fn text_stroke<M>(mut self, stroke: impl IntoSignal<TextStroke, M>) -> Self {
        self.text_mut().stroke = Some(stroke.into_signal());
        self
    }

    /// Cast a soft shadow from the glyphs, as CSS `text-shadow`.
    ///
    /// ```ignore
    /// container().text_shadow(TextShadow::new(0.0, 2.0, 8.0, shadow_color))
    /// ```
    pub fn text_shadow<M>(mut self, shadow: impl IntoSignal<TextShadow, M>) -> Self {
        self.text_mut().shadow = Some(shadow.into_signal());
        self
    }

    /// Set the caret colour of any [`TextInput`](crate::widgets::TextInput)
    /// below this container. Defaults to the text colour.
    pub fn cursor_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_mut().cursor_color = Some(color.into_signal());
        self
    }

    /// Set the selection highlight colour of any
    /// [`TextInput`](crate::widgets::TextInput) below this container.
    pub fn selection_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_mut().selection_color = Some(color.into_signal());
        self
    }

    /// Set the placeholder colour of any
    /// [`TextInput`](crate::widgets::TextInput) below this container.
    ///
    /// Defaults to the inherited text colour at reduced alpha, which is what a
    /// placeholder is: the same text, quieter.
    pub fn placeholder_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.input_mut().placeholder_color = Some(color.into_signal());
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

    /// Set per-corner radii (overrides `corner_radius` for drawing).
    ///
    /// Enables accordion-style lists where the first row rounds only its
    /// top corners and the last row only its bottom ones:
    ///
    /// ```ignore
    /// container().corner_radii(CornerRadii::top(16.0))     // first row
    /// container().corner_radii(CornerRadii::bottom(16.0))  // last row
    /// ```
    ///
    /// Child clipping, blur regions and rounded hit testing keep using a
    /// uniform radius (the largest of the four).
    pub fn corner_radii<M>(
        mut self,
        radii: impl IntoSignal<crate::renderer::CornerRadii, M>,
    ) -> Self {
        self.corner_radii = Some(radii.into_signal());
        self
    }

    /// Set the corner curvature using CSS K-value system
    pub fn corner_curvature<M>(mut self, curvature: impl IntoSignal<f32, M>) -> Self {
        self.corner_curvature = Some(curvature.into_signal());
        self
    }

    /// Blur what is already behind this container.
    ///
    /// Both backdrops are filtered: what this surface has already drawn, and
    /// what the compositor composites below it where the surface is
    /// translucent. Pair it with a translucent
    /// [`background()`](Self::background) so the result shows through.
    ///
    /// ```ignore
    /// container()
    ///     .corner_radius(16.0)
    ///     .backdrop_blur(32.0)
    ///     .background(Color::rgba(0.1, 0.1, 0.15, 0.6))
    /// ```
    ///
    /// Restrict it with [`BackdropSources`] when only one side should soften.
    /// The compositor side needs `ext-background-effect-v1` — check
    /// [`compositor_effects()`](crate::compositor::compositor_effects) — and
    /// carries no radius of its own; the compositor picks one.
    ///
    /// See [`crate::backdrop`] for why both are filtered rather than one
    /// being chosen.
    /// A radius of `0.0` is "no blur", so a blur can be switched off by the
    /// same signal that switches it on — the shape
    /// [`Text::backdrop_blur`](crate::widgets::Text::backdrop_blur) already has.
    pub fn backdrop_blur<M>(mut self, blur: impl IntoSignal<BackdropBlur, M>) -> Self {
        self.backdrop_blur = Some(blur.into_signal());
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

    /// Set a border: a width and a colour, together.
    ///
    /// Both halves, always — a width with no colour and a colour with no width
    /// are the same thing, which is no border, so there is nothing for a
    /// half-declaration to mean. Each half takes a signal of its own, so
    /// anything that has to change over time already can:
    ///
    /// ```ignore
    /// container().border(1.5, move || if failed.get() { theme.danger } else { theme.line })
    /// ```
    ///
    /// A state layer says it the same way, and replaces the whole border:
    ///
    /// ```ignore
    /// container()
    ///     .border(1.5, theme.line)
    ///     .when_focused(|s| s.border(1.5, theme.accent))
    /// ```
    ///
    /// A width repeated across layers is a constant in your own code — there is
    /// deliberately no way to leave half a border unsaid.
    pub fn border<M1, M2>(
        mut self,
        width: impl IntoSignal<f32, M1>,
        color: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.border_width = Some(width.into_signal());
        self.border_color = Some(color.into_signal());
        self
    }

    /// Set a linear gradient background. Replaces the solid fill.
    pub fn gradient<M>(mut self, gradient: impl IntoSignal<LinearGradient, M>) -> Self {
        self.gradient = Some(gradient.into_signal());
        self
    }

    /// Convenience: horizontal gradient.
    pub fn gradient_horizontal<M1, M2>(
        self,
        start: impl IntoSignal<Color, M1>,
        end: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.gradient_between(start, end, GradientDirection::Horizontal)
    }

    /// Convenience: vertical gradient.
    pub fn gradient_vertical<M1, M2>(
        self,
        start: impl IntoSignal<Color, M1>,
        end: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.gradient_between(start, end, GradientDirection::Vertical)
    }

    /// Convenience: diagonal gradient, top-left to bottom-right.
    pub fn gradient_diagonal<M1, M2>(
        self,
        start: impl IntoSignal<Color, M1>,
        end: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.gradient_between(start, end, GradientDirection::Diagonal)
    }

    /// The three shorthands above, which differ only in the direction.
    ///
    /// Two constant endpoints — much the commonest case — build one constant
    /// gradient rather than a derived recomputing a value that cannot change.
    fn gradient_between<M1, M2>(
        self,
        start: impl IntoSignal<Color, M1>,
        end: impl IntoSignal<Color, M2>,
        direction: GradientDirection,
    ) -> Self {
        let (start, end) = (start.into_signal(), end.into_signal());
        match (start.constant(), end.constant()) {
            (Some(start), Some(end)) => self.gradient(LinearGradient::new(start, end, direction)),
            _ => self.gradient(move || LinearGradient::new(start.get(), end.get(), direction)),
        }
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

    /// Set the overflow behaviour for content that exceeds the container bounds.
    pub fn overflow<M>(mut self, overflow: impl IntoSignal<Overflow, M>) -> Self {
        let signal = overflow.into_signal();
        // Seed the cache the event path reads, so a container declared clipped
        // is clipped for the first event too, without depending on a layout
        // having run first.
        self.overflow_resolved.set(signal.get_untracked());
        self.overflow = Some(signal);
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

    /// Called on a left-button press inside the bounds.
    ///
    /// Takes a closure, a [`Callback`](crate::reactive::Callback), or an
    /// `Option<Callback>` — the last being what a `#[component]` callback prop
    /// holds, so a component forwards its own prop with no ceremony:
    ///
    /// ```ignore
    /// #[component]
    /// fn button(#[prop(callback)] on_click: ()) -> impl Widget {
    ///     container().on_click(on_click)
    /// }
    /// ```
    ///
    /// A `None` prop leaves the container without a click handler, but still a
    /// pointer target if anything else made it one.
    pub fn on_click<M>(mut self, callback: impl IntoClickHandler<M>) -> Self {
        let handler = callback.into_click_handler();
        if handler.is_some() || self.interaction.is_some() {
            self.interact_mut().on_click = handler;
        }
        self
    }

    /// Called on a right-button press inside the bounds.
    pub fn on_right_click<F: Fn() + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_right_click = Some(Rc::new(callback));
        self
    }

    /// Called on a middle-button press inside the bounds.
    pub fn on_middle_click<F: Fn() + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_middle_click = Some(Rc::new(callback));
        self
    }

    /// Called for every key press this container's surface receives.
    ///
    /// Delivered while the surface has keyboard focus — a layer surface with
    /// [`KeyboardInteractivity`](crate::platform::KeyboardInteractivity) set,
    /// or a popup holding a grab.
    pub fn on_key_down<F: Fn(Key, Modifiers) + 'static>(mut self, callback: F) -> Self {
        self.interact_mut().on_key_down = Some(Rc::new(callback));
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
                // Builder time, before the widget exists: a snapshot by nature
                let len = w.get_untracked();
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
                let len = h.get_untracked();
                len.exact.or(len.min).unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        self.anims_mut().height = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for background color changes
    pub fn animate_background(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.background.get_or_untracked(Color::TRANSPARENT);
        self.anims_mut().background = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for corner radius changes
    pub fn animate_corner_radius(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.corner_radius.get_or_untracked(0.0);
        self.anims_mut().corner_radius = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for padding changes
    pub fn animate_padding(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.padding.get_or_untracked(Padding::default());
        self.anims_mut().padding = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border width changes.
    ///
    /// Width and colour keep separate `animate_*` declarations even though the
    /// border is *declared* as a pair: these name an animatable channel, not a
    /// way to state a value, and the two channels are different types with
    /// their own curves. `examples/animation_example.rs` springs the width while
    /// easing the colour, which one call could not express.
    pub fn animate_border_width(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.border_width.get_or_untracked(0.0);
        self.anims_mut().border_width = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for border colour changes. See
    /// [`animate_border_width`](Self::animate_border_width) for why the two
    /// halves have their own declarations.
    pub fn animate_border_color(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.border_color.get_or_untracked(Color::TRANSPARENT);
        self.anims_mut().border_color = Some(AnimationState::new(initial, transition));
        self
    }

    /// Animate the text colour of this container and its descendants.
    ///
    /// ```ignore
    /// container()
    ///     .text_color(theme.text_weak)
    ///     .when_hovered(|s| s.text_color(theme.text))
    ///     .animate_text_color(Transition::new(200.0, TimingFunction::EaseOut))
    /// ```
    ///
    /// A transition declared on two levels — an animated colour whose own base
    /// is inherited from an ancestor that is itself animating — currently
    /// retargets every frame, giving a damped chase rather than a transition
    /// with its own curve. CSS starts the inner one once, towards the outer's
    /// *final* value; that is the rule to adopt if it ever comes up. The chase
    /// converges either way.
    pub fn animate_text_color(mut self, transition: impl Into<TransitionConfig>) -> Self {
        self.anims_mut().text_color = Some(AnimationState::new(Color::WHITE, transition.into()));
        self.animated_text = Some(create_signal(None));
        self
    }

    /// Animate elevation changes — the Material lift on hover, in motion
    /// rather than as a jump.
    pub fn animate_elevation(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.elevation.get_or_untracked(0.0);
        self.anims_mut().elevation = Some(AnimationState::new(initial, transition));
        self
    }

    /// Enable animation for transform changes
    pub fn animate_transform(mut self, transition: impl Into<TransitionConfig>) -> Self {
        let initial = self.transform.get_or_untracked(Transform::IDENTITY);
        // Whatever sequence is already here comes along: a fresh state would
        // throw it away, and the order the two builders were written in would
        // decide whether it exists — silently, with the trigger still firing.
        let previous = self.anims_mut().transform.take();
        let mut anim = AnimationState::new(initial, transition);
        anim.adopt_timeline_of(previous);
        self.anims_mut().transform = Some(anim);
        self
    }

    /// Play a sequence of transforms whenever `plays` changes.
    ///
    /// The other animations here move *towards* a value; this one has none. It
    /// plays, and while it plays it replaces whatever `transform` declares,
    /// handing the property back when it ends — the rule CSS gives an
    /// animation over a normal declaration.
    ///
    /// ```ignore
    /// container()
    ///     .keyframes_transform(
    ///         Keyframes::new(320.0)
    ///             .at(0.0, Transform::IDENTITY)
    ///             .at(0.2, Transform::rotate_degrees(1.5))
    ///             .at(0.5, Transform::rotate_degrees(-1.0))
    ///             .at(1.0, Transform::IDENTITY),
    ///         rejections,
    ///     )
    /// ```
    ///
    /// The trigger is a count and not a flag on purpose: a second refusal has
    /// to shake as loudly as the first, and a signal that stays equal notifies
    /// nobody. The count it starts at is whatever it holds when the container
    /// is built, so nothing plays on the first frame.
    pub fn keyframes_transform<M>(
        mut self,
        keyframes: crate::animation::Keyframes<Transform>,
        plays: impl IntoSignal<u32, M>,
    ) -> Self {
        let initial = self.transform.get_or_untracked(Transform::IDENTITY);
        let anim = self
            .anims_mut()
            .transform
            // A transition of no duration: the timeline speaks for this
            // property while it plays, and outside it the declared value
            // applies at once, exactly as it would with no animation at all.
            .get_or_insert_with(|| {
                AnimationState::new(
                    initial,
                    crate::animation::Transition::new(
                        0.0,
                        crate::animation::TimingFunction::Linear,
                    ),
                )
            });
        anim.set_timeline(keyframes);
        anim.set_play_trigger(plays.into_signal());
        self
    }

    /// Enable transform animation with an ENTER transition: on first
    /// layout the transform animates from `enter_from` to its effective
    /// value, so the widget appears mid-animation — no signal flip, no
    /// timer. A menu that scales open declares it directly:
    ///
    /// ```ignore
    /// .transform(move || if open.get() { Transform::IDENTITY } else { collapsed })
    /// .animate_transform_from(collapsed, Transition::spring(SpringConfig::SNAPPY))
    /// // `open` can simply start true: the enter plays from `collapsed`
    /// ```
    pub fn animate_transform_from(
        mut self,
        enter_from: Transform,
        transition: impl Into<TransitionConfig>,
    ) -> Self {
        let initial = self.transform.get_or_untracked(Transform::IDENTITY);
        let previous = self.anims_mut().transform.take();
        let mut anim = AnimationState::new(initial, transition).with_enter_from(enter_from);
        anim.adopt_timeline_of(previous);
        self.anims_mut().transform = Some(anim);
        self
    }

    /// Declare this container an interaction unit.
    ///
    /// Everything inside resolves hover, press and focus from here, until a
    /// nested `control` takes over. That is what lets a button's label light
    /// up while the pointer is on the button's padding, and what lets a label
    /// react to the focus of the input beside it.
    ///
    /// Rarely written by hand: anything the pointer can act on is a unit by
    /// necessity, so `on_click`, `on_hover`, `on_scroll`, `scrollable` and a
    /// declared state layer all imply it. Write it where the boundary is real
    /// but nothing else announces it — a form field's label and input, a row
    /// whose highlight belongs to the row.
    ///
    /// # Example
    /// ```ignore
    /// container().control()
    ///     .child(text("Password").when_focused(|s| s.color(theme.accent)))
    ///     .child(text_input(password))
    /// ```
    pub fn control(mut self) -> Self {
        self.declared_control = true;
        // The flags a descendant subscribes to live here, so they have to
        // exist even for a unit that declares no state of its own.
        self.interact_mut();
        self
    }

    /// Whether this container is an interaction unit.
    ///
    /// A pointer target *is* one — it has to know whether it is being pointed
    /// at — so every behaviour implies it rather than merely allowing it. So
    /// does declaring a state layer, which is what keeps every container
    /// resolving its own states exactly as it did before controls existed.
    pub(super) fn is_control(&self) -> bool {
        if self.declared_control || self.scroll_axis != ScrollAxis::None {
            return true;
        }
        self.interaction.as_ref().is_some_and(|ix| {
            ix.has_any_state()
                || ix.on_click.is_some()
                || ix.on_right_click.is_some()
                || ix.on_middle_click.is_some()
                || ix.on_hover.is_some()
                || ix.on_scroll.is_some()
                || ix.on_pointer_move.is_some()
                || ix.on_mouse_down.is_some()
                || ix.on_mouse_up.is_some()
        })
    }
}

/// The same four declarations a leaf has, in the box's own vocabulary.
///
/// One trait for both means `when_hovered` reads the same on a button and on
/// its label — which is the point of the name: it promises nothing about
/// *whose* hover, because the unit that can be hovered is the control you
/// belong to.
impl Stateful for Container {
    type Style = StateStyle;

    fn push_state_style(&mut self, when: StateWhen, style: StateStyle) {
        self.interact_mut().states.push((when, style));
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Container {
    /// Tear down the owner holding the published text derived.
    ///
    /// That derived is the one reactive resource a container creates outside
    /// any user scope — at registration, where the ambient owner is the
    /// surface's and would hold it until the app exits. A container removed by
    /// a dynamic-children update would leak one derived per rebuild.
    ///
    /// Every other signal a container holds was created in the builder chain,
    /// inside the caller's own scope, and is freed with it.
    fn drop(&mut self) {
        self.dispose_text_owner();
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
            //
            // A snapshot on purpose: this pass consumes the target, it does not
            // subscribe to it. The subscriptions for these very properties are
            // established by `seed_animations` at the first layout and refreshed
            // by `resync_animation_targets` at every paint, both under an
            // Animation tracking scope — see anim_bridge.rs. Without saying so,
            // the debug diagnostic reports each of these reads as a value that
            // "will not update", which is the opposite of true and trains the
            // reader to ignore a warning that is usually right.
            let (
                padding_target,
                border_width_target,
                bg_target,
                corner_radius_target,
                elevation_target,
                border_color_target,
                transform_target,
            ) = crate::reactive::diagnostics::snapshot_zone(|| {
                (
                    self.padding.get_or(Padding::default()),
                    self.effective_border_width_target(id),
                    self.effective_background_target(id),
                    self.effective_corner_radius_target(id),
                    self.effective_elevation_target(id),
                    self.effective_border_color_target(id),
                    self.effective_transform_target(id),
                )
            });
            let text_color_target = self
                .anims
                .as_ref()
                .is_some_and(|a| a.text_color.is_some())
                .then(|| {
                    crate::reactive::diagnostics::snapshot_zone(|| {
                        self.effective_text_color_target(id)
                    })
                });
            let animated_text = self.animated_text;
            let anims = self.anims.as_mut().unwrap();

            // Text colour, by hand rather than through `advance_anim!`: every
            // other animated property is consumed by this container's own
            // paint, but this one is drawn by a descendant, so each step has
            // to leave through a signal. The write is what wakes the text.
            if let (Some(anim), Some(target)) = (anims.text_color.as_mut(), text_color_target) {
                anim.animate_to(target);
                if anim.is_animating() {
                    any_animating = true;
                    let required = if anim.advance().is_changed() {
                        crate::jobs::RequiredJob::Paint
                    } else {
                        crate::jobs::RequiredJob::None
                    };
                    request_job(id, JobRequest::Animation(required));
                }
                if let Some(signal) = animated_text {
                    // `None` once settled, so the derived goes back to the
                    // ordinary fold rather than pinning the last frame.
                    let current = anim.is_animating().then(|| anim.displayed());
                    signal.set(current);
                }
            }
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
            advance_anim!(anims, elevation, elevation_target, id, any_animating, paint);
            advance_anim!(
                anims,
                border_color,
                border_color_target,
                id,
                any_animating,
                paint
            );
            // A trigger that has moved starts the sequence, before the frame
            // that will show its first value. The read is a snapshot: the
            // subscription belongs to `resync_animation_targets`, which asks
            // the same question inside its tracking scope.
            if let Some(anim) = anims.transform.as_mut()
                && crate::reactive::diagnostics::snapshot_zone(|| anim.take_play())
            {
                anim.play();
            }
            advance_anim!(anims, transform, transform_target, id, any_animating, paint);
        }

        // Advance ripple animation
        if let Some(ref mut ix) = self.interaction
            && ix.ripple.is_active()
            && let Some(config) = ix.ripple_config()
        {
            let ripple_animating = ix.ripple.advance(&config, std::time::Instant::now());
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

        // Publish the declared text style on the node so descendants find it
        // by walking up. The signals themselves are stable ids, so a value
        // change needs no rewrite — only a rebuilt container does, and that
        // re-registers anyway.
        let published = self.published_text_style(tree, id);
        tree.set_text_style(id, published);
        tree.set_input_style(id, self.input.as_deref().copied());
        tree.set_control(
            id,
            self.is_control()
                .then(|| {
                    self.interaction
                        .as_ref()
                        .map(|ix| Control::new(id, ix.flags))
                })
                .flatten(),
        );

        // Register pending children
        self.children_source.register_pending(tree, id);
    }

    fn layout_hints(&self) -> LayoutHints {
        // The parent asks this while laying out; snapshots are right here,
        // because the child's own layout subscribes to the same signals and a
        // change re-runs it (and bubbles to the parent) anyway.
        if !self.visible.get_or_untracked(true) {
            return LayoutHints::default();
        }
        LayoutHints {
            fill_width: self
                .width
                .as_ref()
                .map(|w| w.get_untracked().fill)
                .unwrap_or(false),
            fill_height: self
                .height
                .as_ref()
                .map(|h| h.get_untracked().fill)
                .unwrap_or(false),
        }
    }

    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size {
        let is_visible = with_signal_tracking(id, JobType::Layout, || self.visible.get_or(true));
        if !is_visible {
            tree.set_relayout_boundary(id, true);
            let size = Size::zero();
            tree.cache_layout(id, constraints, size);
            tree.clear_needs_layout(id);
            return size;
        }

        tree.set_relayout_boundary(id, self.is_relayout_boundary_for(constraints));
        self.ensure_scrollbar_containers(tree, id);

        // Nothing to redo when the constraints are the same and no tracked
        // signal (or animation) marked us dirty.
        let constraints_changed = tree.cached_constraints(id) != Some(constraints);
        let reactive_changed = tree.needs_layout(id);
        if !constraints_changed && !reactive_changed {
            crate::render_stats::record_layout_skipped();
            return tree.cached_size(id).unwrap_or_default();
        }
        crate::render_stats::record_layout_executed_with_reasons(
            crate::render_stats::LayoutReasons {
                constraints_changed,
                reactive_changed,
            },
        );
        tree.clear_needs_layout(id);

        let lengths = self.read_box_lengths(id, constraints);
        let child = self.child_layout(&lengths, constraints);
        let padding = lengths.padding;

        let children = self.children_source.reconcile_and_get(tree);

        // A layout reads its own reactive properties (Flex's spacing and
        // alignment, whatever a user-written Layout declares) while running.
        // Those reads belong to this container just like padding and width do
        // — without the scope they register no subscriber, and the property
        // silently stops being reactive. Nesting is safe: every container
        // opens its own scope, so a child's reads attribute to the child.
        let layout_impl = &mut self.layout;
        let content_size = if !children.is_empty() {
            with_signal_tracking(id, JobType::Layout, || {
                layout_impl.layout(tree, children, child.constraints, child.origin)
            })
        } else {
            Size::zero()
        };

        // A box sits on the baseline of its content, as CSS has it: without
        // this a styled label reports nothing, and a parent aligning on the
        // baseline falls back to its bottom edge — which is how the idiomatic
        // `container().child(text(..))` would silently miss the alignment it
        // asked for.
        // Child origins are stored relative to their parent, so the child's
        // own offset is all that has to be added.
        if let Some((child_id, child_baseline)) = children
            .iter()
            .find_map(|&cid| tree.baseline(cid).map(|b| (cid, b)))
            && let Some((_, child_y)) = tree.get_origin(child_id)
        {
            tree.set_baseline(id, child_y + child_baseline);
        }

        if self.scroll_axis != ScrollAxis::None {
            let sd = self.scroll_mut();
            sd.scroll_state.content_width = content_size.width + padding.horizontal_total();
            sd.scroll_state.content_height = content_size.height + padding.vertical_total();
            sd.scroll_state.viewport_width = child.viewport.width;
            sd.scroll_state.viewport_height = child.viewport.height;
            sd.scroll_state.clamp_offsets();
        }

        self.update_size_targets(tree, id, &lengths, content_size);
        self.seed_animations(id);

        let size = self.resolve_size(&lengths, constraints, content_size);

        // Layout scrollbar containers after size is determined
        // Note: cache_layout is called at the end which stores size in Tree
        self.layout_scrollbar_containers(tree, id, size);

        // Cache constraints and size for partial layout
        tree.cache_layout(id, constraints, size);

        // An elevation shadow falls outside the box that casts it, so the damage
        // this container reports has to reach past its own bounds.
        //
        // The *largest* elevation it can reach, not the one showing: elevation
        // animates paint-only, so a hover that lifts a card never re-runs this
        // layout, and a reach sized to the resting value would leave the shadow
        // ring outside every damage rect — invisible on the way up, and left
        // behind on the way down. Read under layout tracking, so a declared
        // elevation changing does re-run it.
        let reach = with_signal_tracking(id, JobType::Layout, || self.max_elevation());
        tree.set_paint_overflow(id, style::elevation_to_shadow(reach).extent());

        // Register widget ref so update_widget_refs() can refresh bounds
        if let Some(ref wr) = self.widget_ref {
            register_widget_ref(id, *wr);
        }

        size
    }

    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        if !self.visible.get_or(true) {
            return EventResponse::Ignored;
        }

        let hit = HitContext {
            bounds: tree.get_bounds(id).unwrap_or_default(),
            corner_radius: self.animated_corner_radius(id),
            transform: self.animated_transform(id),
            transform_origin: self.transform_origin.get_or(TransformOrigin::CENTER),
        };

        // Undo our own transform before hit testing against the laid-out bounds
        let local_event: Cow<'_, Event> = match event.coords() {
            Some((x, y)) if !hit.transform.is_identity() => {
                let (local_x, local_y) =
                    untransform_point(&hit.transform, hit.transform_origin, hit.bounds, x, y);
                Cow::Owned(event.with_coords(local_x, local_y))
            }
            _ => Cow::Borrowed(event),
        };

        if let Some(response) = self.handle_scrollbar_event(tree, id, hit.bounds, &local_event) {
            return response;
        }

        self.track_pointer(id, &hit, &local_event);

        // Children are positioned relative to our origin (and to the scroll
        // offset, when we scroll), so their events have to be too.
        let child_event: Cow<'_, Event> = match local_event.coords() {
            Some((x, y)) => {
                let (mut cx, mut cy) = (x - hit.bounds.x, y - hit.bounds.y);
                if self.scroll_axis != ScrollAxis::None {
                    let sd = self.scroll();
                    cx += sd.scroll_state.offset_x;
                    cy += sd.scroll_state.offset_y;
                }
                Cow::Owned(local_event.with_coords(cx, cy))
            }
            None => local_event.clone(),
        };

        // Children clipped away by hidden overflow or scrolling are invisible,
        // and an invisible child must not steal a click from a sibling drawn
        // below it (a collapsed submenu used to do exactly that).
        let clips_children = self.overflow_resolved.get() == Overflow::Hidden
            || self.scroll_axis != ScrollAxis::None;
        let skip_child_dispatch = clips_children
            && local_event
                .coords()
                .is_some_and(|(x, y)| !hit.bounds.contains(x, y));

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

        self.handle_own_event(id, &hit, event, &local_event)
    }

    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext) {
        let is_visible = with_signal_tracking(id, JobType::Paint, || self.visible.get_or(true));
        if !is_visible {
            // An invisible container draws nothing, and must not go on asking
            // the compositor to blur the desktop behind where it would be. The
            // withdrawal has to happen here rather than at the decision below,
            // which this return never reaches.
            if self.backdrop_blur.is_some() {
                crate::blur::unregister_blur(id);
            }
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
            per_corner_radii,
            gradient,
            backdrop_blur,
            overflow,
        ) = with_signal_tracking(id, JobType::Paint, || {
            (
                self.animated_background(id),
                self.animated_corner_radius(id),
                self.corner_curvature.get_or(1.0),
                self.animated_elevation(id),
                self.animated_transform(id),
                self.transform_origin.get_or(TransformOrigin::CENTER),
                self.animated_border_width(id),
                self.animated_border_color(id),
                self.corner_radii.as_ref().map(|s| s.get()),
                self.gradient.as_ref().map(|g| g.get()),
                self.backdrop_blur.as_ref().map(|b| b.get()),
                self.overflow.get_or(Overflow::Visible),
            )
        });
        self.overflow_resolved.set(overflow);

        self.resync_animation_targets(id);

        // Per-corner radii override the uniform (animated) radius for
        // drawing. Clip, blur region and rounded hit testing stay uniform,
        // approximated by the largest corner.
        let corner_radii = per_corner_radii
            .unwrap_or_else(|| crate::renderer::CornerRadii::uniform(corner_radius));
        let corner_radius = corner_radius.max(corner_radii.max());

        // Publish (or withdraw) the compositor blur region: bounds are read
        // fresh from the tree at frame sync, so only the (possibly animated)
        // radius is recorded here.
        //
        // The zero-radius case has to withdraw rather than simply not register.
        // `ext-background-effect-v1` carries no radius of its own, so nothing
        // downstream can tell that this container asked for zero — and the
        // registry only prunes widgets that left the tree, which a container
        // whose blur signal reached zero has not. Without the withdrawal a
        // blur switched on once stayed on for the life of the surface.
        let wants_compositor_blur = backdrop_blur.is_some_and(|blur| {
            blur.sources.contains(BackdropSources::COMPOSITOR) && blur.radius > 0.0
        });
        if wants_compositor_blur {
            crate::blur::register_blur(id, corner_radius);
        } else if self.backdrop_blur.is_some() {
            // Only a container that could have registered needs to withdraw.
            // Every other one would be a borrow and a miss on a mostly empty
            // map, per container, per frame, on the paint path.
            crate::blur::unregister_blur(id);
        }

        // LOCAL bounds: the origin is this container, the parent already
        // positioned the node.
        let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        ctx.set_bounds(local_bounds);

        // Compose our own transform on top of the position the parent set.
        if !user_transform.is_identity() {
            ctx.apply_transform_with_origin(user_transform, transform_origin);
        }

        // Before the decoration: the container paints over its own blurred
        // backdrop, and the effect must read a target that does not yet
        // include this container.
        if let Some(blur) = backdrop_blur
            && blur.sources.contains(BackdropSources::SURFACE)
            && blur.radius > 0.0
        {
            ctx.draw_backdrop_blur(local_bounds, blur.radius, corner_radii, corner_curvature);
        }

        self.paint_decoration(
            ctx,
            local_bounds,
            &Decoration {
                background,
                gradient,
                corner_radii,
                corner_curvature,
                elevation: elevation_level,
                border_width,
                border_color,
            },
        );

        // Determine if we need to clip children
        let is_scrollable = self.scroll_axis != ScrollAxis::None;

        // Set clip region for scrollable or overflow:hidden containers
        // This clips all children to the container bounds
        if is_scrollable || overflow == Overflow::Hidden {
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

        // Draw children - each gets its own node with position transform.
        //
        // For scrollable containers with a single-axis layout, use binary search
        // to find the visible range (O(log n)) instead of iterating all children (O(n)).
        let all_children = self.children_source.get();
        let visible_children: &[WidgetId] = if is_scrollable {
            let sd = self.scroll();
            match self.scroll_axis {
                ScrollAxis::Vertical => {
                    let vp_top = sd.scroll_state.offset_y;
                    let vp_bottom = vp_top + local_bounds.height;
                    let first = all_children.partition_point(|&cid| {
                        tree.get_bounds(cid)
                            .is_some_and(|b| b.y + b.height <= vp_top)
                    });
                    let last = all_children.partition_point(|&cid| {
                        tree.get_bounds(cid).is_some_and(|b| b.y < vp_bottom)
                    });
                    let start = first.saturating_sub(1);
                    let end = (last + 1).min(all_children.len());
                    crate::render_stats::record_scroll_paint_range(
                        all_children.len() as u64,
                        (end - start) as u64,
                    );
                    &all_children[start..end]
                }
                ScrollAxis::Horizontal => {
                    let vp_left = sd.scroll_state.offset_x;
                    let vp_right = vp_left + local_bounds.width;
                    let first = all_children.partition_point(|&cid| {
                        tree.get_bounds(cid)
                            .is_some_and(|b| b.x + b.width <= vp_left)
                    });
                    let last = all_children.partition_point(|&cid| {
                        tree.get_bounds(cid).is_some_and(|b| b.x < vp_right)
                    });
                    let start = first.saturating_sub(1);
                    let end = (last + 1).min(all_children.len());
                    crate::render_stats::record_scroll_paint_range(
                        all_children.len() as u64,
                        (end - start) as u64,
                    );
                    &all_children[start..end]
                }
                _ => all_children, // Both/None: fall back to full iteration
            }
        } else {
            all_children
        };

        let scroll_offset = if is_scrollable {
            let sd = self.scroll();
            (sd.scroll_state.offset_x, sd.scroll_state.offset_y)
        } else {
            (0.0, 0.0)
        };
        paint_children(
            tree,
            ctx,
            visible_children,
            &ChildPaintOptions {
                scroll_offset,
                cull_rect: effective_cull_rect,
                // A scroller's cull rect moves under its content, so a
                // partially visible child has to repaint for its own children
                // to be culled against the current rect.
                cache_requires_full_visibility: is_scrollable,
            },
        );

        // Draw scrollbar containers
        if is_scrollable {
            self.paint_scrollbar_containers(tree, id, ctx);
        }

        // Ripples, oldest first. The state holds how far along each one is;
        // the geometry it resolves against is the container's.
        if let Some(ref ix) = self.interaction
            && let Some(ripple_config) = ix.ripple_config()
            && ix.ripple.iter().any(|r| r.opacity() > 0.0)
        {
            // Clips the ripples without affecting children.
            ctx.set_overlay_clip(local_bounds, corner_radius, corner_curvature);

            for ripple in ix.ripple.iter() {
                let opacity = ripple.opacity();
                if opacity <= 0.0 {
                    continue;
                }
                let (center_x, center_y) = ripple.center(bounds.width, bounds.height);
                let radius = ripple.radius(bounds.width, bounds.height);

                let ripple_color = Color::rgba(
                    ripple_config.color.r,
                    ripple_config.color.g,
                    ripple_config.color.b,
                    ripple_config.color.a * opacity,
                );

                ctx.draw_overlay_circle(center_x, center_y, radius, ripple_color);
            }
        }
    }
}

pub fn container() -> Container {
    Container::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{TimingFunction, Transition};
    use crate::jobs::{self, Job};
    use crate::layout::Constraints;
    use crate::reactive::create_signal;
    use crate::renderer::PaintContext;

    /// Regression test for the missed-write race that left popup menus
    /// permanently collapsed: the Animation subscription for an animated
    /// prop is only registered during paint, but the animation target is
    /// initialized (set_immediate) during the first layout — possibly a
    /// popup measure pass long before a heavy tree finishes its first
    /// paint. A signal write landing in that window notified nobody and
    /// the animation sat on the stale target forever. The first paint must
    /// detect the drift and request an Animation job so
    /// advance_animations adopts the missed target.
    #[test]
    fn write_between_first_layout_and_first_paint_starts_animation() {
        let open = create_signal(false);
        let widget = container()
            .transform(move || {
                if open.get() {
                    Transform::IDENTITY
                } else {
                    Transform::scale_xy(1.0, 0.0)
                }
            })
            .animate_transform(Transition::new(200, TimingFunction::EaseOut));

        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));

        // First layout: initializes the animation to the collapsed state
        // via set_immediate. No Animation subscription exists yet.
        tree.with_widget_mut(id, |w, id, tree| {
            w.layout(tree, id, Constraints::new(0.0, 0.0, 100.0, 100.0));
        });

        // The missed write: no subscriber is registered, so this notifies
        // nobody and pushes no job for the widget.
        open.set(true);

        // Discard anything queued so far so the assertion below only sees
        // jobs produced by the paint.
        let roots: rustc_hash::FxHashSet<WidgetId> = [id].into_iter().collect();
        jobs::distribute_jobs(&tree, &roots);
        jobs::recycle_job_buffer(jobs::drain_surface_jobs(id));
        jobs::recycle_job_buffer(jobs::drain_orphan_jobs());

        // First paint: registers the subscription AND must notice the
        // animation target no longer matches the signal value.
        let mut root_node = crate::renderer::RenderNode::new(id.as_u64());
        tree.with_widget_mut(id, |w, id, tree| {
            let mut ctx = PaintContext::new(&mut root_node);
            w.paint(tree, id, &mut ctx);
        });

        jobs::distribute_jobs(&tree, &roots);
        let drained = jobs::drain_surface_jobs(id);
        assert!(
            drained.contains(&Job {
                widget_id: id,
                job_type: JobType::Animation,
            }),
            "paint must request an Animation job for the missed target change, got {drained:?}"
        );
    }

    /// An enter transition starts animating at first layout — the widget
    /// appears mid-animation, no signal flip or timer needed.
    #[test]
    fn enter_transition_starts_animating_at_first_layout() {
        use crate::animation::TimingFunction;

        let collapsed = Transform::scale_xy(1.0, 0.0);
        let entering = container()
            .transform(Transform::IDENTITY)
            .animate_transform_from(collapsed, Transition::new(200, TimingFunction::EaseOut));
        let plain = container()
            .transform(Transform::IDENTITY)
            .animate_transform(Transition::new(200, TimingFunction::EaseOut));

        let mut tree = Tree::new();
        let entering_id = tree.register(Box::new(entering));
        let plain_id = tree.register(Box::new(plain));
        let c = Constraints::new(0.0, 0.0, 100.0, 100.0);
        for id in [entering_id, plain_id] {
            tree.with_widget_mut(id, |w, id, tree| {
                w.layout(tree, id, c);
            });
        }

        let entering_animating = tree
            .with_widget_mut(entering_id, |w, id, tree| w.advance_animations(tree, id))
            .unwrap();
        let plain_animating = tree
            .with_widget_mut(plain_id, |w, id, tree| w.advance_animations(tree, id))
            .unwrap();
        assert!(
            entering_animating,
            "enter transition must be in flight right after the first layout"
        );
        assert!(
            !plain_animating,
            "a plain animation initializes settled at its target"
        );
    }

    /// Content-sized surfaces measure under the measure-final flag: layout
    /// must report animation TARGETS, not in-flight values, so a growth
    /// animation resizes the surface once instead of once per frame.
    #[test]
    fn measure_final_reads_animation_targets() {
        let height_sig = create_signal(100.0_f32);
        let widget = container()
            .width(50.0)
            .height(move || height_sig.get())
            .animate_height(Transition::new(200, TimingFunction::EaseOut));

        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));
        // The real flow measures under DIFFERENT constraints than the
        // render layout (loose caps vs the exact surface size), which is
        // also what defeats the layout cache here
        let render = Constraints::new(0.0, 0.0, 400.0, 400.0);
        let measure = Constraints::new(0.0, 0.0, 500.0, 500.0);

        // First layout initializes the animation at 100
        tree.with_widget_mut(id, |w, id, tree| {
            w.layout(tree, id, render);
        });

        // Retarget to 180: the animation starts from ~100
        height_sig.set(180.0);
        let mid = tree
            .with_widget_mut(id, |w, id, tree| w.layout(tree, id, render))
            .unwrap();
        assert!(
            mid.height < 180.0,
            "mid-animation layout should not have reached the target yet (got {})",
            mid.height
        );

        let fin = super::animations::with_measure_final(|| {
            tree.with_widget_mut(id, |w, id, tree| w.layout(tree, id, measure))
        })
        .unwrap();
        assert_eq!(
            fin.height, 180.0,
            "measure-final must report the animation target"
        );
    }

    /// The Animation subscription must exist from the FIRST LAYOUT, not the
    /// first paint: creation and first layout run in one synchronous block,
    /// so registering there leaves no window for a write to go unnoticed.
    /// A write right after layout — before any paint — must already
    /// schedule an Animation job. Padding is the prop that was never
    /// covered by the paint-time pass alone.
    #[test]
    fn padding_write_after_first_layout_schedules_animation() {
        let pad = create_signal(4.0_f32);
        let widget = container()
            .padding(move || pad.get())
            .animate_padding(Transition::new(200, TimingFunction::EaseOut));

        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));

        tree.with_widget_mut(id, |w, id, tree| {
            w.layout(tree, id, Constraints::new(0.0, 0.0, 100.0, 100.0));
        });

        // Discard jobs produced so far; only the write below matters.
        let roots: rustc_hash::FxHashSet<WidgetId> = [id].into_iter().collect();
        jobs::distribute_jobs(&tree, &roots);
        jobs::recycle_job_buffer(jobs::drain_surface_jobs(id));
        jobs::recycle_job_buffer(jobs::drain_orphan_jobs());

        pad.set(12.0);

        jobs::distribute_jobs(&tree, &roots);
        let drained = jobs::drain_surface_jobs(id);
        assert!(
            drained.contains(&Job {
                widget_id: id,
                job_type: JobType::Animation,
            }),
            "a write after first layout must schedule an Animation job \
             without waiting for a paint, got {drained:?}"
        );
    }

    /// A layout's own reactive properties must invalidate the container that
    /// runs it. They are read inside `Layout::layout`, which the container
    /// used to call outside any tracking scope: the read registered no
    /// subscriber, so `Flex::spacing(signal)` — and every property of every
    /// user-written `Layout` — silently stopped being reactive.
    #[test]
    fn a_layout_property_invalidates_its_container() {
        let spacing = create_signal(4.0f32);
        let widget = container()
            .layout(crate::layout::Flex::row().spacing(move || spacing.get()))
            .child(container().width(10.0).height(10.0))
            .child(container().width(10.0).height(10.0));

        let mut tree = Tree::new();
        let id = tree.register(Box::new(widget));
        tree.with_widget_mut(id, |w, id, tree| w.register_children(tree, id));
        let size = tree
            .with_widget_mut(id, |w, id, tree| {
                w.layout(tree, id, Constraints::new(0.0, 0.0, 500.0, 500.0))
            })
            .unwrap();
        assert_eq!(size.width, 24.0, "10 + spacing 4 + 10");

        let roots: rustc_hash::FxHashSet<WidgetId> = [id].into_iter().collect();
        jobs::distribute_jobs(&tree, &roots);
        jobs::recycle_job_buffer(jobs::drain_surface_jobs(id));
        jobs::recycle_job_buffer(jobs::drain_orphan_jobs());

        spacing.set(20.0);

        jobs::distribute_jobs(&tree, &roots);
        let drained = jobs::drain_surface_jobs(id);
        assert!(
            drained.contains(&Job {
                widget_id: id,
                job_type: JobType::Layout,
            }),
            "writing a layout's reactive property must queue a Layout job, got {drained:?}"
        );
    }
}
