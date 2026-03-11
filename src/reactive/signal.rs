use std::marker::PhantomData;

use super::invalidation::{notify_signal_change, record_signal_read};
use super::owner::register_signal;
use super::runtime::{
    SignalId, current_write_epoch, queue_bg_write, record_effect_read, try_with_runtime,
};
use super::storage::{
    allocate_signal_slot, compare_and_set_signal_value, compare_and_update_signal_value,
    create_signal_value, create_stored_value, get_signal_value, get_stored_value, has_signal,
    store_derived_closure, try_call_derived, with_signal_value, with_stored_value,
};

/// Implement Clone (via Copy), Copy, PartialEq (by SignalId), and Eq for a signal type.
macro_rules! impl_signal_id_traits {
    ($ty:ident) => {
        impl<T> Clone for $ty<T> {
            fn clone(&self) -> Self {
                *self
            }
        }
        impl<T> Copy for $ty<T> {}
        impl<T> PartialEq for $ty<T> {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }
        impl<T> Eq for $ty<T> {}
    };
}

/// Internal discriminant for the three signal kinds.
/// Same size as `bool` (1 byte) so `Signal<T>` stays at 16 bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SignalKind {
    /// Immutable stored value (`Rc<T>`, no RefCell, no tracking).
    Stored = 0,
    /// Reactive read-write value (`Rc<RefCell<T>>`, tracked).
    Mutable = 1,
    /// Closure-backed derived signal (HashMap lookup).
    Derived = 2,
}

/// Common read operations for signal types.
/// - Stored: no tracking (value never changes), reads `Rc<T>` directly
/// - Mutable: full effect + widget tracking, reads `Rc<RefCell<T>>`
/// - Derived: calls the closure (which tracks its own reads internally)
#[inline]
fn tracked_get<T: Clone + 'static>(id: SignalId, kind: SignalKind) -> T {
    match kind {
        SignalKind::Stored => get_stored_value(id),
        SignalKind::Derived => try_call_derived::<T>(id).expect("derived closure missing"),
        SignalKind::Mutable => {
            record_effect_read(id);
            record_signal_read(id);
            get_signal_value(id)
        }
    }
}

/// Common read-with-borrow operation for signal types.
/// - Stored: no tracking, borrows `Rc<T>` directly
/// - Mutable: full tracking, borrows through `Rc<RefCell<T>>`
/// - Derived: calls the closure and passes the result to `f`
#[inline]
fn tracked_with<T: Clone + 'static, R>(
    id: SignalId,
    kind: SignalKind,
    f: impl FnOnce(&T) -> R,
) -> R {
    match kind {
        SignalKind::Stored => with_stored_value(id, f),
        SignalKind::Derived => {
            let val = try_call_derived::<T>(id).unwrap();
            f(&val)
        }
        SignalKind::Mutable => {
            record_effect_read(id);
            record_signal_read(id);
            with_signal_value(id, f)
        }
    }
}

/// Perform a signal write with change detection and notification (main thread only).
fn write_and_notify<T: Clone + PartialEq + 'static>(id: SignalId, value: T) {
    if compare_and_set_signal_value(id, value) {
        notify_signal_change(id);
        try_with_runtime(|rt| rt.notify_write(id));
    }
}

/// Perform a signal update with change detection and notification (main thread only).
fn update_and_notify<T: Clone + PartialEq + 'static>(id: SignalId, f: impl FnOnce(&mut T)) {
    if compare_and_update_signal_value(id, f) {
        notify_signal_change(id);
        try_with_runtime(|rt| rt.notify_write(id));
    }
}

/// A read-only reactive signal.
///
/// `Signal<T>` provides read access to reactive values. It is returned by
/// [`create_stored`] (static values) and [`create_derived`] (closure-backed).
/// Widget properties accept `Signal<T>` via the [`IntoSignal`] trait.
///
/// To create a read-write signal, use [`create_signal`] which returns [`RwSignal<T>`].
///
/// Signals are `Copy` — they can be freely passed into closures without cloning.
///
/// # Thread Safety
///
/// `Signal<T>` is `!Send` — it can only be used on the main thread.
pub struct Signal<T> {
    id: SignalId,
    /// Discriminant for the three signal kinds (Stored, Mutable, Derived).
    kind: SignalKind,
    _marker: PhantomData<T>,
    _not_send: PhantomData<*const ()>,
}

impl_signal_id_traits!(Signal);

impl<T: Clone + 'static> Signal<T> {
    /// Get the current value (tracks as dependency for effects)
    pub fn get(&self) -> T {
        tracked_get(self.id, self.kind)
    }

    /// Get the current value without tracking
    pub fn get_untracked(&self) -> T {
        match self.kind {
            SignalKind::Stored => get_stored_value(self.id),
            SignalKind::Derived => try_call_derived::<T>(self.id).unwrap(),
            SignalKind::Mutable => get_signal_value(self.id),
        }
    }

    /// Borrow the value for reading
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        tracked_with(self.id, self.kind, f)
    }

    /// Borrow the value without tracking
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        match self.kind {
            SignalKind::Stored => with_stored_value(self.id, f),
            SignalKind::Derived => {
                let val = try_call_derived::<T>(self.id).unwrap();
                f(&val)
            }
            SignalKind::Mutable => with_signal_value(self.id, f),
        }
    }
}

/// A read-write reactive signal.
///
/// Created by [`create_signal`]. Provides both read and write access.
/// Can be converted to a read-only [`Signal<T>`] via [`read_only()`](RwSignal::read_only)
/// or the [`From`] impl. Widget properties accept `RwSignal<T>` via [`IntoSignal`].
///
/// `RwSignal<T>` is `Copy` (8 bytes — just a signal ID).
///
/// # Thread Safety
///
/// `RwSignal<T>` is `!Send`. Use [`.writer()`](RwSignal::writer) to get a
/// [`WriteSignal<T>`] which is `Send` for background thread writes.
pub struct RwSignal<T> {
    id: SignalId,
    _marker: PhantomData<T>,
    _not_send: PhantomData<*const ()>,
}

impl_signal_id_traits!(RwSignal);

impl<T: Clone + 'static> RwSignal<T> {
    /// Get the current value (tracks as dependency for effects)
    #[inline]
    pub fn get(&self) -> T {
        record_effect_read(self.id);
        record_signal_read(self.id);
        get_signal_value(self.id)
    }

    /// Get the current value without tracking
    #[inline]
    pub fn get_untracked(&self) -> T {
        get_signal_value(self.id)
    }

    /// Borrow the value for reading
    #[inline]
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        record_effect_read(self.id);
        record_signal_read(self.id);
        with_signal_value(self.id, f)
    }

    /// Borrow the value without tracking
    #[inline]
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        with_signal_value(self.id, f)
    }

    /// Convert to a read-only [`Signal<T>`].
    pub fn read_only(self) -> Signal<T> {
        Signal {
            id: self.id,
            kind: SignalKind::Mutable,
            _marker: PhantomData,
            _not_send: PhantomData,
        }
    }
}

impl<T: Clone + PartialEq + 'static> RwSignal<T> {
    /// Set a new value (notifies subscribers if changed)
    pub fn set(&self, value: T) {
        write_and_notify(self.id, value);
    }

    /// Update the value using a closure
    pub fn update<F: FnOnce(&mut T)>(&self, f: F) {
        update_and_notify(self.id, f);
    }
}

impl<T: Clone + PartialEq + Send + 'static> RwSignal<T> {
    /// Get a `WriteSignal<T>` for writing from background threads.
    ///
    /// `WriteSignal<T>` is `Send` and can be captured in `create_service` closures.
    /// Writes from background threads are queued and applied on the main thread
    /// at the start of the next frame.
    pub fn writer(&self) -> WriteSignal<T> {
        WriteSignal {
            id: self.id,
            epoch: current_write_epoch(),
            _marker: PhantomData,
        }
    }
}

impl<T: Clone + 'static> From<RwSignal<T>> for Signal<T> {
    fn from(rw: RwSignal<T>) -> Self {
        rw.read_only()
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
/// let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
///     while ctx.is_running() {
///         time_w.set(get_current_time()); // queued for main thread
///         tokio::time::sleep(Duration::from_secs(1)).await;
///     }
/// });
/// ```
pub struct WriteSignal<T> {
    id: SignalId,
    /// Write epoch captured when the writer was created. Writes queued from
    /// a stale epoch (after App restart) are silently discarded.
    epoch: u64,
    _marker: PhantomData<T>,
}

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
    /// Otherwise (background threads): queued for next frame with the epoch
    /// captured when this writer was created.
    pub fn set(&self, value: T) {
        if has_signal(self.id) {
            write_and_notify(self.id, value);
        } else {
            let id = self.id;
            let epoch = self.epoch;
            queue_bg_write(epoch, move || {
                write_and_notify(id, value);
            });
        }
    }

    /// Updates the signal's value using a closure, only triggering updates if the value changed.
    ///
    /// If the signal exists in the current thread's storage: applies immediately.
    /// Otherwise (background threads): queued for next frame with the epoch
    /// captured when this writer was created.
    pub fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut T) + Send + 'static,
    {
        if has_signal(self.id) {
            update_and_notify(self.id, f);
        } else {
            let id = self.id;
            let epoch = self.epoch;
            queue_bg_write(epoch, move || {
                update_and_notify(id, f);
            });
        }
    }
}

/// Create a read-write reactive signal.
///
/// Returns an [`RwSignal<T>`] that supports both reading and writing.
/// Converts to [`Signal<T>`] (read-only) via `.read_only()` or `Into`.
///
/// # Example
///
/// ```ignore
/// let count = create_signal(0);
/// count.set(1);           // write
/// count.get();            // read
/// container().padding(count) // auto-converts to Signal<T> via IntoSignal
/// ```
pub fn create_signal<T: Clone + PartialEq + Send + 'static>(value: T) -> RwSignal<T> {
    let id = create_signal_value(value);
    try_with_runtime(|rt| rt.register_signal(id));
    register_signal(id);
    RwSignal {
        id,
        _marker: PhantomData,
        _not_send: PhantomData,
    }
}

/// Create a read-only signal from a static value.
///
/// Unlike `create_signal`, this only requires `Clone` (no `PartialEq` or `Send`).
/// The returned `Signal<T>` is `Copy` and can be freely passed into closures.
/// It cannot be written to — there is no `set()` or `writer()`.
///
/// # Example
///
/// ```ignore
/// let color = create_stored(Color::RED);
/// container().background(color) // Copy, no clone needed
/// ```
pub fn create_stored<T: Clone + 'static>(value: T) -> Signal<T> {
    let id = create_stored_value(value);
    register_signal(id);
    Signal {
        id,
        kind: SignalKind::Stored,
        _marker: PhantomData,
        _not_send: PhantomData,
    }
}

/// Create a derived signal from a closure.
///
/// The closure is called on each `.get()` — there is no caching. The closure's
/// internal signal reads register dependencies directly with the calling
/// widget/effect, so there is no double-tracking.
///
/// Only requires `T: Clone` (no `PartialEq` or `Send`).
///
/// # Example
///
/// ```ignore
/// let count = create_signal(0);
/// let label = create_derived(move || format!("Count: {}", count.get()));
/// text(label) // Copy, reactive
/// ```
pub fn create_derived<T: Clone + 'static>(f: impl Fn() -> T + 'static) -> Signal<T> {
    let id = allocate_signal_slot();
    store_derived_closure::<T>(id, f);
    try_with_runtime(|rt| rt.register_signal(id));
    register_signal(id);
    Signal {
        id,
        kind: SignalKind::Derived,
        _marker: PhantomData,
        _not_send: PhantomData,
    }
}

/// Extension trait for `Option<Signal<T>>` to support lazy-default widget properties.
///
/// Widget properties stored as `Option<Signal<T>>` start as `None` (zero allocation).
/// A signal is only created when the user sets the property via a builder method.
/// Read sites use `get_or` / `get_or_else` to provide a default without allocating.
pub trait OptionSignalExt<T: Clone + 'static> {
    /// Get the signal's value, or return `default` if no signal exists.
    fn get_or(&self, default: T) -> T;

    /// Get the signal's value, or compute a default if no signal exists.
    fn get_or_else(&self, f: impl FnOnce() -> T) -> T;

    /// Return the existing signal, or create a stored signal from `default`
    /// and write it back into `self` for future use.
    fn signal_or(&mut self, default: T) -> Signal<T>;
}

impl<T: Clone + 'static> OptionSignalExt<T> for Option<Signal<T>> {
    fn get_or(&self, default: T) -> T {
        match self {
            Some(s) => s.get(),
            None => default,
        }
    }

    fn get_or_else(&self, f: impl FnOnce() -> T) -> T {
        match self {
            Some(s) => s.get(),
            None => f(),
        }
    }

    fn signal_or(&mut self, default: T) -> Signal<T> {
        match self {
            Some(s) => *s,
            None => {
                let s = create_stored(default);
                *self = Some(s);
                s
            }
        }
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
    fn test_rw_signal_is_copy() {
        let signal = create_signal(42);
        let _copy1 = signal;
        let _copy2 = signal;
        assert_eq!(signal.get(), 42);
    }

    #[test]
    fn test_rw_signal_to_signal() {
        let rw = create_signal(42);
        let read_only: Signal<i32> = rw.into();
        assert_eq!(read_only.get(), 42);
        rw.set(100);
        assert_eq!(read_only.get(), 100);
    }

    #[test]
    fn test_rw_signal_size() {
        assert_eq!(std::mem::size_of::<RwSignal<i32>>(), 8);
        assert_eq!(std::mem::size_of::<Signal<i32>>(), 16);
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
