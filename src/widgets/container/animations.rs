use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition};

/// Animation state for animatable properties
pub struct AnimationState<T: Animatable> {
    /// Current interpolated value
    current: T,
    /// Target value from MaybeDyn
    target: T,
    /// Value when animation started
    start: T,
    /// Progress from 0.0 to 1.0 (or beyond for overshoot)
    progress: f32,
    /// Time when animation started
    start_time: Instant,
    /// Transition configuration
    transition: Transition,
    /// Spring state (for spring timing functions)
    spring_state: Option<SpringState>,
    /// Whether the animation has been initialized with its first real value
    initialized: bool,
}

impl<T: Animatable> AnimationState<T> {
    pub fn new(initial_value: T, transition: Transition) -> Self {
        let spring_state = if matches!(
            transition.timing,
            crate::animation::TimingFunction::Spring(_)
        ) {
            Some(SpringState::new())
        } else {
            None
        };
        Self {
            current: initial_value.clone(),
            target: initial_value.clone(),
            start: initial_value,
            progress: 1.0, // Start completed
            start_time: Instant::now(),
            transition,
            spring_state,
            initialized: false, // Not yet initialized with real content-based value
        }
    }

    /// Start animating to a new target value
    pub fn animate_to(&mut self, new_target: T) {
        // Don't restart if we're already animating to this target
        if new_target == self.target {
            return;
        }

        self.start = self.current.clone();
        self.target = new_target.clone();
        self.progress = 0.0;
        self.start_time = Instant::now();
        // Reset spring state for new animation
        if self.spring_state.is_some() {
            self.spring_state = Some(SpringState::new());
        }
    }

    /// Advance the animation and return the current value
    pub fn advance(&mut self) -> T {
        if self.progress >= 1.0 && self.spring_state.is_none() {
            return self.current.clone();
        }

        let elapsed = self.start_time.elapsed().as_secs_f32() * 1000.0; // Convert to ms
        let adjusted_elapsed = (elapsed - self.transition.delay_ms).max(0.0);

        if adjusted_elapsed <= 0.0 {
            // Still in delay period
            return self.current.clone();
        }

        // Calculate eased value based on timing function type
        let eased_t = if let Some(ref mut spring_state) = self.spring_state {
            // For spring animations: use real elapsed time in seconds (not normalized)
            // This allows the spring to continue oscillating until it naturally settles
            let elapsed_secs = adjusted_elapsed / 1000.0;
            if let crate::animation::TimingFunction::Spring(ref config) = self.transition.timing {
                spring_state.step(elapsed_secs, config)
            } else {
                // Fallback: shouldn't happen, but use normalized time
                adjusted_elapsed / self.transition.duration_ms
            }
        } else {
            // For non-spring animations: use normalized time 0..1
            let t = (adjusted_elapsed / self.transition.duration_ms).min(1.0);
            self.transition.timing.evaluate(t)
        };

        // Interpolate
        self.current = T::lerp(&self.start, &self.target, eased_t);

        // Update progress
        if let Some(ref state) = self.spring_state {
            // For spring animations, only mark complete when spring has settled
            if state.is_settled(0.01) {
                self.progress = 1.0;
            } else {
                // Keep progress < 1.0 to continue animating
                self.progress = 0.5;
            }
        } else {
            // For non-spring animations, use time-based progress
            let t = (adjusted_elapsed / self.transition.duration_ms).min(1.0);
            self.progress = t;
        }

        self.current.clone()
    }

    /// Check if animation is still running
    pub fn is_animating(&self) -> bool {
        self.progress < 1.0 || (self.spring_state.is_some() && self.progress < 0.99)
    }

    /// Get current value
    pub fn current(&self) -> &T {
        &self.current
    }

    /// Get target value
    pub fn target(&self) -> &T {
        &self.target
    }

    /// Set value immediately without animation (for initialization)
    pub fn set_immediate(&mut self, value: T) {
        self.current = value.clone();
        self.target = value.clone();
        self.start = value;
        self.progress = 1.0;
        self.initialized = true;
    }

    /// Check if animation has never been initialized (first layout)
    pub fn is_initial(&self) -> bool {
        !self.initialized
    }
}

/// Macro to advance an animation field, optionally updating its target first
#[macro_export]
macro_rules! advance_anim {
    // Simple advance (no target update)
    ($self:expr, $anim:ident, $any_animating:expr) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                anim.advance();
                $any_animating = true;
            }
        }
    };
    // With target update
    ($self:expr, $anim:ident, $target_expr:expr, $any_animating:expr) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                anim.advance();
                $any_animating = true;
            }
        }
    };
}

/// Helper to get an animated value or a fallback
#[inline]
pub fn get_animated_value<T: Animatable + Clone>(
    anim: &Option<AnimationState<T>>,
    fallback: impl FnOnce() -> T,
) -> T {
    anim.as_ref()
        .map(|a| a.current().clone())
        .unwrap_or_else(fallback)
}
