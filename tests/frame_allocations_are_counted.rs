#![cfg(feature = "render-stats")]
//! What says the frame counter counts frames, and the region counts regions.
//!
//! `guido::heap` counts the whole program, because the heap is the whole
//! program's: wgpu's threads allocate inside a frame too, and a counter blind
//! to them would call a frame free because the one thread it watched was. What
//! turns that into a frame's number is the window `render_stats` samples over —
//! [`reset_stats`](guido::render_stats::reset_stats) at one end and
//! [`end_frame`](guido::render_stats::end_frame) at the other — and what turns
//! it into a play's is [`Region`], which marks and subtracts. This is where
//! both are asserted: an allocation inside a window is counted once, the same
//! allocation after it is not counted at all, and every figure comes back at
//! the size it was asked for.
//!
//! **Every number here is one a constant cannot be.** The first version of this
//! file put one `Vec::with_capacity(1)` inside the window and asserted the
//! count was 1 — which a `Region::allocations` replaced by the literal `1`
//! passes, and a `-` replaced by `/` passes, and mutation testing said so: the
//! arithmetic the whole measurement rests on was unwatched by the test written
//! to watch it. So the counts are sevens, the sizes are kilobytes, and the
//! assertions are exact. `7` is not `1`, and `n / m` is not `n - m`.
//!
//! **One test, on purpose.** The counter is process-wide, so a second test
//! allocating on another thread would be counted into this one's window — and
//! so would the test runner spawning that thread. A test binary is one process,
//! a file is one binary, so a file with one test is the only place an exact
//! delta can be asserted. Everything smaller — the arithmetic of `alloc`,
//! `dealloc` and `realloc` — is asserted beside the allocator in `src/heap.rs`,
//! on counters of its own; everything larger is the two
//! `*_benchmark_is_repeatable.rs` tests.

use std::hint::black_box;

use guido::heap::Region;
use guido::render_stats::{self, StatsSnapshot};
use guido::tree::DamageRegion;

/// Nothing counts unless the binary asks for it, which is the one part of this
/// that is not the library's to decide.
#[global_allocator]
static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;

/// Blocks per measurement, and the size of one. Seven so that no figure here is
/// zero, one, or the other figure; a round kilobyte so the bytes are the count
/// multiplied by something, and not the count again.
const BLOCKS: usize = 7;
const BLOCK: usize = 1024;

/// What one region is left holding, and what another one peaks at. Both are
/// sizes no mutant can return by accident.
const KEPT: usize = 4096;
const BIG: usize = 1 << 20;

#[test]
fn a_frame_is_charged_for_what_it_allocated_and_for_nothing_else() {
    // Somewhere to keep the blocks, with room for every one this test makes.
    // Built out here because a `Vec` that grew inside a window would be an
    // allocation this test did not ask for and cannot see.
    let mut held: Vec<Vec<u8>> = Vec::with_capacity(BLOCKS * 4);

    // Touch the counters before the window opens. They live in a thread-local
    // that is built on first use, and registering its destructor allocates —
    // inside the window, if the window is what built it.
    let _ = render_stats::get_stats();

    render_stats::reset_stats();
    assert_eq!(
        render_stats::get_stats(),
        StatsSnapshot::default(),
        "the counters did not start from nothing"
    );

    // Inside the window: seven blocks of a kilobyte, so seven requests and
    // seven kilobytes.
    for _ in 0..BLOCKS {
        held.push(black_box(Vec::with_capacity(BLOCK)));
    }
    render_stats::end_frame(&DamageRegion::None);

    let frame = render_stats::get_stats();
    assert_eq!(
        frame.allocations, BLOCKS as u64,
        "{BLOCKS} allocations inside the frame were counted as {}",
        frame.allocations
    );
    assert_eq!(
        frame.bytes_allocated,
        (BLOCKS * BLOCK) as u64,
        "{BLOCKS} blocks of {BLOCK} bytes were counted as {} bytes",
        frame.bytes_allocated
    );

    // Outside it: seven more of the same, and the frame's figures do not move.
    for _ in 0..BLOCKS {
        held.push(black_box(Vec::with_capacity(BLOCK)));
    }
    let after = render_stats::get_stats();
    assert_eq!(
        (after.allocations, after.bytes_allocated),
        (frame.allocations, frame.bytes_allocated),
        "allocations made after the frame ended were charged to the frame"
    );

    // And a `Region`, which is how the benchmarks measure the wider window. In
    // this test rather than one of its own for the reason at the top of the
    // file — a second test is a second thread, and every one of these numbers
    // is the program's.
    let region = Region::start();
    for _ in 0..BLOCKS {
        held.push(black_box(Vec::with_capacity(BLOCK)));
    }
    assert_eq!(
        region.allocations(),
        BLOCKS as u64,
        "the region counted {BLOCKS} requests as {}",
        region.allocations()
    );
    assert_eq!(
        region.bytes(),
        (BLOCKS * BLOCK) as u64,
        "the region counted {} bytes where {BLOCKS} blocks of {BLOCK} were asked for",
        region.bytes()
    );

    // What a region is left holding: exactly the block it still has, and
    // exactly nothing once it lets go.
    let kept_region = Region::start();
    let kept = black_box(Vec::<u8>::with_capacity(KEPT));
    assert_eq!(
        kept_region.retained_bytes(),
        KEPT as i64,
        "{KEPT} bytes were live and the region says it kept {}",
        kept_region.retained_bytes()
    );
    drop(kept);
    assert_eq!(
        kept_region.retained_bytes(),
        0,
        "the block was freed and the region still claims {}",
        kept_region.retained_bytes()
    );

    // And the high-water mark, which comes down to the level the region starts
    // at — so a megabyte held is a megabyte of peak exactly, whatever the
    // process was holding when it started, and it does not come back down.
    let peak_region = Region::start();
    let big = black_box(Vec::<u8>::with_capacity(BIG));
    let peak_while_held = peak_region.peak_live_bytes();
    drop(big);

    assert_eq!(
        peak_while_held, BIG,
        "a megabyte was live and the region's peak was {peak_while_held}"
    );
    assert_eq!(
        peak_region.peak_live_bytes(),
        BIG,
        "the peak is a high-water mark and it came back down to {}",
        peak_region.peak_live_bytes()
    );

    drop(held);
}
