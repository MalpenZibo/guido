//! A signal for a password: reactive like any other, and never copied out.
//!
//! [`RwSignal<String>`](super::RwSignal) is the wrong binding for a password
//! field. Every write drops the previous `String` unwiped inside the reactive
//! storage, and every `get` hands out a copy nobody wipes — so however careful
//! the field is, the password ends up spread across the heap by the thing it is
//! bound to.
//!
//! [`Password`] is the same shape with those two doors shut. It holds a
//! [`Secret`] in the reactive storage, lends it through
//! [`with`](Password::with), and has no `get`: the text is borrowed where it is
//! used, or moved out whole by a submit. Reads subscribe like any signal read,
//! so a button enabled while the field has text is an ordinary closure, and
//! writes notify.
//!
//! GTK 4 draws the same line: an entry's text lives in a buffer the
//! application can create and keep (`gtk_password_entry_buffer_new`), so it
//! can be read or cleared from anywhere, not only from the entry's callbacks.

use super::signal::{RwSignal, new_rw_signal};
use crate::secret::Secret;

/// A password, held by the reactive system and lent, never copied.
///
/// Created by [`create_password`]. `Copy`, like every signal, and owned by
/// the scope that created it.
///
/// ```no_run
/// # use guido::prelude::*;
/// let password = create_password();
/// password.set(Secret::from(String::from("from the keyring")));
///
/// // Reads subscribe: this re-runs as the password fills and empties.
/// let can_sign_in = move || !password.is_empty();
/// # let _ = can_sign_in;
///
/// password.clear(); // wiped in place, e.g. after an idle timeout
/// ```
///
/// An `RwSignal<Secret>` underneath, with every door that would copy the
/// value — `get` above all — left unexposed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Password {
    inner: RwSignal<Secret>,
}

/// Create an empty [`Password`], owned by the current scope.
pub fn create_password() -> Password {
    Password {
        inner: new_rw_signal(Secret::new()),
    }
}

impl Password {
    /// Lend the secret, and subscribe the caller to it.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn with<R>(&self, f: impl FnOnce(&Secret) -> R) -> R {
        self.inner.with(f)
    }

    /// Lend the secret without subscribing — for an event handler, which is
    /// not a tracking scope.
    pub fn with_untracked<R>(&self, f: impl FnOnce(&Secret) -> R) -> R {
        self.inner.with_untracked(f)
    }

    /// Whether the field is empty. Subscribes, like [`with`](Self::with).
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn is_empty(&self) -> bool {
        self.with(Secret::is_empty)
    }

    /// Replace the secret — to pre-fill a field, say. The one it replaces is
    /// wiped as it is dropped.
    pub fn set(&self, secret: Secret) {
        self.inner.update_always(|held| *held = secret);
    }

    /// Wipe the secret in place. Notifies only if there was something to wipe.
    pub fn clear(&self) {
        if !self.with_untracked(Secret::is_empty) {
            self.inner.update_always(Secret::clear);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::create_effect;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn a_reader_follows_it_filling_and_emptying() {
        let password = create_password();
        let seen = Rc::new(Cell::new(None));
        let runs = Rc::new(Cell::new(0));
        let (s, r) = (seen.clone(), runs.clone());
        create_effect(move || {
            s.set(Some(password.is_empty()));
            r.set(r.get() + 1);
        });
        assert_eq!(seen.get(), Some(true));

        password.set(Secret::from(String::from("hunter2")));
        assert_eq!(seen.get(), Some(false), "set did not notify");

        password.clear();
        assert_eq!(seen.get(), Some(true), "clear did not notify");

        let before = runs.get();
        password.clear();
        assert_eq!(runs.get(), before, "clearing an empty password notified");
    }
}
