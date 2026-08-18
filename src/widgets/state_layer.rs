//! State layer system for interaction-based style overrides.
//!
//! A state layer redefines properties — background, border, transform and the
//! rest — while the container is in a given state. Every value is a
//! [`Signal`], like the base properties they override: a state layer that
//! could not follow the theme would be a second, quieter styling system.
//!
//! # Example
//! ```ignore
//! container()
//!     .background(Color::rgb(0.2, 0.2, 0.3))
//!     .hover_state(|s| s.lighter(0.1))
//!     .pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
//!     .child(text("Interactive button"))
//! ```

use crate::reactive::{IntoSignal, Signal};
use crate::transform::Transform;
use crate::widgets::Color;

/// Configuration for ripple effect animation.
#[derive(Clone, Debug)]
pub struct RippleConfig {
    /// Color of the ripple (usually semi-transparent white)
    pub color: Color,
    /// Speed multiplier for ripple expansion (higher = faster)
    pub expand_speed: f32,
    /// Speed multiplier for ripple fade out (higher = faster)
    pub fade_speed: f32,
}

impl Default for RippleConfig {
    fn default() -> Self {
        Self {
            color: Color::rgba(1.0, 1.0, 1.0, 0.3),
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
    pub fn with_color(color: Color) -> Self {
        Self {
            color,
            ..Default::default()
        }
    }
}

/// When a state layer applies.
///
/// The first three are noticed by the container itself — the pointer is inside
/// it, the pointer is down on it, the focus is somewhere below it. The fourth
/// is a condition the app already holds: "the last submit failed", "this row
/// is selected". Nothing has to propagate for that one, so it needs no
/// mechanism beyond reading the signal where the style is resolved.
#[derive(Clone, Copy)]
pub enum StateWhen {
    /// The pointer is inside the container's shape.
    Hovered,
    /// The pointer is down on the container.
    Pressed,
    /// The container, or anything below it, holds the keyboard focus.
    Focused,
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

/// Style overrides to apply during a specific interaction state.
///
/// All fields are optional — `None` means use the base value from the
/// container — and all of them hold signals, so an override tracks whatever it
/// was given just as the base property does.
#[derive(Clone, Default)]
pub struct StateStyle {
    /// Background color override
    pub background: Option<BackgroundOverride>,
    /// Border width override
    pub border_width: Option<Signal<f32>>,
    /// Border color override
    pub border_color: Option<Signal<Color>>,
    /// Corner radius override
    pub corner_radius: Option<Signal<f32>>,
    /// Transform override (e.g., scale on press)
    pub transform: Option<Signal<Transform>>,
    /// Elevation (shadow) override
    pub elevation: Option<Signal<f32>>,
    /// Colour of the text below this container while the state is active.
    ///
    /// Reaches the glyphs, not just the box: the container publishes its text
    /// colour to descendants as a derived over the interaction flags, so a
    /// text that inherited it is subscribed to the flip.
    pub text_color: Option<Signal<Color>>,
    /// Override the background alpha channel (applied after background override)
    pub alpha: Option<Signal<f32>>,
    /// Ripple effect configuration (typically used in pressed_state)
    pub ripple: Option<RippleConfig>,
}

impl StateStyle {
    /// Create a new empty state style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an explicit background color for this state.
    pub fn background<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.background = Some(BackgroundOverride::Exact(color.into_signal()));
        self
    }

    /// Set the colour of the text below this container for this state.
    ///
    /// ```ignore
    /// container()
    ///     .text_color(theme.text_weak)
    ///     .hover_state(|s| s.text_color(theme.text))
    ///     .child(text("Label"))
    /// ```
    pub fn text_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.text_color = Some(color.into_signal());
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
    ///     .hover_state(|s| s.lighter(0.1)) // 10% lighter on hover
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
    ///     .pressed_state(|s| s.darker(0.1)) // 10% darker on press
    /// ```
    pub fn darker<M>(mut self, amount: impl IntoSignal<f32, M>) -> Self {
        self.background = Some(BackgroundOverride::Darker(amount.into_signal()));
        self
    }

    /// Set the border width and color for this state.
    pub fn border<M1, M2>(
        mut self,
        width: impl IntoSignal<f32, M1>,
        color: impl IntoSignal<Color, M2>,
    ) -> Self {
        self.border_width = Some(width.into_signal());
        self.border_color = Some(color.into_signal());
        self
    }

    /// Set just the border width for this state.
    pub fn border_width<M>(mut self, width: impl IntoSignal<f32, M>) -> Self {
        self.border_width = Some(width.into_signal());
        self
    }

    /// Set just the border color for this state.
    pub fn border_color<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.border_color = Some(color.into_signal());
        self
    }

    /// Set the corner radius for this state.
    pub fn corner_radius<M>(mut self, radius: impl IntoSignal<f32, M>) -> Self {
        self.corner_radius = Some(radius.into_signal());
        self
    }

    /// Set the transform for this state.
    ///
    /// Commonly used for press effects like scale-down.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .pressed_state(|s| s.transform(Transform::scale(0.98)))
    /// ```
    pub fn transform<M>(mut self, transform: impl IntoSignal<Transform, M>) -> Self {
        self.transform = Some(transform.into_signal());
        self
    }

    /// Set the elevation (shadow level) for this state.
    pub fn elevation<M>(mut self, elevation: impl IntoSignal<f32, M>) -> Self {
        self.elevation = Some(elevation.into_signal());
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
    ///     .hover_state(|s| s.lighter(0.1).alpha(0.7)) // boost alpha on hover
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
    ///     .pressed_state(|s| s.ripple())
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
    ///     .pressed_state(|s| s.ripple_with_color(Color::rgba(1.0, 0.5, 0.0, 0.3)))
    ///     .child(text("Orange ripple"))
    /// ```
    pub fn ripple_with_color(mut self, color: Color) -> Self {
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
