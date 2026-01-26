# Text Input

The TextInput widget provides single-line text editing with support for selection, clipboard operations, undo/redo, and password masking.

## Basic Usage

```rust
let username = create_signal(String::new());

text_input(username)
```

The widget automatically syncs with the signal, updating when the signal changes and notifying via callbacks when the user edits.

## Styling

### Text Color

```rust
text_input(value)
    .text_color(Color::WHITE)
```

### Cursor Color

```rust
text_input(value)
    .cursor_color(Color::rgb(0.4, 0.8, 1.0))
```

### Selection Color

```rust
text_input(value)
    .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
```

### Font Size

```rust
text_input(value)
    .font_size(16.0)
```

## Password Mode

Hide text input for sensitive data like passwords:

```rust
text_input(password)
    .password(true)
```

By default, characters are masked with `•`. Customize the mask character:

```rust
text_input(password)
    .password(true)
    .mask_char('*')
```

## Callbacks

### On Change

Called whenever the text content changes:

```rust
text_input(value)
    .on_change(|new_text| {
        println!("Text changed: {}", new_text);
    })
```

### On Submit

Called when the user presses Enter:

```rust
text_input(value)
    .on_submit(|text| {
        println!("Submitted: {}", text);
    })
```

## Keyboard Shortcuts

The TextInput widget supports standard text editing shortcuts:

| Shortcut | Action |
|----------|--------|
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy selection |
| `Ctrl+X` | Cut selection |
| `Ctrl+V` | Paste |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` or `Ctrl+Y` | Redo |
| `Left/Right` | Move cursor |
| `Ctrl+Left/Right` | Move by word |
| `Shift+Left/Right` | Extend selection |
| `Home/End` | Move to start/end |
| `Backspace` | Delete before cursor |
| `Delete` | Delete after cursor |

## Styling with Container

TextInput handles text editing but not visual styling like backgrounds and borders. Wrap it in a Container for full styling:

```rust
container()
    .padding(Padding::horizontal(12.0).vertical(8.0))
    .background(Color::rgb(0.15, 0.15, 0.2))
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corner_radius(4.0)
    .child(
        text_input(value)
            .text_color(Color::WHITE)
            .font_size(14.0)
    )
```

### With Focus State

Add visual feedback when the input is focused:

```rust
container()
    .padding(Padding::horizontal(12.0).vertical(8.0))
    .background(Color::rgb(0.15, 0.15, 0.2))
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corner_radius(4.0)
    .focused_state(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
    .child(
        text_input(value)
            .text_color(Color::WHITE)
    )
```

## Complete Example

A login form with username and password fields:

```rust
fn login_form() -> Container {
    let username = create_signal(String::new());
    let password = create_signal(String::new());

    container()
        .padding(24.0)
        .background(Color::rgb(0.1, 0.1, 0.15))
        .corner_radius(12.0)
        .layout(Flex::column().spacing(16.0))
        .children([
            // Username field
            container()
                .layout(Flex::column().spacing(4.0))
                .children([
                    text("Username")
                        .font_size(12.0)
                        .color(Color::rgb(0.6, 0.6, 0.7)),
                    container()
                        .padding(Padding::horizontal(12.0).vertical(8.0))
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(4.0)
                        .focused_state(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            text_input(username)
                                .text_color(Color::WHITE)
                                .font_size(14.0)
                        ),
                ]),
            // Password field
            container()
                .layout(Flex::column().spacing(4.0))
                .children([
                    text("Password")
                        .font_size(12.0)
                        .color(Color::rgb(0.6, 0.6, 0.7)),
                    container()
                        .padding(Padding::horizontal(12.0).vertical(8.0))
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(4.0)
                        .focused_state(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            text_input(password)
                                .password(true)
                                .text_color(Color::WHITE)
                                .font_size(14.0)
                        ),
                ]),
            // Submit button
            container()
                .padding(Padding::horizontal(16.0).vertical(10.0))
                .background(Color::rgb(0.3, 0.5, 0.9))
                .corner_radius(6.0)
                .hover_state(|s| s.lighter(0.1))
                .pressed_state(|s| s.darker(0.1))
                .on_click(move || {
                    println!("Login: {} / {}", username.get(), password.get());
                })
                .child(
                    text("Sign In")
                        .color(Color::WHITE)
                        .font_size(14.0)
                        .bold()
                ),
        ])
}
```

## Features

- **Selection**: Click and drag to select text, or use Shift+Arrow keys
- **Clipboard**: Full copy/cut/paste support via Ctrl+C/X/V
- **Undo/Redo**: History with intelligent coalescing of rapid edits
- **Scrolling**: Long text scrolls horizontally to keep cursor visible
- **Cursor Blinking**: Standard blinking cursor when focused
- **Key Repeat**: Hold keys for continuous input

## API Reference

```rust
text_input(value: impl IntoMaybeDyn<String>) -> TextInput

impl TextInput {
    pub fn text_color(self, color: impl IntoMaybeDyn<Color>) -> Self;
    pub fn cursor_color(self, color: impl IntoMaybeDyn<Color>) -> Self;
    pub fn selection_color(self, color: impl IntoMaybeDyn<Color>) -> Self;
    pub fn font_size(self, size: impl IntoMaybeDyn<f32>) -> Self;
    pub fn password(self, enabled: bool) -> Self;
    pub fn mask_char(self, c: char) -> Self;
    pub fn on_change<F: Fn(&str) + 'static>(self, callback: F) -> Self;
    pub fn on_submit<F: Fn(&str) + 'static>(self, callback: F) -> Self;
}
```
