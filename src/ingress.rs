//! Cross-thread ingress into the main event loop.
//!
//! Background threads must never hand-roll wakeups for the main loop: they
//! send an [`IngressMessage`] through a calloop channel registered as an
//! event source. calloop guarantees that a send wakes the next dispatch and
//! delivers the message before the loop can block again — the message's
//! existence *is* the wakeup, so the lost-wakeup class of bugs (a ping
//! suppressed because some flag was already set, then absorbed by an
//! unrelated consumer) cannot occur by construction.
//!
//! Main-thread code keeps using `jobs::request_frame()`, whose ping is
//! coalesced once per loop iteration (see `jobs::mark_loop_awake`).
//!
//! When no loop is running (setup phase, tests, teardown) [`notify`] falls
//! back to the frame-request ping, which degrades to a plain flag there.

use std::sync::Mutex;

use smithay_client_toolkit::reexports::calloop::channel::Sender;

/// Messages background threads can push into the main loop.
///
/// Variants either carry their payload directly (applied by the loop's
/// channel callback) or act as doorbells for data queued elsewhere.
pub(crate) enum IngressMessage {
    /// Background signal writes were queued (the data lives in the reactive
    /// write queue and is drained at the loop's flush point). The message
    /// only guarantees the loop wakes up to run that flush.
    BgWritesQueued,
    /// Prefetched clipboard/primary-selection content from a reader thread.
    /// Applied by the channel callback (generation-checked against the
    /// current offer in `WaylandState::apply_clipboard_update`).
    ClipboardUpdate {
        kind: crate::platform::wayland::SelectionKind,
        generation: u64,
        content: Option<String>,
    },
}

static INGRESS_SENDER: Mutex<Option<Sender<IngressMessage>>> = Mutex::new(None);

/// Install the loop's channel sender. Called by `App::run` when the event
/// loop is created.
pub(crate) fn install_ingress(sender: Sender<IngressMessage>) {
    if let Ok(mut guard) = INGRESS_SENDER.lock() {
        *guard = Some(sender);
    }
}

/// Get a sender clone for a background thread, if a loop is running.
pub(crate) fn sender() -> Option<Sender<IngressMessage>> {
    INGRESS_SENDER.lock().ok().and_then(|g| g.as_ref().cloned())
}

/// Send a message to the main loop, falling back to the frame-request ping
/// when no loop is running.
pub(crate) fn notify(message: IngressMessage) {
    match sender() {
        Some(tx) => {
            if tx.send(message).is_err() {
                // Receiver died (loop shutting down) — fall back so pending
                // work is at least flagged for a future loop.
                crate::jobs::request_frame();
            }
        }
        None => crate::jobs::request_frame(),
    }
}

/// Reset ingress state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_ingress() {
    if let Ok(mut guard) = INGRESS_SENDER.lock() {
        *guard = None;
    }
}
