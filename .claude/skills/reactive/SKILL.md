---
name: reactive
description: Guido's reactive system — signals, memos, effects, ownership, background tasks and services. Use when touching src/reactive/, when a property needs to update without rebuilding the tree, when a value has to reach the UI from another thread, or when something re-renders too often or not at all.
---

# The reactive system

Single-threaded, SolidJS-shaped, thread-local runtime. Full reference in
[docs/REACTIVE.md](../../../docs/REACTIVE.md); this is what to hold in mind.

## The types

- `RwSignal<T>` (8 bytes) — read and write. `create_signal` (needs
  `Clone + PartialEq + Send`). `.get()`, `.set()`, `.update()`, `.writer()`.
  Becomes a `Signal<T>` with `.read_only()` or `.into()`.
- `Signal<T>` (12 bytes) — read only. `create_stored` (static, `Clone`) or
  `create_derived` (closure-backed). `.get()`, `.with()`, and nothing that
  mutates.
- `Memo<T>` — eager, recomputes when a dependency changes, notifies only when
  the value actually changed (`PartialEq`).
- `create_effect` — a side effect that re-runs when what it read changes. It
  returns nothing on purpose: an effect's lifetime is its scope's.
- `Owner` — the scope that owns signals, effects and custom cleanups.

Dependency tracking is automatic and thread-local. Container paint and layout
track signal reads through `with_signal_tracking()`, which is why a plain
closure works as a reactive property.

## Off the main thread

`Signal<T>` and `RwSignal<T>` are `!Send`. A background thread gets a
`WriteSignal<T>` from `.writer()` — it is `Send`, and its writes are queued and
flushed once per frame by `flush_bg_writes()`.

`create_task` for work that only pushes. `create_service` when the UI also sends
commands back. Both are aborted with the scope, so neither needs manual
teardown.

```rust
let data = create_signal(String::new());
let data_w = data.writer();
create_task(move |ctx| async move {
    while ctx.is_running() {
        data_w.set(fetch().await);
    }
});
```

## What usually goes wrong

- **Nothing updates.** The read happened outside a tracked context — a value
  captured once instead of a closure that reads each time.
- **Everything updates.** A `Signal<T>` of a large struct where a field would
  do: `signal.select(|s| &s.field)` derives a signal that only fires when that
  field changes, and clones only then.
- **An effect that writes what it reads** re-runs itself. Use a `Memo`.
- **A write from a background thread appears a frame late.** That is the queue,
  and it is intentional.

## Before changing the runtime

Effect scheduling, ownership and invalidation are cross-cutting: a change there
is an architectural change, and the rule in [AGENTS.md](../../../AGENTS.md)
applies — explain the problem in the code and get the design agreed first.
