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
        self.inner.value.read().unwrap().clone()
    }

    pub fn get_untracked(&self) -> T {
        self.inner.value.read().unwrap().clone()
    }
}

impl<T> Signal<T> {
    pub fn set(&self, value: T) {
        *self.inner.value.write().unwrap() = value;
        // Only notify if we're on the main thread (runtime available)
        try_with_runtime(|rt| rt.notify_write(self.inner.id));
        // Request a frame to be rendered
        request_frame();
    }

    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(&mut self.inner.value.write().unwrap());
        try_with_runtime(|rt| rt.notify_write(self.inner.id));
        // Request a frame to be rendered
        request_frame();
    }

    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        f(&self.inner.value.read().unwrap())
    }

    pub fn with_untracked<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&self.inner.value.read().unwrap())
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
        self.inner.value.read().unwrap().clone()
    }

    pub fn get_untracked(&self) -> T {
        self.inner.value.read().unwrap().clone()
    }
}

impl<T> ReadSignal<T> {
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        try_with_runtime(|rt| rt.track_read(self.inner.id));
        f(&self.inner.value.read().unwrap())
    }

    pub fn with_untracked<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        f(&self.inner.value.read().unwrap())
    }
}

/// Write-only handle to a signal.
#[derive(Clone)]
pub struct WriteSignal<T> {
    inner: Arc<SignalInner<T>>,
}

unsafe impl<T: Send + Sync> Send for WriteSignal<T> {}
unsafe impl<T: Send + Sync> Sync for WriteSignal<T> {}

impl<T> WriteSignal<T> {
    pub fn set(&self, value: T) {
        *self.inner.value.write().unwrap() = value;
        try_with_runtime(|rt| rt.notify_write(self.inner.id));
        // Request a frame to be rendered
        request_frame();
    }

    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        f(&mut self.inner.value.write().unwrap());
        try_with_runtime(|rt| rt.notify_write(self.inner.id));
        // Request a frame to be rendered
        request_frame();
    }
}

impl<T: Clone> WriteSignal<T> {
    /// Get the current value (useful for read-modify-write patterns)
    pub fn get(&self) -> T {
        self.inner.value.read().unwrap().clone()
    }
}

pub fn create_signal<T>(value: T) -> Signal<T> {
    Signal::new(value)
}
