//! Signals whose owner is the application.
//!
//! A signal belongs to whatever scope is current when `create_signal` runs, and
//! that scope is ambient and time-dependent: the same line inside a widget
//! factory belongs to that surface, inside a click handler to the root, on an
//! effect's first run to whoever created the effect. For state with one instance
//! per process — what the compositor reports, where the keyboard focus is — the
//! owner is none of those. It is the application, and drawing it by lot from
//! whoever reads first is how "signal was disposed" panics get made.
//!
//! A `GlobalSignal` says so at the declaration:
//!
//! ```ignore
//! static MODIFIERS: GlobalSignal<Modifiers> = GlobalSignal::new(Modifiers::default);
//!
//! pub fn keyboard_modifiers() -> Signal<Modifiers> {
//!     MODIFIERS.get().read_only()
//! }
//! ```
//!
//! Created under the root owner on first use — so it outlives every scope that
//! might read it — and forgotten when the `App` is dropped, which the next `App`
//! on this thread needs: `reset_reactive` wipes the signal storage, and an id
//! kept across that points into an arena that no longer exists.
//!
//! The identity is the `static` itself, taken by address, so two globals of the
//! same type are two globals. Nothing has to be registered, listed, or reset by
//! hand.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

use super::owner::with_root_owner;
use super::runtime::SignalId;
use super::signal::{RwSignal, create_signal};

thread_local! {
    /// Which signal each declared global resolved to, keyed by the address of
    /// its `static`. Cleared with the rest of the reactive state, which is the
    /// whole of a global's teardown.
    static GLOBALS: RefCell<FxHashMap<usize, SignalId>> = RefCell::new(FxHashMap::default());
}

/// A signal that lives as long as the `App`.
///
/// Declare it as a `static` and read it through [`get`](GlobalSignal::get);
/// the signal itself is created on first use. See the module docs.
pub(crate) struct GlobalSignal<T: 'static> {
    /// The value the signal starts from. A `fn` pointer rather than a `T` so
    /// the `static` is `Sync` whatever `T` is, and so the value is built when
    /// there is a runtime to build it in.
    init: fn() -> T,
}

impl<T: Clone + Send + 'static> GlobalSignal<T> {
    /// Declare a global signal starting from `init()`.
    pub(crate) const fn new(init: fn() -> T) -> Self {
        Self { init }
    }

    /// The signal, creating it under the root owner if this is its first use.
    pub(crate) fn get(&'static self) -> RwSignal<T> {
        let key = self as *const Self as usize;
        if let Some(id) = GLOBALS.with(|globals| globals.borrow().get(&key).copied()) {
            return RwSignal::from_id(id);
        }
        // Outside the borrow: creating a signal touches the runtime, the owner
        // arena and the storage, and any of them may create a global of its own.
        let signal = with_root_owner(|| create_signal((self.init)()));
        GLOBALS.with(|globals| globals.borrow_mut().insert(key, signal.id()));
        signal
    }
}

/// Forget every global's signal.
///
/// Called from `reset_reactive` at `App::drop`. The signals die with the
/// storage; this drops the ids that pointed at them, so the next `App` on this
/// thread declares them afresh instead of reading into a wiped arena.
pub(crate) fn reset_globals() {
    GLOBALS.with(|globals| globals.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{create_root_owner, dispose_owner_now, with_owner};

    static COUNT: GlobalSignal<u32> = GlobalSignal::new(|| 7);
    static OTHER_COUNT: GlobalSignal<u32> = GlobalSignal::new(|| 100);

    /// The whole point: the first reader does not become the owner.
    #[test]
    fn a_global_outlives_the_scope_that_read_it_first() {
        create_root_owner();
        let ((), first_reader) = with_owner(|| {
            assert_eq!(COUNT.get().get_untracked(), 7);
        });
        dispose_owner_now(first_reader);

        assert_eq!(COUNT.get().get_untracked(), 7, "still readable");
        COUNT.get().set(9);
        assert_eq!(COUNT.get().get_untracked(), 9, "and still writable");
    }

    /// Every read finds the same signal — otherwise a writer and a watcher
    /// would be looking at two different values with the same name.
    #[test]
    fn every_read_of_one_global_is_the_same_signal() {
        create_root_owner();
        assert!(COUNT.get() == COUNT.get());
    }

    /// The identity is the declaration, not the type: two globals holding a
    /// `u32` are two globals.
    #[test]
    fn two_globals_of_one_type_are_two_globals() {
        create_root_owner();
        COUNT.get().set(1);
        OTHER_COUNT.get().set(2);
        assert_eq!(COUNT.get().get_untracked(), 1);
        assert_eq!(OTHER_COUNT.get().get_untracked(), 2);
    }
}
