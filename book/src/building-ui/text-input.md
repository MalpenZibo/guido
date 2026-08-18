# Text Input

The TextInput widget provides single-line text editing with support for selection, clipboard operations, undo/redo, and password masking.

## Basic Usage

```rust
let username = create_signal(String::new());

text_input(username)
```

TextInput uses **two-way binding** with signals:
- When the user types, the signal is automatically updated
- When the signal changes programmatically, the input reflects the new value

No manual synchronization is needed - just pass a `Signal<String>` and the binding works automatically.

## Styling

### Text Color

```rust
container().text_color(Color::WHITE).child(text_input(value))
```

### Cursor Color

```rust
text_input(value).cursor_color(Color::rgb(0.4, 0.8, 1.0))
// or, for every input below one container:
container().cursor_color(Color::rgb(0.4, 0.8, 1.0)).child(text_input(value))
```

### Selection Color

```rust
text_input(value).selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
```

### Font Size

```rust
container().font_size(16.0).child(text_input(value))
```

### Font Family

```rust
// Predefined families
container().font_family(FontFamily::Monospace).child(text_input(value))

// Shorthand for monospace
container().mono().child(text_input(value))

// Custom font
container().font_family(FontFamily::Name("JetBrains Mono".into())).child(text_input(value))
```

### Font Weight

```rust
// Using constants
container().font_weight(FontWeight::BOLD).child(text_input(value))

// Shorthand for bold
container().bold().child(text_input(value))
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

## Initial Focus

A screen that exists to be typed into should not make the user click first — and
on a surface with no pointer there may be nothing to click *with*:

```rust
text_input(password)
    .password(true)
    .autofocus()
```

The input takes the keyboard at its first layout, and only if no widget already
holds focus. That second condition is what makes it safe: an input appearing
later never pulls focus off the one being typed into, two autofocusing inputs do
not fight (the first laid out wins), and the same view built once per output — a
lock screen with two monitors — ends up with one focused field rather than
whichever was laid out last.

The offer is made once. A relayout does not ask again, so a resize or a scale
change cannot drag focus back from wherever the user has since put it.

## Placeholder

What the field says while it is empty:

```rust
text_input(password)
    .password(true)
    .placeholder("Password")
```

It is a **label, not a value**: never masked, so a password field shows the word
rather than eight bullets, and it stands *in place of* the text rather than beside
it — there is nothing to overlap, because it is only drawn when the value is empty.

Reactive, so a prompt that changes changes the empty field with it:

```rust
text_input(answer).placeholder(move || prompt.get())
```

The colour is the inherited text colour at reduced alpha — a placeholder is the
same text, quieter. Declare `placeholder_color` on the field, or on a
container to cover every field below it:

```rust
container()
    .text_color(theme.text)
    .placeholder_color(theme.text_weak)
    .child(text_input(value).placeholder("Search"))
```

## No Caret

```rust
text_input(password)
    .password(true)
    .no_caret()
```

Keeps the focus and everything it carries — click to position, drag to select, the
keyboard — and draws no caret. For a field where the caret says nothing: a masked
one, where every character looks the same and you are always at the end. swaylock
draws none for that reason.

It is also the cheapest field there is. A blinking caret is the one thing a still
screen redraws on its own, twice a second, for as long as it is focused; without
one an idle surface asks the loop for nothing at all.

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
        container().text_color(Color::WHITE).font_size(14.0).child(text_input(value))
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
    .when_focused(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
    .child(
        container().text_color(Color::WHITE).child(text_input(value))
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
                    container().font_size(12.0).text_color(Color::rgb(0.6, 0.6, 0.7)).child(text("Username")),
                    container()
                        .padding(Padding::horizontal(12.0).vertical(8.0))
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(4.0)
                        .when_focused(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            container().text_color(Color::WHITE).font_size(14.0).child(text_input(username))
                        ),
                ]),
            // Password field
            container()
                .layout(Flex::column().spacing(4.0))
                .children([
                    container().font_size(12.0).text_color(Color::rgb(0.6, 0.6, 0.7)).child(text("Password")),
                    container()
                        .padding(Padding::horizontal(12.0).vertical(8.0))
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corner_radius(4.0)
                        .when_focused(|s| s.border_color(Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            container().text_color(Color::WHITE).font_size(14.0).child(text_input(password).password(true))
                        ),
                ]),
            // Submit button
            container()
                .padding(Padding::horizontal(16.0).vertical(10.0))
                .background(Color::rgb(0.3, 0.5, 0.9))
                .corner_radius(6.0)
                .when_hovered(|s| s.lighter(0.1))
                .when_pressed(|s| s.darker(0.1))
                .on_click(move || {
                    println!("Login: {} / {}", username.get(), password.get());
                })
                .child(
                    container().text_color(Color::WHITE).font_size(14.0).bold().child(text("Sign In"))
                ),
        ])
}
```

## Features

- **Selection**: Click and drag to select text, or use Shift+Arrow keys
- **Clipboard**: Full copy/cut/paste support via Ctrl+C/X/V. System
  clipboard contents are prefetched asynchronously whenever another app
  copies, so paste is instant and never blocks the UI
- **Primary selection**: selecting text with the mouse sets the primary
  selection (paste it elsewhere with middle click); middle-clicking the
  input pastes the primary selection at the click position
- **Undo/Redo**: History with intelligent coalescing of rapid edits
- **Scrolling**: Long text scrolls horizontally to keep cursor visible
- **Cursor Blinking**: Standard blinking cursor when focused. It asks the loop to
  wake at its next toggle rather than animating, so a focused field costs two
  wakeups a second instead of a frame at the compositor's rate; `no_caret()` drops
  it entirely and costs nothing
- **Key Repeat**: Hold keys for continuous input, at the rate and delay the
  compositor reports — the same settings every other application on the
  session obeys

## API Reference

```rust
text_input(signal: Signal<String>) -> TextInput

impl TextInput {
    pub fn password(self, enabled: bool) -> Self;
    pub fn mask_char(self, c: char) -> Self;
    pub fn autofocus(self) -> Self;
    pub fn no_caret(self) -> Self;
    pub fn placeholder<M>(self, text: impl IntoSignal<String, M>) -> Self;
    pub fn on_change<F: Fn(&str) + 'static>(self, callback: F) -> Self;
    pub fn on_submit<F: Fn(&str) + 'static>(self, callback: F) -> Self;
}
```

**Note:** The `on_change` callback is optional and is called *in addition* to the automatic signal update. Use it for side effects like validation or logging, not for updating the signal (that happens automatically).
