//! Running a closure on the UI thread after a delay.
//!
//! `create_task` waits on the service runtime, so what runs after its `sleep`
//! is on another thread and can only *write* — through a `WriteSignal` — never
//! read the state it needs to decide whether to act. A timer waits on the loop
//! instead: its callback runs in the loop's own pass, where signals are read and
//! written like anywhere else on the UI thread, and the loop sleeps until the
//! next one is due rather than parking a thread on it.
//!
//! **A deadline is on the pass's clock.** A timer is created unarmed, and the
//! first pass that sees it arms it at that pass's moment plus its delay. Every
//! deadline is therefore a frame moment compared against a frame moment, so a
//! driver that names the moment a pass runs at — `Headless::step_at` — names
//! which timers fire in it, and a test never sleeps. An unarmed timer is a
//! deadline of *now* to the loop, so the arming pass comes at once without the
//! wakeup that would make every surface draw.
//!
//! **A timer belongs to its scope.** It is cancelled when the owner it was
//! created under is disposed, and its callback runs under that owner, so a
//! widget's timers go with the widget. Each run of an effect owns what it
//! makes, so an effect that schedules one per run cancels the last by running
//! again; that is a debounce. leptos-use (`use_timeout_fn`) and Dioxus
//! (`spawn`) tie theirs to the scope the same way; raw Leptos `set_timeout`
//! does not, and leaves the clearing to the caller.
//!
//! **The handle is a name, not an owner.** [`TimerHandle`] is `Copy` and
//! dropping it does nothing, as Leptos's and Floem's are. Slint stops a timer
//! when its handle drops, so `let _ = Timer::single_shot(..)`-shaped mistakes
//! stop a timer the moment it is made; here only [`TimerHandle::cancel`] and the
//! scope do.
//!
//! The callback is an event, not an effect: what it reads is read untracked, and
//! nothing re-runs it when that changes. It is main-thread API, like the other
//! queues in `deferred`: called from another thread it lands in that thread's
//! state, which no loop drains.

use std::time::{Duration, Instant};

use rustc_hash::FxHashSet;

use crate::app_state::with_app_state;
use crate::reactive::owner::{OwnerId, current_owner, on_cleanup, under_scope};

/// The shortest period an interval runs at. A period of zero would be due again
/// the moment it ran, and the loop would never sleep.
const MIN_PERIOD: Duration = Duration::from_millis(1);

/// A pending timer, by name. `Copy`, and dropping it does nothing: a timer stops
/// when it is cancelled or its scope is disposed, never because a handle went
/// out of scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerHandle(u64);

impl TimerHandle {
    /// Stop the timer. A timeout that has already run, a timer already
    /// cancelled, and one whose scope is gone are all left as they are.
    pub fn cancel(self) {
        let removed = with_app_state(|app| {
            let mut timers = app.timers.borrow_mut();
            let at = timers.entries.iter().position(|timer| timer.id == self.0)?;
            Some(timers.entries.remove(at))
        });
        drop(removed);
    }

    /// Whether it will still run: a timeout that has not run yet, or an
    /// interval that has not been stopped.
    pub fn is_pending(self) -> bool {
        with_app_state(|app| {
            app.timers
                .borrow()
                .entries
                .iter()
                .any(|timer| timer.id == self.0)
        })
    }
}

/// Run `f` once on the UI thread, `delay` after the next pass, under the scope
/// this is called in.
///
/// ```no_run
/// # use guido::prelude::*;
/// # use std::time::Duration;
/// let query = create_signal(String::new());
/// let results = create_signal(Vec::<String>::new());
/// // Searched 300 ms after the last keystroke: each run's timer is
/// // cancelled by the next run.
/// create_effect(move || {
///     let q = query.get();
///     set_timeout(Duration::from_millis(300), move || results.set(vec![q]));
/// });
/// ```
pub fn set_timeout(delay: Duration, f: impl FnOnce() + 'static) -> TimerHandle {
    schedule(delay, Run::Once(Box::new(f)))
}

/// Run `f` on the UI thread every `period`, starting one `period` after the
/// next pass, until it is cancelled or its scope is disposed.
///
/// It keeps its own beat — each deadline is the last plus the period — and a
/// pass that arrives several periods late runs it once, not once per period
/// missed. A period shorter than a millisecond is taken as one.
///
/// ```no_run
/// # use guido::prelude::*;
/// # use std::time::Duration;
/// # let now = || String::from("09:41");
/// let time = create_signal(now());
/// set_interval(Duration::from_secs(1), move || time.set(now()));
/// ```
pub fn set_interval(period: Duration, f: impl FnMut() + 'static) -> TimerHandle {
    schedule(period.max(MIN_PERIOD), Run::Every(Box::new(f)))
}

/// The application's timers, in [`AppState`](crate::app_state::AppState).
#[derive(Default)]
pub(crate) struct Timers {
    next_id: u64,
    entries: Vec<Timer>,
    /// The scopes that own a timer and have been given the cleanup that
    /// cancels theirs.
    scopes: FxHashSet<OwnerId>,
}

impl Timers {
    pub(crate) fn successor(&self) -> Self {
        Self {
            next_id: self.next_id,
            ..Self::default()
        }
    }
}

struct Timer {
    id: u64,
    /// The scope it was created under, which its callback runs under.
    owner: Option<OwnerId>,
    /// The delay, or the period of an interval.
    after: Duration,
    /// `None` until a pass arms it.
    deadline: Option<Instant>,
    /// Taken while it runs, so the callback is not borrowed from the table it
    /// may itself schedule into or cancel from.
    run: Option<Run>,
}

enum Run {
    Once(Box<dyn FnOnce()>),
    Every(Box<dyn FnMut()>),
}

fn schedule(after: Duration, run: Run) -> TimerHandle {
    let owner = current_owner();
    let (id, first_of_its_scope) = with_app_state(|app| {
        let mut timers = app.timers.borrow_mut();
        timers.next_id += 1;
        let id = timers.next_id;
        timers.entries.push(Timer {
            id,
            owner,
            after,
            deadline: None,
            run: Some(run),
        });
        let first = owner.is_some_and(|owner| timers.scopes.insert(owner));
        (id, first)
    });
    // One cleanup per scope, not one per timer: an effect that debounces under
    // a long-lived scope schedules on every run, and a cleanup each would pile
    // up there until the scope went.
    if let Some(owner) = owner.filter(|_| first_of_its_scope) {
        on_cleanup(move || {
            let removed = with_app_state(|app| {
                let mut timers = app.timers.borrow_mut();
                timers.scopes.remove(&owner);
                let mut removed = Vec::new();
                timers.entries.retain_mut(|timer| {
                    if timer.owner == Some(owner) {
                        removed.extend(timer.run.take());
                        false
                    } else {
                        true
                    }
                });
                removed
            });
            drop(removed);
        });
    }
    TimerHandle(id)
}

/// When the loop next has to make a pass for a timer: the earliest armed
/// deadline, or now while a timer is waiting to be armed.
///
/// The second is what asks for the arming pass. Through the deadline rather
/// than `wake_loop`, because a wake forces every surface to draw, and a timer
/// being created is no reason to repaint anything.
pub(crate) fn next_deadline() -> Option<Instant> {
    with_app_state(|app| {
        let timers = app.timers.borrow();
        let unarmed = timers.entries.iter().any(|timer| timer.deadline.is_none());
        if unarmed {
            return Some(Instant::now());
        }
        timers
            .entries
            .iter()
            .filter_map(|timer| timer.deadline)
            .min()
    })
}

/// Arm the timers no pass has seen, at `now`, then run every timer due by
/// `now`, earliest first and ties in the order they were created.
///
/// Called once per pass, by the loop, with the pass's own moment. A timer a
/// callback creates is armed by the next pass, so a timeout of zero scheduled
/// from a timeout does not run in the pass that scheduled it.
pub(crate) fn run_due_timers(now: Instant) {
    let due: Vec<u64> = with_app_state(|app| {
        let mut timers = app.timers.borrow_mut();
        for timer in &mut timers.entries {
            timer.deadline.get_or_insert(now + timer.after);
        }
        let mut due: Vec<(Instant, u64)> = timers
            .entries
            .iter()
            .filter_map(|timer| {
                timer
                    .deadline
                    .filter(|at| *at <= now)
                    .map(|at| (at, timer.id))
            })
            .collect();
        due.sort_unstable();
        due.into_iter().map(|(_, id)| id).collect()
    });

    for id in due {
        // Taken out under the borrow and run outside it: a callback may
        // schedule, cancel, or be cancelled by what it does. A timeout leaves
        // the table as it runs; an interval keeps its entry, empty, so a
        // cancel from inside its own callback is seen when it comes back.
        let Some((owner, run)) = with_app_state(|app| {
            let mut timers = app.timers.borrow_mut();
            let at = timers.entries.iter().position(|timer| timer.id == id)?;
            let owner = timers.entries[at].owner;
            let run = timers.entries[at].run.take()?;
            if matches!(run, Run::Once(_)) {
                timers.entries.remove(at);
            }
            Some((owner, run))
        }) else {
            continue;
        };

        match run {
            Run::Once(f) => under_scope(owner, f),
            Run::Every(mut f) => {
                under_scope(owner, &mut f);
                // Back on its next beat, unless it was cancelled while it ran.
                with_app_state(|app| {
                    let mut timers = app.timers.borrow_mut();
                    if let Some(timer) = timers.entries.iter_mut().find(|timer| timer.id == id) {
                        let last = timer.deadline.expect("a timer that ran was armed");
                        let missed = (now - last).as_nanos() / timer.after.as_nanos();
                        let beats = u32::try_from(missed + 1).unwrap_or(u32::MAX);
                        timer.deadline = Some(last + timer.after * beats);
                        timer.run = Some(Run::Every(f));
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::guard::defer;
    use crate::reactive::owner::{dispose_owner_now, with_owner};
    use crate::reactive::{create_effect, create_signal};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn guarded_timer(
        interval: bool,
        cleanup: impl FnOnce() + 'static,
        runs: Rc<Cell<usize>>,
    ) -> TimerHandle {
        let guard = defer(cleanup);
        let callback = move || {
            let _ = &guard;
            runs.set(runs.get() + 1);
        };
        if interval {
            set_interval(Duration::from_millis(10), callback)
        } else {
            set_timeout(Duration::from_millis(10), callback)
        }
    }

    #[test]
    fn cancellation_releases_captures_after_removing_the_timer() {
        for interval in [false, true] {
            let runs = Rc::new(Cell::new(0));
            let cancelled = set_timeout(Duration::ZERO, || panic!("cancelled timer ran"));
            let survivor = set_timeout(Duration::ZERO, || {});
            let handle = Rc::new(Cell::new(None::<TimerHandle>));
            let spawned = Rc::new(Cell::new(None::<TimerHandle>));
            let seen_handle = handle.clone();
            let new_handle = spawned.clone();
            let timer = guarded_timer(
                interval,
                move || {
                    assert!(!seen_handle.get().unwrap().is_pending());
                    assert!(survivor.is_pending());
                    cancelled.cancel();
                    new_handle.set(Some(set_timeout(Duration::ZERO, || {})));
                },
                runs.clone(),
            );
            handle.set(Some(timer));
            timer.cancel();
            timer.cancel();
            assert!(!timer.is_pending());
            assert!(!cancelled.is_pending());
            assert!(survivor.is_pending());
            let spawned = spawned.get().unwrap();
            assert!(spawned.is_pending());
            run_due_timers(Instant::now() + Duration::from_secs(1));
            assert!(!survivor.is_pending());
            assert!(!spawned.is_pending());
            assert_eq!(runs.get(), 0);
        }
    }

    #[test]
    fn scope_cleanup_preserves_reentrant_timer_changes() {
        let survivor = set_timeout(Duration::ZERO, || {});
        let cancelled = set_timeout(Duration::ZERO, || panic!("cancelled timer ran"));
        let spawned = Rc::new(Cell::new(None::<TimerHandle>));
        let runs = Rc::new(Cell::new(0));
        let new_handle = spawned.clone();
        let second_handle = Rc::new(Cell::new(None::<TimerHandle>));
        let seen_second = second_handle.clone();
        let (timers, scope) = with_owner(|| {
            let first = guarded_timer(
                false,
                move || {
                    cancelled.cancel();
                    assert!(!seen_second.get().unwrap().is_pending());
                    assert!(survivor.is_pending());
                    new_handle.set(Some(set_timeout(Duration::ZERO, || {})));
                },
                runs.clone(),
            );
            let interval_runs = runs.clone();
            let second = set_interval(Duration::from_millis(10), move || {
                interval_runs.set(interval_runs.get() + 1);
            });
            second_handle.set(Some(second));
            [first, second]
        });
        dispose_owner_now(scope);
        assert!(timers.into_iter().all(|timer| !timer.is_pending()));
        assert!(!cancelled.is_pending());
        assert!(survivor.is_pending());
        let spawned = spawned.get().unwrap();
        assert!(spawned.is_pending());
        run_due_timers(Instant::now() + Duration::from_secs(1));
        assert!(!survivor.is_pending());
        assert!(!spawned.is_pending());
        assert_eq!(runs.get(), 0);
    }

    #[test]
    fn debounce_cleanup_keeps_the_replacement_timeout() {
        let query = create_signal(0);
        let cancelled = set_timeout(Duration::ZERO, || panic!("cancelled timer ran"));
        let handles = Rc::new(RefCell::new(Vec::new()));
        let runs = Rc::new(RefCell::new(Vec::new()));
        let recorded_handles = handles.clone();
        let recorded_runs = runs.clone();
        let ((), scope) = with_owner(|| {
            create_effect(move || {
                let value = query.get();
                let guard = defer(move || cancelled.cancel());
                let runs = recorded_runs.clone();
                let timer = set_timeout(Duration::from_millis(10), move || {
                    let _ = &guard;
                    runs.borrow_mut().push(value);
                });
                recorded_handles.borrow_mut().push(timer);
            });
        });
        query.set(1);
        assert!(!handles.borrow()[0].is_pending());
        assert!(handles.borrow()[1].is_pending());
        assert!(!cancelled.is_pending());
        let now = Instant::now();
        run_due_timers(now);
        run_due_timers(now + Duration::from_millis(10));
        assert_eq!(*runs.borrow(), [1]);
        query.set(2);
        assert!(handles.borrow()[2].is_pending());
        dispose_owner_now(scope);
        assert!(!handles.borrow()[2].is_pending());
        run_due_timers(now + Duration::from_secs(1));
        assert_eq!(*runs.borrow(), [1]);
    }

    /// The loop sleeps until the next armed timer, and a cancelled one stops
    /// being a reason to wake.
    #[test]
    fn the_loop_sleeps_until_the_next_timer() {
        let now = Instant::now();
        let timer = set_timeout(Duration::from_millis(250), || {});
        assert!(
            next_deadline().is_some_and(|at| at <= Instant::now()),
            "an unarmed timer asks for a pass now, to be armed"
        );

        run_due_timers(now);
        assert_eq!(next_deadline(), Some(now + Duration::from_millis(250)));

        timer.cancel();
        assert_eq!(next_deadline(), None, "a cancelled timer is no deadline");
    }
}
