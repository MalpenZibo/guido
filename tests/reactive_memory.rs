//! What a reactive primitive keeps on the heap, per primitive.
//!
//! A status bar holds tens of thousands of signals, and almost none of them is
//! ever subscribed to by anything: what each one costs while it sits there is
//! most of what the reactive system costs. These are the ceilings, measured by
//! [`CountingAllocator`](guido::heap::CountingAllocator) over a hundred
//! thousand of each, so one primitive's share of an arena's growth is averaged
//! in rather than landing on whichever happened to double it.
//!
//! **No harness**, for the reason `frame_allocations_are_counted.rs` gives: the
//! counter is process-wide, and libtest's own thread allocates into it. This
//! `main` is the only thread there is.

use guido::heap::Region;
use guido::reactive::owner::{dispose_owner_now, with_owner};
use guido::reactive::{RwSignal, create_derived, create_effect, create_signal};

#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

/// How many of each primitive a measurement makes.
const N: usize = 100_000;

/// What `make` keeps per primitive, in bytes, measured over `N` of them made
/// under one scope.
fn retained_per_primitive(make: impl Fn()) -> f64 {
    let region = Region::start();
    let ((), _scope) = with_owner(|| (0..N).for_each(|_| make()));
    region.retained_bytes() as f64 / N as f64
}

fn main() {
    // One of everything first, so what is measured is the primitives and not
    // the arenas coming into existence.
    let source = create_signal(1u32);
    create_effect(move || {
        source.get();
    });

    a_signal_nothing_subscribes_to_keeps_no_subscriber_list();
    an_effect_rerun_allocates_nothing(source);
    a_derived_signal_is_its_closure_and_its_slot(source);
    an_empty_scope_allocates_nothing_it_does_not_hold();
    an_effect_keeps_no_more_than_it_reads();
    an_effect_that_makes_a_signal_each_run_keeps_its_scope(source);
}

/// A signal nobody reads costs its slot and its value, and no subscriber
/// bookkeeping: there used to be an empty list for it in each of two dense
/// indexes, 72 bytes of the 105 it kept.
fn a_signal_nothing_subscribes_to_keeps_no_subscriber_list() {
    let each = retained_per_primitive(|| {
        create_signal(0u32);
    });
    println!("create_signal: {each:.1} B each");
    assert!(each <= 80.0, "a signal keeps {each:.1} B, more than 80");
}

/// An effect's run takes its subscriptions out and puts this run's back, and
/// none of that is allocation: the lists it was in are kept for it, empty,
/// across the run.
fn an_effect_rerun_allocates_nothing(source: RwSignal<u32>) {
    source.set(2);
    let region = Region::start();
    for value in 3..103 {
        source.set(value);
    }
    let allocations = region.allocations();
    println!("100 effect re-runs: {allocations} allocations");
    assert_eq!(allocations, 0, "an effect re-run allocated");
}

/// A derived signal is two allocations: the closure's box, and the `Rc` that
/// holds it with the scope it was written in, which is its slot's value. There
/// was a third, a placeholder value for a slot whose closure lived in a map
/// beside it.
///
/// Counted on a slot a disposed derived left free, so that neither the slot
/// arena nor anything keyed by slot has to grow: what is left is the derived
/// signal's own.
fn a_derived_signal_is_its_closure_and_its_slot(source: RwSignal<u32>) {
    let (_, scope) = with_owner(|| create_derived(move || source.get() + 1));
    dispose_owner_now(scope);

    let region = Region::start();
    let derived = create_derived(move || source.get() + 1);
    let allocations = region.allocations();
    println!("create_derived: {allocations} allocations");
    assert_eq!(
        allocations, 2,
        "a derived signal is its closure and its slot"
    );
    assert_eq!(derived.get_untracked(), source.get_untracked() + 1);
}

/// A scope that holds nothing keeps its slot in the arena and nothing else.
/// Most scopes are empty — every effect has one, and most runs make nothing —
/// and an empty one kept 168 bytes, most of them four empty lists inline in its
/// arena slot.
fn an_empty_scope_allocates_nothing_it_does_not_hold() {
    let each = retained_per_primitive(|| {
        with_owner(|| ());
    });
    println!("empty scope: {each:.1} B each");
    assert!(
        each <= 64.0,
        "an empty scope keeps {each:.1} B, more than 64"
    );
}

/// An effect reading one signal: its slot, its callback, its scope and one
/// subscription. Its scope holds nothing, so it costs what an empty one does.
fn an_effect_keeps_no_more_than_it_reads() {
    let source = create_signal(0u32);
    let each = retained_per_primitive(|| {
        create_effect(move || {
            source.get();
        })
    });
    println!("create_effect: {each:.1} B each");
    assert!(each <= 200.0, "an effect keeps {each:.1} B, more than 200");
}

/// An effect whose every run makes a signal: emptying its scope before the
/// next run keeps what held the last run's signal, so a run allocates the new
/// signal's value and nothing for the scope it is filed in.
fn an_effect_that_makes_a_signal_each_run_keeps_its_scope(source: RwSignal<u32>) {
    create_effect(move || {
        source.get();
        create_signal(0u32);
    });
    source.set(1000);
    source.set(1001);

    let region = Region::start();
    for value in 0..100 {
        source.set(value);
    }
    let allocations = region.allocations();
    println!("100 runs making a signal each: {allocations} allocations");
    assert_eq!(
        allocations, 100,
        "a run allocated more than the one signal it made"
    );
}
