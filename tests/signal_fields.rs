use guido::SignalFields;

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
