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
use crate::reactive::owner::with_owner;
use crate::reactive::{RwSignal, Signal, create_signal};
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
    state: Option<RwSignal<LockState>>,
    lock_requested: bool,
    unlock_requested: bool,
}

thread_local! {
    static LOCK: RefCell<LockData> = RefCell::new(LockData::default());
}

fn state_signal() -> RwSignal<LockState> {
    LOCK.with(|lock| {
        *lock
            .borrow_mut()
            .state
            .get_or_insert_with(|| create_signal(LockState::default()))
    })
}

fn set_state(state: LockState) {
    state_signal().set(state);
}

/// Reactive session-lock lifecycle.
pub fn lock_state() -> Signal<LockState> {
    state_signal().read_only()
}

/// Lock the session (tracked read convenience: `lock_state` == `Locked`).
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
    LOCK.with(|lock| {
        let mut lock = lock.borrow_mut();
        if lock.lock_requested || lock.state.map(|s| s.get_untracked()) == Some(LockState::Locked) {
            log::warn!("lock_session() called while already locked or locking");
            return;
        }
        lock.factory = Some(Box::new(move |info| Box::new(widget_fn(info))));
        lock.lock_requested = true;
    });
    crate::jobs::request_frame();
}

/// Unlock the session and drop all lock surfaces.
pub fn unlock_session() {
    LOCK.with(|lock| lock.borrow_mut().unlock_requested = true);
    crate::jobs::request_frame();
}

/// Drive the session-lock state machine. Called once per main-loop
/// iteration, before surface commands are processed.
pub(crate) fn process_session_lock(
    surface_manager: &mut SurfaceManager,
    wayland_state: &mut WaylandState,
    qh: &QueueHandle<WaylandState>,
    tree: &mut Tree,
) {
    // 1. Pending lock request
    let lock_requested = LOCK.with(|l| std::mem::take(&mut l.borrow_mut().lock_requested));
    if lock_requested {
        if wayland_state.start_session_lock(qh) {
            set_state(LockState::Locking);
        } else {
            LOCK.with(|l| l.borrow_mut().factory = None);
            set_state(LockState::Unlocked);
        }
    }

    // 2. Lock lifecycle events from the compositor
    for event in wayland_state.take_lock_events() {
        match event {
            LockEvent::Locked => set_state(LockState::Locked),
            LockEvent::Finished => {
                // Denied, or the lock ended without our unlock_session().
                teardown_lock_surfaces(surface_manager, wayland_state);
                LOCK.with(|l| l.borrow_mut().factory = None);
                set_state(LockState::Unlocked);
            }
        }
    }

    // 3. Pending unlock request
    let unlock_requested = LOCK.with(|l| std::mem::take(&mut l.borrow_mut().unlock_requested));
    if unlock_requested && state_signal().get_untracked() != LockState::Unlocked {
        wayland_state.unlock_session();
        teardown_lock_surfaces(surface_manager, wayland_state);
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
            surface_manager.remove(sid);
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

fn teardown_lock_surfaces(surface_manager: &mut SurfaceManager, wayland_state: &mut WaylandState) {
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
        surface_manager.remove(sid);
        wayland_state.destroy_surface(sid);
    }
}

/// Reset session-lock state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_session_lock() {
    LOCK.with(|lock| *lock.borrow_mut() = LockData::default());
}
