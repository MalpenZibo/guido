# App Lifecycle

Guido applications can programmatically quit or restart. `App::run()` returns an `ExitReason` so the caller knows why the loop exited.

## Quitting

Call `quit_app()` to request a clean shutdown:

```rust
use guido::prelude::*;

container()
    .padding([8.0, 16.0])
    .background(Color::rgb(0.3, 0.3, 0.4))
    .hover_state(|s| s.lighter(0.1))
    .on_click(|| quit_app())
    .child(text("Quit"))
```

The current `App::run()` loop exits and returns `ExitReason::Quit`.

## Restarting

Call `restart_app()` to request a restart. The loop exits and returns `ExitReason::Restart`, letting the caller re-create the app:

```rust
container()
    .on_click(|| restart_app())
    .child(text("Restart"))
```

### Restart Loop

Use a loop in `main()` to support restart:

```rust
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
        }
    }
}
```

This is useful for reloading configuration, switching themes, or resetting application state.

## Calling from Background Tasks

Both `quit_app()` and `restart_app()` are `Send` — they work from any thread, including background services:

```rust
let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
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
```

## API Reference

### ExitReason

```rust
pub enum ExitReason {
    /// Normal exit (compositor closed, all surfaces destroyed, etc.)
    Quit,
    /// Restart requested. The caller should re-create `App` and run again.
    Restart,
}
```

### Functions

```rust
/// Request a clean application quit.
/// App::run() will return ExitReason::Quit.
pub fn quit_app();

/// Request a clean application restart.
/// App::run() will return ExitReason::Restart.
/// Call from any thread — uses an atomic + ping to wake the event loop.
pub fn restart_app();
```

### App::run

```rust
impl App {
    /// Run the application. Returns the reason the loop exited.
    pub fn run(self, setup: impl FnOnce(&mut Self)) -> ExitReason;
}
```
