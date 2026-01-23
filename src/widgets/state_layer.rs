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
