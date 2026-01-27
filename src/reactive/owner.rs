//! Reactive ownership system for automatic resource cleanup.
//!
//! This module implements a reactive owner pattern (similar to Leptos/SolidJS/Dioxus)
//! where signals and effects belong to an owner, and when the owner is disposed,
//! all owned resources are automatically cleaned up.
//!
//! # Overview
//!
//! - Every signal and effect can belong to an owner
//! - Owners form a tree structure (child owners are disposed before parents)
//! - When an owner is disposed, all owned signals, effects, and cleanup callbacks are cleaned up
//! - `on_cleanup` allows registering custom cleanup logic (timers, connections, etc.)
//!
//! # Example
//!
//! ```ignore
//! // Create a scope with automatic cleanup
//! let (result, owner_id) = with_owner(|| {
//!     let signal = create_signal(42);
//!     let effect = create_effect(move || {
//!         println!("Signal: {}", signal.get());
//!     });
//!
//!     // Register custom cleanup
//!     on_cleanup(|| {
//!         println!("Cleaning up!");
//!     });
//!
//!     signal
//! });
//!
//! // Later, dispose everything in that scope
//! dispose_owner(owner_id);
//! // All signals, effects, and cleanup callbacks are now disposed
//! ```

use std::cell::RefCell;

use super::runtime::{EffectId, SignalId, with_runtime};
use super::storage::dispose_signal;

/// Unique identifier for an owner in the owner arena.
pub type OwnerId = usize;

/// An owner that manages the lifecycle of reactive primitives.
struct Owner {
    #[allow(dead_code)]
    id: OwnerId,
    #[allow(dead_code)]
    parent: Option<OwnerId>,
    signals: Vec<SignalId>,
    effects: Vec<EffectId>,
    cleanups: Vec<Box<dyn FnOnce()>>,
    children: Vec<OwnerId>,
}

impl Owner {
    fn new(id: OwnerId, parent: Option<OwnerId>) -> Self {
        Self {
            id,
            parent,
            signals: Vec::new(),
            effects: Vec::new(),
            cleanups: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Arena-based storage for owners.
struct OwnerArena {
    owners: Vec<Option<Owner>>,
    next_id: OwnerId,
}

impl OwnerArena {
    fn new() -> Self {
        Self {
            owners: Vec::new(),
            next_id: 0,
        }
    }

    fn allocate(&mut self, parent: Option<OwnerId>) -> OwnerId {
        let id = self.next_id;
        self.next_id += 1;
        let owner = Owner::new(id, parent);
        self.owners.push(Some(owner));
        id
    }

    #[allow(dead_code)]
    fn get(&self, id: OwnerId) -> Option<&Owner> {
        self.owners.get(id).and_then(|o| o.as_ref())
    }

    fn get_mut(&mut self, id: OwnerId) -> Option<&mut Owner> {
        self.owners.get_mut(id).and_then(|o| o.as_mut())
    }

    fn take(&mut self, id: OwnerId) -> Option<Owner> {
        self.owners.get_mut(id).and_then(|o| o.take())
    }
}

thread_local! {
    static CURRENT_OWNER: RefCell<Option<OwnerId>> = const { RefCell::new(None) };
    static OWNERS: RefCell<OwnerArena> = RefCell::new(OwnerArena::new());
}

/// Execute a closure within a new owner scope.
///
/// All signals and effects created within the closure will be registered
/// with this owner and automatically cleaned up when the owner is disposed.
///
/// Returns a tuple of the closure's return value and the owner ID.
///
/// This is used internally by the dynamic children system to automatically
/// manage reactive resource lifetimes. User code should use `on_cleanup`
/// inside dynamic children closures to register custom cleanup logic.
///
/// **Note:** This function is not part of the public API and may change.
/// Use `on_cleanup` for registering cleanup callbacks in user code.
pub fn with_owner<T>(f: impl FnOnce() -> T) -> (T, OwnerId) {
    // Get current owner as parent
    let parent = CURRENT_OWNER.with(|current| *current.borrow());

    // Allocate new owner
    let owner_id = OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let id = owners.allocate(parent);

        // Register as child of parent
        if let Some(parent_id) = parent
            && let Some(parent_owner) = owners.get_mut(parent_id)
        {
            parent_owner.children.push(id);
        }

        id
    });

    // Set as current owner
    let prev_owner = CURRENT_OWNER.with(|current| {
        let prev = *current.borrow();
        *current.borrow_mut() = Some(owner_id);
        prev
    });

    // Execute the closure
    let result = f();

    // Restore previous owner
    CURRENT_OWNER.with(|current| {
        *current.borrow_mut() = prev_owner;
    });

    (result, owner_id)
}

/// Get the current owner ID, if any.
///
/// Returns `None` if not currently inside an owner scope.
pub fn current_owner() -> Option<OwnerId> {
    CURRENT_OWNER.with(|current| *current.borrow())
}

/// Dispose an owner and all its resources.
///
/// This will:
/// 1. Recursively dispose all child owners (depth-first)
/// 2. Run all cleanup callbacks in reverse order
/// 3. Dispose all effects
/// 4. Dispose all signals
///
/// After disposal, any attempt to access the disposed signals will panic
/// with a clear error message.
///
/// **Note:** This function is not part of the public API and may change.
/// Cleanup is automatic when using dynamic children or components.
pub fn dispose_owner(id: OwnerId) {
    // Take the owner out of the arena
    let owner = OWNERS.with(|owners| owners.borrow_mut().take(id));

    let Some(owner) = owner else {
        return; // Already disposed
    };

    // Dispose children first (depth-first)
    for child_id in owner.children {
        dispose_owner(child_id);
    }

    // Run cleanup callbacks in reverse order (LIFO)
    for cleanup in owner.cleanups.into_iter().rev() {
        cleanup();
    }

    // Dispose effects
    for effect_id in owner.effects {
        with_runtime(|rt| rt.dispose_effect(effect_id));
    }

    // Dispose signals
    for signal_id in owner.signals {
        dispose_signal(signal_id);
    }
}

/// Register a cleanup callback to run when the current owner is disposed.
///
/// This is useful for cleaning up non-reactive resources like timers,
/// event listeners, or external connections.
///
/// Cleanup callbacks are run in reverse order (LIFO) - the last registered
/// callback runs first.
///
/// # Panics
///
/// This function will silently do nothing if called outside an owner scope.
///
/// # Example
///
/// ```ignore
/// with_owner(|| {
///     // Start a timer
///     let timer_id = start_timer();
///
///     // Register cleanup to stop the timer
///     on_cleanup(move || {
///         stop_timer(timer_id);
///     });
/// });
/// ```
pub fn on_cleanup(f: impl FnOnce() + 'static) {
    if let Some(owner_id) = current_owner() {
        OWNERS.with(|owners| {
            if let Some(owner) = owners.borrow_mut().get_mut(owner_id) {
                owner.cleanups.push(Box::new(f));
            }
        });
    }
}

/// Register a signal with the current owner.
///
/// This is called internally by `create_signal` to register newly created
/// signals for automatic cleanup.
pub(crate) fn register_signal(id: SignalId) {
    if let Some(owner_id) = current_owner() {
        OWNERS.with(|owners| {
            if let Some(owner) = owners.borrow_mut().get_mut(owner_id) {
                owner.signals.push(id);
            }
        });
    }
}

/// Register an effect with the current owner.
///
/// This is called internally by `create_effect` to register newly created
/// effects for automatic cleanup.
pub(crate) fn register_effect(id: EffectId) {
    if let Some(owner_id) = current_owner() {
        OWNERS.with(|owners| {
            if let Some(owner) = owners.borrow_mut().get_mut(owner_id) {
                owner.effects.push(id);
            }
        });
    }
}

/// Check if an effect is owned by any owner.
///
/// This is used by Effect's Drop impl to determine if it should dispose
/// the effect or let the owner handle it.
pub(crate) fn effect_has_owner(id: EffectId) -> bool {
    OWNERS.with(|owners| {
        let owners = owners.borrow();
        for owner in owners.owners.iter().flatten() {
            if owner.effects.contains(&id) {
                return true;
            }
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_owner_basic() {
        let (value, owner_id) = with_owner(|| 42);
        assert_eq!(value, 42);
        assert!(owner_id < 100); // Just check it's a reasonable ID
    }

    #[test]
    fn test_current_owner_inside_scope() {
        let ((inner_owner, outer_owner), _outer_id) = with_owner(|| {
            let outer = current_owner();
            let (inner, _inner_id) = with_owner(current_owner);
            (inner, outer)
        });

        // Both should be Some
        assert!(inner_owner.is_some());
        assert!(outer_owner.is_some());

        // They should be different
        assert_ne!(inner_owner, outer_owner);
    }

    #[test]
    fn test_current_owner_outside_scope() {
        // Outside any scope, should be None
        assert!(current_owner().is_none());
    }

    #[test]
    fn test_nested_owners() {
        let cleanup_order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let order = cleanup_order.clone();
        let (_, outer_id) = with_owner(|| {
            let order_inner = order.clone();
            on_cleanup(move || {
                order_inner.lock().unwrap().push("outer");
            });

            let order_nested = order.clone();
            with_owner(|| {
                on_cleanup(move || {
                    order_nested.lock().unwrap().push("inner");
                });
            });
        });

        // Dispose the outer owner
        dispose_owner(outer_id);

        // Children should be disposed first
        let order = cleanup_order.lock().unwrap();
        assert_eq!(*order, vec!["inner", "outer"]);
    }

    #[test]
    fn test_on_cleanup_reverse_order() {
        let cleanup_order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let order = cleanup_order.clone();
        let (_, owner_id) = with_owner(|| {
            let order1 = order.clone();
            on_cleanup(move || {
                order1.lock().unwrap().push("first");
            });

            let order2 = order.clone();
            on_cleanup(move || {
                order2.lock().unwrap().push("second");
            });

            let order3 = order.clone();
            on_cleanup(move || {
                order3.lock().unwrap().push("third");
            });
        });

        dispose_owner(owner_id);

        // Should be reverse order (LIFO)
        let order = cleanup_order.lock().unwrap();
        assert_eq!(*order, vec!["third", "second", "first"]);
    }

    #[test]
    fn test_dispose_owner_twice_is_safe() {
        let (_, owner_id) = with_owner(|| {});

        // Should not panic
        dispose_owner(owner_id);
        dispose_owner(owner_id);
    }
}
