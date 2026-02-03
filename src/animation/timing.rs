//! Timing functions (easing curves) for animations.
//!
//! Timing functions control the rate of change during an animation, allowing
//! for natural-feeling motion rather than linear interpolation.
//!
//! ## Built-in Easing Functions
//!
//! - [`TimingFunction::Linear`] - Constant speed (no easing)
//! - [`TimingFunction::EaseIn`] - Starts slow, ends fast (acceleration)
//! - [`TimingFunction::EaseOut`] - Starts fast, ends slow (deceleration)
//! - [`TimingFunction::EaseInOut`] - Slow start and end, fast middle
//!
//! ## Advanced Options
//!
//! - [`TimingFunction::CubicBezier`] - CSS-style cubic bezier curve
//! - [`TimingFunction::Spring`] - Physics-based spring (can overshoot)
//! - [`TimingFunction::Custom`] - User-defined function
//!
//! ## Example
//!
//! ```ignore
//! container()
//!     .hover_state(|s| s
//!         .lighter(0.1)
//!         .timing(TimingFunction::EaseOut)
//!         .duration(Duration::from_millis(150)))
//! ```

use super::spring::SpringConfig;
use std::sync::Arc;

/// Timing function that controls the animation curve
#[derive(Clone)]
pub enum TimingFunction {
    /// Linear interpolation (constant speed)
    Linear,
    /// Starts slow, ends fast
    EaseIn,
    /// Starts fast, ends slow
    EaseOut,
    /// Starts slow, speeds up, then slows down
    EaseInOut,
    /// CSS cubic-bezier curve (x1, y1, x2, y2)
    CubicBezier(f32, f32, f32, f32),
    /// Spring physics simulation (can overshoot)
    Spring(SpringConfig),
    /// Custom timing function
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl TimingFunction {
    /// Evaluate the timing function at time t (0.0 to 1.0)
    /// Returns the interpolation factor (can exceed [0, 1] for overshoot)
    ///
    /// Note: Spring animations are handled separately in AnimationState::advance()
    /// using real elapsed time. This method returns t as fallback for springs.
    pub fn evaluate(&self, t: f32) -> f32 {
        match self {
            TimingFunction::Linear => t,
            TimingFunction::EaseIn => ease_in(t),
            TimingFunction::EaseOut => ease_out(t),
            TimingFunction::EaseInOut => ease_in_out(t),
            TimingFunction::CubicBezier(x1, y1, x2, y2) => cubic_bezier(t, *x1, *y1, *x2, *y2),
            TimingFunction::Spring(_) => t, // Springs handled separately with real time
            TimingFunction::Custom(f) => f(t),
        }
    }

    /// Create a custom timing function from a closure
    pub fn custom<F>(f: F) -> Self
    where
        F: Fn(f32) -> f32 + Send + Sync + 'static,
    {
        TimingFunction::Custom(Arc::new(f))
    }
}

impl std::fmt::Debug for TimingFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TimingFunction::Linear => write!(f, "Linear"),
            TimingFunction::EaseIn => write!(f, "EaseIn"),
            TimingFunction::EaseOut => write!(f, "EaseOut"),
            TimingFunction::EaseInOut => write!(f, "EaseInOut"),
            TimingFunction::CubicBezier(x1, y1, x2, y2) => {
                write!(f, "CubicBezier({}, {}, {}, {})", x1, y1, x2, y2)
            }
            TimingFunction::Spring(config) => write!(f, "Spring({:?})", config),
            TimingFunction::Custom(_) => write!(f, "Custom"),
        }
    }
}

// Easing functions

fn ease_in(t: f32) -> f32 {
    t * t
}

fn ease_out(t: f32) -> f32 {
    t * (2.0 - t)
}

fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        -1.0 + (4.0 - 2.0 * t) * t
    }
}

/// Cubic bezier curve evaluation
/// Simplified implementation assuming x1, x2 are in [0, 1]
fn cubic_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    // Use Newton-Raphson to solve for t given x
    let mut current_t = t;
    for _ in 0..8 {
        let current_x = cubic_bezier_x(current_t, x1, x2);
        let current_slope = cubic_bezier_slope(current_t, x1, x2);
        if current_slope.abs() < 1e-6 {
            break;
        }
        current_t -= (current_x - t) / current_slope;
    }
    cubic_bezier_y(current_t, y1, y2)
}

fn cubic_bezier_x(t: f32, x1: f32, x2: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    3.0 * mt2 * t * x1 + 3.0 * mt * t2 * x2 + t3
}

fn cubic_bezier_y(t: f32, y1: f32, y2: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    3.0 * mt2 * t * y1 + 3.0 * mt * t2 * y2 + t3
}

fn cubic_bezier_slope(t: f32, x1: f32, x2: f32) -> f32 {
    let mt = 1.0 - t;
    3.0 * mt * mt * x1 + 6.0 * mt * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear() {
        assert_eq!(TimingFunction::Linear.evaluate(0.0), 0.0);
        assert_eq!(TimingFunction::Linear.evaluate(0.5), 0.5);
        assert_eq!(TimingFunction::Linear.evaluate(1.0), 1.0);
    }

    #[test]
    fn test_ease_in() {
        let result = TimingFunction::EaseIn.evaluate(0.5);
        assert!(result < 0.5); // Should be slower at start
    }

    #[test]
    fn test_ease_out() {
        let result = TimingFunction::EaseOut.evaluate(0.5);
        assert!(result > 0.5); // Should be faster at start
    }
}
