//! Minimal scope-guard utility for unwind-safe state restoration.
//!
//! The reactive system maintains several pieces of thread-local scope state
//! (current owner, tracking contexts, batch depth). All of them must be
//! restored when the scope exits — including via panic. A panic that escapes
//! (or is caught by user code with `catch_unwind`) must never leave the
//! reactive system permanently corrupted.

/// Run `f` when the returned guard is dropped, including during unwind.
pub(crate) fn defer<F: FnOnce()>(f: F) -> impl Drop {
    struct Deferred<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for Deferred<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f();
            }
        }
    }
    Deferred(Some(f))
}
