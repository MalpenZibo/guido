//! Thread-local storage for signal values.
//!
//! Signal values are stored in a thread-local `RefCell`-protected vector,
//! using `Box<dyn Any>` to erase `RefCell<T>` for each signal. This eliminates
//! all locking overhead since signals are only accessed from the main thread.
//!
//! ## Thread Safety
//!
//! - **Reading**: Direct `RefCell` borrow — zero locks.
//! - **Writing**: Direct `RefCell` borrow_mut — zero locks.
//! - **Background writes**: Queued via `WriteSignal` and flushed each frame.
//! - **Disposal**: Disposed signals are marked as `None` and will panic if accessed.
//!
//! ## Type Safety
//!
//! Type information is erased at storage but recovered at access time via `downcast_ref`.
//! Accessing a signal with the wrong type will panic with a clear error message.

use std::any::Any;
use std::cell::RefCell;

use super::runtime::SignalId;

type SignalValue = Box<dyn Any>;

struct SignalStorage {
    values: Vec<Option<SignalValue>>,
    next_id: SignalId,
}

impl SignalStorage {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            next_id: 0,
        }
    }
}

thread_local! {
    static STORAGE: RefCell<SignalStorage> = RefCell::new(SignalStorage::new());
}

/// Get a reference to the signal's RefCell, handling errors consistently.
fn with_signal_cell<T: 'static, R>(
    id: SignalId,
    operation: &str,
    f: impl FnOnce(&RefCell<T>) -> R,
) -> R {
    STORAGE.with(|storage| {
        let storage = storage.borrow();
        let slot = storage
            .values
            .get(id)
            .unwrap_or_else(|| {
                panic!(
                    "Invalid signal ID {}: out of bounds (max ID is {})",
                    id,
                    storage.values.len().saturating_sub(1)
                )
            })
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "Signal {} was disposed - cannot {} after owner cleanup. \
                     This usually means the signal's owner was disposed while you still hold a reference to the signal.",
                    id, operation
                )
            });
        let cell = slot.downcast_ref::<RefCell<T>>().unwrap_or_else(|| {
            panic!(
                "Signal {} type mismatch: stored type does not match requested type {}",
                id,
                std::any::type_name::<T>()
            )
        });
        f(cell)
    })
}

/// Create a new signal and return its ID
pub fn create_signal_value<T: 'static>(value: T) -> SignalId {
    STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        let id = storage.next_id;
        storage.next_id += 1;
        let boxed: Box<dyn Any> = Box::new(RefCell::new(value));
        storage.values.push(Some(boxed));
        id
    })
}

/// Dispose a signal, marking it as unavailable.
///
/// After disposal, any attempt to read or write the signal will panic
/// with a clear error message.
pub fn dispose_signal(id: SignalId) {
    STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        if id < storage.values.len() {
            storage.values[id] = None;
        }
    });
}

/// Get a signal's value (clones it)
pub fn get_signal_value<T: Clone + 'static>(id: SignalId) -> T {
    with_signal_cell(id, "read", |cell: &RefCell<T>| cell.borrow().clone())
}

/// Set a signal's value (in-place replace, no Box allocation)
pub fn set_signal_value<T: 'static>(id: SignalId, value: T) {
    with_signal_cell(id, "write", |cell: &RefCell<T>| {
        *cell.borrow_mut() = value;
    });
}

/// Update a signal's value with a closure. Returns the closure's result.
pub fn update_signal_value<T: 'static, R>(id: SignalId, f: impl FnOnce(&mut T) -> R) -> R {
    with_signal_cell(id, "update", |cell: &RefCell<T>| f(&mut cell.borrow_mut()))
}

/// Borrow a signal's value for reading
pub fn with_signal_value<T: 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    with_signal_cell(id, "borrow", |cell: &RefCell<T>| f(&cell.borrow()))
}

/// Check if a signal exists in the current thread's storage.
/// Used by `WriteSignal` to determine if we can write directly (same thread)
/// or must queue the write for the main thread.
pub fn has_signal(id: SignalId) -> bool {
    STORAGE.with(|storage| {
        let storage = storage.borrow();
        storage.values.get(id).and_then(|v| v.as_ref()).is_some()
    })
}
