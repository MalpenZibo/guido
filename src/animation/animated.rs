//! How a value moves, carried by the value itself.
//!
//! Every property that survives to paint takes a signal as readily as a value.
//! This is that sentence extended to time: the *how* rides with the *what*, so
//! a timing cannot name a property it does not decorate and cannot be declared
//! for a property that was never set.
//!
//! Two verbs, on everything a property setter already accepts — a value, a
//! closure, a signal:
//!
//! ```ignore
//! container()
//!     .background(theme.surface.transition(200.0))
//!     .width((move || if open.get() { 520.0 } else { 120.0 }).transition(SpringConfig::SNAPPY))
//!     .rotate(0.0.timeline(shake(), rejections))
//! ```
//!
//! [`Animated`] is deliberately **not** an [`IntoSignal`]. A state layer
//! supplies values for a property somebody else declared, so it has no timing
//! to give — and because the two traits are separate, saying otherwise is a
//! compile error rather than a value quietly ignored:
//!
//! ```ignore
//! .when_hovered(|s| s.background(HOT.transition(900.0)))   // does not compile
//! ```

use crate::reactive::{IntoSignal, Signal};

use super::{Animatable, Keyframes, TransitionConfig};

/// A value, and how it moves when it changes.
///
/// Built by [`Animate::transition`] or [`Animate::timeline`] on anything that
/// can already be a property, and accepted wherever a `Container` setter
/// declares an animatable property.
pub struct Animated<T> {
    signal: Signal<T>,
    /// Boxed and absent by default, for the reason `AnimationState` boxes its
    /// `Timeline`: every animatable setter routes through here, and almost
    /// every call declares no motion at all. A `Motion` owns a `Keyframes` and
    /// a `TransitionConfig` with two callback slots, so inline it would make
    /// `container().background(RED)` construct, move and drop a hundred and
    /// more bytes to say "nothing".
    motion: Option<Box<Motion<T>>>,
}

impl<T> Animated<T> {
    /// The value and its motion, taken apart by the setter that installs them.
    pub(crate) fn into_parts(self) -> (Signal<T>, Option<Box<Motion<T>>>) {
        (self.signal, self.motion)
    }

    /// The value and the timing it eases with, for a property whose declared
    /// type is not the type it animates: a width and a height say how the box
    /// is sized with a `Length` and move an `f32`.
    ///
    /// A timeline cannot cross that gap, and cannot be written across it
    /// either — [`Animate::timeline`] requires `T: Animatable` and
    /// [`Keyframes`] has no other constructor, so a `Keyframes<Length>` does
    /// not exist. That bound is what makes this a narrowing rather than a
    /// value quietly dropped, and relaxing it would have to bring a spelling
    /// for a timeline on a size with it.
    pub(crate) fn into_eased(self) -> (Signal<T>, Option<TransitionConfig>) {
        let ease = match self.motion.map(|motion| *motion) {
            Some(Motion::Ease(config)) => Some(config),
            None => None,
            // Loud rather than `None`, because the thing that makes this
            // unreachable is a bound three types away: silently dropping a
            // timeline here is the defect this whole shape exists to remove,
            // and the day somebody relaxes that bound is the day this has to
            // say so.
            Some(Motion::Play { .. }) => {
                unreachable!("a timeline needs T: Animatable, which this T is not")
            }
        };
        (self.signal, ease)
    }
}

/// The two ways a value can move. An implementation detail: `motion` already
/// means a pointer event here and scrolling calls its own `momentum`, so a
/// public name about easing would sit between two unrelated meanings.
pub(crate) enum Motion<T> {
    /// Ease to each new value instead of jumping to it.
    Ease(TransitionConfig),
    /// Play a sequence whenever the trigger changes, and rest on the declared
    /// value in between.
    Play {
        keyframes: Keyframes<T>,
        plays: Signal<u32>,
    },
}

/// The two verbs, on everything [`IntoSignal`] accepts.
///
/// There is no separate spelling for closures: the parentheses in
/// `(move || …).transition(..)` are Rust's rule for calling a method on a
/// closure literal, not a second API. The marker generic `M` is the same one
/// `IntoSignal` carries, and for the same reason — a blanket impl over `T` and
/// one over `FnMut() -> T` would overlap without it.
pub trait Animate<T: Clone + 'static, M>: IntoSignal<T, M> + Sized {
    /// Ease to each new value this holds, instead of jumping to it.
    ///
    /// A bare number is milliseconds; a [`Transition`](super::Transition) or a
    /// [`SpringConfig`](super::SpringConfig) says which curve.
    ///
    /// ```ignore
    /// container().background(theme.surface.transition(200.0))
    /// container().width(w.transition(Transition::spring(SpringConfig::SNAPPY)))
    /// ```
    fn transition(self, transition: impl Into<TransitionConfig>) -> Animated<T> {
        Animated {
            signal: self.into_signal(),
            motion: Some(Box::new(Motion::Ease(transition.into()))),
        }
    }

    /// Play `keyframes` whenever `plays` changes, resting on this value in
    /// between.
    ///
    /// The trigger is a count and not a flag on purpose: a second refusal has
    /// to shake as loudly as the first, and a signal that stays equal notifies
    /// nobody. The count it starts at is whatever it holds when the widget is
    /// built, so nothing plays on the first frame.
    ///
    /// The resting value is required because it is what the property *is*
    /// whenever nothing is playing — a shake returns to where it began and
    /// makes it look redundant, but a sequence that ends somewhere else snaps
    /// back to it, and a sequence resting on a live signal has nowhere else to
    /// put its expression:
    ///
    /// ```ignore
    /// container().rotate(0.0.timeline(shake(), rejections))
    /// container().rotate((move || spin.get() * STEP).timeline(shake(), rejections))
    /// ```
    ///
    /// A timeline is for something that happens and is over. A change that
    /// should persist is a [`transition`](Self::transition).
    ///
    /// The `T: Animatable` bound is load-bearing beyond the `Keyframes` it
    /// names: it is what keeps a timeline off a property whose declared type
    /// is not the type it animates. A width and a height say how the box is
    /// sized with a `Length` and move an `f32`, and a `Length` is not
    /// animatable — so `width(w.timeline(..))` does not compile, rather than
    /// compiling and playing nothing.
    fn timeline<M2>(self, keyframes: Keyframes<T>, plays: impl IntoSignal<u32, M2>) -> Animated<T>
    where
        T: Animatable,
    {
        Animated {
            signal: self.into_signal(),
            motion: Some(Box::new(Motion::Play {
                keyframes,
                plays: plays.into_signal(),
            })),
        }
    }
}

impl<T: Clone + 'static, M, S: IntoSignal<T, M>> Animate<T, M> for S {}

#[doc(hidden)]
pub struct AnimatedMarker;

/// The marker for a value arriving with no motion, wrapping the one
/// [`IntoSignal`] picked for it.
///
/// A wrapper rather than `M` passed straight through, because coherence
/// reasons about what *could* be implemented, not only what is: a downstream
/// crate is allowed to write `IntoSignal<Their, AnimatedMarker> for
/// Animated<Their>`, which would make a blanket `IntoAnimated<T, M>` overlap
/// the impl below. Two marker types that cannot unify close that off by
/// construction.
#[doc(hidden)]
pub struct Plain<M>(std::marker::PhantomData<M>);

/// What an animatable property setter takes: everything [`IntoSignal`]
/// accepts, plus a value that carries its own motion.
///
/// The wider bound is what keeps the rule at compile time. `StateStyle`'s
/// setters keep plain `IntoSignal`, so a timing declared on an override — a
/// value for a property somebody else owns — does not compile.
pub trait IntoAnimated<T: Clone + 'static, M> {
    fn into_animated(self) -> Animated<T>;
}

impl<T: Clone + 'static, M, S: IntoSignal<T, M>> IntoAnimated<T, Plain<M>> for S {
    fn into_animated(self) -> Animated<T> {
        Animated {
            signal: self.into_signal(),
            motion: None,
        }
    }
}

impl<T: Clone + 'static> IntoAnimated<T, AnimatedMarker> for Animated<T> {
    fn into_animated(self) -> Animated<T> {
        self
    }
}
