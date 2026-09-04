# Disabling a Subtree

A form that stops responding while a request is in flight is one declaration,
not a check inside every handler:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let busy = create_signal(false);
# let submit = || {};
container()
    .enabled(move || !busy.get())
    .child(container().on_click(submit).child(text("Send")))
# ;
# }
```

`enabled(false)` stops every event at that container. Nothing below it — a
click, a key, a scroll, a nested button's handler — is reached, and nothing
below it has to know why.

## It propagates, and cannot be undone from below

A container declaring `enabled(true)` inside one declaring `enabled(false)` is
**disabled**. There is no way for a descendant to re-enable itself, which is
what lets a whole form be switched off in one place and is the rule Qt, GTK and
SwiftUI all settled on.

## The look is declared beside the behaviour

A disabled subtree is painted exactly as declared — guido does not pick a grey
for you. `when_disabled` is how you pick one, and it reads the same declaration
that stopped the events, so the two cannot come apart:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let busy = create_signal(false);
container()
    .enabled(move || !busy.get())
    .child(
        text("Send")
            .color(Color::WHITE)
            .when_disabled(|s| s.color(Color::rgb(0.5, 0.5, 0.5))),
    )
# ;
# }
```

The text asks its **control** — the nearest enclosing interaction unit, which
declaring `enabled` makes the container into. That is why the label greys with
the form around it without being handed the signal itself.

A condition you already hold reads the same at one call site:
`state(busy, |s| s.darker(0.2))`. What it cannot do is promise that two widgets
in two files mean the same thing by it. `Disabled` is the control's answer, with
its ancestors folded in, so it means one thing everywhere.

## What a disabled subtree gives up, and what it keeps

**It gives up the pointer and the keyboard.** A disabled control is neither
hovered nor pressed nor focused, so `when_hovered`, `when_pressed` and
`when_focused` all switch off with it — no stale highlight, and no focus ring on
something that is not taking keys. Qt, GTK, SwiftUI and `<fieldset disabled>`
all behave this way.

**It is still laid out and still painted.** Disabling changes what an event
does, not where anything sits — so it costs no layout pass.

## Not the same as read-only

There is a second thing you may want, and it is not this one: a field that stops
accepting *edits* but goes on saying the keyboard is aimed at it. A lock screen
that has sent the password to PAM wants exactly that — the typed characters must
go nowhere, but on a multi-monitor setup the `when_focused` ring is the only
thing saying which screen you are typing at, and disabling would take it away.

That is `readonly`, and it is a different setter for a different job:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
# let value = create_signal(String::new());
# let busy = create_signal(false);
text_input(value).readonly(busy).caret(move || !busy.get())
# ;
# }
```

It refuses every edit — typing, backspace, delete, paste, cut, undo — and
nothing else. The caret still moves, a selection can still be made and copied,
`Enter` still reaches `on_submit`, and the focus stays put. The caret is not
part of it: a field that should stop blinking says so with `caret`, driven by
the same signal, as above.

Every toolkit keeps these two apart, and for this reason: `readonly` beside
`disabled` on the web, `setReadOnly` beside `setEnabled` in Qt, `readOnly`
beside `enabled` in Flutter. Reach for `enabled` when the thing is not
available, and for `readonly` when it is yours but frozen.
