//! Thread-local storage for signal values.
//!
//! Signal values are stored in a thread-local `RefCell`-protected vector,
//! using `Rc<dyn Any>` to erase `RefCell<T>` for each signal. This eliminates
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
use std::collections::HashMap;
use std::rc::Rc;

use super::runtime::SignalId;

type SignalValue = Rc<dyn Any>;

/// Storage slot for one signal. The generation survives disposal so recycled
/// indices can be told apart from their previous occupants — a stale
/// `SignalId` fails the generation check instead of silently aliasing.
struct Slot {
    value: Option<SignalValue>,
    generation: u32,
}

struct SignalStorage {
    slots: Vec<Slot>,
    /// Vacant slot indices available for reuse.
    free_indices: Vec<u32>,
    /// Derived closures keyed by SignalId. When a signal has a derived closure,
    /// `.get()` calls the closure instead of reading from `slots`.
    derived: HashMap<SignalId, Rc<dyn Any>>,
}

impl SignalStorage {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_indices: Vec::new(),
            derived: HashMap::new(),
        }
    }
}

thread_local! {
    static STORAGE: RefCell<SignalStorage> = RefCell::new(SignalStorage::new());
}

/// Briefly borrow storage to Rc::clone a signal's value handle.
///
/// Leptos-style: clone the Rc (O(1)), release the storage borrow, then let
/// the caller work with the value. Prevents re-entrant borrow panics when
/// user callbacks create new signals.
fn clone_slot_rc(id: SignalId, operation: &str) -> Rc<dyn Any> {
    STORAGE.with(|storage| {
        let storage = storage.borrow();
        let slot = storage.slots.get(id.index()).unwrap_or_else(|| {
            panic!(
                "Invalid signal ID {}: out of bounds (storage has {} slots)",
                id,
                storage.slots.len()
            )
        });
        // A generation mismatch means the slot was recycled: the signal this
        // handle referred to is gone, same as the vacant-slot case below.
        if slot.generation != id.generation() {
            panic!(
                "Signal {} was disposed - cannot {} after owner cleanup. \
                 This usually means the signal's owner was disposed while you still hold a reference to the signal.",
                id, operation
            );
        }
        let value = slot.value.as_ref().unwrap_or_else(|| {
            panic!(
                "Signal {} was disposed - cannot {} after owner cleanup. \
                 This usually means the signal's owner was disposed while you still hold a reference to the signal.",
                id, operation
            )
        });
        Rc::clone(value)
    })
}

/// Like [`clone_slot_rc`] but returns None for vacant/recycled slots
/// instead of panicking. Used by write paths: writes race disposal by
/// design (queued `WriteSignal` flushes, delayed timers) and must degrade
/// to a no-op, not a crash.
fn try_clone_slot_rc(id: SignalId) -> Option<Rc<dyn Any>> {
    STORAGE.with(|storage| {
        let storage = storage.borrow();
        let slot = storage.slots.get(id.index())?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.value.as_ref().map(Rc::clone)
    })
}

/// Get a reference to the signal's RefCell, handling errors consistently.
fn with_signal_cell<T: 'static, R>(
    id: SignalId,
    operation: &str,
    f: impl FnOnce(&RefCell<T>) -> R,
) -> R {
    let rc = clone_slot_rc(id, operation);
    let cell = rc.downcast_ref::<RefCell<T>>().unwrap_or_else(|| {
        panic!(
            "Signal {} type mismatch: stored type does not match requested type {}",
            id,
            std::any::type_name::<T>()
        )
    });
    f(cell)
}

/// Allocate a slot and store the given value. Reuses slot indices from
/// disposed signals, bumping the generation so stale handles stay invalid.
fn alloc_slot(value: Rc<dyn Any>) -> SignalId {
    STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        if let Some(index) = storage.free_indices.pop() {
            let slot = &mut storage.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.value = Some(value);
            return SignalId::new(index, slot.generation);
        }
        let index = storage.slots.len() as u32;
        storage.slots.push(Slot {
            value: Some(value),
            generation: 0,
        });
        SignalId::new(index, 0)
    })
}

/// Create a new mutable signal value (`Rc<RefCell<T>>`) and return its ID.
pub fn create_signal_value<T: 'static>(value: T) -> SignalId {
    alloc_slot(Rc::new(RefCell::new(value)))
}

/// Create a new immutable stored value (`Rc<T>`, no RefCell) and return its ID.
///
/// This is the cheap path for `create_stored()`: no RefCell wrapping, and the
/// caller skips runtime registration and dependency tracking. Saves per-signal:
/// - 8 bytes (no RefCell borrow flag)
/// - One `Vec::push` in runtime's `signal_subscribers`
/// - `record_effect_read()` + `record_signal_read()` on every `.get()` call
pub fn create_stored_value<T: 'static>(value: T) -> SignalId {
    alloc_slot(Rc::new(value))
}

/// Allocate a signal ID slot without storing a value.
/// Used by `create_derived` — the ID exists in the runtime/owner system,
/// but reads go through the derived closure map instead.
pub fn allocate_signal_slot() -> SignalId {
    alloc_slot(Rc::new(()))
}

/// Store a derived closure for the given signal ID.
pub fn store_derived_closure<T: Clone + 'static>(id: SignalId, closure: impl Fn() -> T + 'static) {
    STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        let boxed: Box<dyn Fn() -> T> = Box::new(closure);
        storage.derived.insert(id, Rc::new(boxed));
    });
}

/// Try to call a derived closure for the given signal ID.
/// Returns `Some(value)` if a derived closure exists, `None` otherwise.
///
/// Leptos-style: Rc::clone the closure handle and release the storage borrow
/// before calling the closure (which will read other signals from storage).
pub fn try_call_derived<T: Clone + 'static>(id: SignalId) -> Option<T> {
    // Phase 1: briefly borrow storage to Rc::clone the closure handle
    let closure_rc: Option<Rc<dyn Any>> =
        STORAGE.with(|storage| storage.borrow().derived.get(&id).map(Rc::clone));

    // Phase 2: storage borrow released — call the closure
    closure_rc.map(|rc| {
        let closure = rc.downcast_ref::<Box<dyn Fn() -> T>>().unwrap_or_else(|| {
            panic!(
                "Derived signal {} type mismatch: closure return type does not match {}",
                id,
                std::any::type_name::<T>()
            )
        });
        closure()
    })
}

/// Dispose a signal, marking it as unavailable and adding its ID to the free list.
///
/// After disposal, any attempt to read or write the signal will panic
/// with a clear error message. The ID will be reused by the next `create_signal_value`.
pub fn dispose_signal(id: SignalId) {
    STORAGE.with(|storage| {
        let mut storage = storage.borrow_mut();
        let Some(slot) = storage
            .slots
            .get_mut(id.index())
            .filter(|slot| slot.generation == id.generation())
        else {
            return; // Stale id: slot already recycled, nothing to dispose
        };
        if slot.value.take().is_some() {
            storage.derived.remove(&id);
            storage.free_indices.push(id.index() as u32);
        }
    });
}

/// Get a stored (immutable) value by cloning it.
///
/// Reads `Rc<T>` directly — no RefCell borrow needed since the value is immutable.
/// Used by `Signal::get()` for `SignalKind::Stored` signals.
pub fn get_stored_value<T: Clone + 'static>(id: SignalId) -> T {
    with_stored_ref(id, |v: &T| v.clone())
}

/// Borrow a stored (immutable) value for reading.
///
/// Reads `Rc<T>` directly — no RefCell borrow needed.
pub fn with_stored_value<T: 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    with_stored_ref(id, f)
}

/// Borrow the Rc<T> for a stored (immutable) signal and apply `f`.
fn with_stored_ref<T: 'static, R>(id: SignalId, f: impl FnOnce(&T) -> R) -> R {
    let rc = clone_slot_rc(id, "read");
    let val = rc.downcast_ref::<T>().unwrap_or_else(|| {
        panic!(
            "Signal {} type mismatch: stored type does not match requested type {}",
            id,
            std::any::type_name::<T>()
        )
    });
    f(val)
}

/// Get a mutable signal's value (clones it)
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

/// Compare and set: if the new value differs from the current value, replace it.
/// Returns `true` if the value was changed.
///
/// This performs the comparison and write in a single `with_signal_cell` call,
/// avoiding the overhead of two separate storage accesses.
pub fn compare_and_set_signal_value<T: PartialEq + 'static>(id: SignalId, value: T) -> bool {
    let Some(rc) = try_clone_slot_rc(id) else {
        // Writes race disposal by design (queued flushes, delayed timers):
        // dropping the write is the correct outcome, not a panic
        log::warn!("ignoring write to disposed signal {id}");
        return false;
    };
    let cell = downcast_cell::<T>(&rc, id);
    let mut current = cell.borrow_mut();
    if *current != value {
        *current = value;
        true
    } else {
        false
    }
}

/// Compare and update: clone the old value, apply the closure, compare, and return
/// whether the value changed. All in a single `with_signal_cell` call.
pub fn compare_and_update_signal_value<T: Clone + PartialEq + 'static>(
    id: SignalId,
    f: impl FnOnce(&mut T),
) -> bool {
    let Some(rc) = try_clone_slot_rc(id) else {
        log::warn!("ignoring update of disposed signal {id}");
        return false;
    };
    let cell = downcast_cell::<T>(&rc, id);
    let mut current = cell.borrow_mut();
    let old = current.clone();
    f(&mut current);
    old != *current
}

/// Set unconditionally: no comparison, no `PartialEq` bound. Returns
/// `true` if the write landed (`false` only when the slot is disposed).
///
/// Backs [`RwSignal::set_always`]: trigger-style writes where every
/// emission must notify (or where the type has no meaningful equality).
pub fn set_signal_value_always<T: 'static>(id: SignalId, value: T) -> bool {
    let Some(rc) = try_clone_slot_rc(id) else {
        log::warn!("ignoring write to disposed signal {id}");
        return false;
    };
    let cell = downcast_cell::<T>(&rc, id);
    *cell.borrow_mut() = value;
    true
}

/// Update unconditionally: no comparison, no `PartialEq` bound. Returns
/// `true` if the update ran (`false` only when the slot is disposed).
pub fn update_signal_value_always<T: 'static>(id: SignalId, f: impl FnOnce(&mut T)) -> bool {
    let Some(rc) = try_clone_slot_rc(id) else {
        log::warn!("ignoring update of disposed signal {id}");
        return false;
    };
    let cell = downcast_cell::<T>(&rc, id);
    f(&mut cell.borrow_mut());
    true
}

fn downcast_cell<T: 'static>(rc: &Rc<dyn Any>, id: SignalId) -> &RefCell<T> {
    rc.downcast_ref::<RefCell<T>>().unwrap_or_else(|| {
        panic!(
            "Signal {} type mismatch: stored type does not match requested type {}",
            id,
            std::any::type_name::<T>()
        )
    })
}

/// Reset all signal storage.
///
/// Called during `App::drop()` to wipe all stored signal values.
pub(crate) fn reset_storage() {
    STORAGE.with(|s| *s.borrow_mut() = SignalStorage::new());
}

/// How many signal slots currently hold a value.
///
/// For leak tests: build something, tear it down, and the count has to come
/// back to where it started.
pub fn live_signal_count() -> usize {
    STORAGE.with(|storage| {
        storage
            .borrow()
            .slots
            .iter()
            .filter(|slot| slot.value.is_some())
            .count()
    })
}

/// Check if a signal exists in the current thread's storage.
/// Used by `WriteSignal` to determine if we can write directly (same thread)
/// or must queue the write for the main thread.
pub fn has_signal(id: SignalId) -> bool {
    STORAGE.with(|storage| {
        let storage = storage.borrow();
        storage
            .slots
            .get(id.index())
            .filter(|slot| slot.generation == id.generation())
            .and_then(|slot| slot.value.as_ref())
            .is_some()
    })
}

#[cfg(test)]
mod dispose_write_tests {
    use crate::reactive::create_signal;
    use crate::reactive::owner::{dispose_owner_now, with_owner};

    /// Writes race disposal by design (queued WriteSignal flushes, delayed
    /// timers): they must be dropped, not panic.
    #[test]
    fn write_to_disposed_signal_is_a_noop() {
        let (sig, owner) = with_owner(|| create_signal(1u32));
        dispose_owner_now(owner);
        sig.set(2); // must not panic
        sig.update(|v| *v += 1); // must not panic
    }
}
