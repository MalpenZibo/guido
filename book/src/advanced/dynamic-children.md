# Dynamic Children

Learn the different ways to add children to containers, from static to fully reactive.

![Children Example](../images/children_example.png)

## Static Children

### Single Child

```rust
container().child(text("Hello"))
```

### Multiple Children

```rust
container().children([
    text("First"),
    text("Second"),
    text("Third"),
])
```

### Conditional Child (Static)

```rust
let show_extra = true;  // Evaluated once at creation

container()
    .child(text("Always shown"))
    .maybe_child(if show_extra {
        Some(text("Conditionally shown"))
    } else {
        None
    })
```

> **Note:** `maybe_child` evaluates the condition once. For reactive conditions, use dynamic children.

## Dynamic Children

For lists that change based on signals:

```rust
let items = create_signal(vec!["A", "B", "C"]);

container()
    .children_dyn(
        move || items.get(),           // Data source
        |item| item.to_string(),       // Key function (for reconciliation)
        |item| text(*item),            // View function
    )
```

### How Keys Work

The key function identifies each item for efficient updates:

```rust
// Good: Unique, stable identifier
|item| item.id.to_string()

// Bad: Index (changes when items reorder)
|index, _| index.to_string()
```

With proper keys:
- **Reordering** preserves widget state
- **Insertions** only create new widgets
- **Deletions** only remove specific widgets

## Reactive List Example

```rust
#[derive(Clone)]
struct Item {
    id: u64,
    name: String,
}

let items = create_signal(vec![
    Item { id: 1, name: "First".into() },
    Item { id: 2, name: "Second".into() },
]);

container()
    .layout(Flex::column().spacing(8.0))
    .children_dyn(
        move || items.get(),
        |item| item.id.to_string(),
        |item| {
            container()
                .padding(8.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .corner_radius(4.0)
                .child(text(item.name.clone()).color(Color::WHITE))
        }
    )
```

### Adding Items

```rust
items.update(|list| {
    list.push(Item {
        id: list.len() as u64 + 1,
        name: "New Item".into(),
    });
});
```

### Removing Items

```rust
items.update(|list| {
    list.retain(|item| item.id != id_to_remove);
});
```

### Reordering

```rust
items.update(|list| {
    list.reverse();  // Keys ensure state preservation
});
```

## Mixing Static and Dynamic

Combine static and dynamic children:

```rust
container()
    .layout(Flex::column().spacing(8.0))
    // Static header
    .child(text("Items:").font_size(18.0).color(Color::WHITE))
    // Dynamic list
    .children_dyn(
        move || items.get(),
        |item| item.id.to_string(),
        |item| item_view(item),
    )
    // Static footer
    .child(text("End of list").color(Color::rgb(0.6, 0.6, 0.7)))
```

## Complete Example

```rust
fn dynamic_list_demo() -> impl Widget {
    let items = create_signal(vec![
        Item { id: 1, name: "Item 1".into() },
        Item { id: 2, name: "Item 2".into() },
        Item { id: 3, name: "Item 3".into() },
    ]);

    container()
        .padding(16.0)
        .layout(Flex::column().spacing(12.0))
        .children([
            // Control buttons
            container()
                .layout(Flex::row().spacing(8.0))
                .children([
                    button("Add", move || {
                        items.update(|list| {
                            let id = list.len() as u64 + 1;
                            list.push(Item { id, name: format!("Item {}", id) });
                        });
                    }),
                    button("Remove", move || {
                        items.update(|list| { list.pop(); });
                    }),
                    button("Reverse", move || {
                        items.update(|list| { list.reverse(); });
                    }),
                ]),

            // Dynamic list
            container()
                .layout(Flex::column().spacing(4.0))
                .children_dyn(
                    move || items.get(),
                    |item| item.id.to_string(),
                    |item| {
                        container()
                            .padding(8.0)
                            .background(Color::rgb(0.2, 0.2, 0.3))
                            .corner_radius(4.0)
                            .hover_state(|s| s.lighter(0.1))
                            .child(text(item.name.clone()).color(Color::WHITE))
                    }
                ),
        ])
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

## API Reference

```rust
impl Container {
    // Single child
    pub fn child(self, child: impl Widget + 'static) -> Self;

    // Multiple static children
    pub fn children<W: Widget + 'static>(
        self,
        children: impl IntoIterator<Item = W>
    ) -> Self;

    // Conditional static child
    pub fn maybe_child<W: Widget + 'static>(
        self,
        child: Option<W>
    ) -> Self;

    // Reactive list
    pub fn children_dyn<T, K, V>(
        self,
        items: impl Fn() -> Vec<T> + 'static,
        key_fn: impl Fn(&T) -> K + 'static,
        view_fn: impl Fn(&T) -> V + 'static,
    ) -> Self
    where
        T: Clone + 'static,
        K: Hash + Eq + 'static,
        V: Widget + 'static;
}
```
