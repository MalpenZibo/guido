use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition};

/// Result of advancing an animation, indicating whether the value changed
#[derive(Debug, Clone, PartialEq)]
pub enum AdvanceResult<T> {
    /// Value did not change (animation not running or same value)
    NoChange,
    /// Value changed to a new value
    Changed(T),
}

impl<T> AdvanceResult<T> {
    /// Returns true if the value changed
    pub fn is_changed(&self) -> bool {
        matches!(self, AdvanceResult::Changed(_))
    }
}

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
    /// Previous value for change detection
    prev_value: Option<T>,
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
            prev_value: None,
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

    /// Advance the animation and return whether the value changed
    pub fn advance(&mut self) -> AdvanceResult<T> {
        if self.progress >= 1.0 && self.spring_state.is_none() {
            return AdvanceResult::NoChange;
        }

        let elapsed = self.start_time.elapsed().as_secs_f32() * 1000.0; // Convert to ms
        let adjusted_elapsed = (elapsed - self.transition.delay_ms).max(0.0);

        if adjusted_elapsed <= 0.0 {
            // Still in delay period
            return AdvanceResult::NoChange;
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
        let new_value = T::lerp(&self.start, &self.target, eased_t);

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

        // Check if value actually changed
        let changed = self.prev_value.as_ref() != Some(&new_value);
        self.current = new_value.clone();
        self.prev_value = Some(new_value.clone());

        if changed {
            AdvanceResult::Changed(new_value)
        } else {
            AdvanceResult::NoChange
        }
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

/// Macro to advance an animation field, optionally updating its target first.
/// Uses AdvanceResult to determine when to mark dirty flags.
#[macro_export]
macro_rules! advance_anim {
    // Layout animation: marks needs_layout when value changes
    ($self:expr, $anim:ident, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                if anim.advance().is_changed() {
                    $crate::jobs::push_job($self.widget_id, $crate::jobs::JobType::Layout);
                }
                $any_animating = true;
            }
        }
    };
    // Layout animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                if anim.advance().is_changed() {
                    $crate::jobs::push_job($self.widget_id, $crate::jobs::JobType::Layout);
                }
                $any_animating = true;
            }
        }
    };
    // Paint animation: push paint job when value changes
    ($self:expr, $anim:ident, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                if anim.advance().is_changed() {
                    $crate::reactive::invalidation::push_job(
                        $self.widget_id,
                        $crate::reactive::invalidation::JobType::Paint,
                    );
                    $crate::reactive::request_frame();
                }
                $any_animating = true;
            }
        }
    };
    // Paint animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                if anim.advance().is_changed() {
                    $crate::jobs::push_job($self.widget_id, $crate::jobs::JobType::Paint);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::TimingFunction;

    #[test]
    fn test_animation_state_new() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let state = AnimationState::new(0.0f32, transition);

        assert_eq!(*state.current(), 0.0);
        assert_eq!(*state.target(), 0.0);
        assert!(!state.is_animating()); // Starts completed
        assert!(state.is_initial()); // Not yet initialized
    }

    #[test]
    fn test_animation_state_animate_to() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.animate_to(100.0);

        assert_eq!(*state.target(), 100.0);
        assert!(state.is_animating());
    }

    #[test]
    fn test_animation_state_animate_to_same_target() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.animate_to(100.0);
        let first_start_time = state.start_time;

        // Animate to same target should not restart
        state.animate_to(100.0);
        assert_eq!(state.start_time, first_start_time);
    }

    #[test]
    fn test_animation_state_set_immediate() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        state.set_immediate(50.0);

        assert_eq!(*state.current(), 50.0);
        assert_eq!(*state.target(), 50.0);
        assert!(!state.is_animating());
        assert!(!state.is_initial()); // Now initialized
    }

    #[test]
    fn test_animation_state_is_initial() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(0.0f32, transition);

        assert!(state.is_initial());

        state.set_immediate(10.0);
        assert!(!state.is_initial());
    }

    #[test]
    fn test_get_animated_value_with_some() {
        let transition = Transition::new(300.0, TimingFunction::Linear);
        let mut state = AnimationState::new(42.0f32, transition);
        state.set_immediate(42.0);
        let opt = Some(state);

        let value = get_animated_value(&opt, || 0.0);
        assert_eq!(value, 42.0);
    }

    #[test]
    fn test_get_animated_value_with_none() {
        let opt: Option<AnimationState<f32>> = None;

        let value = get_animated_value(&opt, || 99.0);
        assert_eq!(value, 99.0);
    }
}
