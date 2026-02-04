//! Global thread-safe storage for signal values.
//!
//! Signal values are stored globally in a static `RwLock`-protected vector,
//! allowing signals to be read and written from any thread. Each signal value
//! is type-erased using `Any` and wrapped in `Arc<RwLock<_>>` for safe concurrent
//! access.
//!
//! ## Thread Safety
//!
//! - **Reading**: Multiple threads can read signal values concurrently.
//! - **Writing**: Writes acquire a write lock and update the value in place.
//! - **Disposal**: Disposed signals are marked as `None` and will panic if accessed.
//!
//! ## Type Safety
//!
//! Type information is erased at storage but recovered at access time via `downcast_ref`.
//! Accessing a signal with the wrong type will panic with a clear error message.

use std::any::Any;
use std::sync::{Arc, OnceLock, RwLock};

use super::runtime::SignalId;

type SignalValue = Arc<RwLock<Box<dyn Any + Send + Sync>>>;

static STORAGE: OnceLock<RwLock<SignalStorage>> = OnceLock::new();

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

fn with_storage<F, R>(f: F) -> R
where
    F: FnOnce(&mut SignalStorage) -> R,
{
    let storage = STORAGE.get_or_init(|| RwLock::new(SignalStorage::new()));
    f(&mut storage.write().unwrap())
}

fn with_storage_read<F, R>(f: F) -> R
where
    F: FnOnce(&SignalStorage) -> R,
{
    let storage = STORAGE.get_or_init(|| RwLock::new(SignalStorage::new()));
    f(&storage.read().unwrap())
}

/// Get the Arc for a signal, handling errors consistently.
fn get_signal_arc(id: SignalId, operation: &str) -> SignalValue {
    with_storage_read(|storage| {
        storage
            .values
            .get(id)
            .unwrap_or_else(|| {
                panic!(
                    "Invalid signal ID {}: out of bounds (max ID is {})",
                    id,
                    storage.values.len().saturating_sub(1)
                )
            })
            .clone()
            .unwrap_or_else(|| {
                panic!(
                    "Signal {} was disposed - cannot {} after owner cleanup. \
                     This usually means the signal's owner was disposed while you still hold a reference to the signal.",
                    id, operation
                )
            })
    })
}

/// Create a new signal and return its ID
pub fn create_signal_value<T: Send + Sync + 'static>(value: T) -> SignalId {
    with_storage(|storage| {
        let id = storage.next_id;
        storage.next_id += 1;
        let boxed: Box<dyn Any + Send + Sync> = Box::new(value);
        storage.values.push(Some(Arc::new(RwLock::new(boxed))));
        id
    })
}

/// Dispose a signal, marking it as unavailable.
///
/// After disposal, any attempt to read or write the signal will panic
/// with a clear error message.
pub fn dispose_signal(id: SignalId) {
    with_storage(|storage| {
        if id < storage.values.len() {
            storage.values[id] = None;
        }
    });
}

/// Downcast helper with consistent error message.
fn downcast_ref_or_panic<T: 'static>(value: &dyn Any, id: SignalId) -> &T {
    value.downcast_ref::<T>().unwrap_or_else(|| {
        panic!(
            "Signal {} type mismatch: stored type does not match requested type {}",
            id,
            std::any::type_name::<T>()
        )
    })
}

/// Downcast helper for mutable access with consistent error message.
fn downcast_mut_or_panic<T: 'static>(value: &mut dyn Any, id: SignalId) -> &mut T {
    // Get the type name before attempting downcast to avoid borrow issues
    let type_name = std::any::type_name::<T>();
    value.downcast_mut::<T>().unwrap_or_else(|| {
        panic!(
            "Signal {} type mismatch: stored type does not match requested type {}",
            id, type_name
        )
    })
}

/// Get a signal's value (clones it)
pub fn get_signal_value<T: Clone + Send + Sync + 'static>(id: SignalId) -> T {
    let arc = get_signal_arc(id, "read");
    let guard = arc.read().unwrap();
    downcast_ref_or_panic::<T>(guard.as_ref(), id).clone()
}

/// Set a signal's value
pub fn set_signal_value<T: Send + Sync + 'static>(id: SignalId, value: T) {
    let arc = get_signal_arc(id, "write");
    let mut guard = arc.write().unwrap();
    *guard = Box::new(value);
}

/// Update a signal's value with a closure
pub fn update_signal_value<T: Clone + Send + Sync + 'static>(id: SignalId, f: impl FnOnce(&mut T)) {
    let arc = get_signal_arc(id, "update");
    let mut guard = arc.write().unwrap();
    let value = downcast_mut_or_panic::<T>(guard.as_mut(), id);
    f(value);
}

/// Borrow a signal's value for reading
pub fn with_signal_value<T: Send + Sync + 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    let arc = get_signal_arc(id, "borrow");
    let guard = arc.read().unwrap();
    f(downcast_ref_or_panic::<T>(guard.as_ref(), id))
}
