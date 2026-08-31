mod animatable;
mod animated;
mod keyframes;
mod spring;
mod timing;

pub(crate) use animatable::carry_velocity;
pub use animatable::{Animatable, Channels};
pub(crate) use animated::Motion;
pub use animated::{Animate, Animated, AnimatedMarker, IntoAnimated, Plain};
pub use keyframes::Keyframes;
pub use spring::{SpringConfig, SpringState};
pub use timing::{CustomCurve, TimingFunction};

/// Configuration for how a property should animate when it changes
#[derive(Clone)]
pub struct Transition {
    /// Duration of the animation in milliseconds
    pub duration_ms: f32,
    /// Timing function controlling the animation curve
    pub timing: TimingFunction,
    /// Delay before animation starts in milliseconds
    pub delay_ms: f32,
    /// Called once when an animation driven by this transition settles.
    /// Direction-specific by construction: a callback on the `reverse`
    /// transition fires exactly when the closing animation completes —
    /// the lifecycle hook for "animate out, then destroy".
    pub on_complete: Option<std::rc::Rc<dyn Fn()>>,
}

impl std::fmt::Debug for Transition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transition")
            .field("duration_ms", &self.duration_ms)
            .field("timing", &self.timing)
            .field("delay_ms", &self.delay_ms)
            .field("on_complete", &self.on_complete.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

impl Transition {
    /// Create a new transition with the given duration and timing function
    pub fn new(duration_ms: impl crate::layout::IntoF32, timing: TimingFunction) -> Self {
        Self {
            duration_ms: duration_ms.into_f32(),
            timing,
            delay_ms: 0.0,
            on_complete: None,
        }
    }

    /// Create a spring-based transition with the given configuration
    pub fn spring(config: SpringConfig) -> Self {
        Self {
            duration_ms: 1000.0, // Spring duration is dynamic, this is max
            timing: TimingFunction::Spring(config),
            delay_ms: 0.0,
            on_complete: None,
        }
    }

    /// Run a callback (main thread) when an animation driven by this
    /// transition settles. Fires once per completed run; a retarget that
    /// completes again fires again.
    pub fn on_complete(mut self, f: impl Fn() + 'static) -> Self {
        self.on_complete = Some(std::rc::Rc::new(f));
        self
    }

    /// Set the delay before the animation starts
    pub fn delay(mut self, delay_ms: impl crate::layout::IntoF32) -> Self {
        self.delay_ms = delay_ms.into_f32();
        self
    }

    /// Set the duration of the animation
    pub fn duration(mut self, duration_ms: impl crate::layout::IntoF32) -> Self {
        self.duration_ms = duration_ms.into_f32();
        self
    }

    /// Set the timing function
    pub fn timing(mut self, timing: TimingFunction) -> Self {
        self.timing = timing;
        self
    }

    /// Use a different transition when the animated value decreases (e.g., closing/shrinking).
    ///
    /// For dimensional values like width/height, "reverse" means the value is getting smaller.
    /// This enables patterns like bouncy spring for open + smooth ease-out for close.
    pub fn reverse(self, reverse: Transition) -> TransitionConfig {
        TransitionConfig {
            forward: self,
            reverse: Some(reverse),
        }
    }
}

impl Default for Transition {
    /// Default transition uses spring physics with pleasant overshoot
    fn default() -> Self {
        Self::spring(SpringConfig::DEFAULT)
    }
}

/// Holds a forward transition and an optional reverse transition.
///
/// When both are set, the forward transition is used when the value increases
/// and the reverse transition when it decreases.
#[derive(Clone, Debug)]
pub struct TransitionConfig {
    pub forward: Transition,
    pub reverse: Option<Transition>,
}

impl From<Transition> for TransitionConfig {
    fn from(t: Transition) -> Self {
        TransitionConfig {
            forward: t,
            reverse: None,
        }
    }
}

/// A bare number is a duration in milliseconds, the way
/// [`Keyframes::new`](Keyframes::new) already takes one:
/// `.background(theme.surface.transition(200.0))`.
///
/// The curve it implies is [`EaseOut`](TimingFunction::EaseOut), and *not*
/// [`Transition::default`]'s spring — a spring has no duration for the number
/// to attach to. So the animation vocabulary has two defaults, deliberately:
/// a transition asked for by name springs, and one asked for by duration eases
/// out. `EaseOut` is what this codebase and its documentation reach for when
/// they name a curve at all, by better than two to one.
impl From<f32> for TransitionConfig {
    fn from(duration_ms: f32) -> Self {
        Transition::new(duration_ms, TimingFunction::EaseOut).into()
    }
}

/// The same, written without a decimal point — `.transition(200)`, matching
/// [`Transition::new`], which takes its duration through
/// [`IntoF32`](crate::layout::IntoF32) for exactly this reason.
///
/// Two impls and not `IntoF32`'s four: a blanket one would collide with
/// [`From<Transition>`], and adding `f64` would make the bare `200.0` above
/// ambiguous — an unsuffixed float literal has to have exactly one impl to
/// land on. `f32` and `i32` are what a literal defaults to, which is the whole
/// of what this shorthand is for; a runtime `f64` says
/// `Transition::new(ms, ..)`.
impl From<i32> for TransitionConfig {
    fn from(duration_ms: i32) -> Self {
        Transition::new(duration_ms, TimingFunction::EaseOut).into()
    }
}

/// A spring named on its own: `.transition(SpringConfig::SNAPPY)`, which is
/// what `Transition::spring(SpringConfig::SNAPPY)` says at greater length.
impl From<SpringConfig> for TransitionConfig {
    fn from(config: SpringConfig) -> Self {
        Transition::spring(config).into()
    }
}
