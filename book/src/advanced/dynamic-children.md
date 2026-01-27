# Dynamic Children

Learn the different ways to add children to containers, from static to fully reactive with automatic resource cleanup.

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

## Dynamic Children with Keyed Reconciliation

For lists that change based on signals, use the keyed children API:

```rust
let items = create_signal(vec![1u64, 2, 3]);

container().children(move || {
    items.get().into_iter().map(|id| {
        // Return (key, closure) - closure creates the widget
        (id, move || {
            container()
                .padding(8.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .child(text(format!("Item {}", id)))
        })
    })
})
```

### The Closure Pattern

The key insight is returning `(key, || widget)` instead of `(key, widget)`:

```rust
// The closure ensures:
// 1. Widget is only created for NEW keys (not every frame)
// 2. Signals/effects inside are automatically owned
// 3. Cleanup runs when the child is removed

(item.id, move || create_item_widget(item))
```

### How Keys Work

The key identifies each item for efficient updates:

```rust
// Good: Unique, stable identifier
(item.id, move || widget)

// Bad: Index changes when items reorder
(index as u64, move || widget)
```

With proper keys:
- **Reordering** preserves widget state
- **Insertions** only create new widgets
- **Deletions** only remove specific widgets

## Automatic Ownership & Cleanup

Signals and effects created inside the child closure are **automatically owned** and cleaned up when the child is removed:

```rust
container().children(move || {
    items.get().into_iter().map(|id| (id, move || {
        // This signal is OWNED by this child
        let local_count = create_signal(0);

        // This effect is also owned
        create_effect(move || {
            println!("Count: {}", local_count.get());
        });

        // Register cleanup for non-reactive resources
        on_cleanup(move || {
            println!("Child {} removed!", id);
        });

        container()
            .on_click(move || local_count.update(|c| *c += 1))
            .child(text(move || local_count.get().to_string()))
    }))
})
```

When a child is removed:
1. The widget is dropped
2. `on_cleanup` callbacks run
3. Effects are disposed
4. Signals are disposed

### Extracting Widget Creation

You can extract the widget creation into a function:

```rust
fn create_item_widget(id: u64, name: String) -> impl Widget {
    // Everything here is automatically owned!
    let hover = create_signal(false);

    on_cleanup(move || {
        log::info!("Item {} cleaned up", id);
    });

    container()
        .padding(8.0)
        .background(move || {
            if hover.get() { Color::rgb(0.3, 0.3, 0.4) }
            else { Color::rgb(0.2, 0.2, 0.3) }
        })
        .on_hover(move |h| hover.set(h))
        .child(text(name).color(Color::WHITE))
}

// Use it with the closure wrapper
container().children(move || {
    items.get().into_iter().map(|item| {
        (item.id, move || create_item_widget(item.id, item.name.clone()))
    })
})
```

## Complete Example

```rust
#[derive(Clone)]
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
            // Control buttons
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
            // Dynamic list with automatic cleanup
            container()
                .layout(Flex::column().spacing(4.0))
                .children(move || {
                    items.get().into_iter().map(|item| {
                        let id = item.id;
                        let name = item.name.clone();
                        (id, move || {
                            // Local state for this item
                            let clicks = create_signal(0);

                            on_cleanup(move || {
                                log::info!("Item {} removed", id);
                            });

                            container()
                                .padding(8.0)
                                .background(Color::rgb(0.2, 0.2, 0.3))
                                .corner_radius(4.0)
                                .hover_state(|s| s.lighter(0.1))
                                .pressed_state(|s| s.ripple())
                                .on_click(move || clicks.update(|c| *c += 1))
                                .child(
                                    text(move || format!("{} (clicks: {})", name, clicks.get()))
                                        .color(Color::WHITE)
                                )
                        })
                    })
                })
        )
}

fn button(label: &str, on_click: impl Fn() + Send + Sync + 'static) -> Container {
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

Combine static and dynamic children freely:

```rust
container()
    .layout(Flex::column().spacing(8.0))
    // Static header
    .child(text("Items:").font_size(18.0).color(Color::WHITE))
    // Dynamic list
    .children(move || {
        items.get().into_iter().map(|item| {
            (item.id, move || item_view(item.clone()))
        })
    })
    // Static footer
    .child(text("End of list").color(Color::rgb(0.6, 0.6, 0.7)))
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

    // Dynamic keyed children with automatic ownership
    pub fn children<F, I, G, W>(self, children: F) -> Self
    where
        F: Fn() -> I + Send + Sync + 'static,
        I: IntoIterator<Item = (u64, G)>,
        G: FnOnce() -> W + 'static,
        W: Widget + 'static;
}

// Cleanup registration (use inside dynamic child closures)
pub fn on_cleanup(f: impl FnOnce() + 'static);
```
