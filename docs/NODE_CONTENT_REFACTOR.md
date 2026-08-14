# Node / Content Refactor (Design)

**Status:** design — not yet implemented.
**Goal:** replace the single `Widget` trait with two honest concepts — a **Node**
(the concrete type you compose and the tree stores) and **Content** (a leaf
payload that declares and draws something: text, image, text input). A **Block**
is the box kind of Node (style, layout, interaction, transform, animation,
children).

This document records the settled design and a step-by-step migration plan so
the work can be paused and resumed (including across machines).

---

## 1. Decisions (settled)

| Topic | Decision |
|---|---|
| **Universal type** | `Node` — the one concrete type you compose and the tree stores, replacing `AnyWidget`/`Box<dyn Widget>`. The tree's per-slot metadata (parent, children, dirty flags, cached paint, origin) stays a **separate** struct; rename today's `tree::Node` to `tree::Slot` to free the name. (These are genuinely different things — a composed `Node` has no parent or dirty flags yet — so the name is freed by renaming, not by merging.) |
| **Box kind** | `Block`, builder **`block()`** (renamed from `Container`/`container()`). |
| **Why rename now** | The migration already rewrites examples, docs, book and the macro, so the rename rides along at near-zero marginal cost. Outside a refactor it would not be worth the churn — this is the one cheap moment. `box` is a Rust keyword (`r#box()` is legal but unreadable); `div`/`pane` were rejected as meaningless; `node()` is ambiguous since `text()` also builds nodes. `block` is the CSS term for exactly this, and the API already speaks CSS (`background`, `padding`, `corner_radius`, `gradient`, `overflow`, `border`). |
| **Content trait** | `Content` — one trait for all three leaves (`Text`, `Image`, `TextInput`). |
| **Interactive leaf** | Kept under the *same* `Content` trait via optional hooks (`event`, `advance_animations`). `TextInput` fits without a second trait; its cursor blink is the one use of the animation hook — the accepted special case. |
| **Sizing method** | `measure`, and **pure**: it takes `&Tree` (not `&mut`), returns size + decoration overflow, and does no bookkeeping. All the cache/dirty/boundary handling moves to the node — see §2. |
| **Content vs children** | Mutually exclusive, **enforced by construction**: `block()` exposes `.child()`/`.children()` and no public `.content()`; `text()`/`image()`/`text_input()` build content nodes with no `.child()`. |
| **Object safety** | `Content` stays object-safe (`Box<dyn Content>`): no generic or `Self`-returning methods. Leaf builder methods (`.nowrap()`, `.password()`, `.content_fit()`) remain inherent on the concrete leaf types, not on the trait. |
| **Tree storage** | `Box<Node>`, **not** an inlined `Node`. See §7 — inlining bloats the hot metadata array and is the one place where the original plan's performance claim was backwards. |
| **Styling a leaf** | Leaves stay minimal: to style or click a text/image you wrap it in a `block()`. Putting style directly on leaves is **deferred**, not rejected — see §3. |

## 2. The prize: hoisting the layout bookkeeping

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

## 3. Deferred: style directly on leaves

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

## 4. Why

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

## 5. The model

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

## 6. The `Content` trait

`Content` is the current `Widget` trait **minus** everything about children, and
minus the bookkeeping hoisted into the node (§2).

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

## 7. What this buys (honestly)

### Performance

- **The one demonstrable win** is §2: `Image` and `TextInput` stop re-measuring
  when nothing changed. That is real work removed, not a micro-optimisation.
- **De-virtualization is modest**, and only in the `Box<Node>` form. Replacing
  `widget: Box<dyn Widget>` with `node: Box<Node>` keeps the same allocation
  count, halves the pointer (8 vs 16 bytes), and makes node-level calls static
  and inlinable. Content dispatch stays `dyn` — minority, little work.
- **Inlining `Node` into the tree slot is rejected.** Rough field count (could
  not be measured in the dev container — no `wayland-client` — so confirm with
  `size_of` before trusting it): `Container` ≈ 400 bytes, of which ~220 is 14
  `Option<Signal>` fields; today's slot ≈ 130 bytes, holding the widget as a
  16-byte fat pointer. Inlining would take every slot — **including every text
  leaf** — to ~500 bytes, a ~4× bloat of the dense array that is walked
  constantly for metadata only (parent chains in `mark_needs_layout`, damage
  accumulation, bounds lookups). The vtable it removes is on calls the job
  system and paint cache already skip on most frames. Cost is certain, benefit
  is not.
- **Dominant frame costs are untouched** by any of this: text shaping, GPU
  submission, reactive tracking scopes. Do not sell this refactor as a
  performance change beyond §2.

### Complexity

- **The types stop lying.** Leaves can no longer claim to have children,
  reconcile, or register descendants. This is the main benefit and it is
  qualitative.
- **Written code shrinks only a little.** Today `Text`/`Image` write 3 trait
  methods and `TextInput` writes 4; the rest are defaults they never touch.
  After the split they write 2–4. (An earlier draft of this document claimed
  "7 methods to 2" — that counted the trait's surface, not what leaves actually
  implement. Corrected.)
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

## 8. Migration plan (atomic steps)

Each step must compile and pass `cargo test` + `cargo clippy --all-targets
--all-features -- -D warnings` on its own. Re-bless render snapshots only when a
geometry/draw change is intended, and read the diff.

0. **Rename `Container` → `Block`, `container()` → `block()`.** Pure mechanical
   commit, no logic changes. Done first so every later step is written in the
   final vocabulary. Touches `src/`, `examples/`, `tests/`, `docs/`, `book/`,
   and the `#[component]` macro.
1. **Introduce the `Content` trait** (`src/widgets/content.rs`), no users yet.
   Define it exactly as in §6. Compiles as dead-but-`pub` API.
2. **Implement `Content` for the three leaves alongside `Widget`.** `Text` /
   `Image`: `measure` + `paint`. `TextInput`: also `event` +
   `advance_animations`. Keep `impl Widget` for now, with its `layout` doing the
   bookkeeping and calling the new pure `measure` — this is the rehearsal for
   §2 and the natural pause point.
3. **Introduce the concrete `Node` and hoist the bookkeeping.** `Node` hosts
   either children or a `Box<dyn Content>`; it performs the early-out, boundary
   marking, tracking scope, cache and dirty-flag handling once, then calls
   `measure`. Sugar `text()`/`image()`/`text_input()` build Content-kind nodes;
   `block()` builds Block-kind. **This is where the win in §2 lands.**
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
