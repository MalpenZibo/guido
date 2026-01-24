# Core Concepts

This section covers the foundational concepts that make Guido work. Understanding these will help you build effective applications.

## The Big Picture

Guido is built on three core ideas:

1. **Reactive Signals** - UI state is stored in signals that automatically track dependencies and notify when changed
2. **Composable Widgets** - UIs are built by composing simple primitives (containers, text) into complex structures
3. **Declarative Styling** - Visual properties are declared through builder methods, not CSS or external files

## How They Work Together

```rust
// 1. Create reactive state
let count = create_signal(0);

// 2. Build composable widgets
let view = container()
    .padding(16.0)
    .background(Color::rgb(0.2, 0.2, 0.3))

    // 3. Declarative styling responds to state
    .child(text(move || format!("Count: {}", count.get())));
```

When `count` changes, only the text updates - the container doesn't need to re-render.

## In This Section

- [Reactive Model](reactive-model.md) - Signals, computed values, and effects
- [Widgets](widgets.md) - The Widget trait and composition patterns
- [Container](container.md) - The primary building block
- [Layout](layout.md) - Flexbox-style layout with Flex

## Key Insight

Unlike traditional retained-mode GUIs that rebuild widget trees on state changes, Guido's reactive system means widgets are created once and their properties update automatically. This leads to efficient rendering and simple mental models.
