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
//!
//! Messages are counted from before the work they speak for is queued until
//! the loop takes delivery ([`in_flight`]), because a wakeup that only exists
//! inside calloop's channel cannot be seen from here — and the loop's
//! pre-block check has to be able to ask whether one is owed before it
//! decides that a non-empty queue is a bug.

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

/// Messages armed but not yet taken delivery of by the loop.
///
/// A calloop send wakes the next dispatch, and the message stays readable
/// until the loop reads it — so a non-zero count means the loop cannot block
/// indefinitely. That is invisible from the `Sender` side, which is why it is
/// counted here: the loop's pre-block check has to be able to ask whether a
/// wakeup is owed, not only whether a queue is empty (`queued_but_unwoken` in
/// `lib.rs`).
///
/// Debug-only, like the check it answers. A status bar queues a background
/// write several times a second, and in a release build there is nobody to
/// answer — so the release half is a set of no-ops and a `false`, and the
/// atomics are not paid for.
#[cfg(debug_assertions)]
mod count {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

    /// `AcqRel`, not `Release`: the invariant is that the count is up
    /// *before* the work becomes visible, which orders the writes that
    /// follow, not only the ones that came before. Today `queue_bg_write`
    /// pushes under a mutex whose unlock would carry it anyway — a queue
    /// without that edge would not.
    pub(super) fn up() {
        IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
    }

    /// Saturating, because [`super::reset_ingress`] can land between a thread
    /// arming and that thread finding out there is no loop left to send to.
    /// Wrapping past zero would leave the count permanently non-zero, which
    /// reads as *a wakeup is always armed* — the check would stop reporting
    /// anything at all, quietly.
    pub(super) fn down() {
        let _ = IN_FLIGHT.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            Some(count.saturating_sub(1))
        });
    }

    /// Whatever was in flight is never arriving: the receiver goes with the
    /// loop.
    pub(super) fn reset() {
        IN_FLIGHT.store(0, Ordering::Release);
    }

    pub(super) fn any() -> bool {
        IN_FLIGHT.load(Ordering::Acquire) > 0
    }
}

#[cfg(not(debug_assertions))]
mod count {
    pub(super) fn up() {}
    pub(super) fn down() {}
    pub(super) fn reset() {}
    /// Nothing is ever counted, so nothing is ever owed — and the only caller
    /// is compiled out anyway.
    pub(super) fn any() -> bool {
        false
    }
}

/// Whether the loop has a message coming that it has not read yet.
pub(crate) fn in_flight() -> bool {
    count::any()
}

/// The loop took delivery of one message. Called by the ingress channel
/// callback, before the drains that message speaks for.
pub(crate) fn delivered() {
    count::down();
}

/// A wakeup owed to the loop, counted from before the work it is owed for
/// becomes visible until the loop takes delivery.
///
/// Arming first is the point: between queueing the work and sending the
/// message there is an instant where the queue is observably non-empty, and
/// a loop that looked at it right then would see work with nothing owed for
/// it. The count is already up by then.
#[must_use = "an armed wakeup that is never sent is a wakeup nobody will get"]
pub(crate) struct Armed {
    /// Taken by [`Armed::notify`]. Still `Some` in `drop` means the message
    /// was never sent — give the count back rather than leaving the loop
    /// owed a wakeup that is not coming.
    message: Option<IngressMessage>,
}

/// Arm a wakeup, to be sent with [`Armed::notify`] once the work it speaks
/// for is queued.
pub(crate) fn arm(message: IngressMessage) -> Armed {
    count::up();
    Armed {
        message: Some(message),
    }
}

impl Armed {
    /// Send the armed message, falling back to the wake ping when no loop is
    /// running or its receiver is gone. The count stays up until the loop
    /// takes delivery — [`delivered`].
    pub(crate) fn notify(mut self) {
        // `arm` is the only constructor and this consumes `self`, so the
        // message is always there; `Drop` reads the same field to tell an
        // armed-and-sent from an armed-and-abandoned.
        let message = self.message.take().expect("an armed message to send");
        if let Some(tx) = sender()
            && tx.send(message).is_ok()
        {
            return;
        }
        // No loop, or the receiver died while shutting down: nothing will
        // ever take delivery. Wake first, give the count back after — the
        // same order this whole module argues for, and for the same reason:
        // in between, the work is queued and the check runs on another
        // thread.
        crate::jobs::wake_loop();
        count::down();
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        if self.message.is_some() {
            count::down();
        }
    }
}

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
///
/// The count is kept around the send, so this is not a way to slip a message
/// past the loop's wakeup check.
pub(crate) struct IngressSender(Sender<IngressMessage>);

impl IngressSender {
    /// Send, dropping the message if that loop is gone. Nothing is woken in
    /// that case: the message carries its own payload, so there is no queue
    /// left behind for anyone to drain.
    pub(crate) fn send(self, message: IngressMessage) {
        count::up();
        if self.0.send(message).is_err() {
            count::down();
        }
    }
}

/// Take a sender for the loop running now, if there is one.
pub(crate) fn sender_handle() -> Option<IngressSender> {
    sender().map(IngressSender)
}

/// Private on purpose: a send that does not go through [`arm`] or
/// [`IngressSender`] is a message the loop takes delivery of — and
/// decrements the count for — without anything having counted it.
fn sender() -> Option<Sender<IngressMessage>> {
    INGRESS_SENDER.lock().ok().and_then(|g| g.as_ref().cloned())
}

/// Send a message to the main loop, falling back to the wake ping when no
/// loop is running.
///
/// For a message whose payload is ready now. A message acting as a doorbell
/// for a queue elsewhere arms first and sends after the push — [`arm`].
#[cfg(test)]
pub(crate) fn notify(message: IngressMessage) {
    arm(message).notify();
}

/// Reset ingress state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_ingress() {
    if let Ok(mut guard) = INGRESS_SENDER.lock() {
        *guard = None;
    }
    count::reset();
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay_client_toolkit::reexports::calloop::ping::make_ping;

    /// `App::drop` resets the count while a reader thread may be holding an
    /// armed message it is about to find no receiver for. Wrapping there
    /// would pin the count above zero for the rest of the process and turn
    /// the loop's check into a no-op that reports nothing, forever.
    /// Debug-only: the count is, and this is about its arithmetic.
    #[cfg(debug_assertions)]
    #[test]
    fn a_reset_between_arming_and_giving_up_does_not_wrap_the_count() {
        let _guard = crate::jobs::wakeup_test_lock();
        let armed = arm(IngressMessage::BgWritesQueued);
        assert!(in_flight());

        reset_ingress();
        drop(armed);

        assert!(
            !in_flight(),
            "the count must be back at zero, not wrapped past it"
        );
    }

    /// The fallback branch, which is the one a real producer takes when the
    /// loop it armed for is gone: the wakeup has to be handed over to the
    /// ping *and* the count given back. Dropping the count without waking
    /// leaves the work queued with nothing owed for it — the state the loop's
    /// check panics on.
    #[test]
    fn giving_up_on_the_channel_hands_the_wakeup_to_the_ping() {
        let _guard = crate::jobs::wakeup_test_lock();
        reset_ingress(); // no receiver: force the fallback
        let (ping, _source) = make_ping().expect("ping");
        crate::jobs::init_wakeup(ping);
        crate::jobs::mark_loop_awake();

        notify(IngressMessage::BgWritesQueued);

        assert!(
            crate::jobs::ping_in_flight(),
            "the fallback must wake the loop it could not send to"
        );
        assert!(
            !in_flight(),
            "and must not leave the count owed for a message nobody has"
        );
    }
}
