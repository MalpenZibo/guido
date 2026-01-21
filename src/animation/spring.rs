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
    /// Default spring with pleasant overshoot
    pub const DEFAULT: Self = Self {
        mass: 1.0,
        stiffness: 180.0,
        damping: 11.0,
    };

    /// Bouncy spring with more overshoot
    pub const BOUNCY: Self = Self {
        mass: 1.0,
        stiffness: 200.0,
        damping: 10.0,
    };

    /// Snappy spring with quick response
    pub const SNAPPY: Self = Self {
        mass: 1.0,
        stiffness: 250.0,
        damping: 14.0,
    };

    /// Gentle spring with subtle motion
    pub const GENTLE: Self = Self {
        mass: 1.0,
        stiffness: 120.0,
        damping: 15.0,
    };
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
    /// Create a new spring state starting at position 0.0
    pub fn new() -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
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
