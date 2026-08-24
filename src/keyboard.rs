//! The keyboard's modifier state, exposed reactively.
//!
//! [`Event::KeyDown`](crate::widgets::Event::KeyDown) already carries the
//! modifiers that were held for *that* keystroke, which is what a shortcut
//! needs. A latched modifier is a different question: caps lock is a state the
//! keyboard is *in*, and something on screen may need to say so before anything
//! is typed at all — a lock screen warning that the password is about to go in
//! upper case is the case this exists for.
//!
//! It cannot be answered from a key event. The compositor sends the modifier
//! update *after* the key that caused it, so the press of caps lock reports the
//! state it had before, and a screen driven from key events would be a
//! keystroke behind — announcing caps lock only once you had already typed
//! into it.
//!
//! ```ignore
//! container().child(move || {
//!     keyboard_modifiers().get().caps_lock.then(|| text("Caps lock is on"))
//! })
//! ```

use crate::reactive::global::GlobalSignal;
use crate::reactive::{RwSignal, Signal};
use crate::widgets::Modifiers;

static MODIFIERS: GlobalSignal<Modifiers> = GlobalSignal::new(Modifiers::default);

fn modifiers_signal() -> RwSignal<Modifiers> {
    MODIFIERS.get()
}

/// Reactive view of the modifiers the keyboard currently reports.
///
/// Everything false until the compositor sends the first modifier update, which
/// it does when a surface takes keyboard focus — so a surface that never has
/// focus reads all-false rather than a stale state from whoever had it.
pub fn keyboard_modifiers() -> Signal<Modifiers> {
    modifiers_signal().read_only()
}

/// Publish a modifier update. Called by the platform layer.
pub(crate) fn set_keyboard_modifiers(modifiers: Modifiers) {
    let signal = modifiers_signal();
    // The compositor re-sends the whole state on every change, including ones
    // that changed nothing we track; writing unconditionally would invalidate
    // every reader of it for a modifier we do not even expose.
    if signal.get_untracked() != modifiers {
        signal.set(modifiers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_starts_empty_and_follows_the_platform() {
        modifiers_signal().set(Modifiers::default());
        assert_eq!(keyboard_modifiers().get_untracked(), Modifiers::default());

        let latched = Modifiers {
            caps_lock: true,
            ..Default::default()
        };
        set_keyboard_modifiers(latched);

        assert_eq!(keyboard_modifiers().get_untracked(), latched);
    }

    #[test]
    fn the_first_reader_does_not_get_to_own_it() {
        // The crash this pins: allock's caps-lock indicator reads this from
        // inside a widget's reactive closure, which on that screen is the first
        // read in the process. The signal was created under the closure's owner,
        // died with it, and the read after that panicked with "signal was
        // disposed" — the thread-local was still holding the dead handle.
        use crate::reactive::owner::{create_root_owner, dispose_owner_now, with_owner};

        create_root_owner();
        modifiers_signal().set(Modifiers::default());

        let (_, scope) = with_owner(|| keyboard_modifiers().get_untracked());
        dispose_owner_now(scope);

        assert_eq!(
            keyboard_modifiers().get_untracked(),
            Modifiers::default(),
            "the state has to outlive whichever scope happened to read it first"
        );
        set_keyboard_modifiers(Modifiers {
            caps_lock: true,
            ..Default::default()
        });
        assert!(keyboard_modifiers().get_untracked().caps_lock);
    }

    /// A second `App` on this thread starts from the declaration, not from
    /// whatever the last one latched. What ends the first one is the storage
    /// being wiped, which is what `App::drop` does — and the global answers by
    /// building another signal rather than reading into the hole.
    #[test]
    fn a_new_app_does_not_inherit_the_last_one_s_state() {
        set_keyboard_modifiers(Modifiers {
            caps_lock: true,
            ..Default::default()
        });
        assert!(keyboard_modifiers().get_untracked().caps_lock);

        crate::reactive::storage::reset_storage();

        assert_eq!(keyboard_modifiers().get_untracked(), Modifiers::default());
    }
}
