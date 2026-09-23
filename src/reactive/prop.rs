//! What a declared property holds: a constant, a signal, or nothing.
//!
//! Every styling property used to be an `Option<Signal<T>>`, and a constant
//! went into it through the cheapest signal there is — [`create_stored`], no
//! `RefCell`, no tracking, documented as such. It was still a signal, and a
//! signal costs an arena slot however little it does with one: an `Rc`, a
//! `Slot` in storage, an entry in the owner's disposal list, and an index in
//! `signal_subscribers` that the next tracked signal pads out even though a
//! stored one never registers there.
//!
//! Measured at ~112 bytes per property set, which on a list of twenty thousand
//! rows is 22% of what a row costs (#450). The constant is 16 of those bytes,
//! and the rest is the machinery for reacting to a value that does not change.
//!
//! So a property keeps the constant where the constant is. [`IntoSignal`] has
//! always known which it was given — `ValueMarker` against `ClosureMarker`,
//! decided at the type level before any of this existed — so `into_prop` reads
//! the answer off the marker rather than asking anything at runtime, and the
//! public spelling does not move: `.background(RED)` and `.background(|| red())`
//! differ by exactly what they differed by before.
//!
//! [`create_stored`]: crate::reactive::create_stored
//! [`IntoSignal`]: crate::reactive::IntoSignal

use super::signal::Signal;

/// A property as it was declared.
///
/// The three cases a setter can be handed, kept apart so the constant one
/// costs a constant. Read it with [`get_or`](Prop::get_or) and its siblings,
/// which answer the same questions `Option<Signal<T>>` answered and in the same
/// words.
#[derive(Clone, Copy, PartialEq)]
pub enum Prop<T> {
    /// Nothing was declared, and the reader falls back to its default.
    Unset,
    /// A value that cannot change. No slot, no subscription, no `Rc`.
    Const(T),
    /// A closure, a signal, a writable signal or a memo — anything whose value
    /// a frame has to go and ask for.
    Reactive(Signal<T>),
}

/// Written out rather than derived, because the derive would bound `T:
/// Default` and nothing here needs one: an undeclared property is `Unset`
/// whatever it would have held. `LayerProps` derives `Default` over five
/// of these, and `Translate` having a default is beside the point.
impl<T> Default for Prop<T> {
    fn default() -> Self {
        Prop::Unset
    }
}

impl<T: Clone + 'static> Prop<T> {
    /// The declared value, or `None` where there is none. Subscribes to a
    /// [`Reactive`](Prop::Reactive) one, as `Signal::get` does, and to nothing
    /// for a constant — which is the same answer as before, since a stored
    /// signal never notified anybody either.
    ///
    /// The other readers are this one with a default applied; the matching
    /// lives here and in [`get_untracked`](Prop::get_untracked), and nowhere
    /// else.
    // track_caller so the snapshot diagnostic names the widget code that read
    // the property, not this helper. `get_or` carries it too and so keeps
    // naming its own caller; the untracked pair take no snapshot and need it
    // no more than the trait they replace did.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get(&self) -> Option<T> {
        match self {
            Prop::Unset => None,
            Prop::Const(value) => Some(value.clone()),
            Prop::Reactive(signal) => Some(signal.get()),
        }
    }

    /// The declared value without subscribing, or `None` where there is none.
    ///
    /// For a read consumed inside the pass that makes it.
    pub fn get_untracked(&self) -> Option<T> {
        match self {
            Prop::Unset => None,
            Prop::Const(value) => Some(value.clone()),
            Prop::Reactive(signal) => Some(signal.get_untracked()),
        }
    }

    /// The declared value, or `default` where nothing was declared.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn get_or(&self, default: T) -> T {
        self.get().unwrap_or(default)
    }

    /// The declared value without subscribing, or `default`.
    pub fn get_or_untracked(&self, default: T) -> T {
        self.get_untracked().unwrap_or(default)
    }
}

impl<T> Prop<T> {
    /// This declaration, or `other` where this one says nothing.
    ///
    /// `Option::or`'s own meaning, for the properties that resolve to a single
    /// declaration by taking the nearest one that has anything to say.
    pub fn or(self, other: Self) -> Self {
        match self {
            Prop::Unset => other,
            declared => declared,
        }
    }

    /// Whether anything was declared at all.
    pub fn is_set(&self) -> bool {
        !matches!(self, Prop::Unset)
    }
}
