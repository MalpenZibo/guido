# Refactor Backlog

Architectural review of the library outside the Node/Content work (which lives
in [NODE_CONTENT_REFACTOR.md](./NODE_CONTENT_REFACTOR.md)). Items are ordered by
value-for-risk, not by size.

**Caveat on every performance claim below.** None of this was measured: the dev
container this review ran in cannot build the crate (`wayland-client` missing),
so all numbers are counted from source, not observed. Everything here is
justified as a *maintainability* change with a plausible performance
side-effect. If the goal is speed, the first move is not on this list — it is to
run a real example under the `render-stats` feature and read the numbers. The
infrastructure already exists (`src/render_stats.rs`, 749 lines) and is there for
exactly this.

---

## 1. Split `WaylandState` — the real god object

`src/platform/wayland.rs`, 2665 lines, one struct with **45+ fields**.

Unlike `Container`, whose forty-odd methods at least all describe one thing (a
box), this one holds concerns that share nothing but the word "Wayland":
registry and compositor state, output enumeration, layer shell, xdg popups,
session lock, pointer, touch, keyboard, cursor shape, clipboard, primary
selection, background-effect blur, input regions.

**Proposed split:** `OutputRegistry` (output ids, hotplug), `InputState`
(pointer, touch, keyboard, modifiers, cursor), `Selections` (clipboard, primary,
prefetch generations, serials), `ShellObjects` (layer shell, xdg shell, session
lock).

The smithay `*Handler` traits must be implemented on a single type, but that is
not an obstacle: the `impl` blocks stay on `WaylandState` and delegate; only the
*state* moves into sub-structs.

**Value:** high — it makes the largest file in the project navigable.
**Risk:** low — moving fields, almost no logic touched.
**Verdict:** best value-for-risk in the codebase. Start here.

## 2. Break up `render_surface`

`src/lib.rs:451`, ~360 lines, **9 parameters**, running fourteen phases in
sequence: event dispatch → distribute jobs → clipboard sync → primary sync →
cursor sync → resize → scale change → frame-pacing gate → drain jobs → layout →
skip check → paint → flatten → damage → present → cache.

The nine parameters are the symptom of a missing `FrameContext`. Split into
named phases (`sync_platform_state`, `pace_frame`, `run_jobs`, `layout_pass`,
`paint_pass`, `present_and_cache`).

The point is not tidiness. The *order* of these phases is the delicate part —
several lines carry comments explaining why they sit where they sit — and right
now that order can only be learned by reading 360 lines top to bottom.

**Value:** high for maintainability. **Risk:** low.

## 3. Unify the per-feature global side channels

There are **24 `thread_local!` blocks across 20 files** and **8 global
`take_*`/`flush_*` drains**: frame request, background writes, pending effects,
dirty segments, cursor, owner disposals, clipboard, primary selection.

Thread-local ambient state is idiomatic for a single-threaded reactive UI
library (Floem and Leptos do the same) and is not the problem. The problem is
that the *drain protocol is ad-hoc, eight times over*: every feature adds a
queue, a drain call at a specific point in the loop, and an obligation to wake
the loop correctly.

This is not hypothetical. `ARCHITECTURE.md` documents two bugs that came from
exactly this seam: the loop spinning at ~260k iterations/s, and a lost wakeup
that left the loop blocked with work queued.

**Proposed:** one `FrameSideEffects` struct with a single `drain()` at one
defined point, or promote each to a real calloop source. The win is that
feature N+1 can no longer get the ordering wrong.

**Value:** high (removes a bug class). **Risk:** medium.

## 4. Collapse the two textured-quad renderers

`src/renderer/image_quad.rs` (888 lines) and `src/renderer/text_quad.rs` (622)
are structurally parallel: `Prepared*Quad`, `Cached*Texture`, `*CacheKey`,
`new(device, format)` building a pipeline, `set_screen_size`, `prepare`,
`render`, each with its own texture cache and eviction. Roughly **1500 lines for
"draw a textured quad", implemented twice.**

A shared `TexturedQuadPipeline`, generic over the cache key and over how the
texture is produced, would collapse a good part of it.

**Value:** medium-high. **Risk:** medium — GPU code, needs on-screen
verification (`grim` screenshots), so it wants a machine that can actually run
the examples.

## 5. Jobs / damage / paint-cache — reviewed on request

This was initially marked "do not touch". After reading it properly that verdict
mostly stands, but with corrections in both directions.

### What is genuinely well built

- **Surface-owned scheduling.** Jobs land in a global inbox, and
  `distribute_jobs` is the single place where ownership resolves. Per-surface
  queues mean a frame-gated surface's animation continuations sit in its own
  queue instead of being advanced by whichever surface renders first. The orphan
  lane guarantees deferred `Unregister` cleanup still runs for destroyed
  surfaces.
- **Dedup and recycling.** `JobQueue` is a `HashSet` for O(1) dedup plus a `Vec`
  for ordered iteration; drained buffers return to a small spare pool.
- **The invariants are pinned by tests, not just by prose.** This corrects the
  concern this review started with. `gated_surface_animations_survive_other_surfaces_drains`
  is a regression test for the busy-spin bug; `dead_surface_queues_are_retired_into_the_orphan_lane`,
  `jobs_without_a_live_surface_go_to_the_orphan_lane` and
  `inbox_dedup_survives_distribution` cover the routing rules;
  `test_mark_subtree_needs_paint_propagates_to_ancestors` covers the
  stale-screen bug; `replaying_a_cached_subtree_reproduces_its_grouping` covers
  flatten-cache replay. The hard-won knowledge is encoded, not just commented.

**Conclusion: do not restructure this.** Its complexity is essential — frame
pacing, multi-surface scheduling and partial paint genuinely interact — and it
is the best-tested part of the library.

### Concrete defects found anyway

**(a) `mark_needs_paint` does the expensive walk before the early-out.**
`src/tree.rs:484`:

```rust
pub fn mark_needs_paint(&mut self, widget_id: WidgetId) {
    // O(depth) walk to sum origins and find the root — runs unconditionally
    if let Some((root, bounds)) = self.surface_relative_bounds_and_root(widget_id) {
        self.expand_damage_rect(root, bounds);
    }
    let mut current = widget_id;
    loop {
        if self.dense[dense_idx].needs_paint {
            return; // already marked — and so are its ancestors
        }
        ...
    }
}
```

A widget marked repeatedly within one frame (several signals, hover plus a
background change, a state layer) pays the parent-chain walk every time, even
though the flag loop short-circuits immediately afterwards. `mark_needs_layout`
has its early-out *first*, which is the right shape.

Not a free fix: if a layout runs between two marks in the same frame the bounds
may legitimately have changed, so skipping would drop damage. Either hoist the
flag check and accept that (documented) tradeoff, or invalidate the cached
damage on layout. Worth doing deliberately, not casually.

**(b) `distribute_jobs` resolves the owning surface per job, per call.**
`tree.surface_root_of(job.widget_id)` is a parent-chain walk run for every job,
and `distribute_jobs` is called at least twice per frame (once in the main loop,
once inside `render_surface` after event dispatch). That is O(jobs × depth) per
frame. A widget's surface root only changes on reparenting, so it is cacheable.

**(c) Damage is a `HashMap<WidgetId, DamageRegion>`** for what is at most a
handful of surfaces. A `SmallVec<[(WidgetId, DamageRegion); 4]>` would be faster
and simpler to read.

**(d) One invariant is documented but untested.** `ARCHITECTURE.md` explains at
length why the wakeup ping must *not* be coalesced through `FRAME_REQUESTED`
(doing so once lost wakeups entirely). Every other documented bug in this
subsystem has a regression test; this one does not — it needs a live event loop,
which makes it the hardest and therefore the most likely to silently regress.

**Value:** (a)–(c) are small, contained wins. (d) is the one worth real effort.
**Risk:** low for (b)/(c), medium for (a), medium for (d).

## 6. Minor: `Container::paint` opens three tracking scopes

`src/widgets/container/mod.rs` calls `with_signal_tracking` three separate times
in one paint — once for `visible`, once for the main property tuple, once for
`corner_radii`. Each is a TLS access, a `RefCell` borrow and a `Vec` push/pop,
paid twice per scope. One scope would do.

**Value:** low but free. **Risk:** none.

---

## Explicitly not recommended

**The reactive subscriber registry** (`src/reactive/invalidation.rs`). Expected
to be sloppy, it is not: a forward index (signal → subscribers), a reverse index
(widget → signals) for O(1) cleanup, and an `active` dedup set whose comment
states the reason — without it, a signal read by N widgets costs a linear scan
per re-read, O(N²) per frame for something like a theme colour. Leave it alone.

**Inlining widget data into the tree slot.** Covered in
[NODE_CONTENT_REFACTOR.md §7](./NODE_CONTENT_REFACTOR.md) — it would bloat the
array that is walked constantly for metadata alone, to remove vtable calls the
job system and paint cache already skip on most frames.
