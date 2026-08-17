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
use std::collections::HashMap;

use super::invalidation::clear_signal_subscribers;
use super::runtime::{EffectId, SignalId, with_runtime};
use super::storage::dispose_signal;

/// Unique identifier for an owner in the owner arena.
///
/// Generational: the arena recycles slot indices and bumps the generation on
/// every reuse, so a stale `OwnerId` held after disposal can never dispose or
/// mutate an unrelated owner that later occupied the same slot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct OwnerId {
    index: u32,
    generation: u32,
}

/// An owner that manages the lifecycle of reactive primitives.
struct Owner {
    /// The owner this one was created under, if any. Used to prune this
    /// owner from the parent's `children` list on disposal.
    parent: Option<OwnerId>,
    signals: Vec<SignalId>,
    effects: Vec<EffectId>,
    cleanups: Vec<Box<dyn FnOnce()>>,
    children: Vec<OwnerId>,
}

impl Owner {
    fn new(parent: Option<OwnerId>) -> Self {
        Self {
            parent,
            signals: Vec::new(),
            effects: Vec::new(),
            cleanups: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Slot in the owner arena. The generation survives vacancy so recycled
/// indices can be distinguished from their previous occupants.
struct OwnerSlot {
    owner: Option<Owner>,
    generation: u32,
}

/// Arena-based storage for owners with slot recycling.
struct OwnerArena {
    slots: Vec<OwnerSlot>,
    /// Vacant slot indices available for reuse.
    free_indices: Vec<u32>,
    /// Reverse mapping from effect ID to owner ID for O(1) lookup.
    /// This avoids linear search through all owners when checking if an effect is owned.
    effect_owners: HashMap<EffectId, OwnerId>,
}

impl OwnerArena {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_indices: Vec::new(),
            effect_owners: HashMap::new(),
        }
    }

    fn allocate(&mut self, parent: Option<OwnerId>) -> OwnerId {
        let owner = Owner::new(parent);
        if let Some(index) = self.free_indices.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.owner = Some(owner);
            OwnerId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(OwnerSlot {
                owner: Some(owner),
                generation: 0,
            });
            OwnerId {
                index,
                generation: 0,
            }
        }
    }

    fn get_mut(&mut self, id: OwnerId) -> Option<&mut Owner> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.owner.as_mut())
    }

    fn take(&mut self, id: OwnerId) -> Option<Owner> {
        let slot = self
            .slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)?;
        let owner = slot.owner.take();
        if owner.is_some() {
            // Recycle the slot. Any still-live OwnerId for it is now stale
            // and will fail the generation check above.
            self.free_indices.push(id.index);
        }
        owner
    }
}

thread_local! {
    static CURRENT_OWNER: RefCell<Option<OwnerId>> = const { RefCell::new(None) };
    static OWNERS: RefCell<OwnerArena> = RefCell::new(OwnerArena::new());
}

/// Create a root owner and set it as the current owner.
///
/// This is used by `App::run()` to establish a root scope for all reactive
/// primitives created during setup. The root owner owns everything — when
/// disposed, all signals, effects, and cleanup callbacks cascade.
pub(crate) fn create_root_owner() -> OwnerId {
    let id = OWNERS.with(|owners| owners.borrow_mut().allocate(None));
    CURRENT_OWNER.with(|current| *current.borrow_mut() = Some(id));
    id
}

/// Reset all owner state (current owner + arena).
///
/// Called during `App::drop()` after `dispose_owner()` has cleaned up the
/// reactive graph. This wipes the arena so the next `App` run starts fresh.
pub(crate) fn reset_owners() {
    CURRENT_OWNER.with(|c| *c.borrow_mut() = None);
    OWNERS.with(|o| *o.borrow_mut() = OwnerArena::new());
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
    // Allocate new owner and register as child of current owner (if any)
    let parent_id = CURRENT_OWNER.with(|current| *current.borrow());
    let owner_id = OWNERS.with(|owners| {
        let mut owners = owners.borrow_mut();
        let id = owners.allocate(parent_id);

        // Register as child of current owner
        if let Some(parent_id) = parent_id
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

    // Restore on unwind too: a leaked owner scope would silently re-parent
    // every reactive resource created afterwards.
    let guard = crate::reactive::guard::defer(move || {
        CURRENT_OWNER.with(|current| {
            *current.borrow_mut() = prev_owner;
        });
    });

    // Execute the closure
    let result = f();
    drop(guard);

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
/// Internal engine: synchronous disposal, used by the library's own
/// teardown paths (surface close, reconcile discard, component Drop)
/// where ordering invariants are controlled. A running effect that
/// disposes its own owner is protected by the runtime's take/put-back
/// callback pattern (`EffectState::DisposedWhileRunning`), but code
/// after the call must not touch the owner's signals — public callers
/// get the deferred [`dispose_owner`] instead, which is safe anywhere.
#[doc(hidden)]
pub fn dispose_owner_now(id: OwnerId) {
    // Take the owner out of the arena and prune it from its parent's
    // children list. Without the pruning, long-lived parents (most notably
    // the root owner) accumulate one dead child entry for every owner ever
    // created under them. During recursive disposal the parent has already
    // been taken from the arena, so `get_mut` fails and the prune is skipped.
    let owner = OWNERS.with(|owners| {
        let mut arena = owners.borrow_mut();
        let owner = arena.take(id)?;
        if let Some(parent_id) = owner.parent
            && let Some(parent) = arena.get_mut(parent_id)
            && let Some(pos) = parent.children.iter().position(|c| *c == id)
        {
            // Sibling disposal order is unspecified, so swap_remove is fine.
            parent.children.swap_remove(pos);
        }
        Some(owner)
    });

    let Some(owner) = owner else {
        return; // Already disposed
    };

    // Dispose children first (depth-first)
    for child_id in owner.children {
        dispose_owner_now(child_id);
    }

    // Run cleanup callbacks in reverse order (LIFO)
    for cleanup in owner.cleanups.into_iter().rev() {
        // Teardown code reads for the current value; the scope is going away
        crate::reactive::diagnostics::snapshot_zone(cleanup);
    }

    // Dispose effects and remove from reverse mapping
    for effect_id in &owner.effects {
        OWNERS.with(|owners| {
            owners.borrow_mut().effect_owners.remove(effect_id);
        });
    }
    for effect_id in owner.effects {
        with_runtime(|rt| rt.dispose_effect(effect_id));
    }

    // Dispose signals (clear widget and effect subscriptions first to
    // prevent stale notifications from a future occupant of the slot)
    for signal_id in owner.signals {
        clear_signal_subscribers(signal_id);
        with_runtime(|rt| rt.dispose_signal_subscriptions(signal_id));
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
            let mut owners = owners.borrow_mut();
            if let Some(owner) = owners.get_mut(owner_id) {
                owner.effects.push(id);
                owners.effect_owners.insert(id, owner_id);
            }
        });
    }
}

/// Check if an effect is owned by any owner.
///
/// This is used by Effect's Drop impl to determine if it should dispose
/// the effect or let the owner handle it.
///
/// Uses O(1) lookup via the reverse mapping instead of linear search.
pub(crate) fn effect_has_owner(id: EffectId) -> bool {
    OWNERS.with(|owners| owners.borrow().effect_owners.contains_key(&id))
}

// Owners scheduled for deferred disposal (see `dispose_owner`).
thread_local! {
    static PENDING_DISPOSALS: std::cell::RefCell<Vec<OwnerId>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Dispose an owner: all its signals, effects, and cleanup callbacks.
///
/// Disposal is **deferred**: the main loop drains pending disposals once
/// per iteration, at a point where no user closure is on the stack. That
/// makes this safe to call from anywhere — including code the owner
/// itself owns (an effect observing a popup's dismissal, a dialog's own
/// close button) and code whose widgets outlive the current instant
/// (a popup surface that lives until its Close command is processed).
///
/// Disposing the same owner twice (or an already-disposed owner) is
/// harmless.
pub fn dispose_owner(id: OwnerId) {
    PENDING_DISPOSALS.with(|v| v.borrow_mut().push(id));
    // Wake the loop so a disposal requested in a quiet moment (compositor
    // dismissal with no other activity) is not postponed indefinitely
    crate::jobs::wake_loop();
}

/// Run every pending deferred disposal. Called by the main loop at a safe
/// point — never call from inside reactive computations.
pub(crate) fn flush_pending_disposals() {
    // split_off keeps draining safe even if a cleanup callback disposes
    // another owner while running (it lands in the next batch)
    loop {
        let batch = PENDING_DISPOSALS.with(|v| v.borrow_mut().split_off(0));
        if batch.is_empty() {
            return;
        }
        for id in batch {
            dispose_owner_now(id);
        }
    }
}

/// Whether any owner is waiting to be disposed.
///
/// Part of the loop's wakeup check — see `queued_but_unwoken` in `lib.rs`.
pub(crate) fn disposals_pending() -> bool {
    PENDING_DISPOSALS.with(|v| !v.borrow().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_owner_basic() {
        let (value, owner_id) = with_owner(|| 42);
        assert_eq!(value, 42);
        dispose_owner_now(owner_id);
    }

    /// The public dispose_owner defers: nothing is disposed until the main
    /// loop flushes, so an owned computation can safely dispose its own
    /// owner ("close myself" UI). Double-disposing is harmless.
    #[test]
    fn test_dispose_owner_defers_disposal() {
        let cleaned = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = cleaned.clone();
        let (_, owner_id) = with_owner(move || {
            on_cleanup(move || flag.set(true));
        });

        dispose_owner(owner_id);
        dispose_owner(owner_id); // idempotent
        assert!(
            !cleaned.get(),
            "public dispose_owner must not dispose synchronously"
        );

        flush_pending_disposals();
        assert!(cleaned.get(), "flush must dispose the pending owner");

        // Flushing again (and a stale dispose) must be a no-op
        dispose_owner(owner_id);
        flush_pending_disposals();
    }

    /// A cleanup callback that disposes ANOTHER owner while the queue is
    /// being flushed must not be lost.
    #[test]
    fn test_dispose_during_flush_is_processed() {
        let cleaned_b = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag_b = cleaned_b.clone();
        let (_, owner_b) = with_owner(move || {
            on_cleanup(move || flag_b.set(true));
        });
        let (_, owner_a) = with_owner(move || {
            on_cleanup(move || dispose_owner(owner_b));
        });

        dispose_owner(owner_a);
        flush_pending_disposals();
        assert!(
            cleaned_b.get(),
            "an owner disposed during the flush must be handled in the same flush"
        );
    }

    /// A disposed owner's slot is recycled with a bumped generation, so a
    /// stale OwnerId must never resolve to (or dispose) the new occupant.
    #[test]
    fn test_stale_owner_id_cannot_alias_recycled_slot() {
        let (_, first) = with_owner(|| ());
        dispose_owner_now(first);

        // Reuses the freed slot
        let disposed = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = disposed.clone();
        let (_, second) = with_owner(move || {
            on_cleanup(move || flag.set(true));
        });
        assert_eq!(first.index, second.index, "slot should be recycled");
        assert_ne!(first.generation, second.generation);

        // Disposing via the stale ID must be a no-op
        dispose_owner_now(first);
        assert!(!disposed.get(), "stale OwnerId disposed the new owner");

        dispose_owner_now(second);
        assert!(disposed.get());
    }

    /// Disposing a child owner must remove it from the parent's children
    /// list; otherwise long-lived parents grow without bound.
    #[test]
    fn test_dispose_prunes_parent_children_list() {
        let (child_ids, parent_id) = with_owner(|| {
            (0..4)
                .map(|_| with_owner(|| ()).1)
                .collect::<Vec<OwnerId>>()
        });

        for child in &child_ids {
            dispose_owner_now(*child);
        }

        OWNERS.with(|owners| {
            let mut arena = owners.borrow_mut();
            let parent = arena.get_mut(parent_id).expect("parent still live");
            assert!(
                parent.children.is_empty(),
                "disposed children not pruned: {:?}",
                parent.children
            );
        });

        dispose_owner_now(parent_id);
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
        dispose_owner_now(outer_id);

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

        dispose_owner_now(owner_id);

        // Should be reverse order (LIFO)
        let order = cleanup_order.lock().unwrap();
        assert_eq!(*order, vec!["third", "second", "first"]);
    }

    #[test]
    fn test_dispose_owner_twice_is_safe() {
        let (_, owner_id) = with_owner(|| {});

        // Should not panic
        dispose_owner_now(owner_id);
        dispose_owner_now(owner_id);
    }

    #[test]
    fn test_effect_registration_and_reverse_mapping() {
        use super::super::effect::create_effect;
        use super::super::signal::create_signal;

        let effect_ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let effect_ran_clone = effect_ran.clone();

        let (effect_id, owner_id) = with_owner(|| {
            let signal = create_signal(0);
            let effect = create_effect(move || {
                let _ = signal.get();
                effect_ran_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            effect.id()
        });

        // Effect should be owned via reverse mapping
        assert!(
            effect_has_owner(effect_id),
            "Effect should be owned after registration"
        );

        // Dispose the owner
        dispose_owner_now(owner_id);

        // Effect should no longer be owned (removed from reverse mapping)
        assert!(
            !effect_has_owner(effect_id),
            "Effect should not be owned after disposal"
        );
    }

    #[test]
    fn test_signal_registration() {
        use super::super::signal::create_signal;

        let (signal, owner_id) = with_owner(|| create_signal(42));

        // Should be able to read before disposal
        assert_eq!(signal.get(), 42);

        // Dispose the owner
        dispose_owner_now(owner_id);

        // Note: accessing disposed signal should panic, but we don't test that
        // here since it would terminate the test. The storage.rs tests cover
        // the panic messages.
    }

    #[test]
    fn test_multiple_effects_registration() {
        use super::super::effect::create_effect;
        use super::super::signal::create_signal;

        let (effect_ids, owner_id) = with_owner(|| {
            let signal = create_signal(0);
            let e1 = create_effect(move || {
                let _ = signal.get();
            });
            let e2 = create_effect(move || {
                let _ = signal.get();
            });
            let e3 = create_effect(move || {
                let _ = signal.get();
            });
            (e1.id(), e2.id(), e3.id())
        });

        // All effects should be owned
        assert!(effect_has_owner(effect_ids.0));
        assert!(effect_has_owner(effect_ids.1));
        assert!(effect_has_owner(effect_ids.2));

        // Dispose
        dispose_owner_now(owner_id);

        // None should be owned anymore
        assert!(!effect_has_owner(effect_ids.0));
        assert!(!effect_has_owner(effect_ids.1));
        assert!(!effect_has_owner(effect_ids.2));
    }

    #[test]
    fn test_nested_owners_effect_cleanup() {
        use super::super::effect::create_effect;
        use super::super::signal::create_signal;

        let ((inner_effect, outer_effect), outer_id) = with_owner(|| {
            let signal = create_signal(0);
            let outer = create_effect(move || {
                let _ = signal.get();
            });

            let (inner, _inner_id) = with_owner(|| {
                create_effect(move || {
                    let _ = signal.get();
                })
            });

            (inner.id(), outer.id())
        });

        // Both should be owned
        assert!(effect_has_owner(inner_effect));
        assert!(effect_has_owner(outer_effect));

        // Dispose outer (which should dispose inner first due to depth-first)
        dispose_owner_now(outer_id);

        // Both should be disposed
        assert!(!effect_has_owner(inner_effect));
        assert!(!effect_has_owner(outer_effect));
    }

    #[test]
    fn test_root_owner_and_reset() {
        use super::super::signal::create_signal;

        // Simulate App lifecycle: create root owner → create signals → dispose → reset
        let root_id = create_root_owner();
        assert!(current_owner().is_some());

        let signal = create_signal(42);
        assert_eq!(signal.get(), 42);

        // Dispose root owner — cascades signal disposal
        dispose_owner_now(root_id);

        // Signal should be disposed (reading would panic)
        let panicked = std::panic::catch_unwind(|| signal.get()).is_err();
        assert!(panicked, "Accessing disposed signal should panic");

        // Reset owner arena
        reset_owners();
        assert!(current_owner().is_none());

        // After reset, creating a new root owner should work cleanly
        let new_root = create_root_owner();
        assert!(current_owner().is_some());
        let signal2 = create_signal(99);
        assert_eq!(signal2.get(), 99);

        // Cleanup
        dispose_owner_now(new_root);
        reset_owners();
    }
}
