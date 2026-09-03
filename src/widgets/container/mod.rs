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

use animations::instant_transition;
pub(crate) use animations::with_measure_final;
pub use animations::{AdvanceResult, AnimationState, get_animated_value};
use interaction::{HitContext, untransform_point};
pub use ripple::{MAX_LIVE_RIPPLES, Ripple, RippleState};
use style::Decoration;

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

use crate::advance_anim;
use crate::animation::{Animatable, IntoAnimated, Motion};
use crate::backdrop::BackdropBlur;
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job};
use crate::layout::{Axis, Constraints, Flex, Layout, Length, Size};
use crate::pivot::Pivot;
use crate::reactive::{
    IntoSignal, OptionSignalExt, RwSignal, Signal, create_signal, focus_path, with_signal_tracking,
};
use crate::renderer::{GradientDir, PaintContext, Shadow};
use crate::transform::{Scale, Transform, Translate};
use crate::tree::{Tree, WidgetId};
use crate::widget_ref::{WidgetRef, register_widget_ref};

use super::children::ChildrenSource;
use super::control::Control;
use super::into_child::{IntoChild, IntoChildren};
use super::paint_children::{ChildPaintOptions, paint_children};
use super::scroll::{Scroll, ScrollAxis, ScrollState, ScrollbarMetrics, ScrollbarVisibility};
use super::state_layer::{
    Moves, RippleConfig, StateStyle, StateWhen, Stateful, resolve_background,
};
use super::widget::{
    Color, Event, EventResponse, Key, LayoutHints, Modifiers, MouseButton, Padding, Point, Rect,
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

    /// Top-left to bottom-right.
    pub fn diagonal(start: Color, end: Color) -> Self {
        Self::new(start, end, GradientDirection::Diagonal)
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

/// The three transform components and the point they act about. Boxed and
/// absent by default, like `anims` and `interaction`.
///
/// `Option<Signal<T>>` is 12 bytes — `Signal` is two `u32` plus a fieldless
/// `SignalKind` whose niche the `Option` reuses — so four of them is 48 on
/// every container in every tree, and the overwhelming majority declare none.
/// Behind a pointer, `Container` measures 312 bytes against main's 328, which
/// spent 24 here on the two fields this replaces.
#[derive(Default)]
pub(super) struct TransformProps {
    pub(super) translate: Option<Signal<Translate>>,
    pub(super) rotate: Option<Signal<f32>>,
    pub(super) scale: Option<Signal<Scale>>,
    pub(super) pivot: Option<Signal<Pivot>>,
}

/// Boxed animation states. Only allocated when a declared value arrives
/// carrying a motion, saving ~400 bytes per non-animated Container.
#[derive(Default)]
pub(super) struct ContainerAnims {
    pub(super) width: Option<AnimationState<f32>>,
    pub(super) height: Option<AnimationState<f32>>,
    pub(super) background: Option<AnimationState<Color>>,
    pub(super) corners: Option<AnimationState<crate::widgets::Corners>>,
    pub(super) elevation: Option<AnimationState<f32>>,
    pub(super) padding: Option<AnimationState<Padding>>,
    pub(super) border_width: Option<AnimationState<f32>>,
    pub(super) border_color: Option<AnimationState<Color>>,
    pub(super) translate: Option<AnimationState<Translate>>,
    pub(super) rotate: Option<AnimationState<f32>>,
    pub(super) scale: Option<AnimationState<Scale>>,
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
    /// Which of `translate`, `rotate` and `scale` the declared layers override
    /// between them.
    ///
    /// Decided when the layer is pushed, because that is when it is knowable —
    /// a layer either names the property or it does not, and no signal is
    /// involved. Kept rather than rescanned because it gates the identity fast
    /// path in `animated_transform`, which runs on every paint and every
    /// coalesced pointer move: a button with hover, focus and pressed layers
    /// would otherwise walk three `StateStyle`s to be told nothing turns.
    ///
    /// Three answers rather than one, because that fast path is per component:
    /// `when_pressed(|s| s.scale(0.98))` is the commonest state layer there is,
    /// and a single bit would have made it pay for a translate and a rotate
    /// nothing declares.
    pub(super) declares_transform: Moves,
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
            declares_transform: Moves::default(),
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
            .and_then(|(_, s)| s.ripple)
    }

    /// Whether the layer is active right now. Reading this is what subscribes
    /// the caller to the state, which is why it is not asked for layers that
    /// declare nothing about the property being resolved.
    pub(super) fn is_active(&self, id: WidgetId, when: &StateWhen) -> bool {
        match when {
            StateWhen::Hovered => self.flags.get().contains(InteractionFlags::HOVERED),
            StateWhen::Pressed => self.flags.get().contains(InteractionFlags::PRESSED),
            // The path, rather than a walk of this container's descendants:
            // the same question has to be answerable from a `create_derived`
            // closure, which has no tree, and that is where a container
            // resolves the text colour it publishes below it.
            StateWhen::Focused => focus_path().contains(id),
            StateWhen::When(condition) => condition.get(),
        }
    }
}

/// Scroll state and configuration, boxed to avoid bloating Container.
/// Only allocated when `.scroll()` is called.
pub(super) struct ScrollData {
    /// What the application declared. Holds the signals and the two styling
    /// closures; nothing reads it per frame except to resolve the metrics.
    pub(super) scroll: Scroll,
    /// The declared measurements, resolved under this pass's layout tracking.
    /// The track and handle rects are arithmetic and read this, not signals.
    pub(super) metrics: ScrollbarMetrics,
    /// What `visibility` said when layout last read it.
    pub(super) scrollbar_visibility: ScrollbarVisibility,
    pub(super) scroll_state: ScrollState,
    pub(super) v_scrollbar_track_id: Option<WidgetId>,
    pub(super) v_scrollbar_handle_id: Option<WidgetId>,
    pub(super) v_scrollbar_scale_anim: Option<AnimationState<f32>>,
    pub(super) h_scrollbar_track_id: Option<WidgetId>,
    pub(super) h_scrollbar_handle_id: Option<WidgetId>,
    pub(super) h_scrollbar_scale_anim: Option<AnimationState<f32>>,
}

impl ScrollData {
    /// Built from the declaration and nothing else — there is no default axis
    /// to invent, because a `Scroll` always names one.
    ///
    /// The resolved fields start at their own defaults and are overwritten by
    /// `resolve_scroll` at the top of the first layout, before anything reads
    /// them.
    fn new(scroll: Scroll) -> Self {
        Self {
            scroll,
            metrics: ScrollbarMetrics::default(),
            scrollbar_visibility: ScrollbarVisibility::Always,
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
    pub(super) gradient: Option<Signal<Option<LinearGradient>>>,
    pub(super) corners: Option<Signal<crate::widgets::Corners>>,
    pub(super) border_width: Option<Signal<f32>>,
    pub(super) border_color: Option<Signal<Color>>,
    pub(super) elevation: Option<Signal<f32>>,
    pub(super) width: Option<Signal<Length>>,
    pub(super) height: Option<Signal<Length>>,
    pub(super) overflow: Option<Signal<Overflow>>,
    /// What `overflow` resolved to in the last layout or paint.
    ///
    /// Event dispatch needs the value the *drawn* frame used, not the current
    /// one: it tests against `hit.bounds`, which come from the last layout, and
    /// a clip that disagreed with the box it is clipping would answer for a
    /// point the geometry says nothing about. The tracked reads that do
    /// subscribe write here on their way past.
    ///
    /// It is also cheaper than re-reading a closure-backed signal for every
    /// container under the pointer on every coalesced `MouseMove` — but only
    /// one such read of several on that path, so that is a bonus rather than
    /// the reason.
    pub(super) overflow_resolved: Cell<Overflow>,
    /// The elevation the last layout sized this container's damage rect for.
    ///
    /// Written by `layout` and read by `animated_elevation`, so the shadow that
    /// is drawn and the rect that is repainted are the *same number* rather than
    /// two computations of it. They were two, and they disagreed: paint asked
    /// [`max_elevation`](style) again, which reads the elevation signal at its
    /// current value, so a card animating from 8 down to 0 was clamped to the 0
    /// it had not reached yet and the shadow vanished in one frame while the
    /// animation went on running.
    pub(super) elevation_reach: Cell<f32>,
    pub(super) visible: Option<Signal<bool>>,
    pub(super) transform: Option<Box<TransformProps>>,

    // Interaction state (callbacks, hover/press, state styles, ripple)
    // Only allocated when interaction features are used
    pub(super) interaction: Option<Box<InteractionState>>,

    // Widget ref for reactive bounds tracking
    pub(super) widget_ref: Option<WidgetRef>,

    // Backdrop blur: this surface's own content, the compositor's, or both.
    pub(super) backdrop_blur: Option<Signal<BackdropBlur>>,

    /// Declared with `control()`. A container is an interaction unit for other
    /// reasons too — see `is_control` — so this is only the explicit half.
    pub(super) declared_control: bool,

    // Animation state (boxed to save ~400 bytes per non-animated container)
    pub(super) anims: Option<Box<ContainerAnims>>,

    // Scroll configuration
    pub(super) scroll_axis: ScrollAxis,
    pub(super) scroll_data: Option<Box<ScrollData>>,

    /// The axis the last layout left the children ordered along, if any.
    ///
    /// `paint` narrows the children to a cull rect with a binary search, and a
    /// binary search needs a partitioned slice. Which axis partitions them —
    /// and whether any does — is a property of what the layout *did*, not of
    /// what the container was declared with: a `Flex::row()` under a vertical
    /// scroller orders its children along x, and a layout free to put them
    /// anywhere orders them along neither.
    ///
    /// So it is measured once per layout, where the walk over the children is
    /// already happening and is off the frame path, rather than asked of
    /// `Layout` at every paint.
    pub(super) children_sorted_along: Option<Axis>,
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
            corners: None,
            border_width: None,
            border_color: None,
            elevation: None,
            width: None,
            height: None,
            overflow: None,
            overflow_resolved: Cell::new(Overflow::Visible),
            elevation_reach: Cell::new(0.0),
            visible: None,
            transform: None,
            interaction: None,
            widget_ref: None,
            backdrop_blur: None,
            declared_control: false,
            anims: None,
            scroll_axis: ScrollAxis::None,
            scroll_data: None,
            children_sorted_along: None,
        }
    }

    /// Where a transform can carry what this container draws, published for
    /// whoever is about to decide whether to paint it.
    ///
    /// Read outside any tracking scope on purpose. The transform components
    /// belong to paint, and subscribing layout to them would make
    /// `.translate(move || ..)` reflow on every write. What keeps the answer
    /// current instead is that the same write schedules a Paint job, and
    /// `refresh_paint_bounds` runs from that job before this frame paints.
    fn publish_paint_reach(&self, tree: &mut Tree, id: WidgetId, bounds: Rect) {
        let shadow = style::elevation_to_shadow(self.elevation_reach.get()).extent();
        // Only this container's own half. What its children add is
        // `children_outset`, which the tree keeps for it — gathered upward as
        // they publish, and re-measured whenever this container lays out. That
        // split is what lets this run from a Paint job without walking
        // anything.
        tree.set_own_paint_reach(id, self.max_transform_reach(bounds, shadow));
    }

    /// Resolve the declared scroll measurements for this pass.
    ///
    /// Under layout tracking, and at the top of `layout`, because the gutter
    /// these describe comes out of the content box before anything is measured
    /// — so a width written to a signal has to re-run this container's layout,
    /// not merely repaint it. The geometry below reads the resolved numbers:
    /// a track rect is arithmetic, and arithmetic should not be reading signals
    /// halfway through.
    fn resolve_scroll(&mut self, id: WidgetId) {
        let Some(data) = self.scroll_data.as_mut() else {
            return;
        };
        let scroll = &data.scroll;
        let defaults = ScrollbarMetrics::default();
        let (metrics, visibility) = with_signal_tracking(id, JobType::Layout, || {
            (
                ScrollbarMetrics {
                    width: scroll.width.get_or(defaults.width),
                    hover_width: scroll.hover_width.get_or(defaults.hover_width),
                    margin: scroll.margin.get_or(defaults.margin),
                    min_handle_size: scroll.min_handle_size.get_or(defaults.min_handle_size),
                    reserve_gutter: scroll.reserve_gutter.get_or(defaults.reserve_gutter),
                },
                scroll.visibility.get_or(ScrollbarVisibility::Always),
            )
        });
        data.metrics = metrics;
        data.scrollbar_visibility = visibility;
    }

    /// Get scroll data (panics if not scrollable — only call when scroll_axis != None)
    fn scroll_data(&self) -> &ScrollData {
        self.scroll_data.as_deref().expect("scroll_data not set")
    }

    /// Get mutable scroll data (panics if not scrollable)
    fn scroll_mut(&mut self) -> &mut ScrollData {
        self.scroll_data
            .as_deref_mut()
            .expect("scroll_data not set")
    }

    /// Get or create scroll data
    /// Get or create the transform components.
    fn transform_mut(&mut self) -> &mut TransformProps {
        self.transform.get_or_insert_with(Box::default)
    }

    pub(super) fn translate_signal(&self) -> Option<Signal<Translate>> {
        self.transform.as_deref().and_then(|t| t.translate)
    }

    pub(super) fn rotate_signal(&self) -> Option<Signal<f32>> {
        self.transform.as_deref().and_then(|t| t.rotate)
    }

    pub(super) fn scale_signal(&self) -> Option<Signal<Scale>> {
        self.transform.as_deref().and_then(|t| t.scale)
    }

    pub(super) fn pivot_signal(&self) -> Option<Signal<Pivot>> {
        self.transform.as_deref().and_then(|t| t.pivot)
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

    /// Add a single child: a widget value, or a closure returning one for
    /// reactive content.
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
    /// - `padding(8.0.transition(200.0))` — eased instead of jumped to
    pub fn padding<M>(mut self, value: impl IntoAnimated<Padding, M>) -> Self {
        self.padding = Some(declare(&mut self.anims, value, |a| &mut a.padding));
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
    /// container().background(theme.surface.transition(200.0))  // eased
    /// container().background(theme.surface.timeline(flash(), errors))
    /// ```
    pub fn background<M>(mut self, color: impl IntoAnimated<Color, M>) -> Self {
        self.background = Some(declare(&mut self.anims, color, |a| &mut a.background));
        self
    }

    // -----------------------------------------------------------------------
    // Shape
    // -----------------------------------------------------------------------

    /// The shape of the corners: how far they are rounded, and how.
    ///
    /// A bare size means rounded corners: one value for all four,
    /// `[top, bottom]` for the two pairs, or `[top-left, top-right,
    /// bottom-right, bottom-left]` clockwise as CSS writes it. A constructor
    /// names another shape:
    ///
    /// ```ignore
    /// container().corners(8.0)
    /// container().corners([16.0, 0.0])
    /// container().corners(Corners::squircle(12.0))
    /// container().corners(Corners::bevel([16.0, 0.0]))
    /// ```
    ///
    /// The shape reaches everything: the box, its border and shadow, the blur
    /// behind it, the clip its children are cut to, and the region that
    /// answers a click.
    ///
    /// A shape that eases carries its own timing —
    /// `corners(8.0.transition(250.0))`. A transition that crosses zero
    /// curvature changes family in one frame: below zero a corner is concave,
    /// and the formula that draws it (and the one that answers a click) is a
    /// different one. Within a family it is continuous.
    pub fn corners<M>(mut self, corners: impl IntoAnimated<crate::widgets::Corners, M>) -> Self {
        self.corners = Some(declare(&mut self.anims, corners, |a| &mut a.corners));
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
    ///     .corners(16.0)
    ///     .backdrop_blur(32.0)
    ///     .background(Color::rgba(0.1, 0.1, 0.15, 0.6))
    /// ```
    ///
    /// Restrict it with [`BackdropSources`](crate::backdrop::BackdropSources) when
    /// only one side should soften.
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
    ///
    /// Each half carries its own timing, which is what the pair wanted: the
    /// two channels are different types and a border can spring open while its
    /// colour eases.
    ///
    /// ```ignore
    /// container().border(
    ///     (move || if thick.get() { 14.0 } else { 2.0 }).transition(SpringConfig::BOUNCY),
    ///     Color::rgb(0.40, 0.50, 0.70).transition(300.0),
    /// )
    /// ```
    pub fn border<M1, M2>(
        mut self,
        width: impl IntoAnimated<f32, M1>,
        color: impl IntoAnimated<Color, M2>,
    ) -> Self {
        self.border_width = Some(declare(&mut self.anims, width, |a| &mut a.border_width));
        self.border_color = Some(declare(&mut self.anims, color, |a| &mut a.border_color));
        self
    }

    /// Set a linear gradient background. Replaces the solid fill while it is
    /// there.
    ///
    /// `None` is "no gradient", so the background shows through again — the same
    /// contract a radius of `0.0` gives
    /// [`backdrop_blur`](Self::backdrop_blur), and for the same reason: without
    /// a value meaning *off*, a gradient that only applies sometimes forces the
    /// caller back to branching in Rust and rebuilding the widget.
    ///
    /// ```ignore
    /// container()
    ///     .background(theme.surface)
    ///     .gradient(move || expanded.get().then(|| palette.get().header()))
    /// ```
    pub fn gradient<M>(mut self, gradient: impl IntoSignal<Option<LinearGradient>, M>) -> Self {
        self.gradient = Some(gradient.into_signal());
        self
    }

    /// Set the width of the container.
    ///
    /// `width(w.transition(200.0))` grows and shrinks over time instead of
    /// jumping. A size follows the content it holds as well as the length
    /// declared here, so the animation is over the resolved extent.
    pub fn width<M>(mut self, width: impl IntoAnimated<Length, M>) -> Self {
        self.width = Some(declare_size(&mut self.anims, width, |a| &mut a.width));
        self
    }

    /// Set the height of the container. Eases the same way
    /// [`width`](Self::width) does.
    pub fn height<M>(mut self, height: impl IntoAnimated<Length, M>) -> Self {
        self.height = Some(declare_size(&mut self.anims, height, |a| &mut a.height));
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

    /// Scroll this container, and say what its scrollbar looks like.
    ///
    /// One value rather than three setters. `scrollbar_visibility` and
    /// `scrollbar` used to be separate and were silent no-ops on a container
    /// that never became scrollable — half a configuration that compiled and
    /// did nothing. There is no way to write that now:
    ///
    /// ```compile_fail,E0599
    /// # use guido::prelude::*;
    /// // The parts cannot be declared apart from the thing they configure.
    /// // E0599 pinned on purpose: a bare `compile_fail` passes on any error at
    /// // all, including a typo in the test itself, so it would go on passing
    /// // long after it stopped meaning anything.
    /// container().scrollbar_visibility(ScrollbarVisibility::Hidden);
    /// ```
    ///
    /// ```ignore
    /// container().scroll(Scroll::vertical())
    /// container().scroll(Scroll::both().overlay().handle(|h| h.background(RED)))
    /// ```
    pub fn scroll(mut self, scroll: Scroll) -> Self {
        self.scroll_axis = scroll.axis;
        self.scroll_data = Some(Box::new(ScrollData::new(scroll)));
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

    /// The Material lift, as a shadow. `elevation(8.0.transition(120.0))`
    /// raises and drops it in motion rather than as a jump.
    pub fn elevation<M>(mut self, level: impl IntoAnimated<f32, M>) -> Self {
        self.elevation = Some(declare(&mut self.anims, level, |a| &mut a.elevation));
        self
    }

    /// Displace this container from where it was laid out.
    ///
    /// Paint-only, like the other two: the space the layout gave it does not
    /// move, so nothing around it shifts.
    ///
    /// The three components move independently: each carries its own timing,
    /// so a card can spring into place while its rotation eases. Declaring one
    /// says nothing about the other two.
    ///
    /// ```ignore
    /// container().translate((20.0, 10.0))
    /// container().translate(move || Translate::new(offset.get(), 0.0))
    /// container().translate(target.transition(SpringConfig::SNAPPY))
    /// container().translate(Translate::NONE.timeline(nod(), refusals))
    /// ```
    pub fn translate<M>(mut self, t: impl IntoAnimated<Translate, M>) -> Self {
        let signal = declare(&mut self.anims, t, |a| &mut a.translate);
        self.transform_mut().translate = Some(signal);
        self
    }

    /// Turn this container, in degrees, clockwise, about its [`pivot`](Self::pivot).
    ///
    /// The angle is a number and is kept as one — 360 is a full turn, not
    /// zero, and 720 is two. Nothing normalises it, so what
    /// [`rotate`](Self::rotate) is given is what an animation interpolates and
    /// what a read gives back.
    ///
    /// ```ignore
    /// container().rotate(45.0)
    /// container().rotate(move || heading.get())
    /// container().rotate(0.0.timeline(shake(), rejections))
    /// ```
    ///
    /// An eased angle is interpolated as the number it is, so a turn to 360°
    /// is a full revolution. Nothing takes a shorter way round on the
    /// container's behalf: an angle that arrives already wrapped — from
    /// `atan2`, say — wraps the animation with it, and unwrapping it is the
    /// caller's to do because only the caller knows which way was meant.
    ///
    /// A declared `.reverse()` follows the *number*, not the turn, because the
    /// angle is an `f32` and `f32::is_reverse` means decreasing. So a tilt from
    /// `0.0` to `-8.0` takes the reverse curve going out and the forward one
    /// coming home. Where that matters, declare the rest angle as the larger
    /// number — `8.0` out and `0.0` home — or leave the reverse undeclared and
    /// use one curve both ways.
    pub fn rotate<M>(mut self, degrees: impl IntoAnimated<f32, M>) -> Self {
        let signal = declare(&mut self.anims, degrees, |a| &mut a.rotate);
        self.transform_mut().rotate = Some(signal);
        self
    }

    /// Resize this container about its [`pivot`](Self::pivot), without
    /// re-running layout.
    ///
    /// A bare factor scales both axes; a pair scales them apart.
    ///
    /// ```ignore
    /// container().scale(1.5)
    /// container().scale((2.0, 0.5))
    /// container().scale(open_size.transition(SpringConfig::SNAPPY))
    /// container().scale(Scale::NONE.timeline(pulse(), beats))
    /// ```
    pub fn scale<M>(mut self, factor: impl IntoAnimated<Scale, M>) -> Self {
        let signal = declare(&mut self.anims, factor, |a| &mut a.scale);
        self.transform_mut().scale = Some(signal);
        self
    }

    /// The point [`rotate`](Self::rotate) turns about and [`scale`](Self::scale)
    /// grows from. The centre of the container by default.
    pub fn pivot<M>(mut self, origin: impl IntoSignal<Pivot, M>) -> Self {
        self.transform_mut().pivot = Some(origin.into_signal());
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
        let moves = style.moves_anything();
        let ix = self.interact_mut();
        ix.declares_transform.merge(moves);
        ix.states.push((when, style));
    }
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
        // One frame, one instant. Every animation below is asked about the
        // same moment — the ripple included, which used to read a clock of its
        // own a few microseconds later than the rest.
        let now = tree.frame_instant();

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
            // Which of the three are actually animated, read before the
            // mutable borrow below. `anims` is `Some` — the block is guarded
            // on it and unwraps it two statements down.
            // *Animated*, which is a narrower question than the one
            // `animated_transform` asks under three names that look the same:
            // there a component counts if anything could move it, here only if
            // there is an animation to aim.
            let declared = self.anims.as_ref().expect("guarded above");
            let (animates_translate, animates_rotate, animates_scale) = (
                declared.translate.is_some(),
                declared.rotate.is_some(),
                declared.scale.is_some(),
            );
            let (
                padding_target,
                border_width_target,
                bg_target,
                corners_target,
                elevation_target,
                border_color_target,
                translate_target,
                rotate_target,
                scale_target,
            ) = crate::reactive::diagnostics::snapshot_zone(|| {
                (
                    self.padding.get_or(Padding::default()),
                    self.effective_border_width_target(id),
                    self.effective_background_target(id),
                    self.effective_corners_target(id),
                    self.effective_elevation_target(id),
                    self.effective_border_color_target(id),
                    // Only where there is an animation to aim: each of these
                    // walks the state layers, and all three used to run for a
                    // container that animates nothing but its background. The
                    // neutral value where there is not, rather than an
                    // `Option` the macro would have to unwrap against an
                    // invariant stated only in prose.
                    if animates_translate {
                        self.effective_translate_target(id)
                    } else {
                        Translate::NONE
                    },
                    if animates_rotate {
                        self.effective_rotate_target(id)
                    } else {
                        0.0
                    },
                    if animates_scale {
                        self.effective_scale_target(id)
                    } else {
                        Scale::NONE
                    },
                )
            });
            let anims = self.anims.as_mut().unwrap();

            // A trigger that has moved starts the sequence, before the frame
            // that will show its first value. The read is a snapshot: the
            // subscription belongs to `resync_animation_targets`, which asks
            // the same question inside its tracking scope.
            macro_rules! start_timeline {
                ($field:ident) => {
                    if let Some(anim) = anims.$field.as_mut()
                        && anim.take_play()
                    {
                        anim.play(now);
                    }
                };
            }
            // Every property that can carry one, not the three transform
            // components a timeline used to be limited to. Width and height
            // are the only pair left out, and they are left out by
            // construction: they declare a `Length`, and `Keyframes<Length>`
            // has no constructor.
            crate::reactive::diagnostics::snapshot_zone(|| {
                start_timeline!(padding);
                start_timeline!(border_width);
                start_timeline!(background);
                start_timeline!(corners);
                start_timeline!(elevation);
                start_timeline!(border_color);
                start_timeline!(translate);
                start_timeline!(rotate);
                start_timeline!(scale);
            });

            // Layout-affecting animations: width, height, padding
            advance_anim!(anims, width, id, any_animating, now, layout);
            advance_anim!(anims, height, id, any_animating, now, layout);
            advance_anim!(
                anims,
                padding,
                padding_target,
                id,
                any_animating,
                now,
                layout
            );

            // Paint-only animations: border_width, background, corners,
            // border_color, and the three transform components
            advance_anim!(
                anims,
                border_width,
                border_width_target,
                id,
                any_animating,
                now,
                paint
            );
            advance_anim!(anims, background, bg_target, id, any_animating, now, paint);
            advance_anim!(
                anims,
                corners,
                corners_target,
                id,
                any_animating,
                now,
                paint
            );
            advance_anim!(
                anims,
                elevation,
                elevation_target,
                id,
                any_animating,
                now,
                paint
            );
            advance_anim!(
                anims,
                border_color,
                border_color_target,
                id,
                any_animating,
                now,
                paint
            );
            advance_anim!(
                anims,
                translate,
                translate_target,
                id,
                any_animating,
                now,
                paint
            );
            advance_anim!(anims, rotate, rotate_target, id, any_animating, now, paint);
            advance_anim!(anims, scale, scale_target, id, any_animating, now, paint);
        }

        // Advance ripple animation
        if let Some(ref mut ix) = self.interaction
            && ix.ripple.is_active()
            && let Some(config) = ix.ripple_config()
        {
            let ripple_animating = ix.ripple.advance(&config, now);
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
                let scroll_animating = sd.scroll_state.advance_momentum(now);
                if scroll_animating {
                    // Kinetic scroll is paint-only, request animation continuation with paint
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }
                any_animating = any_animating || scroll_animating;
            }
        }

        // Advance scrollbar scale animations (for hover expansion effect)
        // Must be done here since scroll/hover is paint-only and layout may not run
        if self.advance_scrollbar_scale_animations_internal(id, now) {
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

        // Before `ensure_scrollbar_containers`, which reads the resolved
        // visibility to decide whether to build the parts at all — and before
        // the gutter `child_layout` takes out of the content box, which is
        // measured from the resolved width and margin.
        self.resolve_scroll(id);

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

        self.children_sorted_along = sorted_axis(tree, children);

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
        //
        // Kept as well as published, because paint clamps to it: the shadow that
        // is drawn and the rect that is repainted have to be one number, not two
        // computations of it made a frame apart.
        let reach = with_signal_tracking(id, JobType::Layout, || self.max_elevation());
        self.elevation_reach.set(reach);
        // The shadow's reach is layout's to publish — it follows the elevation,
        // which layout already tracks. What a transform adds is not: a
        // transform is a paint property and reading it here would make moving a
        // widget reflow it. `refresh_paint_bounds` answers that part, in the
        // pass before paint.
        // The children first, so what the publish below carries upward is what
        // was just measured rather than what stood here before this layout.
        // Layout only: `refresh_paint_bounds` runs from a Paint job, and a walk
        // over the children there would put an O(n) back on the frame path that
        // the window exists to keep off it.
        // What a scroller or an `Overflow::Hidden` box paints is bounded by its
        // own edges, however far the content inside it runs. Counting the
        // overhang would damage, and narrow against, a rect the size of the
        // whole scrolled column.
        //
        // `lengths.overflow` rather than a fresh read: `read_box_lengths`
        // already resolved it under this layout's tracking, so the value is in
        // hand and the dependence is the one that makes a Hidden-to-Visible
        // toggle re-run this.
        let clips = self.scroll_axis != ScrollAxis::None || lengths.overflow == Overflow::Hidden;
        tree.set_clips_children(id, clips);
        if !clips {
            tree.remeasure_children(id);
        }
        self.publish_paint_reach(tree, id, Rect::from_size(size));

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
            corners: self.animated_corners(id),
            transform: self.animated_transform(id),
            pivot: self.pivot_signal().get_or(Pivot::CENTER),
        };

        // Undo our own transform before hit testing against the laid-out
        // bounds. A transform collapsed to a line has no inverse, and the
        // event goes on without a position rather than with the wrong one —
        // it still has to arrive, because a press given up and a hover
        // cleared are things only a delivered event can do.
        let local_event: Cow<'_, Event> = match event.coords() {
            Some(at) if !hit.transform.is_identity() => Cow::Owned(
                event.with_coords(untransform_point(&hit.transform, hit.pivot, hit.bounds, at)),
            ),
            _ => Cow::Borrowed(event),
        };

        if let Some(response) = self.handle_scrollbar_event(tree, id, &hit, &local_event) {
            return response;
        }

        let at = tree.event_instant();
        self.track_pointer(id, &hit, &local_event, at);

        // Children are positioned relative to our origin (and to the scroll
        // offset, when we scroll), so their events have to be too.
        let child_event: Cow<'_, Event> = match local_event.coords() {
            Some(at) => {
                let mut child_at = hit.rebase(at);
                if self.scroll_axis != ScrollAxis::None {
                    let sd = self.scroll_data();
                    child_at = child_at.offset(sd.scroll_state.offset_x, sd.scroll_state.offset_y);
                }
                Cow::Owned(local_event.with_coords(Some(child_at)))
            }
            None => local_event.clone(),
        };

        // Children clipped away by hidden overflow or scrolling are invisible,
        // and an invisible child must not steal a click from a sibling drawn
        // below it (a collapsed submenu used to do exactly that).
        let clips_children = self.overflow_resolved.get() == Overflow::Hidden
            || self.scroll_axis != ScrollAxis::None;
        // An event with no position falls outside nothing, so the children
        // are still asked — and still answer no, because their own bounds
        // test is given the same absence.
        let skip_child_dispatch = clips_children
            && local_event
                .coords()
                .is_some_and(|at| !hit.bounds.contains(at.x, at.y));

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

        self.handle_own_event(id, &hit, event, &local_event, at)
    }

    fn refresh_paint_bounds(&self, tree: &mut Tree, id: WidgetId) {
        let size = tree.cached_size(id).unwrap_or_default();
        self.publish_paint_reach(tree, id, Rect::from_size(size));
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
            corners,
            elevation_level,
            user_transform,
            pivot,
            border_width,
            border_color,
            gradient,
            backdrop_blur,
            overflow,
        ) = with_signal_tracking(id, JobType::Paint, || {
            (
                self.animated_background(id),
                self.animated_corners(id),
                self.animated_elevation(id),
                self.animated_transform(id),
                self.pivot_signal().get_or(Pivot::CENTER),
                self.animated_border_width(id),
                self.animated_border_color(id),
                self.gradient.as_ref().and_then(|g| g.get()),
                self.backdrop_blur.as_ref().map(|b| b.get()),
                self.overflow.get_or(Overflow::Visible),
            )
        });
        self.overflow_resolved.set(overflow);

        self.resync_animation_targets(id);

        let (corner_radii, corner_curvature) = (corners.radii, corners.curvature);

        // LOCAL bounds: the origin is this container, the parent already
        // positioned the node.
        let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);
        ctx.set_bounds(local_bounds);

        // Compose our own transform on top of the position the parent set.
        if !user_transform.is_identity() {
            ctx.apply_transform_with_pivot(user_transform, pivot);
        }

        // Before the decoration: the container paints over its own blurred
        // backdrop, and the effect must read a target that does not yet include
        // this container.
        //
        // One command carries both halves. The renderer filters the surface's
        // own; the compositor's region is read off this same command after
        // flattening, so the two can never disagree about whether the container
        // still wants it — and a blur cached, culled or hidden is carried or
        // dropped by the render tree itself rather than by a registry that has
        // to be told.
        if let Some(blur) = backdrop_blur
            && blur.radius > 0.0
        {
            ctx.draw_backdrop_blur(
                local_bounds,
                blur.sources,
                blur.radius,
                corner_radii,
                corner_curvature,
            );
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
            ctx.set_clip(local_bounds, corner_radii, corner_curvature);
        }

        // Determine the effective cull rect for children.
        // For scrollable containers: viewport mapped to layout space (before scroll transform).
        // For non-scrollable containers: inherited from parent via PaintContext.
        let effective_cull_rect = if is_scrollable {
            let sd = self.scroll_data();
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

        let all_children = self.children_source.get();

        let scroll_offset = if is_scrollable {
            let sd = self.scroll_data();
            (sd.scroll_state.offset_x, sd.scroll_state.offset_y)
        } else {
            (0.0, 0.0)
        };
        paint_children(
            tree,
            ctx,
            all_children,
            &ChildPaintOptions {
                scroll_offset,
                cull_rect: effective_cull_rect,
                children_sorted_along: self.children_sorted_along,
                children_reach: tree.children_reach(id),
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
            ctx.set_overlay_clip(local_bounds, corner_radii, corner_curvature);

            // Once for the frame rather than once per disc, and tracked: a
            // ripple held past its growth stops animating on purpose so the
            // loop can go quiet — see `Ripple::advance` — so a colour written
            // while the finger is still down would reach nothing at all
            // without the subscription. Its own scope rather than the block
            // above, because that block runs whether or not a disc exists and
            // this read is already inside the guard that says one does.
            let declared = with_signal_tracking(id, JobType::Paint, || ripple_config.color.get());

            for ripple in ix.ripple.iter() {
                let opacity = ripple.opacity();
                if opacity <= 0.0 {
                    continue;
                }
                let (center_x, center_y) = ripple.center(bounds.width, bounds.height);
                let radius = ripple.radius(bounds.width, bounds.height);

                let ripple_color =
                    Color::rgba(declared.r, declared.g, declared.b, declared.a * opacity);

                ctx.draw_overlay_circle(center_x, center_y, radius, ripple_color);
            }
        }
    }
}

/// The axis `children` came out ordered along, if any.
///
/// **Ordered** means what the binary search in `paint_children` needs:
/// consecutive children that do not overlap on that axis, so both of the
/// predicates it searches with partition the slice. Anything less and
/// `partition_point` answers about whichever child it happened to probe rather
/// than about the viewport, and a child sitting in plain view is dropped.
///
/// Non-overlap also settles which axis to answer when a layout orders its
/// children along both: a column's children overlap horizontally and a row's
/// overlap vertically, so each rules the other axis out and the answer is the
/// one that narrows.
///
/// Bails on the first pair that overlaps on both, so a layout that stacks its
/// children costs two of them rather than all of them.
fn sorted_axis(tree: &Tree, children: &[WidgetId]) -> Option<Axis> {
    // No children are ordered along nothing in particular, and answering an
    // axis for them would be a claim about nothing.
    if children.is_empty() {
        return None;
    }
    let (mut vertical, mut horizontal) = (true, true);
    let (mut bottom, mut right) = (f32::NEG_INFINITY, f32::NEG_INFINITY);

    for &child in children {
        // A child the tree cannot answer for is a child the search cannot
        // place, and one unplaceable child unpartitions the whole slice.
        let bounds = tree.get_bounds(child)?;
        let (top, next_bottom) = bounds.span(Axis::Vertical);
        let (left, next_right) = bounds.span(Axis::Horizontal);
        vertical &= top >= bottom;
        horizontal &= left >= right;
        if !(vertical || horizontal) {
            return None;
        }
        (bottom, right) = (next_bottom, next_right);
    }

    Some(if vertical {
        Axis::Vertical
    } else {
        Axis::Horizontal
    })
}

/// Declare an animatable property: keep the value's signal, and install
/// whatever motion arrived with it.
///
/// A declaration is the whole property, so the last one wins outright — value
/// *and* motion. Restating a property with a plain value takes its animation
/// away, which is what makes `adopt_declarations_of` unnecessary: there are
/// never two declarations for one property to reconcile, only the one written
/// last.
///
/// The animation is seeded from the signal's value at builder time. For every
/// property but padding and border width that seed is replaced at the first
/// layout by `seed_animations`, which reads the signal rather than this
/// snapshot; those two are read there only for their subscription, so their
/// seed survives into the first advance.
fn declare<T: Animatable, M>(
    anims: &mut Option<Box<ContainerAnims>>,
    value: impl IntoAnimated<T, M>,
    slot: impl FnOnce(&mut ContainerAnims) -> &mut Option<AnimationState<T>>,
) -> Signal<T> {
    let (signal, motion) = value.into_animated().into_parts();
    let installed = motion.map(|motion| {
        let seed = signal.get_untracked();
        match *motion {
            Motion::Ease(config) => AnimationState::new(seed, config),
            Motion::Play { keyframes, plays } => {
                AnimationState::new(seed, instant_transition()).with_timeline(keyframes, plays)
            }
        }
    });
    write_slot(anims, slot, installed);
    signal
}

/// Put an animation, or the absence of one, where the property keeps it.
///
/// Nothing into a container that has no animation box is the overwhelmingly
/// common case — every plain `background(RED)` — so it must not be the thing
/// that allocates one.
fn write_slot<T: Animatable>(
    anims: &mut Option<Box<ContainerAnims>>,
    slot: impl FnOnce(&mut ContainerAnims) -> &mut Option<AnimationState<T>>,
    installed: Option<AnimationState<T>>,
) {
    match anims.as_deref_mut() {
        Some(anims) => *slot(anims) = installed,
        None if installed.is_some() => *slot(anims.get_or_insert_with(Box::default)) = installed,
        None => {}
    }
}

/// The same for a width or a height, the one pair whose declared type is not
/// the type it animates — see [`Animated::into_eased`], which is where that
/// narrowing is argued.
///
/// The seed is where the *first frame* starts, so it reads the declared length
/// alone. `update_size_targets` recomputes the target from the measured
/// content at every layout after that, which is why the two formulas differ.
fn declare_size<M>(
    anims: &mut Option<Box<ContainerAnims>>,
    value: impl IntoAnimated<Length, M>,
    slot: impl FnOnce(&mut ContainerAnims) -> &mut Option<AnimationState<f32>>,
) -> Signal<Length> {
    let (signal, ease) = value.into_animated().into_eased();
    let installed = ease.map(|config| {
        let length = signal.get_untracked();
        AnimationState::new(length.exact.or(length.min).unwrap_or(0.0), config)
    });
    write_slot(anims, slot, installed);
    signal
}

pub fn container() -> Container {
    Container::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Animate, TimingFunction, Transition};
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
        let widget = container().scale(
            (move || {
                if open.get() {
                    Scale::NONE
                } else {
                    Scale::new(1.0, 0.0)
                }
            })
            .transition(Transition::new(200, TimingFunction::EaseOut)),
        );

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

    /// Content-sized surfaces measure under the measure-final flag: layout
    /// must report animation TARGETS, not in-flight values, so a growth
    /// animation resizes the surface once instead of once per frame.
    #[test]
    fn measure_final_reads_animation_targets() {
        let height_sig = create_signal(100.0_f32);
        let widget = container().width(50.0).height(
            (move || height_sig.get()).transition(Transition::new(200, TimingFunction::EaseOut)),
        );

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
            .padding((move || pad.get()).transition(Transition::new(200, TimingFunction::EaseOut)));

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
