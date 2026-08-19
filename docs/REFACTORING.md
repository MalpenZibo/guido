# Refactoring

What is in flight in the library's architecture: what shipped, what is closed
and why, and the one design still open.

| Item | State |
|---|---|
| Split `WaylandState` | **Done** (#169) |
| Break up `render_surface` | **Done** (#170) |
| Collapse the two quad renderers, enforce the wakeup contract | **Done** (#171) |
| Dead code, `tree::Node`→`tree::Slot`, baseline alignment | **Done** (#172) |
| Reactive state layers, ordered triggers, `state(condition, ..)` | **Done** (#183) |
| `InputStyle` out of `TextStyle` | **Done** (#184) |
| Leaf-owned text and input style | **Done** (#185) |
| `with_signal_tracking` exported | **Done** (#186) |
| Node/Content split of the `Widget` trait | **Closed** — see below |
| `mark_needs_paint` walk order | **Closed** — see below |
| `distribute_jobs` root resolution, damage as a `HashMap` | **Closed** — see below |
| Three tracking scopes in one paint | **Closed** — see below |
| Container as a style source | Open — direction settled, not scheduled |
| Interaction groups | **Done** (#188), as `control()`; names unified in #189 |
| Public API audit: reactivity, symmetry, preludes | **Done** — see below |

---

## The API audit

An audit of the whole public surface, read against a real application written
on top of it. What it found was not missing features but *inconsistency*: the
same idea spelled two ways, or reactive in one place and constant in the
neighbouring one, so nothing about the API could be predicted from the rest of
it.

**Reactivity had no rule.** `background` took a signal and `gradient` did not;
`corner_radius` did and `backdrop_blur` did not; `visible` did and `overflow`
did not. Worse, `backdrop_blur` was reactive on `Text` and constant on
`Container` — one name, two contracts. All four are reactive now, and the rule
is stated: **anything that survives to paint takes a signal; anything
structural — the layout, whether a container scrolls — does not.**

**Two spellings for one idea.** `Padding::symmetric(horizontal, vertical)`
against the `[vertical, horizontal]` shorthand every call site uses; a border
that could only be set as a pair while its state-layer counterpart had
`border_width` and `border_color` apart; `on_click_option` beside `on_click`
because a `#[component]` prop is an `Option<Callback>`; `Transform::identity()`
beside `Transform::IDENTITY`. In each case one spelling survives, and it is the
one already used everywhere else.

**A handle that did not speak its config's language.** `SurfaceConfig` takes an
`ExclusiveZone` and a `SurfaceExtent`; `SurfaceHandle` took an `i32` and a
`u32`, so a policy could never be changed to another policy and an axis pinned
once could never go back to `content()`. Margins get a `Margin` type with the
conversions `Padding` has.

**A guard whose Drop depended on ambient state.** `create_effect` returned an
`Effect` that disposed itself unless an owner had claimed it — invisible at the
call site either way. It returns nothing now; an effect's lifetime is its
scope's.

**One prelude for two audiences.** 133 names, mixing what an application needs
with what a widget author does — and failing the widget author anyway, since
`Tree`, `WidgetId`, `RenderNode` and `Layout` were not in it. Split into
`prelude` and `widget_prelude`. `IntoSignal` and `IntoVal` moved the other way:
they look internal and are not, being what makes a custom type usable as a
`#[component]` prop.

Also: `create_task` for the very common push-only service, `keyed()` accepting
any `Hash` key rather than insisting on a `u64`, and `#[prop(default = expr)]`
unquoted, which is what makes a string literal spellable as a default.

### What making a property reactive costs

Review of the branch found seven real bugs, and six of them are the same bug:
**a consumer written for a value that could not move.** Worth recording,
because the next property to become reactive will meet it again.

While `gradient`, `backdrop_blur`, `overflow` and `elevation` were constants,
their interactions were settled in the builder chain and visible statically. A
gradient that dropped the shadow was an authoring mistake you made once and saw.
An elevation table read with `level as i32` was exact, because the only levels
that ever arrived were the six integers. A `border_width` with no colour was a
line you would not write. Once a signal can move any of these between frames,
each becomes a case the paint gate has to handle *at every frame*, and the ones
that only misbehave in the fractions or in one branch are invisible until
something animates through them.

The specific shapes, all fixed with tests:

- **A gate on one half of a pair.** `backdrop_blur`'s surface path checked
  `radius > 0.0`; its compositor path never did, and the region registry had no
  withdrawal — so "0.0 means off" held for the half with a draw command and not
  for the half with a side effect.
- **A quantized consumer.** `elevation_to_shadow` truncated to an integer, which
  is a staircase under animation and, where the table met the formula that
  continues it, not even monotonic.
- **A branch that forgets a sibling property.** The gradient path drew and
  returned without consulting the elevation; reachable by signal, mid-animation.
- **A default that is invisible rather than absent.** `border_color` defaults to
  transparent, so gating the frame on the width alone emitted an invisible
  command every frame once `border_width` became separately spellable.
- **A read on the pointer path.** A closure-backed signal recomputes on every
  read, and `overflow` was read during event dispatch — for every container
  under the pointer, on every coalesced `MouseMove`. The tracked reads in layout
  and paint publish what they resolved, and dispatch reads that; which is also
  the more correct value, since events resolve against the frame on screen, as
  `hit.bounds` already do.
- **A request that outruns the round trip.** `set_surface_size` only sends a
  protocol request; the size arrives back in `configure`. An `Auto` exclusive
  zone resolved against the stale value reserved for the size the surface was
  leaving.

The seventh was independent: widening `keyed()` from `u64` to any `Hash` while
the reconciler still indexed by a 64-bit hash of the key, so two colliding keys
became one row.

The documentation was audited against the code in the same pass. It described
`.children_dyn()`, `.padding_horizontal()`, `.min_width()`, `.gradient_diagonal()`
and `.animate_elevation()`, none of which existed; `Row` and `Column` widgets,
which have never existed; the pre-`Tree` `Widget` signatures; and
`hover_state`/`pressed_state`, renamed in #189. Of that list `gradient_diagonal`
and `animate_elevation` turned out to be worth having and were implemented
rather than deleted from the docs.

---

## Closed, with the reason

These were on a backlog and were investigated rather than dropped. The reasons
are here so nobody rediscovers them the hard way.

**Node/Content split of the `Widget` trait.** The plan was to replace one
`Widget` trait with a concrete `Node` plus a `Content` payload for leaves. Three
things were salvaged out of it and landed on their own — the dead `event`
overrides on `Text`/`Image`, `tree::Node`→`tree::Slot`, baseline alignment
(#172) — which was most of what it was worth. The split itself was closed by an
experiment: a fifth leaf written from *outside* the crate compiles in about ten
lines, so the trait was not the obstacle it was argued to be. The one remaining
gap it would have closed — a leaf forgetting the layout protocol — is narrower
than a refactor of the whole tree.

**`mark_needs_paint` does the damage walk before the flag early-out**
(`tree.rs`). Moving the early-out first would break damage: `cache_layout`
damages the vacated rect and then delegates the *new* one to
`mark_needs_paint`, so skipping on an already-set flag drops the new rect
whenever a resize follows a paint mark — which is most resizes.

**`distribute_jobs` resolves the owning surface per job.** It looks like
O(jobs × depth) twice a frame, but the function returns early when the inbox is
empty, so the walk is paid only when there are jobs to route.

**Damage as a `HashMap` for a handful of surfaces.** A `SmallVec` would be
marginally faster and no clearer. Unmeasurable.

**Three tracking scopes in one `Container::paint`.** There are two, and they are
not duplication: the first reads `visible` and returns, the second reads
everything else. Merging them would evaluate every animated property for an
invisible container. `corner_radii` folded into the main tuple in #169.

---

## Open: the container as a style source

`Container` still carries `text_color`, `font_size`, `cursor_color`,
`placeholder_color` and the rest, published to its descendants. Since #185 those
same properties can be declared on the widget that draws them, which is where
they belong, so the container's copies are now a second way to say the same
thing.

The direction, when this is taken up, is a **courier**: one generic method that
the container does not interpret.

```rust
container().provide(TextStyle::new().color(theme.weak).font_size(12.0))
```

Stored type-erased on the node — `TypeId` plus a box, the shape
`reactive/context.rs` already uses — so `Container` and `Tree` stop naming any
style property at all, and a future `SliderStyle` touches neither. The typed
setters are then deleted, migrating the call sites once.

Two things were considered and rejected. A dedicated `text_style()` *node* buys
no mechanism over this — a container with no box properties is already
layout-transparent — only the same discipline at the cost of a type and a slot.
And grouping the setters into `.text_style(|s| ..)` on the container would
migrate every call site twice, since that surface is meant to go.

For "the same kind of label, many times", the documented answer is not
inheritance at all but a function, which keeps the declaration next to the
widget that draws it: see `docs/STYLING.md`.

---

## Shipped: interaction controls

Everything above concerns style. Putting *states* on leaves —
`text("x").when_hovered(..)` — raised a question style does not. This is the
design that answers it, shipped in #188 under the name `control()`: it says
what the thing *is* rather than what it does, and it is the word accessibility
uses for the same boundary, which is likely where the role, label and Tab order
will eventually attach.

### The question

When a `text` declares a hover style, hover **of what**? The text is not what
the pointer is aimed at. A button's label must light up while the pointer is on
the button's *padding*, nowhere near the glyphs — so "my own bounds" is wrong.
A label inside one button must **not** light up when the pointer is over a
sibling button in the same highlighted row — so "any ancestor" is wrong too.

Focus runs the other way. Hover comes from above: the button is hovered, its
descendants react. Focus comes from below: the input holds focus, its ancestors
react. Two mechanisms pointing in opposite directions, and one case with no
answer at all — a label that must react to the focus of a *sibling* input, the
floating label of every form ever written.

### The proposal

Mark a subtree as one interaction unit. The unit holds the state, and every
widget inside asks the same question — *is my unit in this state?* — whatever
the state is.

```rust
container().group()
    .child(text("Password").when_focused(|s| s.color(theme.accent)))
    .child(text_input(password))
```

Direction stops being a property of the mechanism and becomes only how the unit
notices:

- **hover** — the pointer is inside my bounds
- **focus** — the focus is inside my subtree

After that, resolution is identical for both. One rule, two ways of noticing,
and the sibling case disappears.

| scenario | resolves from | result |
|---|---|---|
| label while the pointer is on the button's padding | the button, pointer inside | lights |
| a button's label while the pointer is over a sibling | its own nested unit, pointer outside | stays |
| container wrapping the focused input | itself, focus inside | shows the border |
| label beside a focused input | the same unit as the input | reacts |

### Nesting scopes resolution, not state

Every unit notices its own state independently: a list row is hovered because
the pointer is inside the row, full stop, even when the pointer is over a button
nested within it. What the nested unit changes is only **who a descendant
asks** — the label inside the button asks the button, not the row.

The other reading, where a nested unit blocks the outer one, breaks the list
row: moving onto the button would switch the row's highlight off while the
pointer is plainly still on the row.

### Most of it already exists

- **The pointer** — `InteractionFlags` is already an `RwSignal`, specifically so
  that a descendant resolving a state subscribes to it
  (`widgets/container/mod.rs`).
- **The focus** — `FocusPath::contains(id)` is already "the focus is inside my
  subtree", on a signal (`reactive/focus.rs`).
- **Finding my unit** — the same shape as `Tree::inherited_text_style`: a slot
  on the node, a walk from the widget upwards, signals returned unread so the
  caller's scope subscribes.
- **Nesting** — `track_pointer` already runs on every ancestor *before* the
  children see the event, precisely so a child handling a `MouseMove` does not
  stop its ancestors tracking their own hover. The runtime already behaves the
  way the table above requires.

So this does not introduce a mechanism. It makes a boundary declarable, and
opens to leaves a resolution only containers can do today.

### The risk to design for

If a text reads its unit's flags, every hover repaints every text in the unit,
including ones that declare nothing. The fix is the one already used in
`resolve_state_value`: ask "does anything declare a state?" before touching any
signal. This has to be in the design, not discovered later.

### It absorbs `interactive()`

A pointer target *is* an interaction unit, and an interaction unit has to know
whether it is being pointed at. So there is one thing to declare, and `on_click`
— and `on_hover`, `on_scroll`, `scrollable` — implies it out of necessity: you
cannot click what is not a target.

### Prior art

Material-UI's `FormControl`: a component wrapping a label, a field and its
helper text, sharing `focused`, `error`, `disabled` and `required` with its
descendants. Arrived at from the same place, forms, and in wide use.

### The four families of state

| state | where it comes from | resolved by |
|---|---|---|
| hover, pressed | geometry — the pointer | is my unit under the pointer |
| focus | structure — the focus path | is the focus inside my unit |
| error, selected, valid… | a signal the app owns | read it directly — **shipped in #183** |
| disabled | a signal the app owns, but blocks input | style as above; behaviour propagates down |

The third row needed no mechanism, which is why it shipped ahead of the rest.

### Also settled

- **`hover_state` becomes `when_hovered`** (shipped in #189). The old name
  reads as "*my* hover state", which is false on a label. `when_hovered`
  promises nothing about whose.
- **Precedence is already last-declared-wins** (#183), which is what this
  design needs.

### Settled while building it

- **No enclosing unit**: the widget becomes its own, and notices the pointer
  over its own bounds — silent declarations are the failure mode this design
  exists to avoid. It is never *pressed*, though, because being pressed means
  being activated and it has nothing to activate.
- **A leaf's state style** is the same partial style it is built with, since
  `TextStyle` implements `TextStyled`. One vocabulary, three places.

### Still open

- **`focus_visible`.** Needed: where an input holds focus essentially always, a
  permanent focus ring is noise. Separate question — it is about how the focus
  arrived, not about who resolves it.
- **Keyboard.** Which widgets may receive keys is unanswered; today keys are
  broadcast and each widget filters by `has_focus`. The unit is a plausible
  boundary, not a worked-through one.
- **`disabled` propagation** is event dispatch, not style resolution.

### Alternatives tried and dropped

**The source is whoever has a behaviour.** No new methods: `on_click` makes you
a source, a state style does not. Works for hover, but the source is *inferred*
rather than visible, and it has no answer for focus — a wrapper with no
behaviour would find no source, while focus containment works today.

**Two method names** (`hover_state` vs `ancestor_hover_state`), or a scope
argument, or a state passed into the closure. None removes the ambiguity; they
restate it, since "the ancestor's hover" still has to decide *which* ancestor.
What they buy is legibility, and the `when_hovered` rename buys that far more
cheaply.

**Inherit resolved values, not states** — what CSS, Floem and guido today all
do. Requires no new concept, and is why it lasted this long. Dropped because it
forces the ancestor to know its subtree contains text: the button has to declare
"my text is white on hover", which is exactly the coupling #185 removed.

**Naming the source explicitly** (`when_hovered_of(button_ref, ..)` via
`WidgetRef`) removes all ambiguity and stays available as an escape hatch. Not
the default: it costs a variable and a binding at every ordinary call site.
