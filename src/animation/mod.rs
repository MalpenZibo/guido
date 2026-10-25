mod animatable;
mod animated;
mod keyframes;
mod spring;
mod timing;

pub(crate) use animatable::carry_velocity;
pub use animatable::{Animatable, Channels};
pub use animated::{Animate, Animated, AnimatedMarker, IntoAnimated, MovesAs, Plain};
pub(crate) use animated::{ExitTo, Motion};
pub use keyframes::{Keyframes, Repeat};
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
    /// Specific to the segment by construction: a transition chosen for the
    /// close carries the callback that fires when the close completes.
    ///
    /// A completion callback, not deferred disposal: a surface or popup that
    /// animates closed keeps itself mounted, runs the transition, and closes
    /// from here. A child of a dynamic list does not need it — declare
    /// [`Animated::exiting_to`] and the container keeps it until it has left.
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
}

impl Default for Transition {
    /// Default transition uses spring physics with pleasant overshoot
    fn default() -> Self {
        Self::spring(SpringConfig::DEFAULT)
    }
}

/// What [`Animate::transition`] takes: how a property chooses the transition
/// for each change.
///
/// Three shapes, and every one is read **when a segment starts** — a
/// retarget, an enter, an exit — and never while it runs:
///
/// - a constant: a [`Transition`], a [`SpringConfig`], or a bare number of
///   milliseconds;
/// - something reactive — a closure, a signal or a memo returning a
///   `Transition` — for the application to say which one plays from its own
///   state: `move || if open.get() { spring() } else { ease() }`;
/// - a closure of the [`Segment`], `|s: Segment<T>| -> Transition`, for what
///   guido knows about the change and the application does not: whether it
///   is an enter, an exit or a retarget, and where it goes from and to — a
///   size the layout measured has a direction no state holds.
///
/// A transition that changes while a segment runs leaves that segment alone
/// and applies from the next one, which is the rule CSS gives a
/// `transition` changed mid-flight.
///
/// The marker generic `M` is [`IntoSignal`](crate::reactive::IntoSignal)'s, and
/// for the same reason: the shapes overlap without it.
pub trait IntoTransition<T, M> {
    fn into_transition(self) -> TransitionSource<T>;
}

/// How a property chooses its transition, as [`IntoTransition`] left it.
/// Opaque: what it holds is the animation's business.
pub struct TransitionSource<T>(pub(crate) Source<T>);

pub(crate) enum Source<T> {
    /// The same transition for every change.
    Fixed(Transition),
    /// Chosen when each segment starts.
    Chosen(Choose<T>),
}

/// How a segment's transition is chosen when it is not a constant.
pub(crate) enum Choose<T> {
    /// Read from the application's state.
    Read(crate::reactive::Signal<Transition>),
    /// Chosen from the segment itself.
    Segment(std::rc::Rc<dyn Fn(Segment<T>) -> Transition>),
}

impl<T> Clone for Choose<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Read(signal) => Self::Read(*signal),
            Self::Segment(choose) => Self::Segment(choose.clone()),
        }
    }
}

impl<T> Clone for TransitionSource<T> {
    fn clone(&self) -> Self {
        Self(match &self.0 {
            Source::Fixed(transition) => Source::Fixed(transition.clone()),
            Source::Chosen(choose) => Source::Chosen(choose.clone()),
        })
    }
}

impl<T> From<Transition> for TransitionSource<T> {
    fn from(transition: Transition) -> Self {
        Self(Source::Fixed(transition))
    }
}

/// A change about to play, as a transition chosen by its segment sees it.
///
/// `from` is where the property is when the change starts, in flight or not —
/// so a grow interrupted by a shrink is a shrink from where it got to — and
/// `to` is where it is going.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment<T> {
    pub from: T,
    pub to: T,
    pub(crate) kind: SegmentKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Retarget,
    Enter,
    Exit,
}

impl<T> Segment<T> {
    /// The widget appearing, from its [`entering_from`](Animated::entering_from).
    ///
    /// Only the first appearance: a row whose exit is cancelled — its key came
    /// back while it was leaving — travels back from where it is, and that is
    /// a [retarget](Self::is_retarget). Asking `is_exit` rather than
    /// `is_enter` gives it the enter's transition.
    pub fn is_enter(&self) -> bool {
        self.kind == SegmentKind::Enter
    }

    /// The widget leaving, to its [`exiting_to`](Animated::exiting_to).
    pub fn is_exit(&self) -> bool {
        self.kind == SegmentKind::Exit
    }

    /// The declared value moving: neither an enter nor an exit. A cancelled
    /// exit going back, and the hand-back after a timeline, are retargets too.
    pub fn is_retarget(&self) -> bool {
        self.kind == SegmentKind::Retarget
    }
}

#[doc(hidden)]
pub struct Shorthand;
#[doc(hidden)]
pub struct Reads<M>(std::marker::PhantomData<M>);
#[doc(hidden)]
pub struct BySegment;

/// A transition, a signal of one or a closure returning one. A constant costs
/// a constant: it is never read again.
impl<T, M, S: crate::reactive::IntoSignal<Transition, M>> IntoTransition<T, Reads<M>> for S {
    fn into_transition(self) -> TransitionSource<T> {
        match self.into_prop() {
            crate::reactive::Prop::Const(transition) => transition.into(),
            crate::reactive::Prop::Reactive(signal) => {
                TransitionSource(Source::Chosen(Choose::Read(signal)))
            }
            crate::reactive::Prop::Unset => {
                unreachable!("`into_prop` always carries a value")
            }
        }
    }
}

impl<T, F: Fn(Segment<T>) -> Transition + 'static> IntoTransition<T, BySegment> for F {
    fn into_transition(self) -> TransitionSource<T> {
        TransitionSource(Source::Chosen(Choose::Segment(std::rc::Rc::new(self))))
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
impl<T> IntoTransition<T, Shorthand> for f32 {
    fn into_transition(self) -> TransitionSource<T> {
        Transition::new(self, TimingFunction::EaseOut).into()
    }
}

/// The same, written without a decimal point — `.transition(200)`, matching
/// [`Transition::new`], which takes its duration through
/// [`IntoF32`](crate::layout::IntoF32) for exactly this reason.
///
/// Two impls and not `IntoF32`'s four: adding `f64` would make the bare
/// `200.0` above ambiguous — an unsuffixed float literal has to have exactly
/// one impl to land on. `f32` and `i32` are what a literal defaults to, which
/// is the whole of what this shorthand is for; a runtime `f64` says
/// `Transition::new(ms, ..)`.
impl<T> IntoTransition<T, Shorthand> for i32 {
    fn into_transition(self) -> TransitionSource<T> {
        Transition::new(self, TimingFunction::EaseOut).into()
    }
}

/// A spring named on its own: `.transition(SpringConfig::SNAPPY)`, which is
/// what `Transition::spring(SpringConfig::SNAPPY)` says at greater length.
impl<T> IntoTransition<T, Shorthand> for SpringConfig {
    fn into_transition(self) -> TransitionSource<T> {
        Transition::spring(self).into()
    }
}
