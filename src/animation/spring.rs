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
//! ```ignore
//! use guido::animation::{TimingFunction, SpringConfig};
//!
//! container()
//!     .transform(Transform::scale(1.0))
//!     .when_hovered(|s| s
//!         .transform(Transform::scale(1.1))
//!         .timing(TimingFunction::Spring(SpringConfig::BOUNCY)))
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
        if zeta >= 1.0 || zeta <= 0.0 {
            return 0.0;
        }
        (-std::f32::consts::PI * zeta / (1.0 - zeta * zeta).sqrt()).exp()
    }
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

        // Cap individual timestep for numerical stability (~30fps minimum)
        let max_dt = 0.033;
        let capped_dt = dt.min(max_dt);

        // Spring force: F = -k * x
        let displacement = self.position - target;
        let spring_force = -config.stiffness * displacement;

        // Damping force: F = -c * v
        let damping_force = -config.damping * self.velocity;

        // Total force
        let force = spring_force + damping_force;

        // Acceleration: a = F / m
        let acceleration = force / config.mass;

        // Update velocity and position (using semi-implicit Euler)
        self.velocity += acceleration * capped_dt;
        self.position += self.velocity * capped_dt;

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
