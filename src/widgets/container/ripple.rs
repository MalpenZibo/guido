use std::time::Instant;

use crate::widgets::state_layer::RippleConfig;

/// Ripple animation state for pressed feedback
#[derive(Debug, Clone, Default)]
pub struct RippleState {
    /// Center point of the ripple in local container coordinates (start position)
    pub center: Option<(f32, f32)>,
    /// Exit center point where ripple contracts toward (release position)
    pub exit_center: Option<(f32, f32)>,
    /// Current ripple expansion progress (0.0 = start, 1.0 = fully expanded)
    pub progress: f32,
    /// Current ripple opacity (1.0 = visible, 0.0 = faded out)
    pub opacity: f32,
    /// Whether the ripple is currently fading out (mouse released)
    pub fading: bool,
    /// Time when ripple animation started (for smooth animation)
    pub start_time: Option<Instant>,
    /// Time when ripple fade/contraction started
    pub fade_start_time: Option<Instant>,
    /// Progress at which fading started (for smooth contraction)
    pub fade_start_progress: f32,
}

impl RippleState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a ripple animation at the given screen coordinates
    pub fn start(&mut self, screen_x: f32, screen_y: f32) {
        self.center = Some((screen_x, screen_y));
        self.progress = 0.0;
        self.opacity = 1.0;
        self.fading = false;
        self.start_time = Some(Instant::now());
    }

    /// Start fading the ripple, contracting toward the given exit point
    pub fn start_fade(&mut self, exit_x: f32, exit_y: f32) {
        if self.center.is_some() && self.opacity > 0.0 {
            self.exit_center = Some((exit_x, exit_y));
            self.fading = true;
            self.fade_start_time = Some(Instant::now());
            self.fade_start_progress = self.progress;
        }
    }

    /// Start fading using the center of the container (for MouseLeave events)
    pub fn start_fade_to_center(&mut self, container_width: f32, container_height: f32) {
        if self.center.is_some() && self.opacity > 0.0 {
            self.exit_center = Some((container_width / 2.0, container_height / 2.0));
            self.fading = true;
            self.fade_start_time = Some(Instant::now());
            self.fade_start_progress = self.progress;
        }
    }

    /// Check if ripple is currently active
    pub fn is_active(&self) -> bool {
        self.center.is_some()
    }

    /// Check if ripple is currently animating
    pub fn is_animating(&self) -> bool {
        self.center.is_some() && (self.progress < 1.0 || self.fading)
    }

    /// Reset ripple state
    pub fn reset(&mut self) {
        self.center = None;
        self.exit_center = None;
        self.progress = 0.0;
        self.opacity = 0.0;
        self.fading = false;
        self.start_time = None;
        self.fade_start_time = None;
        self.fade_start_progress = 0.0;
    }

    /// Advance ripple animation, returns true if still animating
    pub fn advance(&mut self, ripple_config: &RippleConfig) -> bool {
        let Some(start_time) = self.start_time else {
            return false;
        };

        let elapsed = start_time.elapsed().as_secs_f32();

        // Expansion animation (0.4 seconds base, modified by expand_speed)
        let expand_duration = 0.4 / ripple_config.expand_speed;

        if self.fading {
            // Reverse animation: contract toward exit point
            let Some(fade_start) = self.fade_start_time else {
                return false;
            };
            let fade_elapsed = fade_start.elapsed().as_secs_f32();
            let fade_duration = 0.3 / ripple_config.fade_speed;

            // Calculate contraction progress (0 = just started fading, 1 = fully contracted)
            let contraction_t = (fade_elapsed / fade_duration).min(1.0);
            // Use ease-in curve for contraction (accelerates as it shrinks)
            let eased_t = contraction_t * contraction_t;

            // Shrink the ripple from its current progress back to 0
            self.progress = self.fade_start_progress * (1.0 - eased_t);

            // Interpolate center from start toward exit point
            if let (Some((start_x, start_y)), Some((exit_x, exit_y))) =
                (self.center, self.exit_center)
            {
                // The effective center moves toward the exit point as it contracts
                let current_x = start_x + (exit_x - start_x) * eased_t;
                let current_y = start_y + (exit_y - start_y) * eased_t;
                self.center = Some((current_x, current_y));
            }

            // Fade opacity as well for smooth disappearance
            self.opacity = (1.0 - eased_t).max(0.0);

            // Clear ripple when fully contracted
            if contraction_t >= 1.0 {
                self.reset();
                return false;
            }
        } else {
            // Expansion animation
            if self.progress < 1.0 {
                self.progress = (elapsed / expand_duration).min(1.0);
                // Use ease-out curve for expansion
                self.progress = 1.0 - (1.0 - self.progress).powi(3);
            }
        }

        // Still animating if expanding or fading
        self.progress < 1.0 || self.fading
    }
}
