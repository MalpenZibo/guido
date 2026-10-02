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
use guido::reactive::owner::with_owner;
use guido::reactive::{create_effect, create_signal};

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
fn an_effect_rerun_allocates_nothing(source: guido::reactive::RwSignal<u32>) {
    source.set(2);
    let region = Region::start();
    for value in 3..103 {
        source.set(value);
    }
    let allocations = region.allocations();
    println!("100 effect re-runs: {allocations} allocations");
    assert_eq!(allocations, 0, "an effect re-run allocated");
}
