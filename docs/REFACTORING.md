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
| II.7 Style ownership (leaf style, provider nodes, animation) | Open — designed, ready |
| II.8 States: interaction groups | Open — designed; the name is the last question |

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

## II.7 Style ownership: each widget declares what it draws

**The rule.** A widget declares the properties it draws. A container draws a box
— padding, background, border, corner, shadow — and declares those. A text draws
glyphs and declares their colour and metrics. An input draws a caret, a
selection and a placeholder and declares those.

Stated once, the rule makes the current drift **impossible by construction**:
`placeholder_color` cannot land on the container, because the container does not
draw the placeholder.

### What is wrong today

`TextStyle` carries three properties whose own doc comments admit what they are:

```rust
/// Caret colour. Only `TextInput` reads it.
pub cursor_color: Option<Signal<Color>>,
/// Selection highlight colour. Only `TextInput` reads it.
pub selection_color: Option<Signal<Color>>,
/// Colour of an input's placeholder. Only `TextInput` reads it, …
pub placeholder_color: Option<Signal<Color>>,
```

And one feature now lives in two types: the placeholder text is declared on the
widget — `text_input(v).placeholder("Search")` — while its colour is declared on
an ancestor — `container().placeholder_color(gray)`. Nothing about "placeholder"
tells a reader to look in two places for it.

Two forces produced this, neither of them carelessness:

- **A forced move.** Leaves carry no style of their own, so a `TextInput`
  property has nowhere else to go. The constraint leaks into the API.
- **Convention pressure.** `placeholder_color` went to the container *to match
  the existing convention*. The convention now generates the wrong addition
  automatically, for whatever property comes next.

### The vocabulary, written once

```rust
pub trait TextStyled: Sized {
    #[doc(hidden)]
    fn text_style_mut(&mut self) -> &mut TextStyle;

    fn color<M>(mut self, c: impl IntoSignal<Color, M>) -> Self {
        self.text_style_mut().color = Some(c.into_signal());
        self
    }
    fn font_size<M>(…) -> Self { … }
    fn font_family<M>(…) -> Self { … }
    fn font_weight<M>(…) -> Self { … }
    fn bold(self) -> Self { self.font_weight(FontWeight::BOLD) }
    fn mono(self) -> Self { self.font_family(FontFamily::Monospace) }
    fn text_stroke<M>(…) -> Self { … }
    fn text_shadow<M>(…) -> Self { … }
}

pub trait InputStyled: Sized {
    fn input_style_mut(&mut self) -> &mut InputStyle;

    fn cursor_color<M>(…) -> Self { … }
    fn selection_color<M>(…) -> Self { … }
    fn placeholder_color<M>(…) -> Self { … }
}
```

Every method takes `impl IntoSignal<T, M>`, like the rest of the library — these
are reactive properties, not snapshots.

| | `TextStyled` | `InputStyled` |
|---|---|---|
| `Text` | ✅ | — |
| `TextInput` | ✅ | ✅ |
| `text_style()` (provider node) | ✅ | — |
| `input_style()` (provider node) | — | ✅ |
| `Container` | ❌ | ❌ |

The two ❌ are the point of the whole item. Each implementation is three lines —
hand back a reference to its own style struct — and the vocabulary itself is
written once as default methods. This is the shape of Floem's `Decorators`
trait, so it is not a gamble.

`InputStyle` as a separate struct is how the three misplaced properties leave
`TextStyle`: they belong to a different trait, implemented by whoever draws
them. The §II.7 problem does not need a separate fix — it falls out.

### Three places, one grammar

```rust
text("Save").color(theme.weak)                       // on the widget
text("Save").hover_state(|s| s.color(theme.strong))  // as a state override
text_style().color(theme.weak).child(…)              // on the provider node
```

`s` in that closure is the same builder: `TextStyled`'s methods. `TextStyle` is
*already* a partial style — every field is `Option<Signal<T>>` — so a state
override is simply another one layered on top. No second vocabulary.

### The provider nodes

Inheritance survives; what changes is **who provides it**. Today any container
can be a style source, so answering "what colour is this text?" means walking
ten ancestors and checking each. A dedicated node makes the source visible and
intentional — which is *more* fiscal than today, not less.

Prior art: this is Flutter's `DefaultTextStyle`, a widget whose only job is
providing a default text style to its subtree.

Three settled decisions:

1. **`text_color` becomes `color`.** The old name existed only to disambiguate
   from a box's fill. Off the box, `text("x").color(red)` has one reading.
2. **A provider takes exactly one child.** It is a decorator, not a layout: no
   spacing, no arrangement decisions. To dress a group, wrap the group's
   container. Give it multiple children and you have rebuilt a container.
3. **It is layout-transparent.** Constraints pass through, it takes the child's
   size. If it adds a single pixel it becomes "that wrapper that is only there
   for style but shifts everything", and you will hate it within months.

The migration is cheap because the hard part is not in `Container`. Resolution
already lives in the tree — the slot holds `text_style: Option<Box<TextStyle>>`,
`tree.inherited_text_style(id)` does the walk, and `Container` merely *populates*
it. Only the populating changes. The walk, the per-property resolution, the
subscription correctness under skipped layout, the `is_empty()` skip: untouched.

### Resolution is a chain

Per property, first declaration wins:

```
active state overrides (last declared first)
  → the widget's own declaration
    → walk up to the provider nodes
      → default
```

Two steps in front of today's walk; the delicate part — the walk starting *at
the text*, so subscriptions land correctly when layout is skipped — is unchanged.

### Animation lives with the declaration

```rust
text("Save").color(weak).hover_state(|s| s.color(strong))
    .animate_color(Transition::new(200.0, TimingFunction::EaseOut))

text_style().color(theme.text)
    .animate_color(Transition::new(200.0, TimingFunction::EaseOut))
    .child(/* ten texts */)
```

Declared on the text → the text holds the `AnimationState<Color>` and consumes
it in its own paint, exactly as a container does for `background`. Declared on a
provider → **the provider animates once and publishes the displayed value**, and
its subtree follows. That is also the efficient choice: ten texts each animating
their resolved value would run ten identical interpolations that can drift a
frame apart.

Same for `input_style()` on a caret or selection colour.

### What this deletes

Today the container carries, solely because the animated property is drawn by a
*different* widget:

```rust
animated_text: Option<RwSignal<Option<Color>>>,  // one signal write per frame
text_owner:    Option<OwnerId>,                  // plus a dedicated impl Drop
text_base:     Option<Signal<Color>>,
```

Three fields, a `Drop`, a published derived and an owner torn down by hand. When
the declaration and the paint live in the same widget, the value never crosses a
boundary and all of it goes.

A provider publishing an animated signal is *not* the same wart: publishing
style to its subtree is its entire job. The per-frame repaint of the text
remains, and is unavoidable — the current code says so itself: *"a write per
frame, which is what a per-frame repaint of the text costs under any design"*.

### A separate bug this fixes on the way

State overrides are **not reactive today**:

```rust
pub struct StateStyle {
    pub border_color: Option<Color>,   // Color, not Signal<Color>
    …
}
pub fn text_color(mut self, color: impl Into<Color>) -> Self   // not IntoSignal
```

Base properties take signals; state overrides take fixed values. So
`container().background(theme_signal)` follows the theme and
`hover_state(|s| s.background(…))` does not. Making a state override the same
partial-style type as the base fixes this for free.

Two details to keep in mind while doing it:

- **The ripple** is the one thing in `StateStyle` that is an effect with a life
  of its own rather than a property. It stays on the box.
- **`BackgroundOverride::Lighter/Darker`** are relative to the base value, so in
  a layered chain they must resolve *after* the base is found, not during. This
  works today only because there is a single level.

**Value:** high — it closes an unbounded growth axis on the container (leaf
kinds × their properties) before toggle, checkbox and slider arrive.
**Risk:** low. Independent of Part I.

## II.8 States: interaction groups

Everything in §II.7 concerns *style*. Putting **states** on leaves —
`text("x").when_hovered(…)` — raises a question that style does not, and the
answer below came out of a long back-and-forth in which three other candidates
were tried and dropped. They are kept at the end, because the reasons they fail
are the argument for the one that stands.

### The question style does not raise

When a `text` declares a hover style, hover **of what**? The text is not the
thing the pointer is aimed at. A button's label must light up while the pointer
is on the button's *padding*, nowhere near the glyphs — so "my own bounds" is
wrong. And a label inside one button must **not** light up when the pointer is
over a *sibling* button inside the same highlighted row — so "any ancestor" is
wrong too.

Focus makes it worse, because it runs the other way. Hover comes from **above**
(the button is hovered, its descendants should react); focus comes from
**below** (the input holds focus, its ancestors should react). Two mechanisms
pointing in opposite directions is a bad smell, and it left one case with no
answer at all: a label that must react to the focus of a *sibling* input — the
floating label of every form ever written.

### The proposal: a subtree that behaves as one unit

Mark a subtree as a single **interaction group**. The group is the unit that
holds state; every widget inside it asks the same question — *is my group in
this state?* — regardless of which state.

```rust
container().group()
    .child(text("Password").when_focused(|s| s.color(blue)))
    .child(text_input(password))
```

The label and the input are in the same group, so focus on the input is focus
on the group, and the label can react. **The sibling case, which had no clean
answer, simply disappears.**

The direction stops being a property of the mechanism and becomes only how a
group notices:

- **hover** — the pointer is inside my bounds
- **focus** — the focus is inside my subtree

After that the resolution is identical for both. One rule, two ways of noticing.

| scenario | resolves from | result |
|---|---|---|
| label while the pointer is on the button's padding | the button group, pointer inside | lights ✓ |
| a button's label while the pointer is over a sibling | its own (nested) button group, pointer outside | stays ✓ |
| lockscreen container wrapping the focused input | itself, focus inside | shows the border ✓ |
| label beside a focused input | the same group as the input | reacts ✓ |

### What a nested group isolates — and what it must not

Two readings are possible here and only one works.

**It scopes resolution, not state.** Every group notices its own state
independently: a list row is hovered because the pointer is inside the row, full
stop, even when the pointer is over a button nested within it. What the nested
group changes is only **who a descendant asks**: the label inside the button
asks the button, not the row.

The other reading — a nested group *blocks* the outer one — breaks the list row:
moving onto the button would switch the row's highlight off, when the pointer is
plainly still on the row.

### It absorbs `interactive()`

These are not two concepts. A pointer target *is* an interaction unit, and an
interaction unit has to know whether it is being pointed at. So there is one
thing to declare, and `on_click` (and every other behaviour: `on_hover`,
`on_scroll`, `scrollable`) implies it — out of necessity, not convenience: you
cannot click what is not a target.

Against the discarded alternative A, this is what the group buys: the source is
no longer **inferred** from "does it have a behaviour", it is **visible** — the
boundary is written where it is.

### Prior art

This is Material-UI's `FormControl`: a component that wraps a label, a field and
its helper text, and shares `focused`, `error`, `disabled` and `required` with
its descendants. Same design, arrived at from the same place — forms — and in
use by a very large number of people. SwiftUI's `.disabled()` propagating
through the environment is the same shape.

### The four families of state

One mechanism does not explain all of them, and it would be suspicious if it
did.

| state | where it comes from | resolved by |
|---|---|---|
| hover, pressed | geometry — the pointer | is my group under the pointer |
| focus | structure — the focus path | is the focus inside my group |
| error, selected, valid… | a signal the app owns | read the signal directly — no mechanism needed |
| disabled | a signal the app owns, but blocks input | style like the above; behaviour propagates down the group |

The third row is worth noticing: **app-declared states need no propagation at
all**, because the condition is a signal the caller already holds and can read
anywhere.

```rust
container().state(wrong_password, |s| s.border(2.0, red))
    .child(text("Wrong password").state(wrong_password, |s| s.color(red)))
```

Both read the same signal, independently. This is also why one generic
`.state(condition, |s| …)` is better than a named method per case: naming
`error_state`, then `selected_state`, then `checked_state` is the same dynamic
that put `placeholder_color` on the container.

### Naming: the one genuinely open point

`group()` is the working name and it is not settled. It is generic — guido
already groups things for layout — and the concept is narrower than the word.
Candidates:

- **`group()`** — short, but says nothing about *what kind* of group.
- **`interaction_scope()`** — precise, long, and reads like plumbing.
- **`control()`** — says what the thing *is* (a control), and is the word
  accessibility uses for exactly this. It also predicts the a11y role boundary,
  which is likely the same boundary.

Worth settling against real call sites before it spreads.

### Also settled along the way

- **`hover_state` should be renamed `when_hovered`.** The old name reads as "*my*
  hover state", which is false on a label. `when_hovered` promises nothing about
  whose — it says "when there is hover", and the unit that can be hovered is the
  group you belong to. True on the button and on its label alike, with no second
  method.
- **`focus_visible`** stays needed: in the lockscreen an input is focused
  essentially always, so a permanent focus ring is noise. The web separates
  focus arrived at by keyboard from focus arrived at by pointer; it only takes
  remembering how it arrived.
- **Precedence: last declared wins.** CSS's rule at equal specificity — write
  the error state after the focus state and a focused field in error shows the
  error.

### Still open

- **No enclosing group.** A `when_hovered` with no group above it: silent no-op,
  or does the widget become its own group? The latter, probably — silent
  declarations are the failure mode this whole design is trying to avoid, and
  the existing reactive diagnostics are the natural place to warn.
- **Attribute or node?** An attribute on the container is enough almost always,
  since grouping usually implies a container to arrange things in. A standalone
  node only if a case appears for grouping without a box.
- **Keyboard.** Which widgets may receive keys is a separate unanswered
  question: today keys are broadcast to the whole tree and each widget filters
  itself by `has_focus`. The group is a plausible boundary for that too, but it
  has not been worked through.
- **`disabled` propagation** is event dispatch, not style resolution — a
  different mechanism that happens to share the group's boundary.

### Alternatives that were tried and dropped

**A — the source is whoever has a behaviour.** No new methods: `on_click` makes
you a source, a state style does not. It works for every case above, and its
rule is a decent sentence ("hover follows the nearest thing you can actually
activate"). Dropped in favour of the group because the source is *inferred*
rather than visible, and because it has no answer for focus at all — the
lockscreen's wrapper has no behaviour, so under A it would find no source, while
today it works by focus containment.

**B — two method names** (`hover_state` vs `ancestor_hover_state`), or the same
idea as a scope argument, or as a state passed into the style closure. Explored
in some depth and dropped: **none of these removes the ambiguity, they only
restate it** — "the ancestor's hover" still has to decide *which* ancestor, and
every variant needs the same walk. What B does buy is legibility, since a
declaration that depends on an ancestor stops looking self-contained; that gain
is bought far more cheaply by the `when_hovered` rename. Its one uniquely
covered case — an interactive widget that wants an ancestor's state instead of
its own — is a clickable thing inside a clickable thing whose feedback points at
the wrong target, i.e. a UX bug rather than a case to support.

**C — inherit resolved values, not states.** What CSS, Floem and guido today all
do: the interactive ancestor resolves its own style including hover, and the
*resolved* colour descends to a label that knows nothing about hover. Requires
no new concept whatsoever and is the trodden path. Dropped because it forces the
ancestor to know its subtree contains text — the button has to declare "my text
is white on hover" — which is exactly the coupling §II.7 exists to remove.

**Naming the source explicitly** (`hover_state_of(button_ref, …)` via
`WidgetRef`) removes all ambiguity and stays available as an escape hatch for
the rare "react to a further ancestor" case. Not the default: it costs a
variable and a binding at every ordinary call site.

## II.9 Explicitly not recommended

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
