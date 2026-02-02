use std::marker::PhantomData;

use super::invalidation::{notify_signal_change, record_signal_read};
use super::owner::register_signal;
use super::runtime::{SignalId, try_with_runtime};
use super::storage::{
    create_signal_value, get_signal_value, set_signal_value, update_signal_value, with_signal_value,
};

/// A reactive signal that can be read and written from any thread.
///
/// Signals are the core primitive of the reactive system. When a signal's
/// value changes, any effects that depend on it will be re-run (on the main thread).
///
/// Signals are Copy - they can be freely passed into closures without cloning.
///
/// # Thread Safety
/// Signal values can be read and written from any thread. However, effects
/// only run on the main thread. When you call `set()` from a background thread,
/// the value is updated immediately, but effect notification is skipped.
/// The UI will still update because the render loop reads signal values each frame.
pub struct Signal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

// Implement PartialEq by comparing SignalId.
// This allows Signal<T> to be stored in data structures that require PartialEq
// (e.g., Vec<ItemData> where ItemData contains Signal fields).
impl<T> PartialEq for Signal<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for Signal<T> {}

impl<T> Signal<T> {
    /// Get the internal signal ID
    pub fn id(&self) -> usize {
        self.id
    }
}

impl<T: Clone + Send + Sync + 'static> Signal<T> {
    /// Get the current value (tracks as dependency on main thread for effects)
    pub fn get(&self) -> T {
        // Track reads only on main thread (for effects)
        try_with_runtime(|rt| rt.track_read(self.id));
        // Track layout dependencies if layout tracking is active
        record_signal_read(self.id);
        // Get value from global storage (works from any thread)
        get_signal_value(self.id)
    }

    /// Get the current value without tracking
    pub fn get_untracked(&self) -> T {
        get_signal_value(self.id)
    }

    /// Borrow the value for reading
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        try_with_runtime(|rt| rt.track_read(self.id));
        record_signal_read(self.id);
        with_signal_value(self.id, f)
    }

    /// Borrow the value without tracking
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        with_signal_value(self.id, f)
    }
}

impl<T: Clone + PartialEq + Send + Sync + 'static> Signal<T> {
    /// Set a new value (notifies subscribers if changed)
    pub fn set(&self, value: T) {
        // Check if value changed
        let changed = with_signal_value(self.id, |old: &T| *old != value);
        if changed {
            set_signal_value(self.id, value);
            // Notify layout subscribers (widgets depending on this signal)
            notify_signal_change(self.id);
            // Notify runtime (only on main thread)
            try_with_runtime(|rt| rt.notify_write(self.id));
        }
    }

    /// Update the value using a closure
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        let changed = {
            let old = get_signal_value::<T>(self.id);
            update_signal_value(self.id, f);
            let new = get_signal_value::<T>(self.id);
            old != new
        };
        if changed {
            // Notify layout subscribers (widgets depending on this signal)
            notify_signal_change(self.id);
            try_with_runtime(|rt| rt.notify_write(self.id));
        }
    }

    /// Split into read and write handles
    pub fn split(self) -> (ReadSignal<T>, WriteSignal<T>) {
        (
            ReadSignal {
                id: self.id,
                _marker: PhantomData,
            },
            WriteSignal {
                id: self.id,
                _marker: PhantomData,
            },
        )
    }
}

/// Read-only handle to a signal.
pub struct ReadSignal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T> Clone for ReadSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ReadSignal<T> {}

impl<T> ReadSignal<T> {
    /// Get the internal signal ID
    pub fn id(&self) -> usize {
        self.id
    }
}

impl<T: Clone + Send + Sync + 'static> ReadSignal<T> {
    pub fn get(&self) -> T {
        try_with_runtime(|rt| rt.track_read(self.id));
        record_signal_read(self.id);
        get_signal_value(self.id)
    }

    pub fn get_untracked(&self) -> T {
        get_signal_value(self.id)
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        try_with_runtime(|rt| rt.track_read(self.id));
        record_signal_read(self.id);
        with_signal_value(self.id, f)
    }

    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        with_signal_value(self.id, f)
    }
}

/// Write-only handle to a signal.
pub struct WriteSignal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T> Clone for WriteSignal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for WriteSignal<T> {}

impl<T: Clone + PartialEq + Send + Sync + 'static> WriteSignal<T> {
    /// Sets the signal's value, only triggering updates if the value actually changed.
    pub fn set(&self, value: T) {
        let changed = with_signal_value(self.id, |old: &T| *old != value);
        if changed {
            set_signal_value(self.id, value);
            notify_signal_change(self.id);
            try_with_runtime(|rt| rt.notify_write(self.id));
        }
    }

    /// Updates the signal's value using a closure, only triggering updates if the value changed.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T),
    {
        let changed = {
            let old = get_signal_value::<T>(self.id);
            update_signal_value(self.id, f);
            let new = get_signal_value::<T>(self.id);
            old != new
        };
        if changed {
            notify_signal_change(self.id);
            try_with_runtime(|rt| rt.notify_write(self.id));
        }
    }

    /// Get the current value (useful for read-modify-write patterns)
    pub fn get(&self) -> T {
        get_signal_value(self.id)
    }
}

pub fn create_signal<T: Clone + PartialEq + Send + Sync + 'static>(value: T) -> Signal<T> {
    // Create value in global storage
    let id = create_signal_value(value);
    // Register with thread-local runtime for subscriber tracking
    try_with_runtime(|rt| rt.register_signal(id));
    // Register with current owner for automatic cleanup
    register_signal(id);
    Signal {
        id,
        _marker: PhantomData,
    }
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
        let signal2 = signal1; // Copy, not clone!

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

    #[test]
    fn test_signal_is_copy() {
        let signal = create_signal(42);
        let _copy1 = signal;
        let _copy2 = signal;
        // If Signal wasn't Copy, this wouldn't compile
        assert_eq!(signal.get(), 42);
    }
}
