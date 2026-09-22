//! What the program asks of the allocator, and what it keeps.
//!
//! `render_stats` counts what a frame *decided*; nothing counted what it cost
//! the heap, so a change whose whole purpose is to stop allocating had nothing
//! to assert and a change that quietly started allocating had nothing to trip.
//! [`CountingAllocator`] forwards every request to the system allocator and
//! keeps four numbers on the way through: how many times it was asked, how many
//! bytes it was asked for, how many are live, and the most that were ever live
//! at once.
//!
//! **The library never installs it.** A `#[global_allocator]` may only be set
//! by the binary that links the program, and a library that set one would be
//! choosing an allocator for every application that ever depends on it — the
//! same line `dhat` and `stats_alloc` draw. So the two benchmarks, and the
//! tests that read the numbers, install it themselves:
//!
//! ```
//! #[global_allocator]
//! static HEAP: guido::heap::CountingAllocator = guido::heap::CountingAllocator;
//! # fn main() {}
//! ```
//!
//! Without the `render-stats` feature nothing is counted: the three hooks the
//! allocator calls on its way through are empty, so the counters never move and
//! every reader answers zero. The readers themselves are one implementation
//! either way — a second set returning literal zeroes would be a copy no build
//! under test contained, and mutation testing found exactly that.
//!
//! The counters are process-wide, because the heap is: wgpu's threads and the
//! graphics driver's allocate inside a frame as surely as the frame loop does,
//! and a counter that could not see them would report a frame that was cheap
//! only on the thread that was looking. The price is that a delta is only worth
//! reading where one thing is happening at a time — a benchmark, or a test that
//! serialises its plays.

use std::alloc::{GlobalAlloc, Layout, System};

/// A global allocator that forwards to the system one and counts on the way
/// through.
///
/// Relaxed atomics and nothing else. The frame loop is one thread but the
/// threads beside it are not, and an allocator that took a lock — or allocated
/// — would deadlock or recurse into the program it is counting.
pub struct CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            counters::allocated(layout.size());
        }
        ptr
    }

    /// Forwarded rather than left to the default, which would zero a block
    /// this allocator had just asked for by hand and lose the system's
    /// `calloc`.
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            counters::allocated(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        counters::deallocated(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        // A failed reallocation moved nothing and freed nothing.
        if !new_ptr.is_null() {
            counters::reallocated(layout.size(), new_size);
        }
        new_ptr
    }
}

/// How many times the allocator has been asked for a block since the program
/// started, and how many bytes it was asked for.
///
/// Running totals, which is why they are not public: a number that has been
/// climbing since the program started says nothing on its own, and the two ways
/// to make it say something are [`Region`], for a stretch, and `render_stats`,
/// for a frame. The second is the only caller, so they are gone with it when
/// the feature is off.
#[cfg(feature = "render-stats")]
pub(crate) fn allocations() -> u64 {
    counters::allocations()
}

#[cfg(feature = "render-stats")]
pub(crate) fn bytes_allocated() -> u64 {
    counters::bytes_allocated()
}

/// What one stretch of the program did to the heap, measured from where it
/// began.
///
/// The counters are the whole program's totals, so nothing about them is a
/// region's until something subtracts: this is the subtraction, and the shape
/// `stats_alloc` settled on for the same reason. Live bytes need it twice
/// over, being a level rather than a total — the high-water mark comes down to
/// the level at [`start`](Self::start), which is what lets two runs in one
/// process (`benches/static_clip` plays two scenarios) each report their own
/// peak rather than the taller of the two, twice.
pub struct Region {
    allocations: u64,
    bytes: u64,
    live: usize,
}

impl Region {
    /// Begin measuring here.
    ///
    /// **This moves the high-water mark**, which is the one piece of state a
    /// region cannot keep to itself: there is one mark and it comes down to
    /// what is live now. So regions are measured one after another, never one
    /// inside another — an outer region whose inner one started would report a
    /// peak that forgot everything before it, plausibly and silently. The other
    /// three figures are subtractions and nest perfectly well.
    pub fn start() -> Self {
        Self {
            allocations: counters::allocations(),
            bytes: counters::bytes_allocated(),
            live: counters::restart_peak(),
        }
    }

    /// How many times the allocator has been asked for a block since.
    pub fn allocations(&self) -> u64 {
        counters::allocations() - self.allocations
    }

    /// And how many bytes. A reallocation asks for the difference between the
    /// old size and the new, so a buffer that doubles its way to a megabyte
    /// asked for a megabyte rather than two.
    pub fn bytes(&self) -> u64 {
        counters::bytes_allocated() - self.bytes
    }

    /// The most live bytes ever stood above where this region found them.
    pub fn peak_live_bytes(&self) -> usize {
        counters::peak_live_bytes().saturating_sub(self.live)
    }

    /// Where they stand now, above where this region found them: what it kept.
    pub fn retained_bytes(&self) -> i64 {
        counters::live_bytes() as i64 - self.live as i64
    }
}

mod counters {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

    /// The four numbers and the arithmetic that keeps them.
    ///
    /// A type rather than four statics so the arithmetic can be tested on an
    /// instance of its own: the one below is shared with every other thread in
    /// the program, which is the point of it and also why nothing can assert an
    /// exact delta against it.
    struct Counters {
        allocations: AtomicU64,
        bytes: AtomicU64,
        live: AtomicUsize,
        peak: AtomicUsize,
    }

    static COUNTERS: Counters = Counters::new();

    impl Counters {
        const fn new() -> Self {
            Self {
                allocations: AtomicU64::new(0),
                bytes: AtomicU64::new(0),
                live: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
            }
        }

        fn restart_peak(&self) -> usize {
            let live = self.live.load(Relaxed);
            self.peak.store(live, Relaxed);
            live
        }
    }

    /// The writing side, and the whole of what the feature decides.
    ///
    /// Nothing calls into this without `render-stats`, so the counters below
    /// never move and every reader answers zero — which is the same sentence
    /// as before and no longer a second implementation of it. The readers used
    /// to have a stub twin returning literal zeroes, compiled only when the
    /// feature was off, and a build that had the feature on could not tell you
    /// whether the twin was right: five of its mutants survived because no
    /// configuration under test contained them.
    #[cfg(feature = "render-stats")]
    impl Counters {
        #[inline]
        fn allocated(&self, size: usize) {
            self.allocations.fetch_add(1, Relaxed);
            self.bytes.fetch_add(size as u64, Relaxed);
            self.grew(size);
        }

        #[inline]
        fn deallocated(&self, size: usize) {
            self.live.fetch_sub(size, Relaxed);
        }

        /// A reallocation is one request, and what it asks for is the
        /// difference. A `Vec` that doubles its way to a megabyte asked for a
        /// megabyte, not two: counting `new_size` again each time would report
        /// every growing buffer as twice its size, and the bytes column is
        /// there to be compared against the next revision's.
        #[inline]
        fn reallocated(&self, old_size: usize, new_size: usize) {
            self.allocations.fetch_add(1, Relaxed);
            match new_size.checked_sub(old_size) {
                Some(grown) => {
                    self.bytes.fetch_add(grown as u64, Relaxed);
                    self.grew(grown);
                }
                None => {
                    self.live.fetch_sub(old_size - new_size, Relaxed);
                }
            }
        }

        #[inline]
        fn grew(&self, by: usize) {
            let live = self.live.fetch_add(by, Relaxed) + by;
            self.peak.fetch_max(live, Relaxed);
        }
    }

    // What the allocator calls on its way through, and the only thing the
    // feature decides. Without it these are empty and the counters below stay
    // at nothing, which is where they started.

    #[cfg(feature = "render-stats")]
    #[inline]
    pub fn allocated(size: usize) {
        COUNTERS.allocated(size);
    }

    #[cfg(feature = "render-stats")]
    #[inline]
    pub fn deallocated(size: usize) {
        COUNTERS.deallocated(size);
    }

    #[cfg(feature = "render-stats")]
    #[inline]
    pub fn reallocated(old_size: usize, new_size: usize) {
        COUNTERS.reallocated(old_size, new_size);
    }

    #[cfg(not(feature = "render-stats"))]
    #[inline(always)]
    pub fn allocated(_size: usize) {}

    #[cfg(not(feature = "render-stats"))]
    #[inline(always)]
    pub fn deallocated(_size: usize) {}

    #[cfg(not(feature = "render-stats"))]
    #[inline(always)]
    pub fn reallocated(_old_size: usize, _new_size: usize) {}

    // And what reads them, in one version: a reader that answers zero because
    // nothing wrote is the same code as a reader that answers what was
    // written, and a test of either is a test of both.

    pub fn allocations() -> u64 {
        COUNTERS.allocations.load(Relaxed)
    }

    pub fn bytes_allocated() -> u64 {
        COUNTERS.bytes.load(Relaxed)
    }

    pub fn live_bytes() -> usize {
        COUNTERS.live.load(Relaxed)
    }

    pub fn peak_live_bytes() -> usize {
        COUNTERS.peak.load(Relaxed)
    }

    pub fn restart_peak() -> usize {
        COUNTERS.restart_peak()
    }

    #[cfg(all(test, feature = "render-stats"))]
    mod tests {
        use super::*;

        /// What `alloc` does, on counters nothing else can reach.
        #[test]
        fn an_allocation_is_one_request_and_its_bytes() {
            let c = Counters::new();
            c.allocated(64);
            c.allocated(8);

            assert_eq!(c.allocations.load(Relaxed), 2);
            assert_eq!(c.bytes.load(Relaxed), 72);
            assert_eq!(c.live.load(Relaxed), 72);
            assert_eq!(c.peak.load(Relaxed), 72);
        }

        /// Freeing gives back exactly what the layout said, and takes nothing
        /// off the totals: a frame that allocated and freed still allocated.
        #[test]
        fn freeing_lowers_the_live_bytes_and_nothing_else() {
            let c = Counters::new();
            c.allocated(64);
            c.deallocated(64);

            assert_eq!(c.allocations.load(Relaxed), 1);
            assert_eq!(c.bytes.load(Relaxed), 64);
            assert_eq!(c.live.load(Relaxed), 0);
            assert_eq!(c.peak.load(Relaxed), 64, "the peak is a high-water mark");
        }

        /// A growing reallocation is one request for the difference. Counting
        /// the whole new size again is the way this is usually got wrong, and
        /// it would report a buffer that doubled its way to 1024 bytes as 2032
        /// bytes allocated.
        #[test]
        fn a_reallocation_that_grows_asks_for_the_difference() {
            let c = Counters::new();
            c.allocated(16);
            c.reallocated(16, 1024);

            assert_eq!(c.allocations.load(Relaxed), 2);
            assert_eq!(c.bytes.load(Relaxed), 1024);
            assert_eq!(c.live.load(Relaxed), 1024);
            assert_eq!(c.peak.load(Relaxed), 1024);
        }

        /// A shrinking one asks for nothing and hands bytes back.
        #[test]
        fn a_reallocation_that_shrinks_gives_bytes_back() {
            let c = Counters::new();
            c.allocated(1024);
            c.reallocated(1024, 16);

            assert_eq!(c.allocations.load(Relaxed), 2, "it is still a request");
            assert_eq!(c.bytes.load(Relaxed), 1024, "and it asked for nothing");
            assert_eq!(c.live.load(Relaxed), 16);
            assert_eq!(c.peak.load(Relaxed), 1024);
        }

        /// A reallocation to the same size is neither, and must not make the
        /// live bytes drift.
        #[test]
        fn a_reallocation_to_the_same_size_moves_no_bytes() {
            let c = Counters::new();
            c.allocated(128);
            c.reallocated(128, 128);

            assert_eq!(c.bytes.load(Relaxed), 128);
            assert_eq!(c.live.load(Relaxed), 128);
        }

        /// What `Region::start` does to the high-water mark: it comes down
        /// to the level that is live, so what follows is measured from there.
        #[test]
        fn restarting_the_peak_forgets_what_came_before() {
            let c = Counters::new();
            c.allocated(1024);
            c.deallocated(1024);
            c.allocated(16);

            assert_eq!(c.restart_peak(), 16);
            assert_eq!(c.peak.load(Relaxed), 16);

            c.allocated(32);
            assert_eq!(c.peak.load(Relaxed), 48);
        }
    }
}
