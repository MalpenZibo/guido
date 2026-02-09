use std::cell::Cell;

use guido::SignalFields;
use guido::prelude::*;

#[derive(Clone, PartialEq, SignalFields)]
struct TestState {
    count: i32,
    name: String,
}

#[test]
fn test_signal_fields_creation() {
    let signals = TestStateSignals::new(TestState {
        count: 0,
        name: "test".into(),
    });
    assert_eq!(signals.count.get(), 0);
    assert_eq!(signals.name.get(), "test");
}

#[test]
fn test_writers_set_decomposes() {
    let signals = TestStateSignals::new(TestState {
        count: 0,
        name: "a".into(),
    });
    let writers = signals.writers();
    writers.set(TestState {
        count: 5,
        name: "a".into(),
    });
    assert_eq!(signals.count.get(), 5);
    assert_eq!(signals.name.get(), "a");
}

#[test]
fn test_writers_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<TestStateWriters>();
}

#[test]
fn test_signals_are_copy() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<TestStateSignals>();
}

#[test]
fn test_individual_field_update() {
    let signals = TestStateSignals::new(TestState {
        count: 10,
        name: "hello".into(),
    });

    // Update only count via its signal directly
    signals.count.set(20);
    assert_eq!(signals.count.get(), 20);
    assert_eq!(signals.name.get(), "hello"); // unchanged
}

#[test]
fn test_writers_copy() {
    let signals = TestStateSignals::new(TestState {
        count: 0,
        name: "x".into(),
    });
    let w1 = signals.writers();
    let w2 = w1; // Copy
    w2.set(TestState {
        count: 42,
        name: "y".into(),
    });
    assert_eq!(signals.count.get(), 42);
    assert_eq!(signals.name.get(), "y");
    // w1 is still usable (Copy)
    let _ = w1;
}

// Test with pub visibility
#[derive(Clone, PartialEq, SignalFields)]
pub struct PubState {
    pub value: u32,
}

#[test]
fn test_pub_visibility() {
    let signals = PubStateSignals::new(PubState { value: 99 });
    assert_eq!(signals.value.get(), 99);
}

// Test with Vec field
#[derive(Clone, PartialEq, SignalFields)]
struct VecState {
    items: Vec<String>,
    count: usize,
}

#[test]
fn test_vec_field() {
    let signals = VecStateSignals::new(VecState {
        items: vec!["a".into(), "b".into()],
        count: 2,
    });
    assert_eq!(signals.items.get(), vec!["a".to_string(), "b".to_string()]);
    assert_eq!(signals.count.get(), 2);

    let writers = signals.writers();
    writers.set(VecState {
        items: vec!["c".into()],
        count: 1,
    });
    assert_eq!(signals.items.get(), vec!["c".to_string()]);
    assert_eq!(signals.count.get(), 1);
}

// --- Effect batching tests ---

#[test]
fn test_writers_set_batches_effects() {
    let signals = TestStateSignals::new(TestState {
        count: 0,
        name: "a".into(),
    });

    let run_count = Cell::new(0u32);
    // SAFETY: run_count is on the stack and outlives the effect.
    // We use a raw pointer to avoid moving it into the closure.
    let run_count_ptr = &run_count as *const Cell<u32>;

    // Effect reads both fields — should run once on creation.
    // Hold the effect so it doesn't get disposed on drop.
    let _effect = create_effect(move || {
        let _ = signals.count.get();
        let _ = signals.name.get();
        unsafe { &*run_count_ptr }.set(unsafe { &*run_count_ptr }.get() + 1);
    });

    // Effect runs once on creation
    assert_eq!(run_count.get(), 1);

    // writers.set() updates both fields in a batch — effect should run once, not twice
    let writers = signals.writers();
    writers.set(TestState {
        count: 42,
        name: "b".into(),
    });

    assert_eq!(run_count.get(), 2); // 1 (initial) + 1 (batched set) = 2
    assert_eq!(signals.count.get(), 42);
    assert_eq!(signals.name.get(), "b");
}

// --- Generic struct tests ---

#[derive(Clone, PartialEq, SignalFields)]
struct GenericState<T: Clone + PartialEq + Send + 'static> {
    value: T,
    label: String,
}

#[test]
fn test_generic_signal_fields() {
    let signals = GenericStateSignals::new(GenericState {
        value: 42i32,
        label: "hello".into(),
    });
    assert_eq!(signals.value.get(), 42);
    assert_eq!(signals.label.get(), "hello");
}

#[test]
fn test_generic_writers() {
    let signals = GenericStateSignals::new(GenericState {
        value: 0i32,
        label: "a".into(),
    });
    let writers = signals.writers();
    writers.set(GenericState {
        value: 99,
        label: "b".into(),
    });
    assert_eq!(signals.value.get(), 99);
    assert_eq!(signals.label.get(), "b");
}

#[test]
fn test_generic_signals_are_copy() {
    // String is !Copy, but GenericStateSignals<String> should still be Copy
    // because Signal<T> is Copy regardless of T.
    fn assert_copy<T: Copy>() {}
    assert_copy::<GenericStateSignals<String>>();
}

#[test]
fn test_generic_writers_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<GenericStateWriters<String>>();
}

#[derive(Clone, PartialEq, SignalFields)]
struct MultiGeneric<A: Clone + PartialEq + Send + 'static, B: Clone + PartialEq + Send + 'static> {
    first: A,
    second: B,
}

#[test]
fn test_multi_generic() {
    let signals = MultiGenericSignals::new(MultiGeneric {
        first: 1u32,
        second: "x".to_string(),
    });
    assert_eq!(signals.first.get(), 1);
    assert_eq!(signals.second.get(), "x");

    let writers = signals.writers();
    writers.set(MultiGeneric {
        first: 2,
        second: "y".to_string(),
    });
    assert_eq!(signals.first.get(), 2);
    assert_eq!(signals.second.get(), "y");
}

#[derive(Clone, PartialEq, SignalFields)]
struct WhereClauseGeneric<T>
where
    T: Clone + PartialEq + Send + 'static,
{
    data: T,
}

#[test]
fn test_where_clause_generic() {
    let signals = WhereClauseGenericSignals::new(WhereClauseGeneric {
        data: vec![1, 2, 3],
    });
    assert_eq!(signals.data.get(), vec![1, 2, 3]);
}
