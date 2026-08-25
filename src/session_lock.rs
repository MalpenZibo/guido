//! Session lock (`ext-session-lock-v1`) — build lock screens.
//!
//! [`lock_session`] asks the compositor to lock the session and, once the
//! lock is granted, creates one lock surface per output using the provided
//! widget factory (new outputs plugged in while locked get one too). The
//! compositor blanks all outputs and routes input to the lock surfaces, so
//! a `text_input` password field works out of the box.
//!
//! ```ignore
//! lock_session(|output| lock_screen_widget(output));
//! // …later, e.g. after password verification:
//! unlock_session();
//! ```
//!
//! [`lock_state`] exposes the lifecycle reactively. When the compositor
//! refuses or aborts the lock (another lock client active, protocol
//! missing), the state returns to `Unlocked` and the surfaces are dropped.
//!
//! An app whose only surfaces are lock surfaces keeps running after
//! unlocking (it idles waiting for the next [`lock_session`] call) — unlike
//! ordinary surfaces, closing lock surfaces never exits the app.

use std::cell::RefCell;
use std::collections::HashMap;

use smithay_client_toolkit::reexports::client::QueueHandle;

use crate::outputs::{self, OutputId, OutputInfo};
use crate::platform::{LockEvent, WaylandState};
use crate::reactive::global::GlobalSignal;
use crate::reactive::owner::with_owner;
use crate::reactive::{RwSignal, Signal};
use crate::surface::{SurfaceConfig, SurfaceId};
use crate::surface_manager::{ManagedSurface, SurfaceManager};
use crate::tree::Tree;
use crate::widgets::Widget;

/// Lifecycle of the session lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockState {
    /// No lock held (also after a denied or finished lock).
    #[default]
    Unlocked,
    /// Lock requested, waiting for the compositor to grant it.
    Locking,
    /// Lock granted — lock surfaces are shown on every output.
    Locked,
}

type LockWidgetFn = Box<dyn Fn(OutputInfo) -> Box<dyn Widget>>;

#[derive(Default)]
struct LockData {
    /// Builds the lock screen widget for an output. Kept across lock
    /// cycles so hotplugged outputs get surfaces while locked.
    factory: Option<LockWidgetFn>,
    /// Lock surface per output.
    surfaces: HashMap<OutputId, SurfaceId>,
}

/// What the application last asked for, waiting for the state machine.
enum LockRequest {
    /// Lock, building each output's screen with this factory. The factory
    /// travels with the request rather than being installed by the caller:
    /// a request that is replaced takes its factory with it, and the one the
    /// state machine acts on is the one it was handed.
    Lock(LockWidgetFn),
    Unlock,
}

thread_local! {
    static LOCK: RefCell<LockData> = RefCell::new(LockData::default());

    /// A slot, not two flags: "lock" and "unlock" are two answers to one
    /// question, and asking twice in a frame means the second answer is the
    /// one meant. Two independent booleans let both be true at once, which
    /// the loop then acted on in order — a lock sent to the compositor and
    /// undone in the same iteration.
    ///
    /// Setting it is what wakes the loop, so the state machine cannot be
    /// asked for something and left asleep — see `crate::deferred`.
    static REQUEST: crate::deferred::DeferredSlot<LockRequest> =
        const { crate::deferred::DeferredSlot::new() };
}

/// The lifecycle the application watches. Its own global rather than a field
/// of `LockData`: the rest of that struct is the platform's bookkeeping, which
/// dies with the `App`, while this is read from widget scopes that come and go.
static STATE: GlobalSignal<LockState> = GlobalSignal::new(LockState::default);

fn state_signal() -> RwSignal<LockState> {
    STATE.get()
}

fn set_state(state: LockState) {
    state_signal().set(state);
}

/// Reactive session-lock lifecycle.
pub fn lock_state() -> Signal<LockState> {
    state_signal().read_only()
}

/// Whether the session is locked right now — a tracked read, the convenience
/// form of `lock_state() == LockState::Locked`. It reports the state; to
/// *take* the lock, see [`lock_session`].
pub fn session_locked() -> bool {
    lock_state().get() == LockState::Locked
}

/// Ask the compositor to lock the session.
///
/// `widget_fn` builds the lock screen for each output — it is called once
/// per connected output when the lock is granted, and again for outputs
/// plugged in while locked. Does nothing if a lock is already active or
/// pending. Watch [`lock_state`] for the outcome: `Locking` → `Locked`, or
/// back to `Unlocked` when the compositor refuses (e.g. no
/// ext-session-lock-v1, or another lock client is active).
pub fn lock_session<W, F>(widget_fn: F)
where
    W: Widget + 'static,
    F: Fn(OutputInfo) -> W + 'static,
{
    // `Locking` counts: a second request while the first is in flight is
    // granted here, then refused by the platform on the next iteration — and
    // the refusal path clears the factory. The compositor's `Locked` then
    // arrives with nothing to build the lock screen from, which is a locked
    // session with no way to type a password into it.
    //
    // A second request made before the state machine has seen the first needs
    // no guard: it replaces it in the slot, so one request goes out either
    // way, built from the factory the caller asked for last.
    if matches!(
        STATE.get().get_untracked(),
        LockState::Locked | LockState::Locking
    ) {
        log::warn!("lock_session() called while already locked or locking");
        return;
    }
    REQUEST.with(|request| {
        request.set(LockRequest::Lock(Box::new(move |info| {
            Box::new(widget_fn(info))
        })))
    });
}

/// Unlock the session and drop all lock surfaces.
pub fn unlock_session() {
    REQUEST.with(|request| request.set(LockRequest::Unlock));
}

/// Drive the session-lock state machine. Called once per main-loop
/// iteration, before surface commands are processed.
pub(crate) fn process_session_lock(
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut WaylandState,
    qh: &QueueHandle<WaylandState>,
    tree: &mut Tree,
) {
    // 1. Pending request. A lock goes out now; an unlock waits for step 3,
    // after the compositor's events — a `Locked` arriving in the same
    // iteration must land before the teardown, or the state ends up `Locked`
    // with no surfaces under it.
    let mut unlock_requested = false;
    match REQUEST.with(|request| request.take()) {
        Some(LockRequest::Lock(factory)) => {
            LOCK.with(|l| l.borrow_mut().factory = Some(factory));
            if wayland_state.start_session_lock(qh) {
                set_state(LockState::Locking);
            } else {
                LOCK.with(|l| l.borrow_mut().factory = None);
                set_state(LockState::Unlocked);
            }
        }
        Some(LockRequest::Unlock) => unlock_requested = true,
        None => {}
    }

    // 2. Lock lifecycle events from the compositor
    for event in wayland_state.take_lock_events() {
        match event {
            LockEvent::Locked => set_state(LockState::Locked),
            LockEvent::Finished => {
                // Denied, or the lock ended without our unlock_session().
                teardown_lock_surfaces(surface_manager, wayland_state, tree);
                LOCK.with(|l| l.borrow_mut().factory = None);
                set_state(LockState::Unlocked);
            }
        }
    }

    // 3. Pending unlock request, taken above
    if unlock_requested && state_signal().get_untracked() != LockState::Unlocked {
        wayland_state.unlock_session();
        teardown_lock_surfaces(surface_manager, wayland_state, tree);
        LOCK.with(|l| l.borrow_mut().factory = None);
        set_state(LockState::Unlocked);
    }

    // 4. While locked: one lock surface per connected output (covers both
    // the initial grant and hotplug while locked)
    if state_signal().get_untracked() == LockState::Locked {
        let current = outputs::current_outputs();

        // Drop surfaces for disconnected outputs
        let stale: Vec<(OutputId, SurfaceId)> = LOCK.with(|l| {
            let mut lock = l.borrow_mut();
            let stale: Vec<(OutputId, SurfaceId)> = lock
                .surfaces
                .iter()
                .filter(|(out, _)| !current.iter().any(|o| o.id == **out))
                .map(|(out, sid)| (*out, *sid))
                .collect();
            for (out, _) in &stale {
                lock.surfaces.remove(out);
            }
            stale
        });
        for (_, sid) in stale {
            if let Some(managed) = surface_manager.remove(sid) {
                crate::surface_manager::teardown_widget_subtree(tree, managed.widget_id);
            }
            wayland_state.destroy_surface(sid);
        }

        // Create surfaces for new outputs
        for info in current {
            let already = LOCK.with(|l| l.borrow().surfaces.contains_key(&info.id));
            if already {
                continue;
            }

            let id = SurfaceId::next();
            if !wayland_state.create_lock_surface_with_id(qh, id, info.id) {
                continue;
            }

            let output_id = info.id;
            let widget = LOCK.with(|l| {
                l.borrow()
                    .factory
                    .as_ref()
                    .map(|f| with_owner(|| f(info.clone())))
            });
            let Some((widget, owner_id)) = widget else {
                log::error!("Session locked but no lock widget factory is set");
                wayland_state.destroy_surface(id);
                continue;
            };

            // Size arrives with the first configure; the config only
            // contributes the clear color behind the widget tree.
            let config = SurfaceConfig::new().width(0).height(0);
            let managed = ManagedSurface::new(id, config, widget, owner_id, tree);
            surface_manager.add(managed);
            LOCK.with(|l| l.borrow_mut().surfaces.insert(output_id, id));
        }
    }
}

fn teardown_lock_surfaces(
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut WaylandState,
    tree: &mut crate::tree::Tree,
) {
    let surfaces: Vec<SurfaceId> = LOCK.with(|l| {
        l.borrow_mut()
            .surfaces
            .drain()
            .map(|(_, sid)| sid)
            .collect()
    });
    for sid in surfaces {
        // Removed directly — not via SurfaceCommand::Close — so an app whose
        // only surfaces were lock surfaces keeps running after unlock.
        if let Some(managed) = surface_manager.remove(sid) {
            crate::surface_manager::teardown_widget_subtree(tree, managed.widget_id);
        }
        wayland_state.destroy_surface(sid);
    }
}

/// Reset session-lock state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_session_lock() {
    LOCK.with(|lock| *lock.borrow_mut() = LockData::default());
    REQUEST.with(|request| request.clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{create_root_owner, dispose_owner_now, with_owner};

    /// Two answers in one frame, and the state machine acts on the second.
    ///
    /// The pair used to be two independent booleans, so both could be true at
    /// once: the loop started a lock, sent it to the compositor, and undid it
    /// three steps later in the same iteration — a lock screen that flashes up
    /// and vanishes for an application that changed its mind.
    #[test]
    fn the_last_request_of_a_frame_is_the_one_that_counts() {
        create_root_owner();
        reset_session_lock();
        set_state(LockState::Unlocked);

        lock_session(|_| crate::widgets::container());
        unlock_session();

        assert!(
            matches!(REQUEST.with(|r| r.take()), Some(LockRequest::Unlock)),
            "the later request replaces the earlier one rather than joining it"
        );
        assert!(
            REQUEST.with(|r| r.take()).is_none(),
            "and there is only ever the one"
        );

        reset_session_lock();
    }

    /// The lock state is process-wide and lives behind a thread-local built on
    /// first use, so *whoever reads it first* would otherwise decide its owner.
    /// When that first reader is a widget closure or an event handler — the
    /// natural place to ask whether the session is locked — the signal dies
    /// with that scope while the thread-local goes on holding the handle.
    #[test]
    fn the_lock_state_outlives_whoever_reads_it_first() {
        create_root_owner();
        let ((), first_reader) = with_owner(|| {
            let _ = lock_state().get_untracked();
        });
        dispose_owner_now(first_reader);

        assert_eq!(lock_state().get_untracked(), LockState::default());
    }
}
