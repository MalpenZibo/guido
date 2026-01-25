# Container

The Container is Guido's primary building block. Nearly everything you build uses containers - they handle layout, styling, events, and child management.

## Creating Containers

```rust
use guido::prelude::*;

let view = container();
```

## Adding Children

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

### Conditional Children

```rust
let show_extra = create_signal(false);

container().children([
    text("Always shown"),
    container().maybe_child(show_extra, || text("Sometimes shown")),
])
```

## Styling

Containers support extensive styling options:

```rust
container()
    // Background
    .background(Color::rgb(0.2, 0.2, 0.3))

    // Corners
    .corner_radius(8.0)
    .squircle() // iOS-style smooth corners

    // Border
    .border(2.0, Color::WHITE)

    // Spacing
    .padding(16.0)

    // Size
    .width(200.0)
    .height(100.0)
```

See [Building UI](../building-ui/README.md) for complete styling reference.

## Layout

Control how children are arranged:

```rust
container()
    .layout(
        Flex::row()
            .spacing(8.0)
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
    )
    .children([...])
```

See [Layout](layout.md) for details on flex layouts.

## Event Handling

Respond to user interactions:

```rust
container()
    .on_click(|| println!("Clicked!"))
    .on_hover(|hovered| println!("Hover: {}", hovered))
    .on_scroll(|dx, dy, source| println!("Scroll: {}, {}", dx, dy))
```

## State Layers

Add hover and pressed visual feedback:

```rust
container()
    .background(Color::rgb(0.2, 0.2, 0.3))
    .hover_state(|s| s.lighter(0.1))
    .pressed_state(|s| s.ripple())
```

See [Interactivity](../interactivity/README.md) for the full state layer API.

## Transforms

Apply 2D transformations:

```rust
container()
    .translate(10.0, 20.0)  // Move
    .rotate(45.0)           // Rotate degrees
    .scale(1.5)             // Scale
    .transform_origin(TransformOrigin::TOP_LEFT)
```

See [Transforms](../transforms/README.md) for details.

## Animations

Animate property changes:

```rust
container()
    .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
    .animate_transform(Transition::spring(SpringConfig::BOUNCY))
```

See [Animations](../animations/README.md) for timing and spring options.

## Scrolling

Make containers scrollable when content overflows:

```rust
container()
    .width(200.0)
    .height(200.0)
    .scrollable(ScrollAxis::Vertical)
    .child(large_content())
```

### Scroll Axes

| Axis | Description |
|------|-------------|
| `ScrollAxis::None` | No scrolling (default) |
| `ScrollAxis::Vertical` | Vertical scrolling only |
| `ScrollAxis::Horizontal` | Horizontal scrolling only |
| `ScrollAxis::Both` | Both directions |

### Custom Scrollbars

```rust
container()
    .scrollable(ScrollAxis::Vertical)
    .scrollbar(|sb| {
        sb.width(6.0)
          .handle_color(Color::rgb(0.4, 0.6, 0.9))
          .handle_hover_color(Color::rgb(0.5, 0.7, 1.0))
          .handle_corner_radius(3.0)
    })
```

### Hidden Scrollbars

```rust
container()
    .scrollable(ScrollAxis::Vertical)
    .scrollbar_visibility(ScrollbarVisibility::Hidden)
```

## Complete Example

Here's a fully-styled interactive button:

```rust
fn create_button(label: &str, on_click: impl Fn() + 'static) -> Container {
    container()
        // Layout
        .padding(16.0)

        // Styling
        .background(Color::rgb(0.3, 0.5, 0.8))
        .corner_radius(8.0)
        .border(1.0, Color::rgb(0.4, 0.6, 0.9))

        // Animations
        .animate_background(Transition::new(200.0, TimingFunction::EaseOut))
        .animate_border_width(Transition::new(150.0, TimingFunction::EaseOut))

        // State layers
        .hover_state(|s| s.lighter(0.1).border(2.0, Color::rgb(0.5, 0.7, 1.0)))
        .pressed_state(|s| s.ripple().darker(0.05).transform(Transform::scale(0.98)))

        // Event
        .on_click(on_click)

        // Content
        .child(text(label).color(Color::WHITE))
}
```

## Builder Methods Reference

### Children
- `.child(widget)` - Add single child
- `.children([...])` - Add multiple children
- `.maybe_child(condition, factory)` - Conditional child
- `.children_dyn(items, key_fn, view_fn)` - Dynamic list

### Styling
- `.background(color)` - Solid background
- `.gradient_horizontal(start, end)` - Horizontal gradient
- `.gradient_vertical(start, end)` - Vertical gradient
- `.corner_radius(radius)` - Rounded corners
- `.squircle()` / `.bevel()` / `.scoop()` - Corner curvature
- `.border(width, color)` - Border
- `.elevation(level)` - Shadow

### Spacing
- `.padding(all)` - Uniform padding
- `.padding_horizontal(h)` - Left/right padding
- `.padding_vertical(v)` - Top/bottom padding

### Sizing
- `.width(w)` / `.height(h)` - Fixed size
- `.min_width(w)` / `.max_width(w)` - Width constraints
- `.min_height(h)` / `.max_height(h)` - Height constraints

### Layout
- `.layout(Flex::row())` - Horizontal layout
- `.layout(Flex::column())` - Vertical layout

### Events
- `.on_click(handler)` - Click events
- `.on_hover(handler)` - Hover enter/leave
- `.on_scroll(handler)` - Scroll events

### State Layers
- `.hover_state(|s| s...)` - Hover overrides
- `.pressed_state(|s| s...)` - Pressed overrides

### Transforms
- `.translate(x, y)` - Move
- `.rotate(degrees)` - Rotate
- `.scale(factor)` - Scale
- `.transform_origin(origin)` - Pivot point

### Animations
- `.animate_background(transition)` - Animate background
- `.animate_transform(transition)` - Animate transform
- `.animate_border_width(transition)` - Animate border width
- `.animate_border_color(transition)` - Animate border color

### Scrolling
- `.scrollable(axis)` - Enable scrolling (None, Vertical, Horizontal, Both)
- `.scrollbar(|sb| ...)` - Customize scrollbar appearance
- `.scrollbar_visibility(visibility)` - Show or hide scrollbar
