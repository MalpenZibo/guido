//! State layer system for interaction-based style overrides.
//!
//! This module provides types for defining style changes based on widget state
//! (hover, pressed, etc.). State styles allow containers to redefine properties
//! like background color, border, transform, and more when in specific states.
//!
//! # Example
//! ```ignore
//! container()
//!     .background(Color::rgb(0.2, 0.2, 0.3))
//!     .hover_state(|s| s.lighter(0.1))
//!     .pressed_state(|s| s.darker(0.1).transform(Transform::scale(0.98)))
//!     .child(text("Interactive button"))
//! ```

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

/// How to override the background color in a state.
#[derive(Clone, Debug)]
pub enum BackgroundOverride {
    /// Use an explicit color
    Exact(Color),
    /// Lighten the base background by amount (0.0-1.0)
    Lighter(f32),
    /// Darken the base background by amount (0.0-1.0)
    Darker(f32),
}

/// Style overrides to apply during a specific interaction state.
///
/// All fields are optional - `None` means use the base value from the container.
#[derive(Clone, Default, Debug)]
pub struct StateStyle {
    /// Background color override
    pub background: Option<BackgroundOverride>,
    /// Border width override
    pub border_width: Option<f32>,
    /// Border color override
    pub border_color: Option<Color>,
    /// Corner radius override
    pub corner_radius: Option<f32>,
    /// Transform override (e.g., scale on press)
    pub transform: Option<Transform>,
    /// Elevation (shadow) override
    pub elevation: Option<f32>,
    /// Override the background alpha channel (applied after background override)
    pub alpha: Option<f32>,
    /// Ripple effect configuration (typically used in pressed_state)
    pub ripple: Option<RippleConfig>,
}

impl StateStyle {
    /// Create a new empty state style.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set an explicit background color for this state.
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(BackgroundOverride::Exact(color.into()));
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
    pub fn lighter(mut self, amount: f32) -> Self {
        self.background = Some(BackgroundOverride::Lighter(amount));
        self
    }

    /// Darken the base background by amount (0.0-1.0).
    ///
    /// This computes a darker color from the container's base background
    /// by blending toward black.
    ///
    /// # Example
    /// ```ignore
    /// container()
    ///     .background(Color::rgb(0.2, 0.2, 0.3))
    ///     .pressed_state(|s| s.darker(0.1)) // 10% darker on press
    /// ```
    pub fn darker(mut self, amount: f32) -> Self {
        self.background = Some(BackgroundOverride::Darker(amount));
        self
    }

    /// Set the border width and color for this state.
    pub fn border(mut self, width: f32, color: impl Into<Color>) -> Self {
        self.border_width = Some(width);
        self.border_color = Some(color.into());
        self
    }

    /// Set just the border width for this state.
    pub fn border_width(mut self, width: f32) -> Self {
        self.border_width = Some(width);
        self
    }

    /// Set just the border color for this state.
    pub fn border_color(mut self, color: impl Into<Color>) -> Self {
        self.border_color = Some(color.into());
        self
    }

    /// Set the corner radius for this state.
    pub fn corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = Some(radius);
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
    pub fn transform(mut self, transform: Transform) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Set the elevation (shadow level) for this state.
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
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
    pub fn alpha(mut self, alpha: f32) -> Self {
        self.alpha = Some(alpha);
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
pub fn resolve_background(base: Color, override_: &BackgroundOverride) -> Color {
    match override_ {
        BackgroundOverride::Exact(color) => *color,
        BackgroundOverride::Lighter(amount) => {
            // Blend toward white
            Color::rgba(
                base.r + (1.0 - base.r) * amount,
                base.g + (1.0 - base.g) * amount,
                base.b + (1.0 - base.b) * amount,
                base.a,
            )
        }
        BackgroundOverride::Darker(amount) => {
            // Blend toward black
            Color::rgba(
                base.r * (1.0 - amount),
                base.g * (1.0 - amount),
                base.b * (1.0 - amount),
                base.a,
            )
        }
    }
}
