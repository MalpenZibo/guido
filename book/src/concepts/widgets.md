# Widgets

Widgets are the building blocks of Guido UIs. Every visual element is a widget, from simple text to complex layouts.

## The Widget Trait

All widgets implement the `Widget` trait:

```rust
pub trait Widget: Send + Sync {
    fn layout(&mut self, tree: &mut Tree, id: WidgetId, constraints: Constraints) -> Size;
    fn paint(&self, tree: &Tree, id: WidgetId, ctx: &mut PaintContext);
    fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse;
    fn set_origin(&mut self, tree: &mut Tree, id: WidgetId, x: f32, y: f32);
    fn bounds(&self) -> Rect;
}
```

### Methods

- **layout** - Calculate the widget's size given tree access, widget ID, and constraints
- **paint** - Draw the widget using tree for child access
- **event** - Handle input events with tree access
- **set_origin** - Position the widget and store origin in tree (called by parent during layout)
- **bounds** - Get the bounding rectangle for hit testing

## Built-in Widgets

### Container

The primary widget for building UIs. Handles:
- Backgrounds (solid, gradient)
- Borders and corner radius
- Padding and sizing
- Layout of children
- Event handling
- State layers (hover/pressed)
- Transforms

See [Container](container.md) for details.

### Text

Renders text content:

```rust
text("Hello, World!")
    .font_size(16.0)
    .color(Color::WHITE)
    .bold()
```

See [Text](../building-ui/text.md) for styling options.

### TextInput

Single-line text editing with selection, clipboard, and undo/redo:

```rust
let username = create_signal(String::new());

text_input(username)
    .text_color(Color::WHITE)
    .on_submit(|text| println!("Submitted: {}", text))
```

See [Text Input](../building-ui/text-input.md) for details.

## Composition

Guido UIs are built through composition - nesting widgets inside containers:

```rust
container()
    .layout(Flex::column().spacing(8.0))
    .children([
        text("Title").font_size(24.0),
        container()
            .layout(Flex::row().spacing(4.0))
            .children([
                text("Item 1"),
                text("Item 2"),
            ]),
    ])
```

This creates:
```
┌─────────────────┐
│ Title           │
│ ┌─────┬───────┐ │
│ │Item1│ Item2 │ │
│ └─────┴───────┘ │
└─────────────────┘
```

## Widget Functions

Guido provides functions that return configured widgets:

```rust
// Creates a Container
container()

// Creates a Text widget
text("content")
```

These use the builder pattern for configuration:

```rust
container()
    .padding(16.0)          // Returns Container
    .background(Color::RED) // Returns Container
    .corner_radius(8.0)     // Returns Container
```

## The impl Widget Pattern

Functions often return `impl Widget` instead of concrete types:

```rust
fn my_button(label: &str) -> impl Widget {
    container()
        .padding(12.0)
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .child(text(label).color(Color::WHITE))
}
```

This allows returning any widget type without exposing implementation details.

## Constraints and Sizing

During layout, parent widgets pass constraints to children:

```rust
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}
```

Children choose a size within these constraints. This enables flexible layouts where widgets can expand to fill space or shrink to fit content.

### Size Modifiers

Control widget sizing with builder methods:

```rust
container()
    .width(100.0)        // Fixed width
    .height(50.0)        // Fixed height
    .min_width(50.0)     // Minimum width
    .max_width(200.0)    // Maximum width
```

## Event Flow

Events flow from the platform through the widget tree:

1. Platform receives input (mouse, keyboard)
2. Event dispatched to root widget
3. Root checks if event hits its bounds
4. If yes, passes to children (innermost first)
5. Widget handles event or ignores it

```rust
container()
    .on_click(|| println!("Clicked!"))
    .child(text("Click me"))
```

The container receives clicks anywhere within its bounds, including over the text.

## Next Steps

- [Container](container.md) - Deep dive into the Container widget
- [Layout](layout.md) - Learn about flex layout
- [Interactivity](../interactivity/README.md) - Add event handling
