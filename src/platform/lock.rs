//! Session lock (`ext-session-lock-v1`).
//!
//! Locking is a grant, not a request that succeeds locally: the compositor
//! answers asynchronously and may refuse. Until it answers there is no lock,
//! and the surfaces that cover the outputs cannot be created yet — which is
//! why the outcome arrives as an event the main loop drains rather than as a
//! return value.

use smithay_client_toolkit::{
    delegate_session_lock,
    session_lock::{
        SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface,
        SessionLockSurfaceConfigure,
    },
};

use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};

use super::wayland::{SurfaceRole, WaylandState, WaylandSurfaceState};
use crate::outputs::OutputId;
use crate::surface::SurfaceId;

/// Events from the session-lock protocol, drained by the main loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEvent {
    /// The compositor granted the lock; lock surfaces may be created.
    Locked,
    /// The lock ended: denied outright, or unlocked/aborted later.
    Finished,
}

/// The lock grant and the events it produces.
pub struct Lock {
    pub(super) session_lock_state: SessionLockState,
    /// The active lock grant. Written when `start_session_lock` succeeds so a
    /// synchronous double-lock check can reject a second call; cleared by
    /// `finished` or `unlock_session`.
    pub(super) active_lock: Option<SessionLock>,
    /// Lock lifecycle events, drained by the main loop.
    pub(super) lock_events: Vec<LockEvent>,
}

impl Lock {
    pub(super) fn new(session_lock_state: SessionLockState) -> Self {
        Self {
            session_lock_state,
            active_lock: None,
            lock_events: Vec::new(),
        }
    }
}

impl WaylandState {
    /// Request a session lock from the compositor.
    ///
    /// Returns false when a lock is already active or the compositor lacks
    /// ext-session-lock-v1. The grant (or denial) arrives asynchronously as
    /// a [`LockEvent`].
    pub fn start_session_lock(&mut self, qh: &QueueHandle<Self>) -> bool {
        if self.lock.active_lock.is_some() {
            log::warn!("Session lock requested while a lock is already active");
            return false;
        }
        match self.lock.session_lock_state.lock(qh) {
            Ok(lock) => {
                self.lock.active_lock = Some(lock);
                log::info!("Session lock requested");
                true
            }
            Err(e) => {
                log::error!("Compositor does not support ext-session-lock-v1: {e}");
                false
            }
        }
    }

    /// Unlock the session (no-op when not locked).
    pub fn unlock_session(&mut self) {
        if let Some(lock) = self.lock.active_lock.take() {
            lock.unlock();
            log::info!("Session unlocked");
        }
    }

    /// Whether a session lock is currently held and granted.
    pub fn is_session_locked(&self) -> bool {
        self.lock
            .active_lock
            .as_ref()
            .is_some_and(|l| l.is_locked())
    }

    /// Create a lock surface for `output` with a specific SurfaceId.
    ///
    /// The surface starts at 0×0 — its real size arrives with the first
    /// lock-surface configure. Returns false without an active lock or when
    /// the output disconnected.
    pub fn create_lock_surface_with_id(
        &mut self,
        qh: &QueueHandle<Self>,
        id: SurfaceId,
        output: OutputId,
    ) -> bool {
        let Some(lock) = self.lock.active_lock.clone() else {
            log::warn!("Cannot create lock surface without an active session lock");
            return false;
        };
        let Some(wl_output) = self.wl_output_for(output) else {
            log::warn!(
                "Cannot create lock surface: output {:?} is not connected",
                output
            );
            return false;
        };

        let wl_surface = self.compositor_state.create_surface(qh);
        let lock_surface = lock.create_lock_surface(wl_surface.clone(), &wl_output, qh);

        self.surface_lookup.insert(wl_surface.id(), id);
        let surface_state =
            WaylandSurfaceState::new(SurfaceRole::Lock(lock_surface), wl_surface, 0, 0);
        self.surfaces.insert(id, surface_state);

        log::info!("Created lock surface {:?} on output {:?}", id, output);
        true
    }

    /// Drain pending session-lock lifecycle events.
    pub fn take_lock_events(&mut self) -> Vec<LockEvent> {
        std::mem::take(&mut self.lock.lock_events)
    }
}

impl SessionLockHandler for WaylandState {
    fn locked(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _session_lock: SessionLock) {
        log::info!("Session lock granted by compositor");
        self.lock.lock_events.push(LockEvent::Locked);
        crate::jobs::wake_loop();
    }

    fn finished(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _session_lock: SessionLock,
    ) {
        log::info!("Session lock finished (denied or ended)");
        self.lock.active_lock = None;
        self.lock.lock_events.push(LockEvent::Finished);
        crate::jobs::wake_loop();
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: SessionLockSurface,
        configure: SessionLockSurfaceConfigure,
        _serial: u32,
    ) {
        // Route to the matching surface state, mirroring the layer-shell
        // configure path so the same render machinery applies. sctk has
        // already acked the configure.
        let surface_id = self.surface_lookup.get(&surface.wl_surface().id()).copied();
        if let Some(id) = surface_id
            && let Some(surface_state) = self.surfaces.get_mut(&id)
        {
            let (width, height) = configure.new_size;
            log::info!(
                "Lock surface {:?} configure: {}x{} (current {}x{})",
                id,
                width,
                height,
                surface_state.width,
                surface_state.height
            );
            if width > 0 {
                surface_state.width = width;
            }
            if height > 0 {
                surface_state.height = height;
            }
            surface_state.configured = true;
            crate::jobs::wake_loop();
        }
    }
}

delegate_session_lock!(WaylandState);
