# Text Input

The TextInput widget provides single-line text editing with support for selection, clipboard operations and undo/redo. Its sibling, `password_input`, is the same field for a password: masked, and holding the text where it cannot be copied, swapped or dumped.

## Basic Usage

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let username = create_signal(String::new());

text_input(username)
# ;
# }
```

TextInput uses **two-way binding** with signals:
- When the user types, the signal is automatically updated
- When the signal changes programmatically, the input reflects the new value

No manual synchronization is needed - just pass a `Signal<String>` and the binding works automatically.

## Styling

### Text Color

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
container().child(text_input(value).color(Color::WHITE))
# ;
# }
```

### Cursor Color

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
text_input(value).cursor_color(Color::rgb(0.4, 0.8, 1.0))
# ;
# }
```

### Selection Color

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
text_input(value).selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
# ;
# }
```

### Font Size

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
container().child(text_input(value).font_size(16.0))
# ;
# }
```

### Font Family

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
// Predefined families
container().child(text_input(value).font_family(FontFamily::Monospace));

// Shorthand for monospace
container().child(text_input(value));

// Custom font
container().child(text_input(value).font_family(FontFamily::name("JetBrains Mono")))
# ;
# }
```

### Font Weight

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
// Using constants
container().child(text_input(value).font_weight(FontWeight::BOLD));

// Shorthand for bold
container().child(text_input(value))
# ;
# }
```

## Password Input

A password gets its own field, bound to a `Password` rather than a
`RwSignal<String>`:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
let password = create_password();

password_input(password)
    .placeholder("Password")
    .on_submit(|secret: Secret| {
        // Enter moves the field's secret out, and the field is empty again.
        // Hand it to PAM; dropping it wipes it.
        # let _ = secret;
    })
# ;
# }
```

A `String` is the wrong home for a password: every edit and every `get` of one
leaves a copy on the heap that nothing wipes, its pages can be swapped to disk,
and a crash writes it into a core dump. So the field keeps its text in a
`Secret` — pages of its own, locked in memory, left out of core dumps, and
zeroed whenever they are given back — and edits it there, in place. It keeps
no undo history, since every entry would be a copy; nothing can be copied or
cut out of it; and `Ctrl+Left/Right` goes to the edge rather than revealing
where the words end. Pasting in still works, for password managers.

`Password` is a signal in everything but the copy. Reads subscribe, so state
that follows the field is an ordinary closure, and the application reaches the
text whenever it needs to — a Sign In button, a form's Save, a clear after an
idle timeout — by borrowing it:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
# let sign_in = |_: &str| {};
container()
    .enabled(move || !password.is_empty())
    .on_click(move || password.with(|secret| sign_in(secret.expose())))
# ;
password.set(Secret::from(String::from("from the keyring"))); // pre-fill; the String is wiped
password.clear();                                             // wipe in place
# }
```

There is no `get`: whatever the application copies out of `expose()` is the
application's to wipe.

By default, characters are masked with `•`. Customize the mask character:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
password_input(password).mask_char('*')
# ;
# }
```

Everything else here — placeholder, caret, pointer shape, colours, focus,
`readonly` — is the same on both fields.

## Initial Focus

A screen that exists to be typed into should not make the user click first — and
on a surface with no pointer there may be nothing to click *with*:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
password_input(password)
    .autofocus()
# ;
# }
```

The input takes the keyboard at its first layout, and only if no widget already
holds focus. That second condition is what makes it safe: an input appearing
later never pulls focus off the one being typed into, two autofocusing inputs do
not fight (the first laid out wins), and the same view built once per output — a
lock screen with two monitors — ends up with one focused field rather than
whichever was laid out last.

The offer is made once. A relayout does not ask again, so a resize or a scale
change cannot drag focus back from wherever the user has since put it.

## Losing Focus

A press that no widget claimed takes the keyboard off the field. Clicking the
surface behind it, or a decorative panel, or empty space in a row: nothing there
wanted the press, so there is nothing left for the focus to belong to, and the
caret stops.

A press that something *did* claim leaves the focus alone. A container with an
`on_click`, `on_mouse_down` or `on_mouse_up` claims the left presses that land
on it, so a toolbar button that acts on the field it sits beside does not blur
it, and neither does the field's own scrollbar. The other two buttons are
claimed by the handlers that name them, `on_right_click` and `on_middle_click`:
a container with only an `on_click` does not claim a right press, and a right
press it did not claim is a press on nothing like any other.

A press inside the focused field is always the field's, whichever button it
was — and it keeps the keyboard without consuming the press, so a row wrapping
the field still gets its own click or context menu.

The box drawn around a field is the third case, and it is the one worth knowing:
a container that declares `when_focused` and currently holds that focus keeps
that focus against a press inside itself. Clicking its padding, its border, the
space beside the caret, is clicking the field it draws.

The press still travels. The box keeps the keyboard and consumes nothing, so a
clickable ancestor wrapping the field is clicked as it would be anywhere else.

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
container()
    .padding(8.0)
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    // Lights up for the focus, and keeps it: a click on the padding is a
    // click on the field.
    .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
    .child(text_input(value))
# ;
# }
```

The focus also leaves when the whole surface does — the compositor moving the
keyboard elsewhere — and when the field is taken out of the tree.

## Placeholder

What the field says while it is empty:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
password_input(password)
    .placeholder("Password")
# ;
# }
```

It is a **label, not a value**: never masked, so a password field shows the word
rather than eight bullets, and it stands *in place of* the text rather than beside
it — there is nothing to overlap, because it is only drawn when the value is empty.

Reactive, so a prompt that changes changes the empty field with it:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let answer = create_signal(String::new());
# let prompt = create_signal(String::from("Type here"));
text_input(answer).placeholder(move || prompt.get())
# ;
# }
```

The colour is the field's own text colour at reduced alpha — a placeholder is the
same text, quieter. Declare `placeholder_color` on the field itself: only a
field draws a placeholder, so only a field says what colour it is.

```rust
# extern crate guido;
# use guido::prelude::*;
# #[derive(Clone, Copy)]
# struct Theme { text: Color, text_weak: Color, weak: Color, strong: Color, line: Color, accent: Color, error: Color, danger: Color, surface: Color }
# impl Default for Theme { fn default() -> Self { let c = Color::WHITE; Self { text: c, text_weak: c, weak: c, strong: c, line: c, accent: c, error: c, danger: c, surface: c } } }
# fn main() {
# let theme = Theme::default();
# let value = create_signal(String::new());
container()
    .child(text_input(value).placeholder_color(theme.text_weak).color(theme.text).placeholder("Search"))
# ;
# }
```

## No Caret

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
password_input(password)
    .no_caret()
# ;
# }
```

Keeps the focus and everything it carries — click to position, drag to select, the
keyboard — and draws no caret. For a field where the caret says nothing: a masked
one, where every character looks the same and you are always at the end. swaylock
draws none for that reason.

It is also the cheapest field there is. A blinking caret is the one thing a still
screen redraws on its own, twice a second, for as long as it is focused; without
one an idle surface asks the loop for nothing at all.

## Pointer Shape

A field shows the I-beam under the pointer unless it says otherwise, and it takes
the same `cursor` a container does — a value, a closure or a signal:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let password = create_password();
password_input(password)
    .no_caret()
    .cursor(CursorIcon::Hidden)
# ;
# }
```

A masked field with no caret has nothing a click could place, so on a lock screen
whose root hides the pointer the I-beam would promise editing that is not there.
The field is the innermost widget under the pointer, so a shape declared around it
never reaches it: this is the one place to say it.

## Callbacks

### On Change

Called whenever the text content changes:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
text_input(value)
    .on_change(|new_text| {
        println!("Text changed: {}", new_text);
    })
# ;
# }
```

### On Submit

Called when the user presses Enter:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
text_input(value)
    .on_submit(|text| {
        println!("Submitted: {}", text);
    })
# ;
# }
```

## Keyboard Shortcuts

The TextInput widget supports standard text editing shortcuts:

| Shortcut | Action |
|----------|--------|
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy selection (not in a password field) |
| `Ctrl+X` | Cut selection (not in a password field) |
| `Ctrl+V` | Paste |
| `Ctrl+Z` | Undo (not in a password field) |
| `Ctrl+Shift+Z` or `Ctrl+Y` | Redo (not in a password field) |
| `Left/Right` | Move cursor |
| `Ctrl+Left/Right` | Move by word (to the edge in a password field) |
| `Shift+Left/Right` | Extend selection |
| `Home/End` | Move to start/end |
| `Backspace` | Delete before cursor |
| `Delete` | Delete after cursor |

## Styling with Container

TextInput handles text editing but not visual styling like backgrounds and borders. Wrap it in a Container for full styling:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
container()
    .padding([8.0, 12.0])
    .background(Color::rgb(0.15, 0.15, 0.2))
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corners(4.0)
    .child(
        container().child(text_input(value).font_size(14.0).color(Color::WHITE))
    )
# ;
# }
```

### With Focus State

Add visual feedback when the input is focused:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
container()
    .padding([8.0, 12.0])
    .background(Color::rgb(0.15, 0.15, 0.2))
    .border(1.0, Color::rgb(0.3, 0.3, 0.4))
    .corners(4.0)
    .when_focused(|s| s.border(1.0, Color::rgb(0.4, 0.6, 1.0)))
    .child(
        container().child(text_input(value).color(Color::WHITE))
    )
# ;
# }
```

## Complete Example

A login form with username and password fields:

```rust
# extern crate guido;
# use guido::prelude::*;
fn login_form() -> Container {
    let username = create_signal(String::new());
    let password = create_password();

    container()
        .padding(24.0)
        .background(Color::rgb(0.1, 0.1, 0.15))
        .corners(12.0)
        .layout(Flex::column().spacing(16.0))
        .children([
            // Username field
            container()
                .layout(Flex::column().spacing(4.0))
                .children([
                    container().child(text("Username").font_size(12.0).color(Color::rgb(0.6, 0.6, 0.7))),
                    container()
                        .padding([8.0, 12.0])
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corners(4.0)
                        .when_focused(|s| s.border(1.0, Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            container().child(text_input(username).font_size(14.0).color(Color::WHITE))
                        ),
                ]),
            // Password field
            container()
                .layout(Flex::column().spacing(4.0))
                .children([
                    container().child(text("Password").font_size(12.0).color(Color::rgb(0.6, 0.6, 0.7))),
                    container()
                        .padding([8.0, 12.0])
                        .background(Color::rgb(0.15, 0.15, 0.2))
                        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
                        .corners(4.0)
                        .when_focused(|s| s.border(1.0, Color::rgb(0.4, 0.6, 1.0)))
                        .child(
                            container().child(password_input(password).font_size(14.0).color(Color::WHITE))
                        ),
                ]),
            // Submit button
            container()
                .padding([10.0, 16.0])
                .background(Color::rgb(0.3, 0.5, 0.9))
                .corners(6.0)
                .when_hovered(|s| s.lighter(0.1))
                .when_pressed(|s| s.darker(0.1))
                .on_click(move || {
                    password.with(|secret| {
                        println!("Login: {} / {} characters", username.get(), secret.expose().chars().count());
                    });
                })
                .child(
                    container().child(text("Sign In").font_size(14.0).color(Color::WHITE))
                ),
        ])
}
# fn main() {}
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

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
text_input(signal: RwSignal<String>) -> TextInput
password_input(password: Password) -> PasswordInput   // TextInput<Masked>

// Both fields
impl<C> TextInput<C> {
    pub fn autofocus(self) -> Self;
    pub fn caret<M>(self, caret: impl IntoSignal<bool, M>) -> Self;
    pub fn no_caret(self) -> Self;  // Shorthand for caret(false)
    pub fn cursor<M>(self, cursor: impl IntoSignal<CursorIcon, M>) -> Self;  // Text by default
    pub fn placeholder<M>(self, text: impl IntoSignal<String, M>) -> Self;
    pub fn readonly<M>(self, readonly: impl IntoSignal<bool, M>) -> Self;
}

impl TextInput {
    pub fn on_change<F: Fn(&str) + 'static>(self, callback: F) -> Self;
    pub fn on_submit<F: Fn(&str) + 'static>(self, callback: F) -> Self;
}

impl PasswordInput {
    pub fn mask_char<M>(self, c: impl IntoSignal<char, M>) -> Self;
    pub fn on_submit<F: Fn(Secret) + 'static>(self, callback: F) -> Self;
}

create_password() -> Password
impl Password {
    pub fn with<R>(&self, f: impl FnOnce(&Secret) -> R) -> R;  // tracked
    pub fn is_empty(&self) -> bool;                              // tracked
    pub fn set(&self, secret: Secret);
    pub fn clear(&self);
}

impl Secret {
    pub fn expose(&self) -> &str;
    pub fn is_empty(&self) -> bool;
}
impl From<String> for Secret   // wipes the String
```

**Note:** The `on_change` callback is optional and is called *in addition* to the automatic signal update. Use it for side effects like validation or logging, not for updating the signal (that happens automatically).
