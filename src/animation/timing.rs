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
//!     .when_hovered(|s| s
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
    /// A curve of the caller's own, built by [`custom`](TimingFunction::custom).
    ///
    /// The excursion it carries is sampled once at construction rather than the
    /// curve being clamped to a constant: anything sizing a bound from a curve —
    /// a damage rect is why this exists — needs a number that is true of *that*
    /// curve, and a hand-rolled bounce should bounce as far as it was written
    /// to. Which is why [`CustomCurve`] cannot be assembled by hand: a
    /// `Custom(f, 0.0)` written out where the enum is in scope type-checks, and
    /// its excursion is then a claim nobody measured.
    Custom(CustomCurve),
}

/// A caller-supplied easing curve and the excursion measured from it.
///
/// Opaque on purpose. The pair is only meaningful when the second half was
/// measured from the first, so [`TimingFunction::custom`] is the only way to
/// make one — see [`TimingFunction::Custom`].
#[derive(Clone)]
pub struct CustomCurve {
    f: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
    excursion: f32,
}

impl CustomCurve {
    fn evaluate(&self, t: f32) -> f32 {
        (self.f)(t)
    }
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
            TimingFunction::Custom(curve) => curve.evaluate(t),
        }
    }

    /// How far past 1.0 this curve can go, as a fraction of the distance
    /// travelled. Zero for every curve that only eases.
    ///
    /// A bezier is bounded by its control polygon, so its control points give
    /// the bound directly, and a spring's physics give it exactly.
    ///
    /// A [`Custom`](TimingFunction::Custom) curve is an arbitrary closure, so it
    /// cannot be asked — it is **measured**:
    /// [`custom`](TimingFunction::custom) samples it once when it is built and
    /// carries the excursion it found, which is what this reports. The curve
    /// itself is untouched, and [`evaluate`](Self::evaluate) hands back exactly
    /// what it returns.
    ///
    /// Clamping the curve to an assumed allowance would make the bound true by
    /// shortening every hand-rolled bounce, silently, which is the wrong way
    /// round: the bound describes the curve, not the reverse. Assuming one
    /// without measuring is no better — a bounce peaking at 1.5 would have had
    /// its damage rect measured for 1.25, leaving the ring between the two
    /// outside every one of them.
    pub fn peak_overshoot(&self) -> f32 {
        match self {
            TimingFunction::Linear
            | TimingFunction::EaseIn
            | TimingFunction::EaseOut
            | TimingFunction::EaseInOut => 0.0,
            // Both ends. An anticipation curve dips *below* its start before it
            // goes anywhere — `cubic_bezier(0.5, -0.6, 0.5, 1.2)` is the shape
            // every "wind up first" easing has — and a caller sizing a bound from
            // this would otherwise be told the value never leaves [0, 1] from
            // below. The bezier is contained by its control polygon, so the
            // control points bound both directions.
            TimingFunction::CubicBezier(_, y1, _, y2) => {
                let above = (y1.max(*y2) - 1.0).max(0.0);
                let below = (-y1.min(*y2)).max(0.0);
                above.max(below)
            }
            TimingFunction::Spring(config) => config.peak_overshoot(),
            TimingFunction::Custom(curve) => curve.excursion,
        }
    }

    /// How many points [`custom`](TimingFunction::custom) samples a curve at to
    /// find how far it leaves `[0, 1]`.
    ///
    /// Enough that a bounce or an anticipation is caught at its extreme; a curve
    /// with a spike narrower than a 64th of its duration is not a timing curve.
    const CUSTOM_SAMPLES: usize = 65;

    /// Create a custom timing function from a closure.
    ///
    /// The curve is sampled here, once, to find how far it leaves `[0, 1]` — so
    /// building one costs a construction plus 65 evaluations of
    /// the closure, around half a microsecond for a curve with a trig call in
    /// it. Free where a curve is built once and shared, which is the usual
    /// shape; inside a per-row builder rebuilt on every pass it is worth
    /// hoisting, since `TimingFunction` is `Clone`.
    pub fn custom<F>(f: F) -> Self
    where
        F: Fn(f32) -> f32 + Send + Sync + 'static,
    {
        // Measured once, here, rather than clamped at every evaluation: the
        // curve keeps whatever shape it was written with, and `peak_overshoot`
        // reports what that shape actually does instead of a constant everyone
        // has to be held to.
        let mut excursion = 0.0f32;
        for i in 0..Self::CUSTOM_SAMPLES {
            let v = f(i as f32 / (Self::CUSTOM_SAMPLES - 1) as f32);
            excursion = excursion.max(v - 1.0).max(-v);
        }
        TimingFunction::Custom(CustomCurve {
            f: Arc::new(f),
            excursion: excursion.max(0.0),
        })
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
            TimingFunction::Custom(curve) => write!(f, "Custom(±{})", curve.excursion),
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

#[cfg(test)]
mod overshoot_bound_tests {
    use super::*;

    /// A custom curve keeps the shape it was written with, and reports what that
    /// shape actually does. Anything sized from that number — the damage rect an
    /// elevation shadow needs, above all — would otherwise be measured for less
    /// than what is drawn; clamping the curve instead would have made the bound
    /// true by shortening every hand-rolled bounce, silently.
    #[test]
    fn a_custom_curve_is_measured_rather_than_clamped() {
        // Overshoots at the end and anticipates at the start: a bound that holds
        // only above 1 is not a bound, and every "wind up first" easing dips
        // below 0.
        let curve = TimingFunction::custom(|t| t * 2.0 - 0.5);

        assert_eq!(curve.evaluate(1.0), 1.5, "the curve is left alone");
        assert_eq!(curve.evaluate(0.0), -0.5);

        // …and its bound is the larger of the two excursions, measured.
        assert!((curve.peak_overshoot() - 0.5).abs() < 1e-5);

        for step in 0..=100 {
            let t = step as f32 / 100.0;
            let v = curve.evaluate(t);
            let bound = curve.peak_overshoot();
            assert!(
                v <= 1.0 + bound + 1e-4 && v >= -bound - 1e-4,
                "t = {t} evaluated to {v}, outside the bound it reports"
            );
        }
    }

    /// An ordinary custom ease reports nothing, so it costs nothing downstream.
    #[test]
    fn a_custom_curve_that_only_eases_reports_no_excursion() {
        let eased = TimingFunction::custom(|t| t * t);
        assert_eq!(eased.peak_overshoot(), 0.0);
    }

    /// The curves that only ease report nothing, and a bezier reports its own
    /// control points.
    #[test]
    fn the_ordinary_curves_report_what_they_do() {
        assert_eq!(TimingFunction::Linear.peak_overshoot(), 0.0);
        assert_eq!(TimingFunction::EaseInOut.peak_overshoot(), 0.0);
        assert!(
            (TimingFunction::CubicBezier(0.34, 1.56, 0.64, 1.0).peak_overshoot() - 0.56).abs()
                < 1e-5
        );
        assert_eq!(
            TimingFunction::CubicBezier(0.4, 0.0, 0.6, 1.0).peak_overshoot(),
            0.0
        );
        // Anticipation: it dips to -0.6 before it rises to 1.2, and the larger
        // excursion is the one that bounds it.
        assert!(
            (TimingFunction::CubicBezier(0.5, -0.6, 0.5, 1.2).peak_overshoot() - 0.6).abs() < 1e-5
        );
    }
}
