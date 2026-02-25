//! App-global context system for sharing state across widgets.
//!
//! Context provides a way to store and retrieve app-wide values (config, theme,
//! services) without passing them through every level of the widget tree. Values
//! are keyed by their concrete type — one value per type.
//!
//! ## Storage
//!
//! Uses `Vec<(TypeId, Box<dyn Any>)>` with linear scan. Context stores ~3-8
//! values in practice (config, theme, services), so this fits in 1-2 cache
//! lines and avoids HashMap overhead. `TypeId` comparison is a single `u64` eq.
//!
//! ## Reactive Context
//!
//! Storing `Signal<T>` as context is the recommended pattern for mutable state.
//! Any widget reading the signal during paint/layout auto-tracks it:
//!
//! ```ignore
//! // In App::run() setup:
//! let theme = provide_signal_context(MyTheme::default());
//!
//! // In any widget:
//! let theme = expect_context::<Signal<MyTheme>>();
//! container().background(move || theme.get().bg_color)
//! ```

use std::any::{Any, TypeId};
use std::cell::RefCell;

use super::signal::{Signal, create_signal};

thread_local! {
    static CONTEXTS: RefCell<Vec<(TypeId, Box<dyn Any>)>> = const { RefCell::new(Vec::new()) };
}

/// Store a value in the global context, keyed by its type.
///
/// If a value of the same type already exists, it is replaced.
///
/// # Example
///
/// ```ignore
/// App::new().run(|app| {
///     provide_context(MyConfig::load());
///     provide_context(create_signal(Theme::default())); // Signal as context
///     // ...
/// });
/// ```
pub fn provide_context<T: 'static>(value: T) {
    let type_id = TypeId::of::<T>();
    CONTEXTS.with(|ctx| {
        let mut ctx = ctx.borrow_mut();
        // Replace if exists
        for entry in ctx.iter_mut() {
            if entry.0 == type_id {
                entry.1 = Box::new(value);
                return;
            }
        }
        ctx.push((type_id, Box::new(value)));
    });
}

/// Retrieve a context value by type, returning `None` if not provided.
///
/// Clones the value — for large structs, use [`with_context`] to borrow
/// or store a `Signal<T>` instead.
///
/// # Example
///
/// ```ignore
/// if let Some(cfg) = use_context::<MyConfig>() {
///     println!("threshold: {}", cfg.warn_threshold);
/// }
/// ```
pub fn use_context<T: Clone + 'static>() -> Option<T> {
    let type_id = TypeId::of::<T>();
    CONTEXTS.with(|ctx| {
        let ctx = ctx.borrow();
        for entry in ctx.iter() {
            if entry.0 == type_id {
                return Some(
                    entry
                        .1
                        .downcast_ref::<T>()
                        .expect("context type mismatch (should be impossible)")
                        .clone(),
                );
            }
        }
        None
    })
}

/// Retrieve a context value by type, panicking if not provided.
///
/// Use this when the context is required and its absence is a programming error.
///
/// # Panics
///
/// Panics with a helpful message including the type name if the context
/// was not provided.
///
/// # Example
///
/// ```ignore
/// let cfg = expect_context::<MyConfig>();
/// ```
pub fn expect_context<T: Clone + 'static>() -> T {
    use_context::<T>().unwrap_or_else(|| {
        panic!(
            "Context not found for type `{}`.\n\
             Did you forget to call provide_context() in your App::run() setup?",
            std::any::type_name::<T>()
        )
    })
}

/// Borrow a context value without cloning.
///
/// Returns `None` if the context was not provided. Use this for large structs
/// where you only need to read a single field.
///
/// # Example
///
/// ```ignore
/// let threshold = with_context::<Config, _>(|cfg| cfg.cpu.warn_threshold);
/// ```
pub fn with_context<T: 'static, R>(f: impl FnOnce(&T) -> R) -> Option<R> {
    let type_id = TypeId::of::<T>();
    CONTEXTS.with(|ctx| {
        let ctx = ctx.borrow();
        for entry in ctx.iter() {
            if entry.0 == type_id {
                let value = entry
                    .1
                    .downcast_ref::<T>()
                    .expect("context type mismatch (should be impossible)");
                return Some(f(value));
            }
        }
        None
    })
}

/// Check if a context value of type `T` has been provided.
///
/// Useful for optional features: "if a logger context exists, use it."
///
/// # Example
///
/// ```ignore
/// if has_context::<Logger>() {
///     let logger = expect_context::<Logger>();
///     logger.info("widget created");
/// }
/// ```
pub fn has_context<T: 'static>() -> bool {
    let type_id = TypeId::of::<T>();
    CONTEXTS.with(|ctx| {
        let ctx = ctx.borrow();
        ctx.iter().any(|entry| entry.0 == type_id)
    })
}

/// Provide a `Signal<T>` context wrapping the given value.
///
/// This is the recommended pattern for mutable shared state. Creates a signal,
/// stores it as context, and returns it. Any widget reading the signal during
/// paint/layout auto-tracks it for reactive updates.
///
/// # Example
///
/// ```ignore
/// // In setup:
/// let theme = provide_signal_context(Theme::default());
///
/// // In any widget:
/// let theme = expect_context::<Signal<Theme>>();
/// container().background(move || theme.get().bg_color)
/// ```
pub fn provide_signal_context<T: Clone + PartialEq + Send + 'static>(value: T) -> Signal<T> {
    let signal = create_signal(value);
    provide_context(signal);
    signal
}

/// Reset all context state.
///
/// Called during `App::drop()` to wipe thread-local context storage,
/// enabling clean restart of the application.
pub(crate) fn reset_contexts() {
    CONTEXTS.with(|ctx| ctx.borrow_mut().clear());
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reset contexts before each test to avoid cross-test contamination.
    // Tests in Rust run in the same thread sequentially per file.
    fn setup() {
        reset_contexts();
    }

    #[test]
    fn test_provide_and_use_context() {
        setup();
        provide_context(42u32);
        assert_eq!(use_context::<u32>(), Some(42));
    }

    #[test]
    fn test_use_context_returns_none_when_missing() {
        setup();
        assert_eq!(use_context::<String>(), None);
    }

    #[test]
    fn test_expect_context_returns_value() {
        setup();
        provide_context("hello".to_string());
        assert_eq!(expect_context::<String>(), "hello");
    }

    #[test]
    #[should_panic(expected = "Context not found for type")]
    fn test_expect_context_panics_when_missing() {
        setup();
        expect_context::<f64>();
    }

    #[test]
    fn test_with_context_borrows_without_clone() {
        setup();
        provide_context(vec![1, 2, 3]);
        let sum = with_context::<Vec<i32>, _>(|v| v.iter().sum::<i32>());
        assert_eq!(sum, Some(6));
    }

    #[test]
    fn test_with_context_returns_none_when_missing() {
        setup();
        let result = with_context::<Vec<i32>, _>(|v| v.len());
        assert_eq!(result, None);
    }

    #[test]
    fn test_has_context() {
        setup();
        assert!(!has_context::<u64>());
        provide_context(99u64);
        assert!(has_context::<u64>());
    }

    #[test]
    fn test_provide_replaces_existing() {
        setup();
        provide_context(10u32);
        provide_context(20u32);
        assert_eq!(use_context::<u32>(), Some(20));
    }

    #[test]
    fn test_multiple_types() {
        setup();
        provide_context(42u32);
        provide_context("hello".to_string());
        provide_context(2.72f64);

        assert_eq!(use_context::<u32>(), Some(42));
        assert_eq!(use_context::<String>(), Some("hello".to_string()));
        assert_eq!(use_context::<f64>(), Some(2.72));
    }

    #[test]
    fn test_reset_clears_all() {
        setup();
        provide_context(42u32);
        provide_context("hello".to_string());
        reset_contexts();
        assert_eq!(use_context::<u32>(), None);
        assert_eq!(use_context::<String>(), None);
    }

    #[test]
    fn test_provide_signal_context() {
        setup();
        let signal = provide_signal_context(100i32);
        assert_eq!(signal.get(), 100);

        // Retrieve via use_context
        let retrieved = use_context::<Signal<i32>>().unwrap();
        assert_eq!(retrieved.get(), 100);

        // Signal set propagates
        signal.set(200);
        assert_eq!(retrieved.get(), 200);
    }
}
