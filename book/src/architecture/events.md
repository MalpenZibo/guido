# Event System

This page explains how input events flow through Guido.

## Event Flow

```
Wayland → Platform → App → Widget Tree
                              │
                              ├─ MouseMove
                              ├─ MouseEnter/MouseLeave
                              ├─ MouseDown/MouseUp
                              └─ Scroll
```

## Event Types

### Mouse Movement

```rust
Event::MouseMove { x, y }
Event::MouseEnter
Event::MouseLeave
```

Tracked for hover states. The platform layer determines which widget the cursor is over.

### Mouse Buttons

```rust
Event::MouseDown { x, y, button }
Event::MouseUp { x, y, button }
```

Used for click detection and pressed states.

### Scrolling

```rust
Event::Scroll { dx, dy, source }
```

- `dx` - Horizontal scroll amount
- `dy` - Vertical scroll amount
- `source` - Wheel or touchpad

## Event Propagation

Events propagate from children to parents (bubble up):

1. Event received at root
2. Hit test finds deepest widget under cursor
3. Event sent to that widget first
4. If not handled, bubbles to parent
5. Continues until handled or reaches root

```rust
fn event(&mut self, event: &Event) -> EventResponse {
    // Check children first (innermost)
    for child in self.children.iter_mut().rev() {
        if child.bounds().contains(event.position()) {
            if child.event(event) == EventResponse::Handled {
                return EventResponse::Handled;
            }
        }
    }

    // Then handle locally
    if self.handles_event(event) {
        return EventResponse::Handled;
    }

    EventResponse::Ignored
}
```

## Hit Testing

### Basic Hit Test

```rust
fn contains(&self, x: f32, y: f32) -> bool {
    x >= self.x && x <= self.x + self.width &&
    y >= self.y && y <= self.y + self.height
}
```

### With Corner Radius

Clicks outside rounded corners don't register:

```rust
// SDF-based hit test
let dist = sdf_rounded_rect(point, bounds, radius, k);
dist <= 0.0  // Inside if distance is negative
```

### With Transforms

Inverse transform applied to test point:

```rust
fn contains_transformed(&self, x: f32, y: f32) -> bool {
    let (local_x, local_y) = self.transform.inverse().transform_point(x, y);
    self.bounds.contains(local_x, local_y)
}
```

## Event Handlers

Containers register callbacks:

```rust
container()
    .on_click(|| println!("Clicked!"))
    .on_hover(|hovered| println!("Hover: {}", hovered))
    .on_scroll(|dx, dy, source| println!("Scroll"))
```

Internally stored as optional closures:

```rust
pub struct Container {
    on_click: Option<Box<dyn Fn()>>,
    on_hover: Option<Box<dyn Fn(bool)>>,
    on_scroll: Option<Box<dyn Fn(f32, f32, ScrollSource)>>,
}
```

## State Layer Integration

The state layer system uses events internally:

1. **MouseEnter** → Set hover state true
2. **MouseLeave** → Set hover state false
3. **MouseDown** → Set pressed state true, record click point
4. **MouseUp** → Set pressed state false, trigger ripple contraction

```rust
fn event(&mut self, event: &Event) -> EventResponse {
    match event {
        Event::MouseEnter => {
            self.hover_state = true;
        }
        Event::MouseDown { x, y, .. } => {
            self.pressed_state = true;
            self.press_point = Some((*x, *y));
        }
        // ...
    }
}
```

## EventResponse

Widgets return whether they handled the event:

```rust
pub enum EventResponse {
    Handled,   // Stop propagation
    Ignored,   // Continue to parent
}
```

## Platform Integration

### Wayland Events

The platform layer receives Wayland protocol events:

```rust
// From wl_pointer
fn pointer_motion(x: f32, y: f32) {
    self.cursor_x = x;
    self.cursor_y = y;
    self.dispatch(Event::MouseMove { x, y });
}

fn pointer_button(button: u32, state: ButtonState) {
    match state {
        ButtonState::Pressed => self.dispatch(Event::MouseDown { ... }),
        ButtonState::Released => self.dispatch(Event::MouseUp { ... }),
    }
}
```

### Event Loop

Uses calloop for event loop integration:

```rust
// Main loop
loop {
    // 1. Process Wayland events
    event_queue.dispatch_pending()?;

    // 2. Layout and paint
    widget.layout(constraints);
    widget.paint(&mut ctx);

    // 3. Render to screen
    renderer.render(&ctx);
}
```

## Keyboard Events

Currently not implemented. Future work includes:

- Key press/release events
- Focus management
- Text input for text fields
