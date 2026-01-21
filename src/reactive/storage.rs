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

/// Get a signal's value (clones it)
pub fn get_signal_value<T: Clone + Send + Sync + 'static>(id: SignalId) -> T {
    let arc = with_storage_read(|storage| storage.values[id].clone().expect("Signal disposed"));
    let guard = arc.read().unwrap();
    guard.downcast_ref::<T>().expect("Type mismatch").clone()
}

/// Set a signal's value
pub fn set_signal_value<T: Send + Sync + 'static>(id: SignalId, value: T) {
    let arc = with_storage_read(|storage| storage.values[id].clone().expect("Signal disposed"));
    let mut guard = arc.write().unwrap();
    *guard = Box::new(value);
}

/// Update a signal's value with a closure
pub fn update_signal_value<T: Clone + Send + Sync + 'static>(id: SignalId, f: impl FnOnce(&mut T)) {
    let arc = with_storage_read(|storage| storage.values[id].clone().expect("Signal disposed"));
    let mut guard = arc.write().unwrap();
    let value = guard.downcast_mut::<T>().expect("Type mismatch");
    f(value);
}

/// Borrow a signal's value for reading
pub fn with_signal_value<T: Send + Sync + 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    let arc = with_storage_read(|storage| storage.values[id].clone().expect("Signal disposed"));
    let guard = arc.read().unwrap();
    f(guard.downcast_ref::<T>().expect("Type mismatch"))
}
