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
//! Main-thread code keeps using `jobs::wake_loop()`, whose ping is
//! coalesced once per loop iteration (see `jobs::mark_loop_awake`).
//!
//! When no loop is running (setup phase, tests, teardown) [`notify`] falls
//! back to the wake ping, which degrades to a plain flag there.

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
        kind: crate::platform::SelectionKind,
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

/// A sender bound to the loop that was running when it was taken, for a
/// producer that only has its payload much later — a selection read can take
/// seconds. Resolving the sender at send time instead would let the result of
/// one session's read land in the next session's loop, which restarts its
/// generation counters and cannot tell the two apart.
pub(crate) struct IngressSender(Sender<IngressMessage>);

impl IngressSender {
    /// Send, or hand the wakeup to the ping if that loop is gone.
    ///
    /// The same fallback as [`notify`], for the same reason: nothing here
    /// knows whether the message is carrying its own payload or standing in
    /// for a queue somewhere else, and dropping the second kind silently
    /// strands the work it speaks for.
    pub(crate) fn send(self, message: IngressMessage) {
        if self.0.send(message).is_err() {
            log::debug!("ingress receiver is gone; waking through the ping instead");
            crate::jobs::wake_loop();
        }
    }
}

/// Take a sender for the loop running now, if there is one.
pub(crate) fn sender_handle() -> Option<IngressSender> {
    sender().map(IngressSender)
}

/// Private on purpose: [`notify`] and [`IngressSender`] are the two ways in,
/// and both send from the same gesture that queued the work.
fn sender() -> Option<Sender<IngressMessage>> {
    INGRESS_SENDER.lock().ok().and_then(|g| g.as_ref().cloned())
}

/// Send a message to the main loop, falling back to the wake ping when there
/// is no loop to send to or its receiver has already gone.
///
/// Called from the same function that queued the work it speaks for — see
/// `reactive::runtime::queue_bg_write`, and `deferred` for the main-thread
/// half of the same rule.
pub(crate) fn notify(message: IngressMessage) {
    match sender() {
        Some(tx) if tx.send(message).is_ok() => {}
        // No loop, or the receiver died while shutting down: nothing will
        // take delivery, so hand the wakeup to the ping instead. The work is
        // then at least flagged for whatever loop comes next.
        _ => crate::jobs::wake_loop(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::calloop::ping::make_ping;

    /// The fallback branch, which is the one a real producer takes when the
    /// loop it queued for is gone: the wakeup has to be handed over to the
    /// ping rather than dropped, or the work sits in the write queue with
    /// nothing coming to flush it.
    ///
    /// Retried: `reactive::memo`'s tests call `jobs::reset_jobs()` without
    /// taking the lock above, and that drops the ping handle this one just
    /// installed. A regression fails every attempt — the fallback either
    /// wakes or it does not — so one clean window out of many is enough.
    #[test]
    fn giving_up_on_the_channel_hands_the_wakeup_to_the_ping() {
        let _state = crate::jobs::wakeup_test_lock();
        let (ping, _source) = make_ping().expect("ping");

        let woken = (0..64).any(|_| {
            reset_ingress(); // no receiver: force the fallback
            crate::jobs::init_wakeup(ping.clone());
            crate::jobs::mark_loop_awake();

            notify(IngressMessage::BgWritesQueued);

            crate::jobs::ping_in_flight()
        });
        assert!(
            woken,
            "the fallback must wake the loop it could not send to"
        );
    }
}
