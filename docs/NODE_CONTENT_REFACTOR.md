# Node / Content Refactor (Design)

**Status:** design — not yet implemented.
**Goal:** split the single `Widget` trait into two honest concepts — a **Node**
(the box: layout, style, interaction, transform, animation, children) and
**Content** (a leaf that declares and draws something: text, image, text input).

This document records the agreed design and a step-by-step migration plan so the
work can be paused and resumed (including across machines).

---

## 1. Why

The runtime has *already* drifted to a two-category world; only the type system
still pretends everything is one uniform `Widget`.

Evidence in the current code:

- **Only `Container` has children.** `register_children` / `reconcile_children`
  are overridden by `Container` alone. `Text`, `Image`, `TextInput` are always
  leaves.
- **Leaves do not own their geometry.** They paint in local coordinates
  `(0,0)`; the parent container positions them. Bounds/origin live in the
  `Tree`, not in the widget (`src/tree.rs`).
- **Leaves do not own their style.** `Text` / `TextInput` resolve
  `tree.inherited_text_style(id)` — colour, font, size flow down from the
  enclosing container (`src/widgets/text.rs:77`).
- **Leaves fill the trait with empty defaults.** `Text` and `Image` really
  implement only `layout` (measure) and `paint` (draw); `event → Ignored`, no
  animations, no reconcile, no `register_children`, no `layout_hints`.

The only concrete `Widget` implementors in the whole library are `Container`
plus the three leaves (`OwnedWidget` and `Box<dyn Widget>` are wrappers). There
is **no** third-party or internal widget that has children other than
`Container`. The extension axis for *arrangement* is the `Layout` trait, not new
widget types.

So: formalizing the split aligns the types with the runtime that already exists.

## 2. The model

Two roles, and the tree is homogeneous over **one concrete Node type**.

- **`Node`** — the current `Container`, made a concrete type (not
  `Box<dyn Widget>`). Carries style, padding, corner/border/shadow, transform,
  interaction (click/hover/scroll/keyboard), animations, scroll, backdrop blur,
  text-style declaration, and **children**. Arrangement of children is delegated
  to a `Layout` (unchanged).
- **`Content`** — a single trait implemented by the leaves (`Text`, `Image`,
  `TextInput`). A leaf `Node` hosts one `Box<dyn Content>` **instead of**
  children. Because the tree citizen is always a `Node`, a row of
  `[text, image, text]` still works: each leaf is a `Node` whose payload is a
  `Content`, laid out among its siblings.

### Decision: one `Content` trait, including the interactive leaf

`TextInput` is *content that is interactive*: it handles keyboard, mouse, focus,
selection, and it animates (cursor blink). Rather than split content into
"static" vs "interactive", we keep **a single `Content` trait** whose behaviour
hooks are optional (default no-ops). `Text`/`Image` implement only `measure` +
`paint`; `TextInput` additionally overrides `event` and `advance_animations`.

The cursor blink is the *only* use of the animation hook today — treat it as the
accepted special case, not as a reason to grow a second trait.

```
                       ┌───────────────────────────────────────────┐
   tree (homogeneous)  │                  Node                      │
                       │  style · layout · interaction · transform  │
                       │  animation · scroll · blur · children      │
                       └───────────────────────────────────────────┘
                                  │                    │
                       children (Vec<Node>)      OR    content (Box<dyn Content>)
                                  │                    │
                          ┌───────┴───────┐     ┌──────┴───────────────────┐
                          │  child Nodes  │     │  Text · Image · TextInput │
                          └───────────────┘     └──────────────────────────┘
```

A `Node` is therefore in one of two shapes:

- **Box node** — has `children` + a `Layout`; no content payload.
- **Leaf node** — has one `Content` payload; no children.

(Starting rule: children and content are mutually exclusive on a node. If a
mixed case ever appears, revisit — but nothing in the codebase needs it today.)

## 3. The `Content` trait

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

Removed relative to `Widget`:

- `register_children` — leaves have none.
- `reconcile_children` — leaves have none.
- `layout_hints` — fill hints are a box concern (a leaf never fills; its host
  node does).

Renamed: `layout` → `measure`, to say out loud that a leaf measures itself and
never lays out children.

### Node's own methods (concrete, not a trait)

`Node` keeps the full set as inherent methods on the concrete type, so calls on
the busy path (layout of children, event routing, animation, reconcile) become
static instead of virtual: `layout`, `paint`, `event`, `advance_animations`,
`reconcile_children`, `register_children`, `layout_hints`. When a `Node` is a
leaf, each of these delegates to its `Content` payload where meaningful
(`measure`/`paint`/`event`/`advance_animations`) and is a no-op for the rest.

## 4. What this buys (and what it does not)

Grounded in the storage + dispatch analysis (`src/tree.rs`, `src/jobs.rs`,
`src/widgets/paint_children.rs`).

### Performance

- **Split as two boxed traits (`dyn Node` + `dyn Content`) ⇒ ~0 gain.** The
  tree already dispatches through `Box<dyn Widget>`; renaming the vtable changes
  nothing.
- **Split → concrete `Node` ⇒ modest, real gain.** The tree stops holding
  `Box<dyn Widget>` and holds a concrete `Node`; container-path methods
  de-virtualize (that is the busy path — most working nodes are boxes), one
  pointer indirection per node access disappears (box data inlines into the
  `Node`), and tree construction does fewer heap allocations. Only leaf
  `measure`/`paint` stays virtual (`dyn Content`), and leaves are the minority
  doing little work.
- **Bounded, not a revolution.** Paint already reuses clean children via
  `Rc::clone` with *no* `paint` call (`paint_children.rs`), and animations /
  reconcile only run for jobbed widgets (`jobs.rs`), so dispatch is already
  well-pruned on stable frames. The de-virtualization bites on *dirty* widgets
  and on tree build. Dominant frame costs (text shaping, GPU submission,
  reactive tracking scopes) are untouched. **Profile before treating this as a
  perf change.**

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

**Not improved by this refactor:** the size of `Node`/`Container` itself (~6k
lines). That is *feature count* (style + layout + interaction + transform +
animation + scroll + blur), not trait shape. It is addressed by the ongoing
sub-module/sub-struct decomposition (`InteractionState`, `ScrollData`,
`ContainerAnims`, `TextStyle`) — an orthogonal axis. Keep the two efforts
separate.

## 5. Migration plan (atomic steps)

Each step must compile and pass `cargo test` + `cargo clippy --all-targets
--all-features -- -D warnings` on its own. Re-bless render snapshots only when a
geometry/draw change is intended, and read the diff.

1. **Introduce the `Content` trait** (`src/widgets/content.rs`), no users yet.
   Define it exactly as in §3. Compiles as dead-but-`pub` API.
2. **Implement `Content` for the three leaves alongside `Widget`.** `Text` /
   `Image`: `measure` + `paint`. `TextInput`: also `event` +
   `advance_animations`. Keep the existing `impl Widget` for now (delegating to
   the new methods) so nothing else breaks. This is the biggest reviewable
   "prove the ergonomics" step and a natural pause point.
3. **Teach `Node` to host content.** Add `content: Option<Box<dyn Content>>` to
   the container/node and route `layout→measure`, `paint`, `event`,
   `advance_animations` to it when present. Sugar `text()`, `image()`,
   `text_input()` build a leaf node wrapping the content.
4. **Make the tree store a concrete `Node`.** Replace `Box<dyn Widget>` in
   `Tree` with `Node`; convert the main-loop / `jobs.rs` dispatch sites to
   concrete calls that branch box-vs-leaf. This is the high-blast-radius step —
   do it alone.
5. **Delete the leaves' `impl Widget`** and remove `Widget`, `AnyWidget`,
   `into_any`, `OwnedWidget`, the blanket impl, and the now-dead genericity in
   `paint_children.rs`. Simplify `IntoChild`/`IntoChildren`.
6. **Rename** `Container` → `Node`/`Element` (pick one) and retire "widget" from
   the public vocabulary. Update `#[component]` macro output, `docs/`, `book/`,
   and snapshots.

Steps 1–2 are safe and self-contained (good first PR / good handoff point).
Steps 3–4 are the core. Steps 5–6 are cleanup + naming.

## 6. Open questions

- **Name for the box:** `Node` vs `Element`. `Node` collides with the internal
  `tree::Node`; `Element` is free. Decide before step 6.
- **`measure` vs `layout` naming** for the `Content` method — `measure` is
  proposed; confirm it reads well at call sites.
- **Mixed content + children** — deliberately disallowed for now; document the
  restriction where the node's builder rejects it (or make it type-level if
  cheap).
- **`Content` object safety** — the trait must stay object-safe (`Box<dyn
  Content>`); no generic methods, no `Self`-returning methods.
