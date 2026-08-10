# Dynamic Children

Learn the different ways to add children to containers, from static to fully reactive with automatic resource cleanup.

![Children Example](../images/children_example.png)

## The Model

Children come in exactly four forms — two static, two reactive:

```rust
container()
    .child(text("Hello"))                                  // static single
    .children([text("A"), text("B")])                      // static list
    .child(dynamic(move || data.get(), build_widget))      // reactive single
    .children(keyed(move || items.get(), |i| i.id, row))   // reactive list
```

`dynamic()` and `keyed()` are values, not methods: they pair a **tracked data
closure** with an **untracked builder**. The data closure is the only reactive
part — the signals it reads decide when to re-evaluate, and its result is what
the builder receives. Data the widget must react to has to flow through the
data closure; that is what makes stale content unexpressible.

Bare closures are **rejected at compile time**. With `.child(move || ...)`
there would be no way to know which data the widget was built from, so content
updates could be silently discarded — the compiler error points you to
`dynamic()` / `keyed()` instead.

## Reactive Single Child: `dynamic(data, build)`

```rust
// Content that re-renders when the data changes
container().child(dynamic(
    move || menu_entries.get(),   // tracked; T: Clone + PartialEq
    |entries| build_menu(entries) // untracked; runs only when T changed
))
```

The data result is compared with the previous value (`PartialEq`):

- **unchanged** → the existing widget is kept untouched: inner state,
  animations and subscriptions persist, the builder does not run
- **changed** → the builder runs and the rebuilt widget replaces the old one
  (its owner scope is disposed)

The builder may return `Option<widget>` to also control presence:

```rust
// Shown only while there is an active window
container().child(dynamic(
    move || state.active_window.get(),
    |window| window.map(title_widget),
))
```

The builder runs with signal tracking suspended: reads inside it see current
values but create no dependencies. Choose what goes through the data closure
and what stays internally reactive:

```rust
// Rebuild the row when the label changes...
dynamic(move || item.get(), |item| text(item.label))

// ...or keep the widget and let it track the label itself
dynamic(move || item.with(|i| i.id), |_| text(move || item.with(|i| i.label.clone())))
```

## Reactive Lists: `keyed(data, key, build)`

```rust
let items = create_signal(vec![
    Item { id: 1, name: "First".into() },
    Item { id: 2, name: "Second".into() },
]);

container().children(keyed(
    move || items.get(),   // tracked; items: Clone + PartialEq
    |item| item.id,        // stable identity (survives reorders)
    |item| item_widget(item),
))
```

Per item, identity (`key`) and content (`PartialEq`) are diffed separately:

- **new key** → the builder runs
- **known key, item unchanged** → the existing widget is kept, including
  across reorders (state and animations persist)
- **known key, item changed** → only that row is rebuilt
- **key gone** → the widget is dropped with owner cleanup

Keys must be unique and stable — never use the index:

```rust
keyed(data, |item| item.id, build)        // Good: stable identity
keyed(data, |(index, _)| *index as u64, build)  // Bad: reorder loses state
```

The item type chooses the update granularity, exactly like `dynamic()`:
fields included in the item trigger a row rebuild when they change; fields
left out can be read via signals inside the row for in-place updates.

## Automatic Ownership & Cleanup

Builders run inside an **owner scope**: signals and effects created there are
automatically owned and cleaned up when the child is removed or rebuilt.

```rust
fn item_widget(item: Item) -> impl Widget {
    // This signal is OWNED by this row
    let clicks = create_signal(0);

    // Register cleanup for non-reactive resources
    on_cleanup(move || {
        log::info!("Item {} removed", item.id);
    });

    container()
        .padding(8.0)
        .hover_state(|s| s.lighter(0.1))
        .on_click(move || clicks.update(|c| *c += 1))
        .child(text(move || format!("{} (clicks: {})", item.name, clicks.get())))
}

container().children(keyed(move || items.get(), |i| i.id, item_widget))
```

When a child is removed or rebuilt:
1. The widget is dropped
2. `on_cleanup` callbacks run
3. Effects are disposed
4. Signals are disposed

## Complete Example

```rust
#[derive(Clone, PartialEq)]
struct Item {
    id: u64,
    name: String,
}

fn dynamic_list_demo() -> impl Widget {
    let items = create_signal(vec![
        Item { id: 1, name: "First".into() },
        Item { id: 2, name: "Second".into() },
        Item { id: 3, name: "Third".into() },
    ]);
    let next_id = create_signal(4u64);

    container()
        .padding(16.0)
        .layout(Flex::column().spacing(12.0))
        .child(
            container()
                .layout(Flex::row().spacing(8.0))
                .children([
                    button("Add", move || {
                        let id = next_id.get();
                        next_id.set(id + 1);
                        items.update(|list| {
                            list.push(Item { id, name: format!("Item {}", id) });
                        });
                    }),
                    button("Remove Last", move || {
                        items.update(|list| { list.pop(); });
                    }),
                    button("Reverse", move || {
                        items.update(|list| { list.reverse(); });
                    }),
                ])
        )
        .child(
            container()
                .layout(Flex::column().spacing(4.0))
                .children(keyed(move || items.get(), |i| i.id, item_widget))
        )
}

fn button(label: &str, on_click: impl Fn() + 'static) -> Container {
    container()
        .padding(8.0)
        .background(Color::rgb(0.3, 0.3, 0.4))
        .corner_radius(4.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .on_click(on_click)
        .child(text(label).color(Color::WHITE))
}
```

## Mixing Static and Dynamic

Combine static and dynamic children freely, in any order:

```rust
container()
    .layout(Flex::column().spacing(8.0))
    // Static header
    .child(text("Items:").font_size(18.0).color(Color::WHITE))
    // Reactive list
    .children(keyed(move || items.get(), |i| i.id, item_view))
    // Static footer
    .child(text("End of list").color(Color::rgb(0.6, 0.6, 0.7)))
```

## API Reference

```rust
impl Container {
    // A widget value, or dynamic(data, build)
    pub fn child<M>(self, child: impl IntoChild<M>) -> Self;

    // An iterator of widgets, or keyed(data, key, build)
    pub fn children<M>(self, children: impl IntoChildren<M>) -> Self;
}

// Reactive single child: tracked data + untracked builder.
// The builder may return a widget, or Option<widget> for presence.
pub fn dynamic<T, C>(
    data: impl Fn() -> T + 'static,
    build: impl Fn(T) -> C + 'static,
) -> DynChild<T, C>
where
    T: Clone + PartialEq + 'static,
    C: IntoDynChild;

// Reactive children list: tracked data, key-based identity,
// untracked per-item builder with content diffing.
pub fn keyed<T, I, W>(
    data: impl Fn() -> I + 'static,
    key: impl Fn(&T) -> u64 + 'static,
    build: impl Fn(T) -> W + 'static,
) -> KeyedChildren<T, I, W>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = T>,
    W: Widget + 'static;

// Cleanup registration (use inside builders)
pub fn on_cleanup(f: impl FnOnce() + 'static);
```
