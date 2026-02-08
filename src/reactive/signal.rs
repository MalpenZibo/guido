use std::marker::PhantomData;

use super::invalidation::{
    notify_signal_change, record_signal_read, register_signal_callback, unregister_signal_callback,
};
use super::owner::{on_cleanup, register_signal};
use super::runtime::{SignalId, queue_bg_write, record_effect_read, try_with_runtime};
use super::storage::{
    create_signal_value, get_signal_value, has_signal, set_signal_value, update_signal_value,
    with_signal_value,
};

/// Common read operations for signal types.
/// Tracks reads for effect dependencies and layout invalidation.
fn tracked_get<T: Clone + 'static>(id: SignalId) -> T {
    record_effect_read(id);
    try_with_runtime(|rt| rt.track_read(id));
    record_signal_read(id);
    get_signal_value(id)
}

/// Common read-with-borrow operation for signal types.
fn tracked_with<T: 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    record_effect_read(id);
    try_with_runtime(|rt| rt.track_read(id));
    record_signal_read(id);
    with_signal_value(id, f)
}

/// Perform a signal write with change detection and notification (main thread only).
fn write_and_notify<T: Clone + PartialEq + 'static>(id: SignalId, value: T) {
    let changed = with_signal_value(id, |old: &T| *old != value);
    if changed {
        set_signal_value(id, value);
        notify_signal_change(id);
        try_with_runtime(|rt| rt.notify_write(id));
    }
}

/// Perform a signal update with change detection and notification (main thread only).
fn update_and_notify<T: Clone + PartialEq + 'static>(id: SignalId, f: impl FnOnce(&mut T)) {
    let old = get_signal_value::<T>(id);
    update_signal_value(id, f);
    let changed = with_signal_value(id, |new: &T| old != *new);
    if changed {
        notify_signal_change(id);
        try_with_runtime(|rt| rt.notify_write(id));
    }
}

/// A reactive signal that can be read and written on the main thread.
///
/// Signals are the core primitive of the reactive system. When a signal's
/// value changes, any effects that depend on it will be re-run.
///
/// Signals are Copy - they can be freely passed into closures without cloning.
///
/// # Thread Safety
///
/// `Signal<T>` is `!Send` — it can only be used on the main thread where it
/// was created. To write from a background thread, use [`Signal::writer()`]
/// to get a [`WriteSignal<T>`] which is `Send`.
pub struct Signal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
    _not_send: PhantomData<*const ()>, // makes Signal !Send !Sync
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

// Implement PartialEq by comparing SignalId.
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

impl<T: Clone + 'static> Signal<T> {
    /// Get the current value (tracks as dependency for effects)
    pub fn get(&self) -> T {
        tracked_get(self.id)
    }

    /// Get the current value without tracking
    pub fn get_untracked(&self) -> T {
        get_signal_value(self.id)
    }

    /// Borrow the value for reading
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        tracked_with(self.id, f)
    }

    /// Borrow the value without tracking
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        with_signal_value(self.id, f)
    }
}

impl<T: Clone + PartialEq + Send + 'static> Signal<T> {
    /// Get a `WriteSignal<T>` for writing from background threads.
    ///
    /// `WriteSignal<T>` is `Send` and can be captured in `create_service` closures.
    /// Writes from background threads are queued and applied on the main thread
    /// at the start of the next frame.
    pub fn writer(&self) -> WriteSignal<T> {
        WriteSignal {
            id: self.id,
            _marker: PhantomData,
        }
    }
}

impl<T: Clone + PartialEq + 'static> Signal<T> {
    /// Set a new value (notifies subscribers if changed)
    pub fn set(&self, value: T) {
        write_and_notify(self.id, value);
    }

    /// Update the value using a closure
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        update_and_notify(self.id, f);
    }

    /// Split into read and write handles
    pub fn split(self) -> (ReadSignal<T>, WriteSignal<T>) {
        (
            ReadSignal {
                id: self.id,
                _marker: PhantomData,
                _not_send: PhantomData,
            },
            WriteSignal {
                id: self.id,
                _marker: PhantomData,
            },
        )
    }

    /// Create a derived signal that tracks a specific field of this signal's value.
    ///
    /// The selector function borrows the parent value and returns a reference to
    /// a field. The derived signal only updates (clones) when the selected field
    /// actually changes, avoiding unnecessary clones of the entire parent object.
    ///
    /// This uses the invalidation system, NOT effects, so it works correctly
    /// with background thread updates from `create_service`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let data = create_signal(MyStruct { name: "Alice".into(), count: 0 });
    /// let name: Signal<String> = data.select(|d| &d.name);
    ///
    /// // `name` only updates when the `name` field actually changes
    /// data.update(|d| d.count += 1); // name signal NOT updated
    /// data.update(|d| d.name = "Bob".into()); // name signal IS updated
    /// ```
    pub fn select<U, F>(&self, f: F) -> Signal<U>
    where
        U: Clone + PartialEq + Send + 'static,
        F: Fn(&T) -> &U + 'static,
    {
        let source = *self;
        let derived = create_signal(self.with(|v| f(v).clone()));

        let callback_id = register_signal_callback(source.id(), move || {
            source.with(|parent| {
                let field_ref: &U = f(parent);
                let changed = derived.with_untracked(|current| current != field_ref);
                if changed {
                    derived.set(field_ref.clone());
                }
            });
        });

        let source_id = source.id();
        on_cleanup(move || {
            unregister_signal_callback(source_id, callback_id);
        });

        derived
    }
}

/// Read-only handle to a signal.
pub struct ReadSignal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
    _not_send: PhantomData<*const ()>, // makes ReadSignal !Send !Sync
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

impl<T: Clone + 'static> ReadSignal<T> {
    pub fn get(&self) -> T {
        tracked_get(self.id)
    }

    pub fn get_untracked(&self) -> T {
        get_signal_value(self.id)
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        tracked_with(self.id, f)
    }

    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        with_signal_value(self.id, f)
    }
}

impl<T: Clone + PartialEq + 'static> ReadSignal<T> {
    /// Create a derived signal that tracks a specific field of this signal's value.
    ///
    /// See [`Signal::select()`] for full documentation.
    pub fn select<U, F>(&self, f: F) -> Signal<U>
    where
        U: Clone + PartialEq + Send + 'static,
        F: Fn(&T) -> &U + 'static,
    {
        let source_id = self.id;
        let derived = create_signal(self.with(|v| f(v).clone()));

        let callback_id = register_signal_callback(source_id, move || {
            with_signal_value(source_id, |parent: &T| {
                let field_ref: &U = f(parent);
                let changed = derived.with_untracked(|current| current != field_ref);
                if changed {
                    derived.set(field_ref.clone());
                }
            });
        });

        on_cleanup(move || {
            unregister_signal_callback(source_id, callback_id);
        });

        derived
    }
}

/// Write-only handle to a signal. `Send` — can be used from background threads.
///
/// On the main thread, writes are applied immediately with change detection.
/// On background threads, writes are queued and applied at the start of the
/// next frame on the main thread.
///
/// # Example
///
/// ```ignore
/// let time = create_signal(get_current_time());
/// let time_w = time.writer();
///
/// let _ = create_service::<(), _>(move |_rx, ctx| {
///     while ctx.is_running() {
///         time_w.set(get_current_time()); // queued for main thread
///         std::thread::sleep(Duration::from_secs(1));
///     }
/// });
/// ```
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

impl<T: Clone + PartialEq + Send + 'static> WriteSignal<T> {
    /// Sets the signal's value, only triggering updates if the value actually changed.
    ///
    /// If the signal exists in the current thread's storage: applies immediately.
    /// Otherwise (background threads): queued for next frame.
    pub fn set(&self, value: T) {
        if has_signal(self.id) {
            write_and_notify(self.id, value);
        } else {
            let id = self.id;
            queue_bg_write(move || {
                write_and_notify(id, value);
            });
        }
    }

    /// Updates the signal's value using a closure, only triggering updates if the value changed.
    ///
    /// If the signal exists in the current thread's storage: applies immediately.
    /// Otherwise (background threads): queued for next frame.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T) + Send + 'static,
    {
        if has_signal(self.id) {
            update_and_notify(self.id, f);
        } else {
            let id = self.id;
            queue_bg_write(move || {
                update_and_notify(id, f);
            });
        }
    }

    /// Get the current value (useful for read-modify-write patterns on main thread)
    pub fn get(&self) -> T {
        get_signal_value(self.id)
    }
}

pub fn create_signal<T: Clone + PartialEq + Send + 'static>(value: T) -> Signal<T> {
    // Create value in thread-local storage
    let id = create_signal_value(value);
    // Register with thread-local runtime for subscriber tracking
    try_with_runtime(|rt| rt.register_signal(id));
    // Register with current owner for automatic cleanup
    register_signal(id);
    Signal {
        id,
        _marker: PhantomData,
        _not_send: PhantomData,
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

    // ================================================================
    // select() tests
    // ================================================================

    #[derive(Clone, Debug, PartialEq)]
    struct Inner {
        value: i32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Outer {
        name: String,
        count: i32,
        inner: Inner,
    }

    #[test]
    fn test_select_returns_field_value() {
        let source = create_signal(Outer {
            name: "Alice".into(),
            count: 0,
            inner: Inner { value: 42 },
        });
        let name = source.select(|o| &o.name);
        assert_eq!(name.get(), "Alice");
    }

    #[test]
    fn test_select_updates_when_field_changes() {
        let source = create_signal(Outer {
            name: "Alice".into(),
            count: 0,
            inner: Inner { value: 42 },
        });
        let name = source.select(|o| &o.name);

        source.update(|o| o.name = "Bob".into());
        assert_eq!(name.get(), "Bob");
    }

    #[test]
    fn test_select_no_update_when_field_unchanged() {
        let source = create_signal(Outer {
            name: "Alice".into(),
            count: 0,
            inner: Inner { value: 42 },
        });
        let name = source.select(|o| &o.name);
        let count = source.select(|o| &o.count);

        // Change only count — name should remain unchanged
        source.update(|o| o.count += 1);
        assert_eq!(name.get(), "Alice");
        assert_eq!(count.get(), 1);

        // Verify name signal ID hasn't changed (same derived signal)
        let name_id = name.id();
        source.update(|o| o.count += 1);
        assert_eq!(name.id(), name_id);
        assert_eq!(name.get(), "Alice");
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn test_select_chaining() {
        let source = create_signal(Outer {
            name: "Alice".into(),
            count: 0,
            inner: Inner { value: 42 },
        });
        let inner = source.select(|o| &o.inner);
        let value = inner.select(|i| &i.value);

        assert_eq!(value.get(), 42);

        source.update(|o| o.inner.value = 99);
        assert_eq!(value.get(), 99);

        // Changing unrelated field should not affect chained selects
        source.update(|o| o.name = "Bob".into());
        assert_eq!(value.get(), 99);
    }

    #[test]
    fn test_select_on_read_signal() {
        let source = create_signal(Outer {
            name: "Alice".into(),
            count: 0,
            inner: Inner { value: 42 },
        });
        let (read, write) = source.split();

        let name = read.select(|o| &o.name);
        assert_eq!(name.get(), "Alice");

        write.update(|o| o.name = "Bob".into());
        assert_eq!(name.get(), "Bob");
    }

    // ================================================================
    // writer() tests
    // ================================================================

    #[test]
    fn test_writer_set_on_main_thread() {
        let signal = create_signal(42);
        let writer = signal.writer();
        writer.set(100);
        assert_eq!(signal.get(), 100);
    }

    #[test]
    fn test_writer_update_on_main_thread() {
        let signal = create_signal(10);
        let writer = signal.writer();
        writer.update(|v| *v += 5);
        assert_eq!(signal.get(), 15);
    }
}
