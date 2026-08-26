# Event System

This page explains how input events flow through Guido.

## Event Flow

```
Wayland → Platform → App → Widget Tree
                              │
                              ├─ MouseMove
                              ├─ MouseEnter/MouseLeave
                              ├─ MouseDown/MouseUp
                              └─ Scroll/ScrollEnd
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
Event::Scroll { x, y, delta_x, delta_y, source }
Event::ScrollEnd { x, y }
```

- `x`, `y` - Pointer position
- `delta_x` - Horizontal scroll amount, in pixels
- `delta_y` - Vertical scroll amount, in pixels
- `source` - `Wheel`, `Finger` or `Continuous`

`ScrollEnd` is the end of a continuous gesture — the finger lifting off a
touchpad, which the compositor reports as `wl_pointer.axis_stop`. It carries no
delta because nothing moved. A wheel never sends one: its steps are discrete,
there is no finger to lift, and nothing carries on afterwards.

It is what decides when momentum scrolling may begin. Without it the end of a
gesture has to be guessed from a gap between samples, and a slow scroll is made
of gaps.

## Event Propagation

Events propagate from children to parents (bubble up):

1. Event received at root
2. Hit test finds deepest widget under cursor
3. Event sent to that widget first
4. If not handled, bubbles to parent
5. Continues until handled or reaches root

```rust
fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
    // Check children first (innermost)
    for &child_id in self.children.iter().rev() {
        // Get child bounds from Tree
        let child_bounds = tree.get_bounds(child_id).unwrap_or_default();
        if child_bounds.contains(event.position()) {
            let response = tree.with_widget_mut(child_id, |child, cid, tree| {
                child.event(tree, cid, event)
            });
            if response == Some(EventResponse::Handled) {
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
fn event(&mut self, tree: &mut Tree, id: WidgetId, event: &Event) -> EventResponse {
    match event {
        Event::MouseEnter => {
            self.flags.update(|f| f.insert(InteractionFlags::HOVERED));
        }
        Event::MouseDown { x, y, .. } => {
            self.flags.update(|f| f.insert(InteractionFlags::PRESSED));
            self.ripple.start(*x, *y, Instant::now());
        }
        // ...
    }
    EventResponse::Ignored
}
```

The flags live in a signal rather than a plain field, so that a *descendant*
resolving a state layer subscribes to them — see
[Interactivity](../interactivity/README.md).

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
