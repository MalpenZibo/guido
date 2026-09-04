//! State layer system for interaction-based style overrides.
//!
//! A state layer redefines properties — background, border, scale and the
//! rest — while the container is in a given state. Every value is a
//! [`Signal`], like the base properties they override: a state layer that
//! could not follow the theme would be a second, quieter styling system.
//!
//! # Example
//! ```ignore
//! container()
//!     .background(Color::rgb(0.2, 0.2, 0.3))
//!     .when_hovered(|s| s.lighter(0.1))
//!     .when_pressed(|s| s.darker(0.1).scale(0.98))
//!     .child(text("Interactive button"))
//! ```

use crate::reactive::{IntoSignal, Signal, create_stored};
use crate::renderer::Shadow;
use crate::transform::{Scale, Translate};
use crate::widgets::Color;

/// Configuration for ripple effect animation.
#[derive(Clone, Copy)]
pub struct RippleConfig {
    /// Color of the ripple (usually semi-transparent white).
    ///
    /// A signal, as `BorderOverride`'s halves are, so a theme switch reaches the
    /// one colour that animates under a finger. The two speeds beside it stay
    /// plain: nobody themes how fast a ripple expands, and a signal there would
    /// be a guess at a caller that does not exist.
    pub color: Signal<Color>,
    /// Speed multiplier for ripple expansion (higher = faster)
    pub expand_speed: f32,
    /// Speed multiplier for ripple fade out (higher = faster)
    pub fade_speed: f32,
}

impl Default for RippleConfig {
    fn default() -> Self {
        Self {
            color: create_stored(Color::rgba(1.0, 1.0, 1.0, 0.3)),
            expand_speed: 1.0,
            fade_speed: 1.0,
        }
    }
}

impl RippleConfig {
    /// Create a new ripple config with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a ripple config with a custom color.
    pub fn with_color<M>(color: impl IntoSignal<Color, M>) -> Self {
        // The speeds spelled out rather than `..Default::default()`, which now
        // builds a stored signal for the colour and drops it — and a dropped
        // `Signal` frees nothing, since the slot lives as long as its owner.
        Self {
            color: color.into_signal(),
            expand_speed: 1.0,
            fade_speed: 1.0,
        }
    }
}

/// When a state layer applies.
///
/// The first three are noticed by the container itself — the pointer is inside
/// it, the pointer is down on it, the focus is somewhere below it. The fifth
/// is a condition the app already holds: "the last submit failed", "this row
/// is selected". Nothing has to propagate for that one, so it needs no
/// mechanism beyond reading the signal where the style is resolved.
///
/// [`Disabled`](Self::Disabled) is the one that propagates: it is declared
/// with [`enabled`](crate::widgets::Container::enabled) on a container and
/// answered by every widget below it, which is what a bare `When(busy)` cannot
/// promise across two widgets in two files.
#[derive(Clone, Copy)]
pub enum StateWhen {
    /// The pointer is inside the container's shape.
    Hovered,
    /// The pointer is down on the container.
    Pressed,
    /// The container, or anything below it, holds the keyboard focus.
    Focused,
    /// The control, or something above it, was declared not enabled.
    Disabled,
    /// A condition the app owns.
    When(Signal<bool>),
}

/// How to override the background color in a state.
#[derive(Clone, Copy)]
pub enum BackgroundOverride {
    /// Use an explicit color
    Exact(Signal<Color>),
    /// Lighten the base background by amount (0.0-1.0)
    Lighter(Signal<f32>),
    /// Darken the base background by amount (0.0-1.0)
    Darker(Signal<f32>),
}

/// A border an active state draws instead of the declared one.
///
/// Named rather than a tuple, as [`BackgroundOverride`] is, and one value rather
/// than two fields so that half a border cannot be built even by hand.
#[derive(Clone, Copy)]
pub struct BorderOverride {
    pub width: Signal<f32>,
    pub color: Signal<Color>,
}

/// Style overrides to apply during a specific interaction state.
///
/// All fields are optional — `None` means use the base value from the
/// container — and all of them hold signals, so an override tracks whatever it
/// was given just as the base property does.
#[derive(Clone, Default)]
pub struct StateStyle {
    /// Background color override
    pub background: Option<BackgroundOverride>,
    /// Border override — both halves or neither. A width with no colour and a
    /// colour with no width are the same thing, which is no border, so the pair
    /// is one field rather than two that could disagree.
    pub border: Option<BorderOverride>,
    /// Corner radius override
    pub corners: Option<Signal<crate::widgets::Corners>>,
    /// Displacement override
    pub translate: Option<Signal<Translate>>,
    /// Rotation override, in degrees
    pub rotate: Option<Signal<f32>>,
    /// Scale override (e.g., the shrink on press)
    pub scale: Option<Signal<Scale>>,
    /// Shadow override
    pub shadow: Option<Signal<Shadow>>,
    /// Override the background alpha channel (applied after background override)
    pub alpha: Option<Signal<f32>>,
    /// Ripple effect configuration (typically used in a pressed layer)
    pub ripple: Option<RippleConfig>,
}

/// Which transform components a set of state layers between them override.
///
/// Three answers and not one, because `animated_transform` resolves each
/// component separately and a single "some layer moves something" would light
/// all three — so `when_pressed(|s| s.scale(0.98))`, the commonest state layer
/// there is, would pay two state-layer walks per paint and per coalesced
/// pointer move for components nothing declares.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Moves {
    pub(crate) translate: bool,
    pub(crate) rotate: bool,
    pub(crate) scale: bool,
}

impl Moves {
    /// Whether anything moves at all — the early-out a plain layout box takes,
    /// and most containers are one.
    pub(crate) fn any(&self) -> bool {
        self.translate || self.rotate || self.scale
    }

    pub(crate) fn merge(&mut self, other: Moves) {
        self.translate |= other.translate;
        self.rotate |= other.rotate;
        self.scale |= other.scale;
    }
}

impl StateStyle {
    /// Whether this layer moves, turns or resizes what it styles.
    ///
    /// Lives here, beside the fields, because that is where it stays true: a
    /// component added to `StateStyle` is added a few lines above this, and a
    /// caller that cached the answer somewhere else would go quietly stale.
    /// `Container` gates its identity fast path on it — see
    /// `InteractionState::declares_transform`.
    pub(crate) fn moves_anything(&self) -> Moves {
        Moves {
            translate: self.translate.is_some(),
            rotate: self.rotate.is_some(),
            scale: self.scale.is_some(),
        }
    }

    /// Create a new empty state style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an explicit background color for this state.
    ///
    /// A value, and nothing about time. The motion belongs to the property,
    /// declared once beside its base value, and a state layer *overrides* a
    /// property somebody else owns — so a hover already eases under whatever
    /// the base declared:
    ///
    /// ```ignore
    /// container()
    ///     .background(base.transition(200.0))    // the property eases
    ///     .when_hovered(|s| s.background(HOT.transition(900.0)))   // the override changes the value
    /// ```
    ///
    /// Declaring a timing here is a compile error rather than a value quietly
    /// ignored, which is the whole reason [`Animated`](crate::animation::Animated)
    /// is not an [`IntoSignal`]:
    ///
    /// ```compile_fail
    /// use guido::prelude::*;
    ///
    /// const HOT: Color = Color::rgb(0.9, 0.3, 0.2);
    /// let _ = container()
    ///     .background(Color::BLACK)
    ///     .when_hovered(|s| s.background(HOT.transition(900.0)));
    /// ```
    pub fn background<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.background = Some(BackgroundOverride::Exact(color.into_signal()));
        self
    }

    /// Lighten the base background by amount (0.0-1.0).
    ///
    /// This computes a lighter color from the container's base background
    /// by blending toward white.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgb(0.2, 0.2, 0.3))
    ///     .when_hovered(|s| s.lighter(0.1)) // 10% lighter on hover
    /// ```
    pub fn lighter<M>(mut self, amount: impl IntoSignal<f32, M>) -> Self {
        self.background = Some(BackgroundOverride::Lighter(amount.into_signal()));
        self
    }

    /// Darken the base background by amount (0.0-1.0).
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgb(0.2, 0.2, 0.3))
    ///     .when_pressed(|s| s.darker(0.1)) // 10% darker on press
    /// ```
    pub fn darker<M>(mut self, amount: impl IntoSignal<f32, M>) -> Self {
        self.background = Some(BackgroundOverride::Darker(amount.into_signal()));
        self
    }

    /// Override the border while this state is active.
    ///
    /// Both halves, as on the container — see
    /// [`Container::border`](crate::widgets::Container::border) for why there is
    /// no way to say half of one.
    pub fn border<M1, M2>(
        mut self,
        width: impl IntoSignal<f32, M1>,
        color: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.border = Some(BorderOverride {
            width: width.into_signal(),
            color: color.into_signal(),
        });
        self
    }

    /// The corner shape while this state is active.
    ///
    /// It replaces the whole shape, as the border override replaces the whole
    /// border: a bare size means *rounded* at that size, so a squircle that
    /// wants to grow on hover has to say so —
    /// `when_hovered(|s| s.corners(Corners::squircle(20.0)))`. Radius and
    /// curvature are one value precisely because half a corner is not a
    /// corner.
    pub fn corners<M>(mut self, corners: impl IntoSignal<crate::widgets::Corners, M>) -> Self {
        self.corners = Some(corners.into_signal());
        self
    }

    /// Displace the container while it is in this state.
    pub fn translate<M>(mut self, translate: impl IntoSignal<Translate, M>) -> Self {
        self.translate = Some(translate.into_signal());
        self
    }

    /// Turn the container while it is in this state, in degrees.
    pub fn rotate<M>(mut self, degrees: impl IntoSignal<f32, M>) -> Self {
        self.rotate = Some(degrees.into_signal());
        self
    }

    /// Resize the container while it is in this state.
    ///
    /// The press effect, most of the time:
    ///
    /// ```ignore
    /// container().when_pressed(|s| s.scale(0.98))
    /// ```
    pub fn scale<M>(mut self, factor: impl IntoSignal<Scale, M>) -> Self {
        self.scale = Some(factor.into_signal());
        self
    }

    /// Set the shadow for this state.
    ///
    /// ```
    /// use guido::prelude::*;
    ///
    /// const LIFTED: Shadow = Shadow::new((0.0, 8.0), 16.0, 0.0, Color::BLACK);
    /// let _ = container()
    ///     .shadow(Shadow::none().transition(120.0))
    ///     .when_hovered(|s| s.shadow(LIFTED));
    /// ```
    ///
    /// A value, never a motion: the timing belongs to the declaration this
    /// overrides, so a transition here does not compile rather than being
    /// quietly ignored.
    ///
    /// ```compile_fail
    /// use guido::prelude::*;
    ///
    /// const LIFTED: Shadow = Shadow::new((0.0, 8.0), 16.0, 0.0, Color::BLACK);
    /// let _ = container()
    ///     .shadow(Shadow::none())
    ///     .when_hovered(|s| s.shadow(LIFTED.transition(80.0)));
    /// ```
    pub fn shadow<M>(mut self, shadow: impl IntoSignal<Shadow, M>) -> Self {
        self.shadow = Some(shadow.into_signal());
        self
    }

    /// Override the background alpha channel.
    ///
    /// Applied after any background color override (lighter/darker/exact).
    /// Useful for making semi-transparent elements more visible on hover.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgba(1.0, 0.5, 0.0, 0.4))
    ///     .when_hovered(|s| s.lighter(0.1).alpha(0.7)) // boost alpha on hover
    /// ```
    pub fn alpha<M>(mut self, alpha: impl IntoSignal<f32, M>) -> Self {
        self.alpha = Some(alpha.into_signal());
        self
    }

    /// Enable ripple effect with default settings.
    ///
    /// The ripple expands from the click point and fades out when released.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .when_pressed(|s| s.ripple())
    ///     .child(text("Click for ripple"))
    /// ```
    pub fn ripple(mut self) -> Self {
        self.ripple = Some(RippleConfig::default());
        self
    }

    /// Enable ripple effect with a custom color.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .when_pressed(|s| s.ripple_with_color(Color::rgba(1.0, 0.5, 0.0, 0.3)))
    ///     .child(text("Orange ripple"))
    /// ```
    pub fn ripple_with_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.ripple = Some(RippleConfig::with_color(color));
        self
    }

    /// Enable ripple effect with custom configuration.
    pub fn ripple_config(mut self, config: RippleConfig) -> Self {
        self.ripple = Some(config);
        self
    }
}

/// Resolve a background override to an actual color.
///
/// Reads the override's signal, so the caller's tracking scope subscribes to
/// it — the same contract the base property has.
pub fn resolve_background(base: Color, override_: &BackgroundOverride) -> Color {
    match override_ {
        BackgroundOverride::Exact(color) => color.get(),
        BackgroundOverride::Lighter(amount) => base.lighter(amount.get()),
        BackgroundOverride::Darker(amount) => base.darker(amount.get()),
    }
}

/// Declaring how a widget looks while its control is in a state.
///
/// The unit that holds the state is the widget's
/// [`Control`](crate::widgets::Control) — the nearest enclosing container
/// marked as one — so a button's label reacts to the button being hovered, and
/// a form label reacts to the focus of the input beside it. A widget with no
/// control above it is its own unit, and notices the pointer over its own
/// bounds.
///
/// The closure receives the same partial style the widget itself is built
/// with, because an override *is* another partial style:
///
/// ```ignore
/// text("Save").color(theme.weak).when_hovered(|s| s.color(theme.strong))
/// ```
///
/// Layers resolve in reverse declaration order, per property, exactly as a
/// container's do.
pub trait Stateful: Sized {
    /// The partial style an override is written in.
    type Style: Default;

    #[doc(hidden)]
    fn push_state_style(&mut self, when: StateWhen, style: Self::Style);

    /// While the pointer is inside the control.
    fn when_hovered(mut self, f: impl FnOnce(Self::Style) -> Self::Style) -> Self {
        self.push_state_style(StateWhen::Hovered, f(Self::Style::default()));
        self
    }

    /// While the pointer is down on the control.
    ///
    /// A widget with no control above it is never pressed: being pressed means
    /// being activated, and a widget that is its own unit has nothing to
    /// activate.
    fn when_pressed(mut self, f: impl FnOnce(Self::Style) -> Self::Style) -> Self {
        self.push_state_style(StateWhen::Pressed, f(Self::Style::default()));
        self
    }

    /// While the keyboard focus is inside the control.
    ///
    /// This is the one the direction of the old mechanism could not express:
    /// the focus is in a *sibling*, and both belong to the same control.
    fn when_focused(mut self, f: impl FnOnce(Self::Style) -> Self::Style) -> Self {
        self.push_state_style(StateWhen::Focused, f(Self::Style::default()));
        self
    }

    /// While the control refuses input.
    ///
    /// The look declared beside the behaviour:
    /// [`enabled`](crate::widgets::Container::enabled) is what stops the
    /// events, and this is what says so — so a greyed field and a dead one
    /// cannot come apart, and `Disabled` means the same thing in every file.
    ///
    /// It reads the *effective* answer, ancestors folded in, so a label inside
    /// a disabled form greys with the form without being told twice.
    fn when_disabled(mut self, f: impl FnOnce(Self::Style) -> Self::Style) -> Self {
        self.push_state_style(StateWhen::Disabled, f(Self::Style::default()));
        self
    }

    /// While `condition` holds — a state the app owns, needing no control at
    /// all, since the signal is one the caller already has.
    fn state<M>(
        mut self,
        condition: impl IntoSignal<bool, M>,
        f: impl FnOnce(Self::Style) -> Self::Style,
    ) -> Self {
        self.push_state_style(
            StateWhen::When(condition.into_signal()),
            f(Self::Style::default()),
        );
        self
    }
}
