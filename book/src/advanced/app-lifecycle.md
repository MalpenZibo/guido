# App Lifecycle

Guido applications can programmatically quit or restart. `App::run()` returns an `ExitReason` so the caller knows why the loop exited: `Quit`, `Restart`, or `Error(PlatformError)` when the platform layer fails (no Wayland session, a compositor without layer-shell support such as GNOME, or a lost connection) — these conditions are reported instead of panicking.

## Quitting

Call `quit_app()` to request a clean shutdown:

```rust
# extern crate guido;
# fn main() {
use guido::prelude::*;

container()
    .padding([8.0, 16.0])
    .background(Color::rgb(0.3, 0.3, 0.4))
    .when_hovered(|s| s.lighter(0.1))
    .on_click(|| quit_app())
    .child(text("Quit"))
# ;
# }
```

The current `App::run()` loop exits and returns `ExitReason::Quit`.

## Restarting

Call `restart_app()` to request a restart. The loop exits and returns `ExitReason::Restart`, letting the caller re-create the app:

```rust
# extern crate guido;
# use guido::prelude::*;
# fn main() {
container()
    .on_click(|| restart_app())
    .child(text("Restart"))
# ;
# }
```

### Restart Loop

Use a loop in `main()` to support restart:

```rust,no_run
# extern crate guido;
# fn build_ui() -> Container { container() }
use guido::prelude::*;

fn main() {
    loop {
        let reason = App::new().run(|app| {
            app.add_surface(
                SurfaceConfig::new()
                    .height(32)
                    .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                    .layer(Layer::Top)
                    .namespace("my-bar")
                    .background_color(Color::rgb(0.1, 0.1, 0.15)),
                || build_ui(),
            );
        });

        match reason {
            ExitReason::Quit => break,
            ExitReason::Restart => continue,
            ExitReason::Error(e) => {
                eprintln!("guido could not run: {e}");
                break;
            }
        }
    }
}
```

This is useful for reloading configuration, switching themes, or resetting application state.

## Calling from Background Tasks

Both `quit_app()` and `restart_app()` are `Send` — they work from any thread, including background services:

```rust,ignore
# // not compiled: the task body is tokio's, and the book's samples are
# // compiled without it.
# extern crate guido;
# use guido::prelude::*;
# fn main() {
create_task(move |ctx| async move {
    loop {
        tokio::select! {
            _ = watch_config_file() => {
                // Config changed — trigger restart
                restart_app();
                break;
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if !ctx.is_running() { break; }
            }
        }
    }
});
# ;
# }
```

## API Reference

### ExitReason

```rust,ignore
# // not compiled: a listing of a type the crate owns — a copy of it declared
# // here would compile without checking the original.
pub enum ExitReason {
    /// Normal exit (compositor closed, all surfaces destroyed, etc.)
    Quit,
    /// Restart requested. The caller should re-create `App` and run again.
    Restart,
    /// The platform layer failed: no Wayland session, a compositor without
    /// layer shell, or a lost connection.
    Error(PlatformError),
}
```

### Functions

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
/// Request a clean application quit.
/// App::run() will return ExitReason::Quit.
pub fn quit_app();

/// Request a clean application restart.
/// App::run() will return ExitReason::Restart.
/// Call from any thread — uses an atomic + ping to wake the event loop.
pub fn restart_app();
```

### App::run

```rust,ignore
# // not compiled: a signature listing — these declarations have no bodies.
impl App {
    /// Run the application. Returns the reason the loop exited.
    pub fn run(self, setup: impl FnOnce(&mut Self)) -> ExitReason;
}
```
