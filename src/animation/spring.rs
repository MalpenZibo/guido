//! Spring physics animation system.
//!
//! Provides physically-based spring animations that can overshoot and oscillate
//! before settling, creating natural-feeling motion.
//!
//! ## Spring Parameters
//!
//! - **Mass**: Higher mass = slower response, more momentum
//! - **Stiffness**: Higher stiffness = faster, snappier motion
//! - **Damping**: Higher damping = less oscillation, faster settling
//!
//! ## Presets
//!
//! - [`SpringConfig::DEFAULT`] - Fluid motion with moderate overshoot
//! - [`SpringConfig::BOUNCY`] - Noticeable overshoot and oscillation
//! - [`SpringConfig::SNAPPY`] - Quick response with subtle overshoot
//! - [`SpringConfig::GENTLE`] - Slow, smooth motion with minimal overshoot
//!
//! ## Usage
//!
//! A spring is a timing, and a timing rides with the value it moves — a state
//! layer supplies the value to move *to* and says nothing about how:
//!
//! ```no_run
//! use guido::prelude::*;
//! use guido::animation::SpringConfig;
//!
//! container()
//!     .scale(Scale::NONE.transition(SpringConfig::BOUNCY))
//!     .when_hovered(|s| s.scale(1.1));
//! ```

/// Configuration for spring physics animation
#[derive(Clone, Copy, Debug)]
pub struct SpringConfig {
    /// Mass of the spring (default: 1.0)
    pub mass: f32,
    /// Stiffness of the spring (default: 100.0)
    pub stiffness: f32,
    /// Damping coefficient (default: 10.0)
    pub damping: f32,
}

impl SpringConfig {
    /// Default spring - fluid motion with moderate overshoot.
    /// Settles in ~270ms (damping ratio 0.66, ~6% overshoot).
    pub const DEFAULT: Self = Self {
        mass: 1.0,
        stiffness: 500.0,
        damping: 29.5,
    };

    /// Bouncy spring with noticeable overshoot.
    /// Settles in ~410ms (damping ratio 0.49, ~17% overshoot).
    pub const BOUNCY: Self = Self {
        mass: 1.0,
        stiffness: 400.0,
        damping: 19.6,
    };

    /// Snappy spring - quickest response, subtle overshoot.
    /// Settles in ~185ms (damping ratio 0.72, ~4% overshoot).
    pub const SNAPPY: Self = Self {
        mass: 1.0,
        stiffness: 900.0,
        damping: 43.0,
    };

    /// Gentle spring - slower and smooth, minimal overshoot.
    /// Settles in ~325ms (damping ratio 0.78, ~3% overshoot).
    pub const GENTLE: Self = Self {
        mass: 1.0,
        stiffness: 250.0,
        damping: 24.7,
    };

    /// How far past its target this spring goes at the peak, as a fraction of
    /// the distance travelled.
    ///
    /// The standard result for a second-order system:
    /// `exp(-πζ / sqrt(1 - ζ²))` for a damping ratio `ζ = c / (2·sqrt(m·k))`
    /// below 1, and nothing at all at or above it.
    ///
    /// Anything sizing a bound from an animation's *target* needs this, because
    /// a spring does not stop there — a damage rect being the case that found
    /// it, since the shadow at the peak falls outside a rect measured for the
    /// resting value.
    pub fn peak_overshoot(&self) -> f32 {
        let denominator = 2.0 * (self.mass * self.stiffness).sqrt();
        if denominator <= 0.0 {
            return 0.0;
        }
        let zeta = self.damping / denominator;
        if !zeta.is_finite() {
            // A negative mass or stiffness makes the ratio NaN, and NaN fails
            // every comparison below on its way to `exp`. A NaN here is not a
            // number that stays here: it multiplies a damage reach, which sizes
            // a rect, and `f32::min` hands back the *other* operand — so the
            // clamp that reads the reach would quietly stop clamping too.
            return 0.0;
        }
        if zeta < 0.0 {
            // Negative damping is not a spring, it is an explosion. Nothing
            // bounds it, so it reports nothing rather than a number a caller
            // would size a rect from.
            return 0.0;
        }
        if zeta == 0.0 {
            // Undamped: it swings the whole way past its target, forever. The
            // formula below is the limit of exactly this, and reporting zero for
            // the spring that overshoots *most* was the wrong way round.
            return 1.0;
        }
        if zeta >= 1.0 {
            return 0.0;
        }
        (-std::f32::consts::PI * zeta / (1.0 - zeta * zeta).sqrt()).exp()
    }
}

/// The longest step the simulation takes, whatever the spring: short enough
/// that the presets follow the same path at any frame rate.
const MAX_SUBSTEP: f32 = 1.0 / 240.0;

/// The most steps one frame takes, so a spring stiffer or more damped than any
/// sensible one costs a bounded amount: past it the steps grow, as they did.
const MAX_SUBSTEPS: f32 = 256.0;

/// The step `h` at which `(2·damping·h + stiffness·h²) / mass` is 1: a quarter
/// of the bound past which a semi-implicit Euler step grows, and short enough
/// that damping alone never turns the velocity round (`damping·h / mass` stays
/// under one half). Written as the root of `stiffness·h² + 2·damping·h = mass`
/// that does not cancel when `damping` dwarfs `stiffness`.
fn stable_step(config: &SpringConfig) -> f32 {
    config.mass
        / (config.damping
            + (config.damping * config.damping + config.stiffness * config.mass).sqrt())
}

/// State for spring physics simulation
#[derive(Clone, Debug)]
pub struct SpringState {
    /// Current position (0.0 = start, 1.0 = target)
    pub position: f32,
    /// Current velocity
    pub velocity: f32,
    /// Last evaluation time
    pub last_t: f32,
}

impl SpringState {
    /// Create a new spring state starting at position 0.0, at rest.
    pub fn new() -> Self {
        Self::moving_at(0.0)
    }

    /// The same, already moving.
    ///
    /// For a spring that inherits the momentum of the one it interrupted:
    /// velocity is in units of the segment per second, so a value of 1.0 is
    /// "crossing the whole distance every second".
    pub fn moving_at(velocity: f32) -> Self {
        Self {
            position: 0.0,
            velocity,
            last_t: 0.0,
        }
    }

    /// Step the spring simulation forward using real elapsed time in seconds.
    /// Unlike normalized time (0.0 to 1.0), this allows the spring to continue
    /// oscillating until it naturally settles, regardless of any duration setting.
    ///
    /// `elapsed_secs` - Total elapsed time since animation started, in seconds
    /// Returns the current position (can overshoot 1.0)
    pub fn step(&mut self, elapsed_secs: f32, config: &SpringConfig) -> f32 {
        // Calculate delta time since last step
        let dt = (elapsed_secs - self.last_t).max(0.0);
        self.last_t = elapsed_secs;

        // Skip if time hasn't advanced
        if dt < 1e-6 {
            return self.position;
        }

        // Target is always 1.0 (we're animating from 0 to 1)
        let target = 1.0;

        // A slow frame's worth of time at most, past the 33 ms of a frame at
        // 30 a second so that rate loses nothing: after a stall the spring
        // takes up where it was rather than leaping the whole way.
        let max_dt = 0.05;
        let capped_dt = dt.min(max_dt);

        // In substeps, each short enough for this spring that damping alone
        // never reverses the velocity and a step never grows the motion: one
        // step a frame made a stiff spring swing across its target at 30
        // frames a second and a stiff, critically damped one run away (#809).
        // A config with no such step gets the 1/240 s step if the bound is
        // NaN, one substep if it is negative, and at most `MAX_SUBSTEPS` if it
        // is next to nothing.
        let longest = stable_step(config).min(MAX_SUBSTEP);
        let substeps = (capped_dt / longest).ceil().clamp(1.0, MAX_SUBSTEPS);
        let h = capped_dt / substeps;

        for _ in 0..substeps as usize {
            // Spring force: F = -k * x
            let displacement = self.position - target;
            let spring_force = -config.stiffness * displacement;

            // Damping force: F = -c * v
            let damping_force = -config.damping * self.velocity;

            // Acceleration: a = F / m
            let acceleration = (spring_force + damping_force) / config.mass;

            // Update velocity and position (using semi-implicit Euler)
            self.velocity += acceleration * h;
            self.position += self.velocity * h;
        }

        // Return current position
        self.position
    }

    /// Check if the spring has settled (position near target, velocity near zero)
    pub fn is_settled(&self, threshold: f32) -> bool {
        (self.position - 1.0).abs() < threshold && self.velocity.abs() < threshold
    }
}

impl Default for SpringState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spring built from a nonsense config must not seed NaN into anything
    /// that sizes a rect from it.
    #[test]
    fn an_impossible_spring_reports_no_overshoot_rather_than_nan() {
        for config in [
            SpringConfig {
                mass: -1.0,
                stiffness: 400.0,
                damping: 20.0,
            },
            SpringConfig {
                mass: 1.0,
                stiffness: -400.0,
                damping: 20.0,
            },
            SpringConfig {
                mass: f32::NAN,
                stiffness: 400.0,
                damping: 20.0,
            },
        ] {
            let overshoot = config.peak_overshoot();
            assert!(
                overshoot.is_finite(),
                "{config:?} reported {overshoot}, which would poison a damage rect"
            );
        }
    }

    #[test]
    fn test_spring_reaches_target() {
        let mut state = SpringState::new();
        let config = SpringConfig::DEFAULT;

        // Simulate spring over 2 seconds with 60fps
        let mut position = 0.0;
        for i in 0..120 {
            let elapsed_secs = i as f32 / 60.0; // 60 fps for 2 seconds
            position = state.step(elapsed_secs, &config);
        }

        // Should be close to target (may overshoot then settle)
        assert!(
            (position - 1.0).abs() < 0.1,
            "Spring should settle near target, got {}",
            position
        );
    }

    #[test]
    fn test_spring_overshoots() {
        let mut state = SpringState::new();
        let config = SpringConfig::BOUNCY;

        let mut max_position: f32 = 0.0;
        for i in 0..120 {
            let elapsed_secs = i as f32 / 60.0; // 60 fps for 2 seconds
            let pos = state.step(elapsed_secs, &config);
            max_position = max_position.max(pos);
        }

        // Bouncy spring should overshoot
        assert!(
            max_position > 1.0,
            "Bouncy spring should overshoot, max was {}",
            max_position
        );
    }
}

#[cfg(test)]
mod overshoot_tests {
    use super::*;

    /// Each preset overshoots by about what its doc comment claims, so the two
    /// cannot drift apart. The tolerance is a point and a half because the
    /// comments round, `GENTLE` being the one that rounds up.
    #[test]
    fn the_presets_overshoot_by_what_they_say() {
        let pairs = [
            (SpringConfig::DEFAULT, 0.06),
            (SpringConfig::BOUNCY, 0.17),
            (SpringConfig::SNAPPY, 0.04),
            (SpringConfig::GENTLE, 0.03),
        ];
        for (config, documented) in pairs {
            let computed = config.peak_overshoot();
            assert!(
                (computed - documented).abs() < 0.015,
                "{config:?} documents {documented} and computes {computed}"
            );
        }
    }

    /// The spring that oscillates most reported the least. Undamped, it swings
    /// the whole way past its target and keeps doing it, so anything sizing a
    /// bound from the target has to be told that.
    #[test]
    fn an_undamped_spring_reports_a_full_overshoot() {
        let undamped = SpringConfig {
            mass: 1.0,
            stiffness: 100.0,
            damping: 0.0,
        };
        assert_eq!(undamped.peak_overshoot(), 1.0);

        // And it is the limit the formula approaches, so nothing jumps at zero.
        let nearly = SpringConfig {
            damping: 0.02,
            ..undamped
        };
        assert!(nearly.peak_overshoot() > 0.99);
    }

    /// A spring that starts already moving goes further than one released from
    /// rest, so the step-response figure `peak_overshoot` reports is *not* a
    /// bound for a retargeted animation — `retarget` restarts the spring with
    /// the velocity it was carrying.
    ///
    /// This is why anything sizing a rect from that figure has to clamp what it
    /// draws to it rather than assume the value stays inside: see
    /// `Container::animated_shadow`.
    #[test]
    fn a_spring_given_velocity_overshoots_more_than_one_at_rest() {
        let config = SpringConfig::BOUNCY;
        let reported = 1.0 + config.peak_overshoot();

        let peak_of = |initial_velocity: f32| {
            let mut state = SpringState::moving_at(initial_velocity);
            let mut elapsed = 0.0f32;
            let mut peak = 0.0f32;
            // Finely enough to catch the peak rather than a point beside it.
            for _ in 0..4000 {
                elapsed += 1.0 / 480.0;
                peak = peak.max(state.step(elapsed, &config));
            }
            peak
        };

        assert!(peak_of(0.0) <= reported + 0.01, "from rest it holds");

        let carried = peak_of(16.0);
        assert!(
            carried > reported + 0.05,
            "and carrying velocity it does not: {carried} against {reported}"
        );
    }

    /// A critically damped or overdamped spring never passes its target.
    #[test]
    fn a_spring_that_cannot_bounce_reports_nothing() {
        let critical = SpringConfig {
            mass: 1.0,
            stiffness: 100.0,
            damping: 20.0,
        };
        assert_eq!(critical.peak_overshoot(), 0.0);

        let overdamped = SpringConfig {
            mass: 1.0,
            stiffness: 100.0,
            damping: 40.0,
        };
        assert_eq!(overdamped.peak_overshoot(), 0.0);
    }

    /// Where a spring is at `elapsed` seconds, sampled `rate` times a second
    /// from rest.
    fn path(config: SpringConfig, rate: f32, seconds: f32) -> Vec<(f32, f32)> {
        let mut state = SpringState::new();
        (1..=(seconds * rate) as usize)
            .map(|i| {
                let elapsed = i as f32 / rate;
                (elapsed, state.step(elapsed, &config))
            })
            .collect()
    }

    /// A spring follows the same path whatever the frame rate: sampled at 30
    /// frames a second it is where it is at 240 at the same instants. One step
    /// a frame made `SNAPPY` swing from side to side of its target at 30
    /// (#809).
    #[test]
    fn a_spring_follows_its_path_at_any_frame_rate() {
        for config in [
            SpringConfig::DEFAULT,
            SpringConfig::BOUNCY,
            SpringConfig::SNAPPY,
            SpringConfig::GENTLE,
        ] {
            let fine = path(config, 240.0, 1.0);
            for (elapsed, at) in path(config, 30.0, 1.0) {
                let (_, there) = fine[(elapsed * 240.0).round() as usize - 1];
                assert!(
                    (at - there).abs() < 0.02,
                    "{config:?} at {elapsed}s: {at} at 30 frames a second, {there} at 240"
                );
            }
        }
    }

    /// The step every substep stays within is the one at which
    /// `(2·damping·h + stiffness·h²) / mass` is 1, for springs soft and stiff,
    /// light and heavy, barely and heavily damped.
    #[test]
    fn the_stable_step_is_where_the_bound_is_one() {
        for config in [
            SpringConfig::DEFAULT,
            SpringConfig::SNAPPY,
            SpringConfig {
                mass: 2.0,
                stiffness: 100_000.0,
                damping: 30.0,
            },
            SpringConfig {
                mass: 0.5,
                stiffness: 400.0,
                damping: 1000.0,
            },
        ] {
            let h = stable_step(&config);
            let bound = (2.0 * config.damping * h + config.stiffness * h * h) / config.mass;
            assert!(
                (bound - 1.0).abs() < 1e-4,
                "{config:?}: a step of {h} puts the bound at {bound}"
            );
        }
    }

    /// Only stiffness and damping over mass move a spring: one with all three
    /// doubled follows the same path, here at 30 frames a second.
    #[test]
    fn a_spring_is_moved_by_its_ratios_to_its_mass() {
        let snappy = SpringConfig::SNAPPY;
        let doubled = SpringConfig {
            mass: snappy.mass * 2.0,
            stiffness: snappy.stiffness * 2.0,
            damping: snappy.damping * 2.0,
        };
        for ((elapsed, at), (_, there)) in path(snappy, 30.0, 1.0)
            .into_iter()
            .zip(path(doubled, 30.0, 1.0))
        {
            assert!(
                (at - there).abs() < 1e-4,
                "at {elapsed}s: {at}, and {there} with everything doubled"
            );
        }
    }

    /// `SNAPPY` at 30 frames a second settles within twice the ~185 ms its doc
    /// gives, where one step a frame took two seconds (#809).
    #[test]
    fn snappy_settles_promptly_at_30_frames() {
        let mut state = SpringState::new();
        let settled = (1..=90).map(|i| i as f32 / 30.0).find(|&elapsed| {
            state.step(elapsed, &SpringConfig::SNAPPY);
            state.is_settled(0.01)
        });
        assert!(
            settled.is_some_and(|at| at <= 0.37),
            "settled at {settled:?}"
        );
    }

    /// A spring damped at or past critical never passes its target, and never
    /// runs away, at 30 frames a second either. One step a frame sent a stiff
    /// critically damped spring to infinity (#809).
    #[test]
    fn a_spring_that_cannot_bounce_stays_short_of_its_target_at_30_frames() {
        for config in [
            SpringConfig {
                mass: 1.0,
                stiffness: 900.0,
                damping: 60.0,
            },
            SpringConfig {
                mass: 1.0,
                stiffness: 400.0,
                damping: 1000.0,
            },
        ] {
            for (elapsed, at) in path(config, 30.0, 3.0) {
                assert!(
                    at.is_finite() && at <= 1.0 + 1e-4,
                    "{config:?} at {elapsed}s is at {at}"
                );
            }
        }
    }

    /// A spring really does go past its target by about what it reports — the
    /// step simulation is the thing being bounded, so it is what is checked.
    #[test]
    fn the_simulation_stays_within_the_reported_peak() {
        for config in [SpringConfig::DEFAULT, SpringConfig::BOUNCY] {
            let mut state = SpringState::new();
            let mut peak = 0.0f32;
            // `step` takes the total elapsed time, not a delta.
            let mut elapsed = 0.0f32;
            for _ in 0..2000 {
                elapsed += 1.0 / 240.0;
                peak = peak.max(state.step(elapsed, &config));
            }
            let bound = 1.0 + config.peak_overshoot();
            assert!(
                peak <= bound + 0.01 && peak > 1.0,
                "{config:?} peaked at {peak}, bound {bound}"
            );
        }
    }
}
