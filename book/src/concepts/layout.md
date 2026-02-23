# Layout

Guido uses a flexbox-style layout system for arranging widgets. The `Flex` layout handles rows and columns with spacing and alignment options.

![Flex Layout](../images/flex_layout.png)

## Basic Layout

### Row (Horizontal)

```rust
container()
    .layout(Flex::row())
    .children([
        text("Left"),
        text("Center"),
        text("Right"),
    ])
```

### Column (Vertical)

```rust
container()
    .layout(Flex::column())
    .children([
        text("Top"),
        text("Middle"),
        text("Bottom"),
    ])
```

## Spacing

Add space between children:

```rust
container()
    .layout(Flex::row().spacing(8.0))
    .children([...])
```

## Main Axis Alignment

Control distribution along the layout direction:

```rust
Flex::row().main_alignment(MainAlignment::Center)
```

### Options

| Alignment | Description |
|-----------|-------------|
| `Start` | Pack at the beginning |
| `Center` | Center in available space |
| `End` | Pack at the end |
| `SpaceBetween` | Equal space between, none at edges |
| `SpaceAround` | Equal space around each item |
| `SpaceEvenly` | Equal space including edges |

### Visual Examples

```
Start:        [A][B][C]
Center:          [A][B][C]
End:                      [A][B][C]
SpaceBetween: [A]      [B]      [C]
SpaceAround:   [A]    [B]    [C]
SpaceEvenly:    [A]   [B]   [C]
```

## Cross Axis Alignment

Control alignment perpendicular to the layout direction:

```rust
Flex::row().cross_alignment(CrossAlignment::Center)
```

### Options

| Alignment | Description |
|-----------|-------------|
| `Start` | Align to start of cross axis |
| `Center` | Center on cross axis |
| `End` | Align to end of cross axis |
| `Stretch` | Stretch to fill cross axis |

### Visual Example (Row)

```
Start:    ┌───┐┌─┐┌──┐
          │ A ││B││ C│
          └───┘│ │└──┘
               └─┘

Center:        ┌─┐
          ┌───┐│B│┌──┐
          │ A │└─┘│ C│
          └───┘   └──┘

End:           ┌─┐
               │B│
          ┌───┐└─┘┌──┐
          │ A │   │ C│
          └───┘   └──┘

Stretch:  ┌───┐┌─┐┌──┐
          │   ││ ││  │
          │ A ││B││ C│
          │   ││ ││  │
          └───┘└─┘└──┘
```

## Complete Example

```rust
container()
    .layout(
        Flex::row()
            .spacing(16.0)
            .main_alignment(MainAlignment::SpaceBetween)
            .cross_alignment(CrossAlignment::Center)
    )
    .padding(20.0)
    .children([
        text("Left").font_size(24.0),
        container()
            .layout(Flex::column().spacing(4.0))
            .children([
                text("Center"),
                text("Items"),
            ]),
        text("Right").font_size(24.0),
    ])
```

## Nested Layouts

Combine rows and columns for complex layouts:

```rust
container()
    .layout(Flex::column().spacing(16.0))
    .children([
        // Header row
        container()
            .layout(Flex::row().main_alignment(MainAlignment::SpaceBetween))
            .children([
                text("Logo"),
                text("Menu"),
            ]),
        // Content row
        container()
            .layout(Flex::row().spacing(16.0))
            .children([
                sidebar(),
                main_content(),
            ]),
        // Footer row
        container()
            .layout(Flex::row().main_alignment(MainAlignment::Center))
            .child(text("Footer")),
    ])
```

## Size Constraints

Control how children size within layouts:

### Fixed Size

```rust
container()
    .width(200.0)
    .height(100.0)
```

### Minimum/Maximum

```rust
container()
    .min_width(100.0)
    .max_width(300.0)
```

### At Least

Request at least a certain size:

```rust
container()
    .width(at_least(200.0))  // At least 200px, can grow
```

### Fill Available Space

Make a container expand to fill all available space:

```rust
container()
    .height(fill())  // Fills available height
    .width(fill())   // Fills available width
```

This is particularly useful for root containers that should fill their surface, or for creating layouts where children are centered within the full available space:

```rust
container()
    .height(fill())
    .layout(
        Flex::row()
            .main_alignment(MainAlignment::Center)
            .cross_alignment(CrossAlignment::Center)
    )
    .child(text("Centered in available space"))
```

## Layout Without Explicit Flex

Containers without `.layout()` stack children (each child fills the container):

```rust
// Children overlap, each filling the container
container()
    .children([
        background_image(),
        overlay_content(),
    ])
```

## API Reference

### Flex Builder

```rust
Flex::row() -> Flex                    // Horizontal layout
Flex::column() -> Flex                 // Vertical layout
.spacing(f32) -> Flex                  // Space between children
.main_alignment(MainAlignment) -> Flex
.cross_alignment(CrossAlignment) -> Flex
```

### MainAlignment

```rust
MainAlignment::Start
MainAlignment::Center
MainAlignment::End
MainAlignment::SpaceBetween
MainAlignment::SpaceAround
MainAlignment::SpaceEvenly
```

### CrossAlignment

```rust
CrossAlignment::Start
CrossAlignment::Center
CrossAlignment::End
CrossAlignment::Stretch
```
