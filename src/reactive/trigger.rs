//! Data-less reactive notifier (Leptos/Floem-style `Trigger`).

use super::signal::{RwSignal, create_signal};

/// A data-less reactive notifier: `notify()` re-runs everything that
/// `track()`ed it, unconditionally.
///
/// Sugar over an `RwSignal<()>` written with [`RwSignal::set_always`] —
/// the trigger-style write with no equality semantics. Use it to push a
/// "something happened" pulse through the reactive graph when there is no
/// value to carry (external data changed, a cache was invalidated):
///
/// ```ignore
/// let refresh = create_trigger();
///
/// create_effect(move || {
///     refresh.track();
///     rebuild_from_external_source();
/// });
///
/// refresh.notify(); // effect re-runs, every time
/// ```
///
/// `Copy` like every signal; owner-scoped like every signal (disposed with
/// the scope that created it). Notifications coalesce per flush like any
/// signal write — event streams that must not lose emissions belong in an
/// async channel, not in the reactive graph.
#[derive(Clone, Copy)]
pub struct Trigger {
    inner: RwSignal<()>,
}

/// Create a [`Trigger`], owned by the current owner scope.
///
/// Free-function constructor matching the rest of the reactive API
/// (`create_signal`, `create_memo`, `create_effect`).
pub fn create_trigger() -> Trigger {
    Trigger {
        inner: create_signal(()),
    }
}

impl Trigger {
    /// Notify every tracker, unconditionally.
    pub fn notify(&self) {
        self.inner.set_always(());
    }

    /// Track this trigger from reactive code (an effect, a widget
    /// closure): the caller re-runs on every [`Trigger::notify`].
    pub fn track(&self) {
        self.inner.get();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::create_effect;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn notify_reruns_trackers_every_time() {
        let trigger = create_trigger();
        let runs = Rc::new(Cell::new(0));
        let counter = runs.clone();
        create_effect(move || {
            trigger.track();
            counter.set(counter.get() + 1);
        });
        assert_eq!(runs.get(), 1, "initial run");

        trigger.notify();
        trigger.notify();
        assert_eq!(
            runs.get(),
            3,
            "every notify must re-run the tracker — no equality dedup on ()"
        );
    }
}
