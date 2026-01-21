use std::sync::{Arc, RwLock};

use super::invalidation::request_frame;
use super::runtime::{try_with_runtime, with_runtime, SignalId};

struct SignalInner<T> {
    id: SignalId,
    value: RwLock<T>,
}

/// A reactive signal that can be read and written from any thread.
///
/// Signals are the core primitive of the reactive system. When a signal's
/// value changes, any effects that depend on it will be re-run (on the main thread).
///
/// # Thread Safety
/// Signal values can be read and written from any thread. However, effects
/// only run on the main thread. When you call `set()` from a background thread,
/// the value is updated immediately, but effect notification is skipped.
/// The UI will still update because the render loop reads signal values each frame.
#[derive(Clone)]
pub struct Signal<T> {
    inner: Arc<SignalInner<T>>,
}

// Signal is Send + Sync when T is Send + Sync
unsafe impl<T: Send + Sync> Send for Signal<T> {}
unsafe impl<T: Send + Sync> Sync for Signal<T> {}

impl<T> Signal<T> {
    pub fn new(value: T) -> Self {
        let id = with_runtime(|rt| rt.allocate_signal());
        Self {
            inner: Arc::new(SignalInner {
                id,
                value: RwLock::new(value),
            }),
        }
    }

    pub fn split(self) -> (ReadSignal<T>, WriteSignal<T>) {
        (
            ReadSignal {
                inner: self.inner.clone(),
            },
            WriteSignal { inner: self.inner },
        )
    }
}

impl<T: Clone> Signal<T> {
    pub fn get(&self) -> T {
        // Only track reads if we're on the main thread (runtime available)
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        self.inner
            .value
            .read()
            .expect("signal lock poisoned")
            .clone()
    }

    pub fn get_untracked(&self) -> T {
        self.inner
            .value
            .read()
            .expect("signal lock poisoned")
            .clone()
    }
}

impl<T: PartialEq> Signal<T> {
    /// Sets the signal's value, only triggering updates if the value actually changed.
    pub fn set(&self, value: T) {
        let Ok(mut guard) = self.inner.value.write() else {
            return; // Lock poisoned, skip update silently
        };
        if *guard != value {
            *guard = value;
            drop(guard);
            // Only notify if we're on the main thread (runtime available)
            try_with_runtime(|rt| rt.notify_write(self.inner.id));
            // Request a frame to be rendered
            request_frame();
        }
    }
}

impl<T: PartialEq + Clone> Signal<T> {
    /// Updates the signal's value using a closure, only triggering updates if the value changed.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let Ok(mut guard) = self.inner.value.write() else {
            return; // Lock poisoned, skip update silently
        };
        let old_value = guard.clone();
        f(&mut *guard);
        if *guard != old_value {
            drop(guard);
            try_with_runtime(|rt| rt.notify_write(self.inner.id));
            // Request a frame to be rendered
            request_frame();
        }
    }
}

impl<T> Signal<T> {
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        f(&self.inner.value.read().expect("signal lock poisoned"))
    }

    pub fn with_untracked<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&self.inner.value.read().expect("signal lock poisoned"))
    }
}

/// Read-only handle to a signal.
#[derive(Clone)]
pub struct ReadSignal<T> {
    inner: Arc<SignalInner<T>>,
}

unsafe impl<T: Send + Sync> Send for ReadSignal<T> {}
unsafe impl<T: Send + Sync> Sync for ReadSignal<T> {}

impl<T: Clone> ReadSignal<T> {
    pub fn get(&self) -> T {
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        self.inner
            .value
            .read()
            .expect("signal lock poisoned")
            .clone()
    }

    pub fn get_untracked(&self) -> T {
        self.inner
            .value
            .read()
            .expect("signal lock poisoned")
            .clone()
    }
}

impl<T> ReadSignal<T> {
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        f(&self.inner.value.read().expect("signal lock poisoned"))
    }

    pub fn with_untracked<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&self.inner.value.read().expect("signal lock poisoned"))
    }
}

/// Write-only handle to a signal.
#[derive(Clone)]
pub struct WriteSignal<T> {
    inner: Arc<SignalInner<T>>,
}

unsafe impl<T: Send + Sync> Send for WriteSignal<T> {}
unsafe impl<T: Send + Sync> Sync for WriteSignal<T> {}

impl<T: PartialEq> WriteSignal<T> {
    /// Sets the signal's value, only triggering updates if the value actually changed.
    pub fn set(&self, value: T) {
        let Ok(mut guard) = self.inner.value.write() else {
            return; // Lock poisoned, skip update silently
        };
        if *guard != value {
            *guard = value;
            drop(guard);
            try_with_runtime(|rt| rt.notify_write(self.inner.id));
            // Request a frame to be rendered
            request_frame();
        }
    }
}

impl<T: PartialEq + Clone> WriteSignal<T> {
    /// Updates the signal's value using a closure, only triggering updates if the value changed.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let Ok(mut guard) = self.inner.value.write() else {
            return; // Lock poisoned, skip update silently
        };
        let old_value = guard.clone();
        f(&mut *guard);
        if *guard != old_value {
            drop(guard);
            try_with_runtime(|rt| rt.notify_write(self.inner.id));
            // Request a frame to be rendered
            request_frame();
        }
    }
}

impl<T: Clone> WriteSignal<T> {
    /// Get the current value (useful for read-modify-write patterns)
    pub fn get(&self) -> T {
        self.inner
            .value
            .read()
            .expect("signal lock poisoned")
            .clone()
    }
}

pub fn create_signal<T>(value: T) -> Signal<T> {
    Signal::new(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_signal_and_get() {
        let signal = create_signal(42);
        assert_eq!(signal.get(), 42);
    }

    #[test]
    fn test_set_updates_value() {
        let signal = create_signal(10);
        signal.set(20);
        assert_eq!(signal.get(), 20);
    }

    #[test]
    fn test_update_with_closure() {
        let signal = create_signal(5);
        signal.update(|v| *v += 10);
        assert_eq!(signal.get(), 15);
    }

    #[test]
    fn test_with_for_borrowing() {
        let signal = create_signal(String::from("hello"));
        let length = signal.with(|s| s.len());
        assert_eq!(length, 5);
    }

    #[test]
    fn test_get_untracked() {
        let signal = create_signal(100);
        let value = signal.get_untracked();
        assert_eq!(value, 100);
    }

    #[test]
    fn test_split_into_read_write_handles() {
        let signal = create_signal(7);
        let (read, write) = signal.split();

        assert_eq!(read.get(), 7);
        write.set(14);
        assert_eq!(read.get(), 14);
    }

    #[test]
    fn test_clone_shares_underlying_value() {
        let signal1 = create_signal(50);
        let signal2 = signal1.clone();

        signal1.set(75);
        assert_eq!(signal2.get(), 75);

        signal2.set(100);
        assert_eq!(signal1.get(), 100);
    }

    #[test]
    fn test_with_untracked() {
        let signal = create_signal(String::from("test"));
        let result = signal.with_untracked(|s| format!("{}ing", s));
        assert_eq!(result, "testing");
    }

    #[test]
    fn test_update_only_triggers_on_change() {
        let signal = create_signal(10);
        signal.update(|v| *v = 10); // No actual change
        assert_eq!(signal.get(), 10);
    }

    #[test]
    fn test_set_only_triggers_on_change() {
        let signal = create_signal(5);
        signal.set(5); // No actual change
        assert_eq!(signal.get(), 5);
        signal.set(10); // Actual change
        assert_eq!(signal.get(), 10);
    }
}
