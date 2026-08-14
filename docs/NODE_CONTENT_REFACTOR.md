# Node / Content Refactor (Design)

**Status:** design — not yet implemented.
**Goal:** replace the single `Widget` trait with two honest concepts — a **Node**
(the concrete tree citizen: everything the tree stores and you compose) and
**Content** (a leaf payload that declares and draws something: text, image, text
input). A **Container** is the box kind of Node (style, layout, interaction,
transform, animation, children).

This document records the settled design and a step-by-step migration plan so
the work can be paused and resumed (including across machines).

---

## 1. Decisions (settled)

| Topic | Decision |
|---|---|
| **Universal type** | `Node` — the one concrete type the tree stores and you compose. Absorbs today's `AnyWidget`/`Box<dyn Widget>` **and** the internal `tree::Node` slot role, so there is no name collision. |
| **Box kind** | Stays `Container`; the builder stays **`container()`** (no abbreviation — `div`/`pane`/`block`/`panel`/`group` were weighed and rejected; `container` is self-documenting and the churn/aliasing isn't worth it). |
| **Content trait** | `Content` — one trait for all three leaves (`Text`, `Image`, `TextInput`). |
| **Interactive leaf** | Kept under the *same* `Content` trait via optional hooks (`event`, `advance_animations`). `TextInput` fits without a second trait; its cursor blink is the one use of the animation hook — the accepted special case. |
| **Sizing method** | `measure` (not `layout`): a leaf measures itself, it never lays out children. |
| **Content vs children** | Mutually exclusive, **enforced by construction**: `container()` exposes `.child()`/`.children()` and no public `.content()`; `text()`/`image()`/`text_input()` build content nodes with no `.child()`. To style a leaf you wrap it in a `container()`, as today. |
| **Object safety** | `Content` stays object-safe (`Box<dyn Content>`): no generic or `Self`-returning methods. Leaf builder methods (`.nowrap()`, `.password()`, `.content_fit()`) remain inherent on the concrete leaf types, not on the trait. |
| **Styling a leaf** | Leaves stay minimal: to style or click a text/image you wrap it in a `container()`, as today. Putting style directly on leaves is **deferred**, not rejected — see §2. |

## 2. Deferred: style directly on leaves

A leaf could instead carry its own box properties, so a button is one node
rather than two:

```rust
text("Save").padding(8).background(blue).on_click(save)   // one node
```

**Deferred on purpose.** The change is *additive*: every existing form
(`container().padding(8).child(text("x"))`) keeps working untouched, and none of
the decisions above are invalidated by it — `Content`, `measure` and the
content/children exclusivity all stand either way. So it can be decided later,
with more information, at no cost in rework beyond the work itself.

**If it is ever taken up, it must be the shared-trait route**, i.e. box
properties written once as default methods of a `Styled` trait
(`fn box_data(&mut self) -> &mut BoxData`), implemented by `Container` and the
three leaves, with each leaf holding `Option<Box<BoxData>>` so an unstyled leaf
pays one null pointer and no allocation. Leaf builders keep returning `Self`, so
`.nowrap()` and `.background()` chain in any order. Critically, `.child()`,
`.children()`, `.layout()` and `.scrollable()` stay **off** the shared trait —
they remain inherent to `Container`.

**Rejected: the auto-wrapping sugar.** Giving leaves convenience methods that
build the wrapper for them (`Text::padding()` returning a `Container` that wraps
`self`) is cheap but opens a hole in the API — from the second call onwards the
caller holds a `Container`, so this type-checks and reads as nonsense:

```rust
text("ciao").padding(8).layout(Flex::row()).child(text("altro"))
```

A text with a row layout and a text inside it is not a thing. The shared-trait
route does not have this flaw.

## 3. Why

The runtime has *already* drifted to a two-category world; only the type system
still pretends everything is one uniform `Widget`.

Evidence in the current code:

- **Only `Container` has children.** `register_children` / `reconcile_children`
  are overridden by `Container` alone. `Text`, `Image`, `TextInput` are always
  leaves.
- **Leaves do not own their geometry.** They paint in local coordinates
  `(0,0)`; the parent positions them. Bounds/origin live in the `Tree`, not in
  the widget (`src/tree.rs`).
- **Leaves do not own their style.** `Text` / `TextInput` resolve
  `tree.inherited_text_style(id)` — colour, font, size flow down from the
  enclosing container (`src/widgets/text.rs:77`).
- **Leaves fill the trait with empty defaults.** `Text` and `Image` really
  implement only `layout` (measure) and `paint` (draw); `event → Ignored`, no
  animations, no reconcile, no `register_children`, no `layout_hints`.

The only concrete `Widget` implementors in the whole library are `Container`
plus the three leaves (`OwnedWidget` and `Box<dyn Widget>` are wrappers). There
is **no** third-party or internal widget with children other than `Container`.
The extension axis for *arrangement* is the `Layout` trait, not new widget
types. Formalizing the split aligns the types with the runtime that already
exists.

## 4. The model

Two axes, and they are **independent**. This is the key correction over an
earlier, too-coarse framing ("a node is either a box with children or a leaf
with content") — which forgot the childless styled box.

- **Topology axis:** has children (internal node) vs no children (leaf).
- **Kind axis:** `Container` (styling + layout) vs `Content` (Text/Image/TextInput payload).

|                | has children  | no children (leaf)        |
|----------------|---------------|---------------------------|
| **Container**  | internal node | empty styled box (spacer, colored rect, ripple surface) |
| **Content**    | — (never)     | always here               |

So a `Container` lives on *both* cells of its row: it may have children or be a
childless styled box. A `Content` lives *only* in the leaf cell.

Everything in the tree is a **`Node`**. A `Node` is exactly one kind:

- **Container-kind:** box data (padding, background, border, corner, shadow,
  transform, interaction, animations, scroll, backdrop blur, text-style
  declaration) + children (possibly empty). Arrangement delegated to a `Layout`.
- **Content-kind:** one `Box<dyn Content>` payload. Always a leaf.

`Content` is a *payload inside a Node*, never a peer tree citizen: `text("hi")`
is sugar that builds a Content-kind `Node`. That is why `row[text, image, text]`
still works — each leaf is a `Node` laid out among its siblings.

```
                        the tree stores Nodes
                    ┌──────────────┴───────────────┐
             Container-kind                   Content-kind
        (style · layout · children)        (Box<dyn Content>)
          │                  │                     │
    has children       no children          Text · Image · TextInput
    (internal node)  (empty styled box)        (always a leaf)
```

## 5. The `Content` trait

`Content` is the current `Widget` trait **minus** everything about children.

```rust
pub trait Content {
    /// Measure the intrinsic size of this content within `constraints`.
    /// A leaf has no children to lay out — this is measurement only.
    fn measure(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size;

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
`layout_hints` (all box concerns). Renamed: `layout` → `measure`.

### Node's own methods (concrete, not a trait)

`Node` keeps the full set as inherent methods on the concrete type, so calls on
the busy path (layout of children, event routing, animation, reconcile) become
static instead of virtual: `layout`, `paint`, `event`, `advance_animations`,
`reconcile_children`, `register_children`, `layout_hints`. For a Content-kind
node these delegate to the `Content` payload where meaningful
(`measure`/`paint`/`event`/`advance_animations`) and are no-ops for the rest.

## 6. What this buys (and what it does not)

Grounded in the storage + dispatch analysis (`src/tree.rs`, `src/jobs.rs`,
`src/widgets/paint_children.rs`).

### Performance

- **Two boxed traits (`dyn` box + `dyn Content`) ⇒ ~0 gain.** The tree already
  dispatches through `Box<dyn Widget>`; renaming the vtable changes nothing.
- **A concrete `Node` ⇒ modest, real gain.** The tree stops holding
  `Box<dyn Widget>` and holds a concrete `Node`; Container-path methods
  de-virtualize (that is the busy path — most working nodes are containers), one
  pointer indirection per node access disappears (box data inlines into the
  `Node`), and tree construction does fewer heap allocations. Only leaf
  `measure`/`paint` stays virtual (`dyn Content`), and leaves are the minority
  doing little work.
- **Bounded, not a revolution.** Paint already reuses clean children via
  `Rc::clone` with *no* `paint` call (`paint_children.rs`), and animations /
  reconcile only run for jobbed widgets (`jobs.rs`), so dispatch is already
  well-pruned on stable frames. De-virtualization bites on *dirty* widgets and
  on tree build. Dominant frame costs (text shaping, GPU submission, reactive
  tracking scopes) are untouched. **Profile before treating this as a perf
  change.**

### Complexity

Real reduction, concentrated in the leaf/dispatch/composition machinery:

- Leaf contract drops from **7 methods to 2 (+2 optional)**; the types stop
  claiming leaves can have children.
- `AnyWidget` / `into_any()` type-erasure largely evaporates: content becomes
  data, conditional branches return the same `Node` type.
- `OwnedWidget` wrapper and the `Widget for Box<dyn Widget>` blanket impl shrink
  or disappear.
- `IntoChild` / `IntoChildren` routing narrows.
- Speculative genericity goes away: `paint_children.rs` exists "so an external
  composite widget gets the same behaviour" — but no such widget exists; with a
  concrete `Node` it is just a `Node` method.

**Not improved by this refactor:** the size of `Container` itself (~6k lines).
That is *feature count* (style + layout + interaction + transform + animation +
scroll + blur), not trait shape. It is addressed by the ongoing
sub-module/sub-struct decomposition (`InteractionState`, `ScrollData`,
`ContainerAnims`, `TextStyle`) — an orthogonal axis. Keep the two efforts
separate.

## 7. Migration plan (atomic steps)

Each step must compile and pass `cargo test` + `cargo clippy --all-targets
--all-features -- -D warnings` on its own. Re-bless render snapshots only when a
geometry/draw change is intended, and read the diff.

1. **Introduce the `Content` trait** (`src/widgets/content.rs`), no users yet.
   Define it exactly as in §4. Compiles as dead-but-`pub` API.
2. **Implement `Content` for the three leaves alongside `Widget`.** `Text` /
   `Image`: `measure` + `paint`. `TextInput`: also `event` +
   `advance_animations`. Keep the existing `impl Widget` for now (delegating to
   the new methods) so nothing else breaks. Biggest "prove the ergonomics" step
   and a natural pause point.
3. **Introduce the concrete `Node`** (Container-kind | Content-kind) and teach
   it to host content. Sugar `text()` / `image()` / `text_input()` build
   Content-kind nodes; `container()` builds Container-kind. Route
   `layout→measure`, `paint`, `event`, `advance_animations` to the payload.
4. **Make the tree store a concrete `Node`.** Replace `Box<dyn Widget>` in the
   `Tree` (merging the slot role) with `Node`; convert the main-loop / `jobs.rs`
   dispatch sites to concrete calls that branch by kind. Highest blast radius —
   do it alone.
5. **Delete the leaves' `impl Widget`** and remove `Widget`, `AnyWidget`,
   `into_any`, `OwnedWidget`, the blanket impl, and the now-dead genericity in
   `paint_children.rs`. Simplify `IntoChild`/`IntoChildren`.
6. **Vocabulary sweep.** Update `#[component]` macro output, `docs/`, `book/`,
   and snapshots to the `Node` / `Container` / `Content` vocabulary. `Container`
   and `container()` keep their names; "widget" retires as the public noun.

Steps 1–2 are safe and self-contained (good first PR / handoff point). Steps 3–4
are the core. Steps 5–6 are cleanup.
