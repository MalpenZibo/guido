//! Work handed to the main loop, and the wakeup that comes with it.
//!
//! The loop blocks when there is nothing to do, so anything that queues work
//! for it has to wake it — and for as long as those were two statements next
//! to each other, the second one could be left out. It was, twice, and both
//! times the symptom was an application that went deaf until an unrelated
//! compositor event happened along.
//!
//! Here they are one call. [`DeferredQueue::push`] and [`DeferredSlot::set`]
//! *are* the wakeup: the cell is private to the type, so there is no way to
//! put something in one and not ask for the pass that takes it out.
//!
//! Two shapes, because the queues in this crate are two shapes:
//!
//! - [`DeferredQueue`] accumulates — every disposal and every surface command
//!   has to happen, and none replaces another;
//! - [`DeferredSlot`] is last-one-wins — two cursor shapes in one frame are
//!   two answers to the same question, and the second is the answer.
//!
//! What is *not* here is as deliberate. Scheduled jobs (`jobs::request_job_at`)
//! own no wakeup on purpose: the loop is not late for them, it sleeps exactly
//! until the deadline. Widget jobs have their own machinery — dedup, ownership
//! resolution, per-surface lanes — and wake from inside it. Background writes
//! wake through the calloop ingress channel rather than the ping, and there is
//! one of them; a type with a single instance encodes nothing but itself. A
//! parked focus request is the opposite invariant again: it *waits*, for a
//! widget that may not exist for many frames, so "still full" is its resting
//! state rather than a failure.
//!
//! The loop drains everything here unconditionally, once per iteration.

use std::cell::RefCell;

/// Deferred work that accumulates, drained by the loop once per iteration.
pub(crate) struct DeferredQueue<T> {
    items: RefCell<Vec<T>>,
}

impl<T> DeferredQueue<T> {
    pub(crate) const fn new() -> Self {
        Self {
            items: RefCell::new(Vec::new()),
        }
    }

    /// Queue an item and ask for the pass that will take it. One call,
    /// because they are one gesture.
    pub(crate) fn push(&self, item: T) {
        self.items.borrow_mut().push(item);
        crate::jobs::wake_loop();
    }

    /// Take everything queued, leaving the queue ready to receive more.
    ///
    /// Taking rather than iterating in place is what makes it safe for a
    /// drained item to queue another while it runs — a cleanup callback that
    /// disposes a second owner lands in the next batch instead of invalidating
    /// the borrow of this one.
    pub(crate) fn drain(&self) -> Vec<T> {
        std::mem::take(&mut *self.items.borrow_mut())
    }

    /// Whether anything is waiting. Nobody outside asks any more — the loop
    /// drains unconditionally rather than deciding whether to — so this is
    /// what the tests below assert against.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.items.borrow().is_empty()
    }

    /// Forget everything queued, for `App::drop`.
    pub(crate) fn clear(&self) {
        self.items.borrow_mut().clear();
    }

    /// Look at what is queued without taking it. For tests that assert *what*
    /// a gesture queued, which is the half `is_empty` cannot show.
    #[cfg(test)]
    pub(crate) fn with_items<R>(&self, f: impl FnOnce(&[T]) -> R) -> R {
        f(&self.items.borrow())
    }
}

/// Deferred work where the latest value is the only one that matters.
pub(crate) struct DeferredSlot<T> {
    item: RefCell<Option<T>>,
}

impl<T> DeferredSlot<T> {
    pub(crate) const fn new() -> Self {
        Self {
            item: RefCell::new(None),
        }
    }

    /// Put the value in the slot, replacing whatever was there, and ask for
    /// the pass that will take it.
    pub(crate) fn set(&self, value: T) {
        *self.item.borrow_mut() = Some(value);
        crate::jobs::wake_loop();
    }

    pub(crate) fn take(&self) -> Option<T> {
        self.item.borrow_mut().take()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.item.borrow().is_none()
    }

    /// Forget what is waiting, for `App::drop`.
    pub(crate) fn clear(&self) {
        *self.item.borrow_mut() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole reason both types exist: there is no way to reach the cell
    /// without the wakeup, so this is the only shape a producer can be
    /// written in. What the test can check is that the gesture is one call —
    /// the wakeup is `jobs`' to prove, and it does.
    #[test]
    fn a_queue_accumulates_and_a_slot_keeps_the_last() {
        let queue: DeferredQueue<u8> = DeferredQueue::new();
        assert!(queue.is_empty());
        queue.push(1);
        queue.push(2);
        assert!(!queue.is_empty());
        assert_eq!(queue.drain(), vec![1, 2], "a queue owes every item");
        assert!(queue.is_empty(), "and is ready for the next batch");

        let slot: DeferredSlot<u8> = DeferredSlot::new();
        assert!(slot.is_empty());
        slot.set(1);
        slot.set(2);
        assert_eq!(
            slot.take(),
            Some(2),
            "a slot owes the answer, not the history"
        );
        assert!(slot.is_empty());
    }
}
