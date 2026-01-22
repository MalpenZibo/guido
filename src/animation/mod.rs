mod animatable;
mod spring;
mod timing;

pub use animatable::Animatable;
pub use spring::{SpringConfig, SpringState};
pub use timing::TimingFunction;

/// Configuration for how a property should animate when it changes
#[derive(Clone, Debug)]
pub struct Transition {
    /// Duration of the animation in milliseconds
    pub duration_ms: f32,
    /// Timing function controlling the animation curve
    pub timing: TimingFunction,
    /// Delay before animation starts in milliseconds
    pub delay_ms: f32,
}

impl Transition {
    /// Create a new transition with the given duration and timing function
    pub fn new(duration_ms: f32, timing: TimingFunction) -> Self {
        Self {
            duration_ms,
            timing,
            delay_ms: 0.0,
        }
    }

    /// Create a spring-based transition with the given configuration
    pub fn spring(config: SpringConfig) -> Self {
        Self {
            duration_ms: 1000.0, // Spring duration is dynamic, this is max
            timing: TimingFunction::Spring(config),
            delay_ms: 0.0,
        }
    }

    /// Set the delay before the animation starts
    pub fn delay(mut self, delay_ms: f32) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    /// Set the duration of the animation
    pub fn duration(mut self, duration_ms: f32) -> Self {
        self.duration_ms = duration_ms;
        self
    }

    /// Set the timing function
    pub fn timing(mut self, timing: TimingFunction) -> Self {
        self.timing = timing;
        self
    }
}

impl Default for Transition {
    /// Default transition uses spring physics with pleasant overshoot
    fn default() -> Self {
        Self::spring(SpringConfig::DEFAULT)
    }
}
