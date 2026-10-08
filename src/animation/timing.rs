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
//! A curve rides with the value it moves, in the [`Transition`](super::Transition)
//! the property is declared with. A state layer supplies a value and never a
//! timing, so the curve is named once, where the property is:
//!
//! ```no_run
//! # use guido::prelude::*;
//! # let surface = Color::rgb(0.3, 0.3, 0.4);
//! container()
//!     .background(surface.transition(Transition::new(150.0, TimingFunction::EaseOut)))
//!     .when_hovered(|s| s.lighter(0.1));
//! ```

use super::spring::SpringConfig;
use std::cmp::Ordering;
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

/// Cubic bezier curve evaluation, for x1 and x2 in [0, 1].
fn cubic_bezier(t: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let x = Cubic::new(f64::from(x1), f64::from(x2));
    let y = Cubic::new(f64::from(y1), f64::from(y2));
    match x.parameter_at(f64::from(t)) {
        Some((parameter, _)) => y.at(parameter) as f32,
        // A NaN progress or control has no parameter to find.
        None => f32::NAN,
    }
}

/// Newton steps tried before a parameter is left to bisection, as in WebKit's
/// `UnitBezier` and Chromium's `gfx::CubicBezier`.
const NEWTON_STEPS: u32 = 8;

/// One coordinate of a curve from 0 to 1 whose controls are p1 and p2, as the
/// polynomial a·t³ + b·t² + c·t.
#[derive(Clone, Copy)]
struct Cubic {
    a: f64,
    b: f64,
    c: f64,
}

impl Cubic {
    fn new(p1: f64, p2: f64) -> Self {
        let c = 3.0 * p1;
        let b = 3.0 * (p2 - p1) - c;
        Self {
            a: 1.0 - c - b,
            b,
            c,
        }
    }

    fn at(self, t: f64) -> f64 {
        ((self.a * t + self.b) * t + self.c) * t
    }

    fn slope(self, t: f64) -> f64 {
        (3.0 * self.a * t + 2.0 * self.b) * t + self.c
    }

    /// The parameter where this coordinate is `value`, and how many steps it
    /// took to find it; `None` when either is NaN.
    ///
    /// With controls in [0, 1] the coordinate never decreases, so every
    /// evaluation tells which side of it the answer lies and a bracket closes
    /// on it. A Newton step is taken only when it lands strictly inside that
    /// bracket, and the bracket is halved otherwise: an unguarded step at a flat
    /// slope is thrown off the curve (#629). Nothing stops on a tolerance: the
    /// answer is exact, Newton's step no longer moves it, or the bracket's ends
    /// are adjacent floats. Newton converges in a few steps on an ordinary
    /// curve; where it has not after [`NEWTON_STEPS`], the bracket it narrowed
    /// is halved until its ends meet.
    fn parameter_at(self, value: f64) -> Option<(f64, u32)> {
        let (mut low, mut high) = (0.0, 1.0);
        let mut parameter = value;
        for step in 1..=NEWTON_STEPS {
            let residual = self.at(parameter) - value;
            match residual.partial_cmp(&0.0)? {
                Ordering::Less => low = parameter,
                Ordering::Greater => high = parameter,
                Ordering::Equal => return Some((parameter, step)),
            }
            let newton = parameter - residual / self.slope(parameter);
            // Converged: the step no longer moves the parameter.
            if newton == parameter {
                return Some((parameter, step));
            }
            let next = if strictly_between(newton, low, high) {
                newton
            } else {
                0.5 * (low + high)
            };
            if !strictly_between(next, low, high) {
                return Some((parameter, step));
            }
            parameter = next;
        }
        for halving in 1..=f64::MANTISSA_DIGITS {
            let middle = 0.5 * (low + high);
            if !strictly_between(middle, low, high) {
                return Some((low, NEWTON_STEPS + halving));
            }
            match self.at(middle).partial_cmp(&value)? {
                Ordering::Greater => high = middle,
                _ => low = middle,
            }
        }
        Some((low, NEWTON_STEPS + f64::MANTISSA_DIGITS))
    }
}

/// Whether `value` lies inside the open interval, as an ordering rather than a
/// `<` that could as well have been `<=`: at the interval's ends the two differ
/// only in how long a solve takes.
fn strictly_between(value: f64, low: f64, high: f64) -> bool {
    value.partial_cmp(&low) == Some(Ordering::Greater)
        && value.partial_cmp(&high) == Some(Ordering::Less)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezier_samples_agree_with_de_casteljau_inversion() {
        // This oracle bisects too, but evaluates by de Casteljau rather than the
        // solver's polynomial; the analytic inverses below are what share nothing.
        let sample = |u: f64, a: f64, b: f64| {
            let lerp = |p, q| p + (q - p) * u;
            lerp(
                lerp(lerp(0.0, a), lerp(a, b)),
                lerp(lerp(a, b), lerp(b, 1.0)),
            )
        };
        for (x1, y1, x2, y2) in [
            (0.0, 0.0, 0.0, 1.0),
            (1.0, 0.0, 1.0, 1.0),
            (1.0, 0.0, 0.0, 1.0),
            (0.25, 0.1, 0.25, 1.0),
            (0.42, 0.0, 0.58, 1.0),
            (0.34, 1.56, 0.64, 1.0),
            (0.5, -0.6, 0.5, 1.2),
            (0.45, -2.8, 0.92, 0.6),
        ] {
            let curve = TimingFunction::CubicBezier(x1, y1, x2, y2);
            assert_eq!(curve.evaluate(0.0), 0.0);
            assert_eq!(curve.evaluate(1.0), 1.0);
            for step in 1..1000 {
                let progress = step as f32 / 1000.0;
                let (mut low, mut high) = (0.0, 1.0);
                for _ in 0..50 {
                    let u = (low + high) * 0.5;
                    if sample(u, f64::from(x1), f64::from(x2)) < f64::from(progress) {
                        low = u;
                    } else {
                        high = u;
                    }
                }
                let expected = sample((low + high) * 0.5, f64::from(y1), f64::from(y2)) as f32;
                let actual = curve.evaluate(progress);
                assert!(
                    (actual - expected).abs() < 1e-5,
                    "{curve:?} at {progress}: expected {expected}, got {actual}"
                );
            }
        }
    }

    #[test]
    fn bezier_anticipation_and_overshoot_are_not_clamped() {
        assert!(TimingFunction::CubicBezier(0.5, -0.6, 0.5, 1.2).evaluate(0.1) < 0.0);
        assert!(TimingFunction::CubicBezier(0.34, 1.56, 0.64, 1.0).evaluate(0.7) > 1.0);
    }

    #[test]
    fn bezier_nan_inputs_propagate_without_stalling() {
        for (progress, x1, x2) in [
            (f32::NAN, 0.0, 0.0),
            (0.5, f32::NAN, 0.0),
            (0.5, 0.0, f32::NAN),
        ] {
            assert!(
                TimingFunction::CubicBezier(x1, 0.0, x2, 1.0)
                    .evaluate(progress)
                    .is_nan()
            );
        }
    }

    /// A solve's cost is the one thing a wrong slope, a broken Newton step or a
    /// missing bracket guard changes: the bracket still lands every answer on
    /// the curve, only later. So the work is counted, and pinned. A change that
    /// moves these numbers says why.
    #[test]
    fn bezier_solves_take_the_steps_they_are_known_to() {
        let steps = |x1: f32, x2: f32| {
            let x = Cubic::new(f64::from(x1), f64::from(x2));
            (0..=1000).map(move |k| x.parameter_at(f64::from(k as f32 / 1000.0)).unwrap().1)
        };
        // Without a flat stretch, Newton alone converges.
        for (x1, x2) in [(0.25, 0.25), (0.42, 0.58), (0.34, 0.64), (0.5, 0.5)] {
            assert!(
                steps(x1, x2).all(|taken| taken <= NEWTON_STEPS),
                "x controls {x1},{x2} fell back to bisection"
            );
        }
        let curves = [
            (0.0, 0.0),
            (1.0, 1.0),
            (1.0, 0.0),
            (0.25, 0.25),
            (0.42, 0.58),
            (0.34, 0.64),
            (0.5, 0.5),
            (0.45, 0.92),
            (0.0, 1.0),
        ];
        let total: u32 = curves
            .iter()
            .map(|&(x1, x2)| steps(x1, x2).sum::<u32>())
            .sum();
        assert_eq!(total, 56_740);
        // Too near zero for the bracket's ends ever to meet: every halving runs.
        let (_, taken) = Cubic::new(0.0, 0.0)
            .parameter_at(f64::from(1e-30_f32))
            .unwrap();
        assert_eq!(taken, NEWTON_STEPS + f64::MANTISSA_DIGITS);
    }

    #[test]
    fn flat_bezier_slopes_stay_on_the_curve() {
        // With y controls 0 and 1, y(u) = 3u^2 - 2u^3; each x(u) inverts exactly.
        // The f32 neighbours of 0.5, one ulp to either side.
        let beside_half = [
            f32::from_bits(0.5_f32.to_bits() - 1),
            f32::from_bits(0.5_f32.to_bits() + 1),
        ];
        type Inverse = fn(f64) -> f64;
        let cases: [(f32, f32, Inverse, &[f32]); 3] = [
            // x(u) = u^3: flat at the start.
            (0.0, 0.0, f64::cbrt, &[0.001, 0.01, 0.1, 0.5]),
            // x(u) = 1 - (1 - u)^3: flat at the end.
            (
                1.0,
                1.0,
                |p| 1.0 - (1.0 - p).cbrt(),
                &[0.5, 0.9, 0.99, 0.999],
            ),
            // x(u) = 0.5 + 4(u - 0.5)^3: flat in the middle.
            (
                1.0,
                0.0,
                |p| 0.5 + ((p - 0.5) / 4.0).cbrt(),
                &[
                    0.49,
                    0.4999,
                    beside_half[0],
                    0.5,
                    beside_half[1],
                    0.5001,
                    0.51,
                ],
            ),
        ];
        for (x1, x2, inverse, points) in cases {
            let curve = TimingFunction::CubicBezier(x1, 0.0, x2, 1.0);
            for &progress in points {
                let u = inverse(f64::from(progress));
                let expected = (3.0 * u * u - 2.0 * u * u * u) as f32;
                let actual = curve.evaluate(progress);
                assert!(
                    (actual - expected).abs() < 1e-5,
                    "{curve:?} at {progress}: expected {expected}, got {actual}"
                );
            }
        }
    }

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
    /// shape actually does. Anything sized from that number — the damage rect a
    /// shadow needs, above all — would otherwise be measured for less than what
    /// is drawn; clamping the curve instead would have made the bound true by
    /// shortening every hand-rolled bounce, silently.
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
