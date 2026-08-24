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

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::hash_map::Entry;

use rustc_hash::FxHashMap;

use super::owner::with_root_owner;
use super::runtime::SignalId;
use super::signal::{RwSignal, create_signal};

thread_local! {
    /// Which signal each declared global resolved to, keyed by the address of
    /// its `static` and tagged with the type it was declared as.
    static GLOBALS: RefCell<FxHashMap<usize, (SignalId, TypeId)>> =
        RefCell::new(FxHashMap::default());
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

    /// The signal, creating it if this is its first use — or if the last one
    /// is gone.
    ///
    /// The recreation is what keeps this from being one more thing to remember:
    /// a global whose signal died with a previous `App` answers by making a new
    /// one, so `reset_globals` is a tidying rather than a step the teardown has
    /// to get in the right order. Reading a global while an `App` is being torn
    /// down — a widget's cleanup, a drop — therefore cannot panic.
    pub(crate) fn get(&'static self) -> RwSignal<T> {
        let key = self as *const Self as usize;
        let cached = GLOBALS.with(|globals| globals.borrow().get(&key).copied());
        if let Some((id, declared)) = cached {
            // The address of the `static` is the identity, and nothing enforces
            // that two of them cannot share one — a linker folding identical
            // initialisers would. The type it was declared as is carried
            // alongside so that would be caught here rather than surface as a
            // downcast panic on somebody else's signal.
            debug_assert_eq!(
                declared,
                TypeId::of::<T>(),
                "two globals share an address: one of them is reading the other's signal"
            );
            if crate::reactive::storage::has_signal(id) {
                return RwSignal::from_id(id);
            }
        }
        // Outside the borrow: creating a signal touches the runtime, the owner
        // arena and the storage, and any of them may create a global of its own
        // — including, re-entrantly, this one.
        let signal = with_root_owner(|| create_signal((self.init)()));
        GLOBALS.with(|globals| {
            match globals.borrow_mut().entry(key) {
                // A re-entrant call got there first: it wins, and the signal
                // built here is dropped with the scope that owns it. Two live
                // signals under one name would be worse than one wasted slot.
                Entry::Occupied(e) => RwSignal::from_id(e.get().0),
                Entry::Vacant(e) => {
                    e.insert((signal.id(), TypeId::of::<T>()));
                    signal
                }
            }
        })
    }
}

/// Forget every global's signal.
///
/// Called from `reset_reactive` at `App::drop`. Not load-bearing: a global
/// whose signal is gone builds another on the next read. This keeps the map
/// from carrying one dead entry per global into the next `App`.
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

    /// A global whose signal died with the last `App` builds another rather
    /// than reading into a wiped arena. That is what keeps the teardown from
    /// having an order to get right: `App::drop` disposes the root owner —
    /// which owns every global — and only afterwards clears the tree, running
    /// widget drops that may read one.
    #[test]
    fn a_global_whose_signal_is_gone_makes_another() {
        create_root_owner();
        COUNT.get().set(3);
        assert_eq!(COUNT.get().get_untracked(), 3);

        // What `App::drop` does before it gets to `reset_globals`.
        crate::reactive::storage::reset_storage();

        assert_eq!(
            COUNT.get().get_untracked(),
            7,
            "back to what the declaration says, not a panic"
        );
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
