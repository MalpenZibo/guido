use super::effect::create_effect;
use super::into_signal::{IntoSignal, MemoMarker};
use super::signal::{RwSignal, Signal, create_signal};

/// Eager computed value that recomputes immediately when dependencies change.
///
/// A `Memo<T>` updates eagerly whenever any dependency signal changes.
/// It only notifies downstream subscribers when the computed result actually
/// differs (`PartialEq`), which prevents unnecessary repaints/relayouts.
///
/// `Memo<T>` is `Copy` (like `Signal<T>`) and can be used directly as a
/// widget property via `IntoSignal`.
///
/// # Example
///
/// ```ignore
/// let count = create_signal(0);
/// let doubled = create_memo(move || count.get() * 2);
///
/// container().background(move || {
///     if doubled.get() > 10 { Color::RED } else { Color::BLUE }
/// })
/// ```
pub struct Memo<T: Clone + PartialEq + Send + 'static> {
    signal: RwSignal<T>,
}

// Manually implement Clone and Copy to avoid unnecessary bounds on T
impl<T: Clone + PartialEq + Send + 'static> Clone for Memo<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Clone + PartialEq + Send + 'static> Copy for Memo<T> {}

/// Create an eagerly-evaluated memo that recomputes when dependencies change.
///
/// The memo only notifies subscribers when its computed value actually changes
/// (compared via `PartialEq`), preventing unnecessary downstream updates.
///
/// # Example
///
/// ```ignore
/// let count = create_signal(0);
/// let label = create_memo(move || format!("Count: {}", count.get()));
/// text(label)  // Only repaints when the formatted string actually changes
/// ```
pub fn create_memo<T, F>(f: F) -> Memo<T>
where
    T: Clone + PartialEq + Send + 'static,
    F: Fn() -> T + 'static,
{
    let initial = f();
    let signal = create_signal(initial);
    // The effect runs immediately (establishing dependencies) and re-runs
    // whenever any dependency changes. Signal::set() uses PartialEq to
    // skip notification when the value hasn't changed.
    //
    // The effect is registered with the current owner via register_effect().
    // Effect::Drop checks effect_has_owner() and skips disposal for owned
    // effects, so the underscore-prefixed binding is safe here.
    let _effect = create_effect(move || {
        signal.set(f());
    });
    Memo { signal }
}

impl<T: Clone + PartialEq + Send + 'static> Memo<T> {
    /// Get the current memo value (tracked for dependency tracking).
    pub fn get(&self) -> T {
        self.signal.get()
    }

    /// Borrow the current value (tracked for dependency tracking).
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.signal.with(f)
    }

    /// Extract as a read-only signal.
    pub fn into_signal(self) -> Signal<T> {
        self.signal.read_only()
    }
}

impl<T: Clone + PartialEq + Send + 'static> IntoSignal<T, MemoMarker> for Memo<T> {
    fn into_signal(self) -> Signal<T> {
        self.signal.read_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memo_initial_value() {
        let signal = create_signal(5);
        let memo = create_memo(move || signal.get() * 2);
        assert_eq!(memo.get(), 10);
    }

    #[test]
    fn test_memo_is_copy() {
        let signal = create_signal(1);
        let memo = create_memo(move || signal.get());
        let memo2 = memo; // Copy
        assert_eq!(memo.get(), 1);
        assert_eq!(memo2.get(), 1);
    }

    #[test]
    fn test_memo_with() {
        let signal = create_signal(String::from("hello"));
        let memo = create_memo(move || signal.get());
        let len = memo.with(|s| s.len());
        assert_eq!(len, 5);
    }

    #[test]
    fn test_memo_into_signal() {
        let signal = create_signal(7);
        let memo = create_memo(move || signal.get() + 3);
        let sig: Signal<i32> = memo.into_signal();
        assert_eq!(sig.get(), 10);
    }
}
