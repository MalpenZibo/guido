use std::time::Instant;

use crate::animation::{Animatable, SpringState, Transition, TransitionConfig};

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
    /// Target value from Signal
    target: T,
    /// Value when animation started
    start: T,
    /// Progress from 0.0 to 1.0 (or beyond for overshoot)
    progress: f32,
    /// Time when animation started
    start_time: Instant,
    /// Forward transition (used when value increases or no reverse is set)
    transition: Transition,
    /// Optional reverse transition (used when value decreases)
    reverse_transition: Option<Transition>,
    /// Whether the current animation is using the reverse transition
    using_reverse: bool,
    /// Spring state (for spring timing functions)
    spring_state: Option<SpringState>,
    /// Whether the animation has been initialized with its first real value
    initialized: bool,
    /// Previous value for change detection
    prev_value: Option<T>,
}

impl<T: Animatable> AnimationState<T> {
    pub fn new(initial_value: T, config: impl Into<TransitionConfig>) -> Self {
        let config = config.into();
        let spring_state = if matches!(
            config.forward.timing,
            crate::animation::TimingFunction::Spring(_)
        ) {
            Some(SpringState::new())
        } else {
            None
        };
        Self {
            current: initial_value,
            target: initial_value,
            start: initial_value,
            progress: 1.0, // Start completed
            start_time: Instant::now(),
            transition: config.forward,
            reverse_transition: config.reverse,
            using_reverse: false,
            spring_state,
            initialized: false, // Not yet initialized with real content-based value
            prev_value: None,
        }
    }

    /// Get the currently active transition (forward or reverse).
    fn active_transition(&self) -> &Transition {
        if self.using_reverse {
            self.reverse_transition.as_ref().unwrap_or(&self.transition)
        } else {
            &self.transition
        }
    }

    /// Start animating to a new target value
    pub fn animate_to(&mut self, new_target: T) {
        // Don't restart if we're already animating to this target
        if new_target == self.target {
            return;
        }

        // Detect direction and select transition
        self.using_reverse =
            self.reverse_transition.is_some() && T::is_reverse(&self.current, &new_target);

        // Check if active transition is spring before mutating other fields
        let is_spring = matches!(
            self.active_transition().timing,
            crate::animation::TimingFunction::Spring(_)
        );

        self.start = self.current;
        self.target = new_target;
        self.progress = 0.0;
        self.start_time = Instant::now();
        self.spring_state = if is_spring {
            Some(SpringState::new())
        } else {
            None
        };
    }

    /// Advance the animation and return whether the value changed
    pub fn advance(&mut self) -> AdvanceResult<T> {
        if self.progress >= 1.0 && self.spring_state.is_none() {
            return AdvanceResult::NoChange;
        }

        // Extract scalar transition values upfront to avoid borrow conflicts
        // with self.spring_state. Copy SpringConfig (which is Copy) instead of
        // cloning the entire TimingFunction (which may contain an Arc).
        let active = self.active_transition();
        let delay_ms = active.delay_ms;
        let duration_ms = active.duration_ms;
        let spring_config = match active.timing {
            crate::animation::TimingFunction::Spring(config) => Some(config),
            _ => None,
        };

        let elapsed = self.start_time.elapsed().as_secs_f32() * 1000.0; // Convert to ms
        let adjusted_elapsed = (elapsed - delay_ms).max(0.0);

        if adjusted_elapsed <= 0.0 {
            // Still in delay period
            return AdvanceResult::NoChange;
        }

        // Calculate eased value based on timing function type
        let eased_t = if let Some(ref mut spring_state) = self.spring_state {
            // For spring animations: use real elapsed time in seconds (not normalized)
            // This allows the spring to continue oscillating until it naturally settles
            let elapsed_secs = adjusted_elapsed / 1000.0;
            if let Some(ref config) = spring_config {
                spring_state.step(elapsed_secs, config)
            } else {
                // Fallback: shouldn't happen, but use normalized time
                adjusted_elapsed / duration_ms
            }
        } else {
            // For non-spring animations: use normalized time 0..1
            let t = (adjusted_elapsed / duration_ms).min(1.0);
            // Safe to borrow self again — spring_state mutable borrow ended above
            self.active_transition().timing.evaluate(t)
        };

        // Interpolate
        let mut new_value = T::lerp(&self.start, &self.target, eased_t);

        // Update progress
        if let Some(ref state) = self.spring_state {
            // For spring animations, only mark complete when spring has settled
            if state.is_settled(0.01) {
                self.progress = 1.0;
                // Snap to exact target to avoid floating-point drift.
                // The spring settles within 0.01 of the target, but downstream
                // checks (e.g. Transform::is_translation_only) use much tighter
                // tolerances (1e-6), so the lerped value must be exact.
                new_value = self.target;
            } else {
                // Keep progress < 1.0 to continue animating
                self.progress = 0.5;
            }
        } else {
            // For non-spring animations, use time-based progress
            let t = (adjusted_elapsed / duration_ms).min(1.0);
            self.progress = t;
        }

        // Check if value actually changed
        let changed = self.prev_value.as_ref() != Some(&new_value);
        self.current = new_value;
        self.prev_value = Some(new_value);

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
        self.current = value;
        self.target = value;
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
/// Pushes Animation job with appropriate RequiredJob for continuation.
#[macro_export]
macro_rules! advance_anim {
    // Layout animation: marks needs_layout when value changes
    ($self:expr, $anim:ident, $id:expr, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Layout
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Layout animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $id:expr, $any_animating:expr, layout) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Layout
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Paint animation: push paint job when value changes
    ($self:expr, $anim:ident, $id:expr, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Paint
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
    // Paint animation with target update
    ($self:expr, $anim:ident, $target_expr:expr, $id:expr, $any_animating:expr, paint) => {
        if let Some(ref mut anim) = $self.$anim {
            anim.animate_to($target_expr);
            if anim.is_animating() {
                $any_animating = true;
                let required = if anim.advance().is_changed() {
                    $crate::jobs::RequiredJob::Paint
                } else {
                    $crate::jobs::RequiredJob::None
                };
                $crate::jobs::request_job($id, $crate::jobs::JobRequest::Animation(required));
            }
        }
    };
}

/// Helper to get an animated value or a fallback
#[inline]
pub fn get_animated_value<T: Animatable + Copy>(
    anim: Option<&AnimationState<T>>,
    fallback: impl FnOnce() -> T,
) -> T {
    match anim {
        Some(a) => *a.current(),
        None => fallback(),
    }
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

        let value = get_animated_value(Some(&state), || 0.0);
        assert_eq!(value, 42.0);
    }

    #[test]
    fn test_get_animated_value_with_none() {
        let value = get_animated_value::<f32>(None, || 99.0);
        assert_eq!(value, 99.0);
    }
}
