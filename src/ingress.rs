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
use std::sync::atomic::{AtomicUsize, Ordering};

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
/// until the loop reads it — so a non-zero count means the loop cannot
/// block indefinitely. That is invisible from the `Sender` side, which is
/// why it is counted here: the loop's pre-block check has to be able to ask
/// whether a wakeup is owed, not only whether a queue is empty
/// (`queued_but_unwoken` in `lib.rs`).
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Whether the loop has a message coming that it has not read yet.
pub(crate) fn in_flight() -> bool {
    IN_FLIGHT.load(Ordering::Acquire) > 0
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
    IN_FLIGHT.fetch_add(1, Ordering::Release);
    Armed {
        message: Some(message),
    }
}

impl Armed {
    /// Send the armed message, falling back to the wake ping when no loop is
    /// running or its receiver is gone. The count stays up until the loop
    /// takes delivery — `delivered()`.
    pub(crate) fn notify(mut self) {
        let message = match self.message.take() {
            Some(message) => message,
            None => return,
        };
        match sender() {
            Some(tx) if tx.send(message).is_ok() => {}
            // No loop, or the receiver died while shutting down: nothing will
            // ever take delivery, so the count comes back down and the work is
            // at least flagged for a future loop.
            _ => {
                disarm();
                crate::jobs::wake_loop();
            }
        }
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        if self.message.is_some() {
            disarm();
        }
    }
}

/// The loop took delivery of one message. Called by the ingress channel
/// callback, before the drains that message speaks for.
pub(crate) fn delivered() {
    release_one();
}

/// Give back a wakeup that is not coming after all.
fn disarm() {
    release_one();
}

/// Saturating, because [`reset_ingress`] can land between a thread arming
/// and that thread finding out there is no loop left to send to. Wrapping
/// past zero would leave the count permanently non-zero, which reads as *a
/// wakeup is always armed* — the check would stop reporting anything at all,
/// quietly.
fn release_one() {
    let _ = IN_FLIGHT.fetch_update(Ordering::Release, Ordering::Acquire, |count| {
        Some(count.saturating_sub(1))
    });
}

/// Install the loop's channel sender. Called by `App::run` when the event
/// loop is created.
pub(crate) fn install_ingress(sender: Sender<IngressMessage>) {
    if let Ok(mut guard) = INGRESS_SENDER.lock() {
        *guard = Some(sender);
    }
}

/// Whether a loop is running to deliver messages to. For a producer that
/// wants to give up early rather than arm a wakeup nobody will take.
pub(crate) fn loop_running() -> bool {
    sender().is_some()
}

/// Get a sender clone, if a loop is running. Private on purpose: a send that
/// does not go through [`arm`] is a message the loop takes delivery of —
/// and decrements the count for — without anything having counted it.
fn sender() -> Option<Sender<IngressMessage>> {
    INGRESS_SENDER.lock().ok().and_then(|g| g.as_ref().cloned())
}

/// Send a message to the main loop, falling back to the wake ping
/// when no loop is running.
///
/// For a message that carries its own payload. A message acting as a
/// doorbell for a queue elsewhere arms first and sends after the push —
/// [`arm`].
pub(crate) fn notify(message: IngressMessage) {
    arm(message).notify();
}

/// The count is process-wide, and so are the tests that drive it directly
/// — here and in `lib.rs`. Taking this first keeps one from reading the
/// other's arming and calling it its own.
#[cfg(test)]
pub(crate) fn test_count_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Reset ingress state.
///
/// Called during `App::drop()`.
pub(crate) fn reset_ingress() {
    if let Ok(mut guard) = INGRESS_SENDER.lock() {
        *guard = None;
    }
    // Whatever was in flight is never arriving: the receiver goes with the loop.
    IN_FLIGHT.store(0, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `App::drop` resets the count while a reader thread may be holding an
    /// armed message it is about to find no receiver for. Wrapping there
    /// would pin the count above zero for the rest of the process and turn
    /// the loop's check into a no-op that reports nothing, forever.
    #[test]
    fn a_reset_between_arming_and_giving_up_does_not_wrap_the_count() {
        let _guard = test_count_lock();
        let armed = arm(IngressMessage::BgWritesQueued);
        assert!(in_flight());

        reset_ingress();
        drop(armed);

        assert!(
            !in_flight(),
            "the count must be back at zero, not wrapped past it"
        );
    }
}
