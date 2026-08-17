# Refactoring

Working notes for the library's architecture: what landed, what is still open,
and where this document turned out to be wrong.

| Item | State |
|---|---|
| I. Node/Content split | **Closed via salvage** (#172) — the split itself was not carried out |
| II.1 Split `WaylandState` | **Done** (#169) |
| II.2 Break up `render_surface` | **Done** (#170) |
| II.3 Unify the global side channels | **Closed, solved differently** (#171) — see §IV.2 |
| II.4 Collapse the two quad renderers | **Done** (#171) |
| II.5 (a) `mark_needs_paint` walk order | Open |
| II.5 (b) `distribute_jobs` root resolution | Open |
| II.5 (c) damage as a `HashMap` | Open |
| II.5 (d) untested wakeup invariant | **Done** (#171) |
| II.6 Three tracking scopes in one paint | Open |
| II.7 Container collecting one-widget properties | Open |

**Caveat on every performance claim below.** None of it was measured — the
environment this review ran in cannot build the crate — so numbers are counted
from source, not observed. Everything is argued as a *maintainability* change
with a plausible performance side-effect. For speed, the first move is neither
list: run a real example under `render-stats` and read the numbers.

---

# Part I — Node / Content

**Status: closed via salvage (#172). The split was not carried out.**

Three things were taken out of this design and landed on their own, which is
most of what it was actually worth:

- **The dead `event` overrides on `Text` and `Image` were deleted.** Both
  returned `EventResponse::Ignored`, which is the trait default — twelve lines
  saying nothing. This was §I.4's evidence, and removing it needed no refactor.
- **`tree::Node` was renamed `tree::Slot`** (§I.1's decision), because "node"
  already meant two other things in the codebase.
- **Baseline alignment** came out of §I.6's discussion — and disproved part of
  it in the process; see §IV.1.

What remains below is the design record, kept because the *reasoning* is still
the reference for how leaves and boxes relate, not because it is a live plan.

**Goal (as designed):** replace the single `Widget` trait with two concepts — a **Node**
(the concrete type you compose and the tree stores) and **Content** (a leaf
payload that declares and draws something: text, image, text input). A **Block**
is the box kind of Node (style, layout, interaction, transform, animation,
children).

## I.1 Decisions (settled)

| Topic | Decision |
|---|---|
| **Universal type** | `Node` — the one concrete type you compose and the tree stores, replacing `AnyWidget`/`Box<dyn Widget>`. The tree's per-slot metadata (parent, children, dirty flags, cached paint, origin) stays a **separate** struct; rename today's `tree::Node` to `tree::Slot` to free the name. (These are genuinely different things — a composed `Node` has no parent or dirty flags yet — so the name is freed by renaming, not by merging.) |
| **Box kind** | `Block`, builder **`block()`** (renamed from `Container`/`container()`). |
| **Why rename now** | The migration already rewrites examples, docs, book and the macro, so the rename rides along at near-zero marginal cost. Outside a refactor it would not be worth the churn — this is the one cheap moment. `box` is a Rust keyword (`r#box()` is legal but unreadable); `div`/`pane` were rejected as meaningless; `node()` is ambiguous since `text()` also builds nodes. `block` is the CSS term for exactly this, and the API already speaks CSS (`background`, `padding`, `corner_radius`, `gradient`, `overflow`, `border`). |
| **Content trait** | `Content` — one trait for all three leaves (`Text`, `Image`, `TextInput`). |
| **Interactive leaf** | Kept under the *same* `Content` trait via optional hooks (`event`, `advance_animations`). `TextInput` fits without a second trait; its cursor blink is the one use of the animation hook — the accepted special case. |
| **Sizing method** | `measure`, and **pure**: it takes `&Tree` (not `&mut`), returns size + decoration overflow, and does no bookkeeping. All the cache/dirty/boundary handling moves to the node — see §I.2. |
| **Content vs children** | Mutually exclusive, **enforced by construction**: `block()` exposes `.child()`/`.children()` and no public `.content()`; `text()`/`image()`/`text_input()` build content nodes with no `.child()`. |
| **Object safety** | `Content` stays object-safe (`Box<dyn Content>`): no generic or `Self`-returning methods. Leaf builder methods (`.nowrap()`, `.password()`, `.content_fit()`) remain inherent on the concrete leaf types, not on the trait. |
| **Tree storage** | `Box<Node>`, **not** an inlined `Node`. See §I.7 — inlining bloats the hot metadata array and is the one place where the original plan's performance claim was backwards. |
| **Styling a leaf** | Leaves stay minimal: to style or click a text/image you wrap it in a `block()`. Putting style directly on leaves is **deferred**, not rejected — see §I.3. |

## I.2 The prize: hoisting the layout bookkeeping

This is the strongest reason to do the refactor, and the only part with a
*demonstrable* runtime win. It deserves to lead.

All three leaves re-implement the same layout protocol by hand:

```
set_relayout_boundary(false) → (early-out?) → refresh → set_paint_overflow
  → measure → constrain → cache_layout → clear_needs_layout
```

**Two of the three get it wrong.** `Text` has the early-out that skips the work
when constraints are unchanged and no tracked signal marked it dirty
(`src/widgets/text.rs:98-103`). `Image` and `TextInput` **do not** — so they
re-measure every time an ancestor re-runs layout, even when nothing about them
changed. For `TextInput` that means re-running the whole glyph-position pass.

The refactor fixes this structurally rather than by patching two files: the node
does early-out + boundary + cache + clear **once**, and `Content::measure`
becomes just "measure yourself". A leaf can no longer forget the protocol
because the protocol is no longer its job. The same applies to the signal
tracking scope — the node opens `with_signal_tracking(id, JobType::Layout, …)`
around `measure`, instead of each leaf remembering to.

That is the shape of the win: **not** less code in absolute terms, but one
correct copy of a protocol that currently exists in three copies, two of them
wrong.

## I.3 Deferred: style directly on leaves

A leaf could instead carry its own box properties, so a button is one node
rather than two:

```rust
text("Save").padding(8).background(blue).on_click(save)   // one node
```

**Deferred on purpose.** The change is *additive*: every existing form
(`block().padding(8).child(text("x"))`) keeps working untouched, and none of the
decisions above are invalidated by it — `Content`, `measure` and the
content/children exclusivity all stand either way. So it can be decided later,
with more information, at no cost in rework beyond the work itself.

**If it is ever taken up, it must be the shared-trait route**, i.e. box
properties written once as default methods of a `Styled` trait
(`fn box_data(&mut self) -> &mut BoxData`), implemented by `Block` and the three
leaves, with each leaf holding `Option<Box<BoxData>>` so an unstyled leaf pays
one null pointer and no allocation. Leaf builders keep returning `Self`, so
`.nowrap()` and `.background()` chain in any order. Critically, `.child()`,
`.children()`, `.layout()` and `.scrollable()` stay **off** the shared trait —
they remain inherent to `Block`.

**Rejected: the auto-wrapping sugar.** Giving leaves convenience methods that
build the wrapper for them (`Text::padding()` returning a `Block` that wraps
`self`) is cheap but opens a hole in the API — from the second call onwards the
caller holds a `Block`, so this type-checks and reads as nonsense:

```rust
text("ciao").padding(8).layout(Flex::row()).child(text("altro"))
```

A text with a row layout and a text inside it is not a thing. The shared-trait
route does not have this flaw.

## I.4 Why

The runtime has *already* drifted to a two-category world; only the type system
still pretends everything is one uniform `Widget`.

- **Only the box has children.** `register_children` / `reconcile_children` are
  overridden by `Container` alone. `Text`, `Image`, `TextInput` are always leaves.
- **Leaves do not own their geometry.** They paint in local coordinates `(0,0)`;
  the parent positions them. Bounds/origin live in the tree, not in the widget.
- **Leaves do not own their style.** `Text` / `TextInput` resolve
  `tree.inherited_text_style(id)` — colour, font, size flow down from the
  enclosing box (`src/widgets/text.rs:77`).
- **Leaves fill the trait with empty defaults.** `event → Ignored`, no
  animations, no reconcile, no `register_children`, no `layout_hints`.

The only concrete `Widget` implementors in the whole library are `Container`
plus the three leaves (`OwnedWidget` and `Box<dyn Widget>` are wrappers). There
is **no** third-party or internal widget with children other than `Container`.
The extension axis for *arrangement* is the `Layout` trait, not new widget
types.

## I.5 The model

Two axes, and they are **independent**:

- **Topology:** has children (internal node) vs no children (leaf).
- **Kind:** `Block` (styling + layout) vs `Content` (Text/Image/TextInput payload).

|             | has children  | no children (leaf)                                      |
|-------------|---------------|---------------------------------------------------------|
| **Block**   | internal node | empty styled box (spacer, colored rect, ripple surface) |
| **Content** | — (never)     | always here                                             |

A `Block` lives on *both* cells of its row: it may have children or be a
childless styled box. A `Content` lives *only* in the leaf cell. (This is the
correction over an earlier, too-coarse framing that forgot the childless styled
box.)

Everything in the tree is a **`Node`**, which is exactly one kind:

- **Block-kind:** box data + children (possibly empty), arrangement delegated to
  a `Layout`.
- **Content-kind:** one `Box<dyn Content>` payload. Always a leaf.

`Content` is a *payload inside a Node*, never a peer tree citizen: `text("hi")`
is sugar that builds a Content-kind `Node`. That is why `row[text, image, text]`
still works — each leaf is a `Node` laid out among its siblings.

## I.6 The `Content` trait

`Content` is the current `Widget` trait **minus** everything about children, and
minus the bookkeeping hoisted into the node (§I.2).

```rust
/// What a leaf reports from measurement.
pub struct Measured {
    pub size: Size,
    /// How far decoration (glyph stroke/shadow) reaches past the glyphs.
    /// The node records it as damage slop; it must not affect layout.
    pub overflow: f32,
}

pub trait Content {
    /// Measure this content within `constraints`. Pure: no cache writes, no
    /// dirty-flag clearing, no relayout-boundary marking — the node does all
    /// of that. Called inside the node's Layout tracking scope, so signal
    /// reads here subscribe correctly.
    fn measure(&mut self, tree: &Tree, id: WidgetId, constraints: Constraints) -> Measured;

    /// Draw in LOCAL coordinates (0,0 = the hosting node's origin).
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext);

    /// Interactive content only. Default: ignore.
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
        let _ = (tree, id, event);
        EventResponse::Ignored
    }

    /// Interactive content only (e.g. TextInput cursor blink).
    /// Returns true while an animation is still active. Default: none.
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let _ = (tree, id);
        false
    }
}
```

- `Text` / `Image` implement `measure` + `paint` only.
- `TextInput` additionally overrides `event` + `advance_animations`.

Removed relative to `Widget`: `register_children`, `reconcile_children`,
`layout_hints` (all box concerns). Renamed and narrowed: `layout` → `measure`.

`Node` keeps the full set as **inherent methods on the concrete type**, so calls
on the busy path are static rather than virtual. For a Content-kind node they
delegate to the payload where meaningful and are no-ops for the rest.

## I.7 What this buys (honestly)

### Performance

- **The one demonstrable win** is §I.2: `Image` and `TextInput` stop re-measuring
  when nothing changed. That is real work removed, not a micro-optimisation.
- **De-virtualization is modest**, and only in the `Box<Node>` form. Replacing
  `widget: Box<dyn Widget>` with `node: Box<Node>` keeps the same allocation
  count, halves the pointer (8 vs 16 bytes), and makes node-level calls static
  and inlinable. Content dispatch stays `dyn` — minority, little work.
- **Inlining `Node` into the tree slot is rejected.** Rough field count (confirm
  with `size_of` before trusting it): `Container` ≈ 400 bytes, of which ~220 is
  14 `Option<Signal>` fields; today's slot ≈ 130 bytes, holding the widget as a
  16-byte fat pointer. Inlining would take every slot — **including every text
  leaf** — to ~500 bytes, a ~4× bloat of the dense array that is walked
  constantly for metadata only (parent chains in `mark_needs_layout`, damage
  accumulation, bounds lookups). The vtable it removes is on calls the job
  system and paint cache already skip on most frames. Cost is certain, benefit
  is not.
- **Dominant frame costs are untouched** by any of this: text shaping, GPU
  submission, reactive tracking scopes. Do not sell this refactor as a
  performance change beyond §I.2.

### Complexity

- **The types stop lying.** Leaves can no longer claim to have children,
  reconcile, or register descendants. This is the main benefit and it is
  qualitative.
- **Written code shrinks only a little.** Today `Text`/`Image` write 3 trait
  methods and `TextInput` writes 4; the rest are defaults they never touch.
  After the split they write 2–4. (An earlier draft claimed "7 methods to 2" —
  that counted the trait's surface, not what leaves actually implement.)
- **`AnyWidget` narrows rather than evaporates.** If `text()` returns `Text` so
  `.nowrap()` can chain, then a mixed `if/else` still needs a conversion — it
  becomes `.into_node()` instead of `.into_any()`. Better (one concrete type, no
  trait object) but the pattern survives.
- **`OwnedWidget`, the `Widget for Box<dyn Widget>` blanket impl, and the
  speculative genericity in `paint_children.rs`** do go away. That module exists
  "so an external composite widget gets the same behaviour" — no such widget
  exists; with a concrete `Node` it is just a method.

**Not improved:** the size of `Block` itself (~6k lines). That is *feature
count* (style + layout + interaction + transform + animation + scroll + blur),
not trait shape, and it is addressed by the ongoing sub-struct decomposition
(`InteractionState`, `ScrollData`, `ContainerAnims`, `TextStyle`) — an
orthogonal axis. Keep the two efforts separate.

## I.8 Migration plan (atomic steps)

Each step must compile and pass `cargo test` + `cargo clippy --all-targets
--all-features -- -D warnings` on its own. Re-bless render snapshots only when a
geometry/draw change is intended, and read the diff.

0. **Rename `Container` → `Block`, `container()` → `block()`.** Pure mechanical
   commit, no logic changes. Done first so every later step is written in the
   final vocabulary. Touches `src/`, `examples/`, `tests/`, `docs/`, `book/`,
   and the `#[component]` macro.
1. **Introduce the `Content` trait** (`src/widgets/content.rs`), no users yet.
   Define it exactly as in §I.6. Compiles as dead-but-`pub` API.
2. **Implement `Content` for the three leaves alongside `Widget`.** `Text` /
   `Image`: `measure` + `paint`. `TextInput`: also `event` +
   `advance_animations`. Keep `impl Widget` for now, with its `layout` doing the
   bookkeeping and calling the new pure `measure` — this is the rehearsal for
   §I.2 and the natural pause point.
3. **Introduce the concrete `Node` and hoist the bookkeeping.** `Node` hosts
   either children or a `Box<dyn Content>`; it performs the early-out, boundary
   marking, tracking scope, cache and dirty-flag handling once, then calls
   `measure`. Sugar `text()`/`image()`/`text_input()` build Content-kind nodes;
   `block()` builds Block-kind. **This is where the win in §I.2 lands.**
4. **Switch tree storage to `Box<Node>`.** Replace `widget: Box<dyn Widget>`,
   rename `tree::Node` → `tree::Slot`, convert main-loop and `jobs.rs` dispatch
   to concrete calls. Then delete `Widget`, `AnyWidget`, `into_any`,
   `OwnedWidget`, the blanket impl, and the dead genericity in
   `paint_children.rs`; simplify `IntoChild`/`IntoChildren` into `IntoNode`.

Steps 0–2 are safe and self-contained (good first PR / handoff point). Step 3 is
the core and carries the payoff. Step 4 is cleanup plus the storage swap.

An earlier draft had a sixth "vocabulary sweep" step (retiring "widget" as the
public noun everywhere). Dropped: with step 0 doing the rename and step 4
deleting the trait, what is left is cosmetic churn across prose for no
functional gain.

---

# Part II — Backlog

Independent of Part I, ranked by value-for-risk.

## II.1–II.4 — landed

Four items from the original review are merged; the reasoning now lives in the
commits, which are the better record.

- **II.1 Split `WaylandState`** (#169). 2665 lines and 45+ fields covering
  concerns that shared nothing but the word "Wayland", split by concern.
- **II.2 Break up `render_surface`** (#170). The ~360-line, nine-parameter
  frame pipeline became named phases.
- **II.3 Unify the global side channels** (#171) — **not** as proposed here.
  The single-drain idea did not survive contact with the code; the bug class was
  real and was closed another way. See §IV.2.
- **II.4 Collapse the two textured-quad renderers** (#171). With debug labels
  neutralised the two wgpu pipelines were identical byte for byte: same shader,
  vertex format, blend state, sampler. Now one shared pipeline, with only
  texture *production* kept separate. The point was never the ~100 lines — it
  was that the blend state could have drifted on one side with nothing to say so.

## II.5 Jobs / damage / paint-cache — reviewed on request

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

**(d) One invariant is documented but untested — DONE (#171).** The wakeup
contract is now *checked* rather than only described: `queued_but_unwoken()`
names every queue the loop drains, and debug builds assert none is outstanding
when about to block indefinitely. Sound rather than arbitrary — each queue is
drained unconditionally once per iteration, so anything still queued was
produced after its own drain, and only a frame request brings the loop back.
A non-empty queue at that moment means nobody asked to be woken, which is the
exact shape of both historical bugs. Verified in both directions: six examples
idling four seconds each without a false positive, and stubbing out one drain
panics on the next idle, naming the queue.

**Still open:** (a), (b), (c) — all three are unchanged on `main`.
**Value:** small, contained wins. **Risk:** low for (b)/(c), medium for (a) —
its damage/bounds interaction needs deciding, not just moving.

## II.6 Minor: `Container::paint` opens three tracking scopes

`src/widgets/container/mod.rs` calls `with_signal_tracking` three separate times
in one paint — once for `visible`, once for the main property tuple, once for
`corner_radii`. Each is a TLS access, a `RefCell` borrow and a `Vec` push/pop,
paid twice per scope. One scope would do.

**Value:** low but free. **Risk:** none.

## II.7 The container is collecting properties that belong to one widget

`TextStyle` now holds three properties whose own doc comments admit what they
are (`src/widgets/text_style.rs`):

```rust
/// Caret colour. Only `TextInput` reads it.
pub cursor_color: Option<Signal<Color>>,
/// Selection highlight colour. Only `TextInput` reads it.
pub selection_color: Option<Signal<Color>>,
/// Colour of an input's placeholder. Only `TextInput` reads it, …
pub placeholder_color: Option<Signal<Color>>,
```

`placeholder_color` did not start this; it is the third, and the one where it
became visible.

**The smell in its purest form:** one feature now lives in two places. The text
is declared on the widget — `text_input(v).placeholder("Search")` — and its
colour on an ancestor — `container().placeholder_color(gray)`. Nothing about
"placeholder" tells a reader to look in two different types for it.

### The line being crossed

`TextStyle` mixes two different kinds of thing:

- **Genuinely inherited.** `color`, `font_size`, `font_family`, `font_weight`,
  `stroke`, `shadow`. *Any* text-bearing descendant reads them — `Text`,
  `TextInput`, and whatever text-bearing leaf comes next. This is the CSS
  cascade and it is a sound concept.
- **A remote control for one widget.** `cursor_color`, `selection_color`,
  `placeholder_color`. There is no cascade: exactly one widget type reads them.
  They sit on the container only so they can be set from a distance.

Two tests separate them:

1. **Does more than one kind of descendant read it?**
2. **Does it participate in the container's state-layer and animation
   machinery?** This is the *original* justification, from `text_style.rs`'s own
   module docs: text colour lives on the container so it reaches
   `hover_state(|s| s.text_color(…))` and `animate_text_color` instead of
   needing a second copy of both. Nobody animates a placeholder colour on hover.
   **The justification that legitimises `text_color` does not transfer.**

### Why it happened

Two forces, neither of them carelessness:

- **A forced move.** Leaves carry no style of their own (§I.3), so when a
  `TextInput` property is needed, the container is the only place it can go. The
  constraint leaks into the API. This is the second time that deferred decision
  has presented a bill; a third occurrence is grounds to reopen it.
- **Convention pressure.** `placeholder_color` went to the container *to match
  the existing convention*. The convention itself now generates the wrong
  addition, automatically, for whatever property comes next.

### Proposed

1. **Close the inherited set** at the six that genuinely cascade. A deliberately
   small closed set, as CSS itself keeps it.
2. **Widget-specific properties go on the widget**:
   `text_input(v).placeholder_color(gray)`. Today, styling an input's
   placeholder means looking on the *container* — nobody guesses that.
3. **Serve "set it once for the whole app" with context, not with the
   container.** That need is *theming*, not cascade, and
   `src/reactive/context.rs` already exists with theming as its documented use
   case. `TextInput` would resolve: own declaration → theme from context →
   default (today's fallback, text colour at reduced alpha, is already a good
   default and removes most of the need). This scales to toggle, checkbox and
   slider without any of their properties reaching the container.
4. **Move `cursor_color` and `selection_color` too.** Moving only the placeholder
   leaves an inconsistent API and the same pressure to add the next one. The
   force that created the problem is consistency; it has to be turned around.

**Honest tradeoff:** context in guido is app-global, not per-subtree, so this
gives up per-branch overrides for these properties. Unlikely to matter for a
caret or a placeholder, but it is a real thing given up, not a free win.

**Value:** high — it stops an unbounded growth axis on the container (leaf types
× their properties) before toggle/checkbox/slider arrive. **Risk:** low, and it
is independent of Part I.

## II.8 Explicitly not recommended

**The reactive subscriber registry** (`src/reactive/invalidation.rs`). Expected
to be sloppy, it is not: a forward index (signal → subscribers), a reverse index
(widget → signals) for O(1) cleanup, and an `active` dedup set whose comment
states the reason — without it, a signal read by N widgets costs a linear scan
per re-read, O(N²) per frame for something like a theme colour. Leave it alone.

**Inlining widget data into the tree slot.** Covered in §I.7 — it would bloat
the array that is walked constantly for metadata alone, to remove vtable calls
the job system and paint cache already skip on most frames.

---

# Part IV — Where this document was wrong

Two recommendations here did not survive contact with the code. Both are worth
keeping written down: the reasoning that produced them looks sound in the
abstract and is the kind of mistake that repeats.

## IV.1 The `Measured` return type was unnecessary (§I.6)

This document proposed making `Content::measure` "pure" by returning
`Measured { size, overflow }` instead of writing through the tree, on the
argument that a new per-leaf fact added later would otherwise mean touching
every leaf.

That argument was wrong, and adding baseline alignment (#172) is what showed it.
The tree **already** carries per-widget facts a leaf reports during layout —
`set_paint_overflow` is exactly that shape — so `set_baseline` slotted in beside
it and the `Widget` trait did not move at all.

The lesson inverts the original claim: the tree write-back is the *extensible*
pattern, and the "pure" return type is the one that would have needed growing
for every new fact. Purity was being valued for its own sake, against a codebase
that already had a working idiom for the same job.

## IV.2 One drain point was the one thing that could not be done (§II.3)

This document proposed collapsing the eight `take_*`/`flush_*` drains into a
single `FrameSideEffects::drain()`.

It does not survive the code (#171). The drains are **not interchangeable** —
each sits where it does for a reason written beside it: owner disposals run
where no user closure is on the stack, background writes must land before
`take_frame_request()` consumes the flag, orphan jobs need ownership resolved
first. And the clipboard, primary and cursor syncs are not even the same kind of
thing, being outbound and per-surface. Moving them all to one point was the only
option genuinely unavailable.

But the *bug class* behind the request was real and had bitten twice. It was
never about where the drains sit; it is that a producer must also guarantee a
wakeup and nothing enforced it — `clipboard_copy` and `set_cursor` set a flag
and return, working only because a drain happens a few lines later in the same
iteration. An accident of ordering, not a contract.

So the fix was to **enforce the contract instead of moving the code**: assert at
the one moment where breaking it is fatal (§II.5 (d)).

The lesson: this review correctly identified a bug class from its symptoms —
repeated ad-hoc drains, two historical bugs at the same seam — and then reached
for the wrong remedy, tidiness, because the symptom *looked* like duplication.
When several things that resemble each other each carry a written reason for
sitting where they do, the reasons are the design, and unifying them deletes it.
