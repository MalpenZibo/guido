---
name: widgets
description: Guido's widget layer — the Widget trait, Container's builder API, layouts, state layers, the #[component] macro, and the rule that decides which properties are reactive. Use when adding or changing a widget, a widget property, a layout, or a conversion accepted by a property.
---

# Widgets

## The reactivity rule

**Everything that survives to paint takes `impl IntoSignal<T, M>`. Structural
declarations do not.**

Reactive: `background`, `gradient`, `backdrop_blur`, `overflow`, `corners`,
`border`, `outline`, `translate`, `rotate`, `scale`, `pivot`, `opacity`, `opacity_mode`, `blur`, `width`, `height`,
`padding`, `visible`, `align_self`, `enabled`, `takes_input`, `cursor`, `shadow` — and beyond `Container`, `Text`'s `wrap`, `align`, `max_lines`, `overflow` and `text_box`,
`Image`'s `content_fit`, `TextInput`'s `caret`, `readonly` and `cursor`, `PasswordInput`'s `mask_char` and `reveal`,
`RippleConfig`'s colour, `Arc`'s `size`, `stroke`, `track`, `caps`, `start`
and `sweep`, the direction a `Flex` is built with, and a `ZStack`'s alignment.

Structural: `layout`, `child`, `children`, `control`, `focusable`, the axis a `Scroll` is
built with, the event handlers — and the *motion* a value is declared with,
which is the next section.

A click handler is structural in what it runs and not in whether it is there:
`on_click` and its three siblings take, through `IntoHandler`, a closure or a
signal giving an `Option<Callback>`, which is what an optional `#[component]`
callback prop holds. A stored signal, which cannot change, is resolved where it
is given, so a prop nobody set costs the container nothing. Any other is read
when the press arrives, not tracked, and the container stays registered as a
control while it holds `None` — only the hit test and the press claim follow
it.

`Scroll` is what a family of setters looks like once it is one value.
`scrollable`, `scrollbar_visibility` and `scrollbar` were three, and the last
two were silent no-ops on a container that never became scrollable. Now
`container().scroll(Scroll::vertical())`, and the parts cannot be spelled apart
from the thing they configure. Its measurements — `width`, `hover_width`,
`margin`, `min_handle_size`, `reserve_gutter`, `visibility`, `fill_viewport`
— are reactive; its appearance is `Container`'s own vocabulary, reached
through `.track(|t| ..)` and `.handle(|h| ..)`, because the scrollbar *is* two
containers. Styling a part means styling a container, so nothing has to be
mirrored into a second vocabulary as `Container` grows.

`autofocus` is the one that looks structural and is neither: it is a one-shot
consumed at the first layout, so "take focus when this appears" is an event,
and a signal form would mean something else.

A zero-argument setter naming the off case — `nowrap()`, `no_caret()` — is a
shorthand for the property beside it (`wrap(false)`, `caret(false)`), not a
second way to say it. The property is what takes the signal.

**A value read from an event handler is the exception, and it is a real one.**
`TextInput`'s two clipboard guards ask whether the field masks *now*, untracked,
rather than reading the copy layout took: an event does not wait for a layout
pass, so the cached answer would export a secret for the frame after the field
was told to hide it. Reading a declared value outside layout or paint means
asking why the cache is not good enough, and saying so where it is read.

An animatable property takes `impl IntoAnimated<T, M>` instead, which is
everything `IntoSignal` accepts plus a value carrying its own motion:
`background`, `gradient`, `corners`, `padding`, `border` (each half), `outline`
(each of its three), `shadow`, `width`,
`height`, `translate`, `rotate`, `scale`, `opacity`, `blur` — and beyond `Container`,
`Text`'s `color` and `font_size`, `Image`'s `tint`, and `Arc`'s `color`, `track` and `progress`. The others keep plain `IntoSignal`,
and so does every setter on `StateStyle` — a state layer supplies a value for a
property somebody else declared, so a timing there is a compile error rather
than a value quietly ignored.

A new property lands on one side of that line. If it changes what a pixel looks
like, it is reactive.

## A reactive property has five spellings, and they must agree

`IntoSignal` accepts a value, a closure, a signal, a writable signal or a memo.
A property is only properly reactive when the *same expression* compiles in
every position:

```rust
container().width(100.0)             // value
container().width(move || w.get())   // closure
container().width(w)                 // signal, writable signal or memo
```

**A conversion is declared once, and `converts!` emits every form:**

```rust
crate::reactive::converts!(
    f32 => Length,
    f64 => Length,
);
```

That writes `IntoVal<Length> for f32` — the closure form — and the
`Signal<f32>`, `RwSignal<f32>` and `Memo<f32>` impls into `Length`. The value
form is the `From<f32> for Length` beside the type, which the `=>` arm calls,
so a pair that compiles through `converts!` has all five by construction.

A widening std has no `From` for is spelled `f64 as f32`, and there the value
form is the macro's too. A pair that *has* a `From` must not use `as`: both
value impls would then apply and the marker generic would be undecidable.

It was two lists that mirrored each other by hand, and they drifted: nothing
failed to build when one was missing, and a call site refused months later.
That is what #225 fixed by hand and #226 closed at the definition.

The signal impls still name their source type rather than being one blanket
impl: `IntoVal` is reflexive, so a blanket `S: IntoVal<T>` also covers
`Signal<T> -> T`, collides with the passthrough impl, and leaves the marker
generic undecidable. Excluding the reflexive case needs negative bounds or
specialisation, neither stable. So the pairs are a list — but one list, and the
macro is the only way to write an entry in it.

`tests/signal_conversions.rs` is the list of spellings a caller can reach, and
`one_declaration_covers_every_form` in `reactive::into_signal` is what says the
macro still emits all five. **Add a spelling to the first in the same change.**

## The Widget trait

Two methods are required, the rest default:

- `layout(&mut self, ctx, constraints) -> Size` — measure, and place the
  children with `ctx.layout_child(child, constraints)`. Nothing else: the skip
  when there is nothing to redo, the tracking scope, which pass is running and
  the cache write all belong to that call, which is the only way a layout is
  ever reached. `ctx.measuring()` says whether this is the measure a
  content-sized surface is configured from. A custom `Layout` that reads a
  child's size before deciding its room asks `ctx.measure_child`, then places
  it with `layout_child`: the answers are cached apart, where two
  `layout_child` calls evict each other and run the child twice a pass
- `paint(&self, ctx)` — draw into the `PaintContext`, which carries the tree
  and the widget id (`ctx.tree()`, `ctx.id()`). Called through
  `PaintContext::paint_child`, which opens the widget's own Paint scope, so a
  read made while drawing belongs to it without the widget asking
- `event(&mut self, tree, id, event) -> EventResponse` — defaults to `Ignored`
- `advance_animations` — a default, called on a frame that asked for one.
  Its `bool` is the next frame: `true` is another call, `false` is none, and
  the loop asks rather than the widget. The first frame of a motion is
  `LayoutCtx::request_animation_frame` from `layout`, or
  `Tree::request_animation_frame` elsewhere. A frame is not a paint: what moved
  asks for that itself, with a Paint or Layout job inside the crate and
  `Tree::mark_needs_paint` outside it. A blink, due on a deadline, answers
  `false` — a `true` for it would be a frame every vsync
- `reconcile_children`, `layout_hints`, `register_children` — defaults
- `begin_exit`, `is_exiting`, `cancel_exit` — defaults: what the reconciler
  asks a removed widget and what is built inside it, as far as the next
  dynamic list, and `Container` is the one that answers
- `refresh_paint_bounds` — a default too, and the one a widget that transforms
  itself has to override: a parent narrows its children to the visible rect
  before painting them, by their laid-out bounds, so a widget that draws
  somewhere else says how far with `Tree::set_own_paint_reach`. Called from the
  Paint job *and* after every layout, which is where a reach that follows a
  paint-only property belongs — a transform must not reflow anything. A reach
  that follows a layout-tracked signal is published from `layout` instead, as
  `Text` and `TextInput` do for their stroke and shadow

Position and bounds live in the `Tree`, never on the widget. Writing a widget
from outside the crate needs `guido::widget_prelude::*` alongside the ordinary
prelude — see `tests/external_widget.rs`.

## Layout

`Container` is the one box. Layouts plug into it: `.layout(Flex::row())`,
`Flex::column()`, `ZStack`. The `Layout` trait is public, so an application can
write its own.

A `ZStack` places each child by an `Alignment` — `ZStack::new().align(..)` for
all of them, `.align_self(..)` on a child for itself — rather than wrapping the
child in a `fill()` container with a `Flex` of its own: the wrapper becomes the
child the stack sees, and an exit, a widget ref or a hit test then belongs to
it. A child's own alignment reaches the stack in `LayoutHints`, read untracked;
`Container::layout` reads it tracked, which is what lays the stack out again
when it changes. A `Flex` reads the half of it across its axis. An
`Alignment` is a point on the parent and a point on the child that meet —
the same point unless `Alignment::to` names the child's — so hanging a child
outside an edge is an alignment, resolved every layout, not a `translate`
computed from a size read back.
`out_of_flow` travels the same way, but no layout reads it: the container
takes such a child out before its layout runs (`lay_out_children`), so it
holds in every layout, a user-written one too. A property a child declares
to its parent means something in every layout, not in one.

A child playing its exit (`Widget::is_exiting`) reaches the stack as
`LayoutHints::leaving`, and leads only an axis no staying child leads — so a
shape morphing between two keyed views moves toward the arriving one at once,
and one whose only children are leaving keeps its size while they fade. It is
still laid out, aligned and painted, and a leaving `fill()` still follows. The
stack is laid out again when a child starts leaving and when its key comes
back, because both happen in the reconcile that marks it for layout. `Flex`
does not read the hint: a leaving row keeping its space is often what is wanted
there.

A child is never larger than its parent offers — an exact length included,
which `exact_within` cuts to the parent's maximum — unless its length is
`natural()`. `Container::own_constraints` lifts the parent's minimum and
maximum on that axis before anything else in the layout reads them, so the
content is measured unbounded and the box is its content's size; `least_size`
then holds it there in a row too short for it. It overhangs the parent, cut
off only where the parent has `Overflow::Hidden`. A custom `Layout` written to
lay children out unbounded so they keep their size while a clipping parent
changes around them is what `natural()` is for.

Children: `.child()` for a static one — a widget, or an `Option` of one that
is simply absent when `None`. `.children()` takes
whatever `IntoChildren` fits — an iterator of widgets is static, a closure is
dynamic and re-runs when what it read changes, `keyed(data, key, build)`
reconciles by key so widget state survives a reorder, and a `ChildrenSource` is
the set a component was handed by its caller, taking its place where the call is
written:

```rust
container().children(keyed(
    move || tabs.get(),
    |tab| tab.title.clone(),
    tab_button,
))
```

## Components

An example, a book sample or a crate doc that builds the same widget in more
than one place, with named, optional or callback props, writes it as a
`#[component]` — the rule the book's Components chapter and the crate docs give
an application's author, and the one an agent copying our examples learns. A
function returning `Container` is for a piece built once. The macro itself is
`guido-macros/src/lib.rs`; `tests/required_props.rs` and the samples beside
`pub use guido_macros::component` in `src/lib.rs` are what pin its spellings.

## State layers

Hover, press and focus are overrides, not separate widgets, and they come from
the `Stateful` trait (`src/widgets/state_layer.rs`) — so anything implementing
it takes them, text as well as containers:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .when_hovered(|s| s.lighter(0.1))
    .when_pressed(|s| s.ripple())
    .when_focused(|s| s.outline(2.0, Color::WHITE, 2.0))
    .on_click(move || count.update(|c| *c += 1))
```

The closure receives a default style and returns the overrides; only what it
sets is overridden, and the rest is inherited. A layer's `opacity` overrides the container's own
(children included); `background_alpha` touches only the background. See
[docs/STATE_LAYER.md](../../../docs/STATE_LAYER.md).

## Corners

Radius and curvature are one property. A bare size means circular corners and
takes what `padding` takes; a constructor names another shape.

```rust
container().corners(12.0)
container().corners([16.0, 0.0])                  // rounded on top, square below
container().corners(Corners::squircle(12.0))      // K=2, iOS-style
container().corners(Corners::bevel(12.0))         // K=0, diagonal
container().corners(Corners::scoop(12.0))         // K=-1, concave
container().corners(Corners::superellipse(12.0, 1.5))
```

## Adding a widget property, end to end

1. Decide reactive or structural (the rule above).
2. Add the setter; if it takes a new conversion, add one `converts!` entry and
   a line in `tests/signal_conversions.rs`.
3. Make it reach paint, and add a scenario to `tests/golden_images.rs` if it
   changes pixels — see the `visual-verification` skill.
4. Update `docs/` where the pattern is described, and the book chapter that
   teaches it — see the `book` skill.

## The motion rides with the value

`.transition(..)` and `.timeline(..)` hang off `IntoSignal` itself, so they are
available on everything a property setter already accepts, and each returns an
`Animated<T>`:

```rust
container()
    .background(theme.surface.transition(200.0))
    .width((move || if open.get() { 520.0 } else { 120.0 }).transition(SpringConfig::SNAPPY))
    .rotate(0.0.timeline(shake().played_by(rejections)))
```

A bare number is milliseconds, eased out — the one place the animation
vocabulary has two defaults, because `Transition::default()` is a spring and
has no duration for a number to attach to.

`.entering_from(..)` is the third verb, and a modifier on the first rather than
a peer: a transition says how a value moves, an enter says where it starts the
one time the widget appears, and CSS keeps the same two apart as `transition`
and `@starting-style`. It plays once, at the first layout, and is consumed
there.

```rust
container().background(theme.surface.transition(200.0).entering_from(Color::TRANSPARENT))
```

`.exiting_to(..)` is its mirror: where the property goes when the widget is
removed from a dynamic children list. It is asked at removal, not at build —
the caller knows the direction only then — and a removed child that declares
one is *detached* rather than torn down: it keeps its slot, takes no input,
subscribes to nothing, its effects are paused, and it is disposed when its last
exit settles. Detach and dispose are the two halves of `teardown_widget_subtree`
in `src/jobs.rs`, and a `keyed(..)` key that comes back mid-exit reclaims its
row — resuming its effects, each one that missed a write running once.

The take lives on `AnimationState`, not in the initialiser that seeds most
properties — there are four of those, and an enter asked for in one of them
compiles on every setter and works on half. That is #303's own defect, and the
rule is that a property is *placed* in exactly one place, whichever it is.

Three rules the shape depends on, in order of how easily they are broken:

- **A declaration is the whole property.** Restating one replaces the value
  *and* the motion, so `.background(x.transition(200.0)).background(y)` leaves
  no animation behind. There are never two declarations for one property to
  reconcile.
- **`Animated<T>` is not an `IntoSignal`.** That is what makes a timing on a
  state-layer override a compile error. Adding an `IntoSignal` impl for it
  would put the rule back into prose.
- **`timeline` needs `T: Animatable`.** That is what keeps a timeline off a
  property whose declared type is not the type it animates — `width` and
  `height` declare a `Length` and move an `f32` — so `width(w.timeline(..))`
  does not compile rather than compiling and playing nothing.

A new animatable property is declared the way every property is — a field, a
setter, and an `effective_*_target` that resolves it through the state layers
— plus one row in `animated_properties!`
(`src/widgets/container/animated_properties.rs`). The row
states what it animates, the `&self` method that reads where it is heading —
or `none` for `width` and `height`, whose target the layout supplies —
whether moving it re-measures the box, and its own test recipe. Everything
else is emitted from that: the store, the seed, the drift check, the advance,
the timeline start, and the test that asks the property whether it enters,
eases, adopts a write, leaves and plays.

It used to be five lists, and was really nine. Missing one made the property
silently never play, never wake, or keep whatever value the builder happened
to see; two of those were found by tests written after the fact, and the rest
by nobody.
