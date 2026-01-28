# Background Threads

Guido signals are thread-safe and can be updated from background threads. The `create_service` API provides a convenient way to spawn background services that are automatically cleaned up when the component unmounts.

## Basic Pattern: Read-Only Service

For services that only push data to signals (no commands from UI):

```rust
use std::time::Duration;

let time = create_signal(String::new());

// Spawn a read-only service - use () as command type
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        time.set(chrono::Local::now().format("%H:%M:%S").to_string());
        std::thread::sleep(Duration::from_secs(1));
    }
});

// The service automatically stops when the component unmounts
```

## Bidirectional Service

For services that also receive commands from the UI:

```rust
enum Cmd {
    Refresh,
    SetInterval(u64),
}

let data = create_signal(String::new());

let service = create_service(move |rx, ctx| {
    let mut interval = Duration::from_secs(1);

    while ctx.is_running() {
        // Handle commands from UI
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Refresh => {
                    data.set(fetch_data());
                }
                Cmd::SetInterval(secs) => {
                    interval = Duration::from_secs(secs);
                }
            }
        }

        // Periodic update
        data.set(fetch_data());
        std::thread::sleep(interval);
    }
});

// Send commands from UI callbacks
container()
    .on_click(move || service.send(Cmd::Refresh))
    .child(text("Refresh"))
```

## Complete Example: System Monitor

```rust
use guido::prelude::*;
use std::time::Duration;

fn main() {
    // Signals for system data
    let cpu_usage = create_signal(0.0f32);
    let memory_usage = create_signal(0.0f32);
    let time = create_signal(String::new());

    // Background monitoring service
    let _ = create_service::<(), _>(move |_rx, ctx| {
        while ctx.is_running() {
            // Simulate system monitoring
            cpu_usage.set(rand::random::<f32>() * 100.0);
            memory_usage.set(rand::random::<f32>() * 100.0);
            time.set(chrono::Local::now().format("%H:%M:%S").to_string());

            std::thread::sleep(Duration::from_secs(1));
        }
    });

    // Build UI
    let view = container()
        .layout(Flex::column().spacing(8.0))
        .padding(16.0)
        .children([
            text(move || format!("CPU: {:.1}%", cpu_usage.get())).color(Color::WHITE),
            text(move || format!("Memory: {:.1}%", memory_usage.get())).color(Color::WHITE),
            text(move || format!("Time: {}", time.get())).color(Color::WHITE),
        ]);

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(200)
            .height(100)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || view,
    );
    app.run();
}
```

## Multiple Services

You can create multiple independent services:

```rust
let weather = create_signal(String::new());
let news = create_signal(String::new());

// Weather service
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        weather.set(fetch_weather());
        std::thread::sleep(Duration::from_secs(300)); // Every 5 minutes
    }
});

// News service
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        news.set(fetch_news());
        std::thread::sleep(Duration::from_secs(60)); // Every minute
    }
});
```

## Error Handling

Handle errors from background services:

```rust
enum DataState {
    Loading,
    Success(String),
    Error(String),
}

let status = create_signal(DataState::Loading);

let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        match fetch_data() {
            Ok(data) => status.set(DataState::Success(data)),
            Err(e) => status.set(DataState::Error(e.to_string())),
        }
        std::thread::sleep(Duration::from_secs(1));
    }
});

// In UI
text(move || match status.get() {
    DataState::Loading => "Loading...".to_string(),
    DataState::Success(s) => s,
    DataState::Error(e) => format!("Error: {}", e),
})
```

## Timer Example

Simple clock using a service:

```rust
let time = create_signal(String::new());

let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        let now = chrono::Local::now();
        time.set(now.format("%H:%M:%S").to_string());
        std::thread::sleep(Duration::from_millis(100));
    }
});

let view = container()
    .padding(20.0)
    .child(text(move || time.get()).font_size(48.0).color(Color::WHITE));
```

## Best Practices

### Check `is_running()` Frequently

```rust
// Good: Check frequently so service stops promptly
while ctx.is_running() {
    // Short sleep
    std::thread::sleep(Duration::from_millis(50));
}

// Less responsive: Long sleep means slow shutdown
while ctx.is_running() {
    std::thread::sleep(Duration::from_secs(60));
}
```

### Use Non-Blocking Operations

```rust
// Good: Non-blocking receive with timeout
while ctx.is_running() {
    if let Ok(cmd) = rx.try_recv() {
        handle_command(cmd);
    }
    std::thread::sleep(Duration::from_millis(16));
}

// Bad: Blocking receive prevents checking is_running()
while ctx.is_running() {
    let cmd = rx.recv().unwrap(); // Blocks forever!
}
```

### Batch Signal Updates

If multiple signals update together, update them in sequence:

```rust
let _ = create_service::<(), _>(move |_rx, ctx| {
    while ctx.is_running() {
        let data = fetch_all_data();

        // All updates happen before next render
        cpu.set(data.cpu);
        memory.set(data.memory);
        disk.set(data.disk);

        std::thread::sleep(Duration::from_secs(1));
    }
});
```

### Clone Service Handle for Multiple Callbacks

```rust
let service = create_service(...);

// Clone for each callback that needs it
let svc1 = service.clone();
let svc2 = service.clone();

container()
    .child(
        container()
            .on_click(move || svc1.send(Cmd::Action1))
            .child(text("Action 1"))
    )
    .child(
        container()
            .on_click(move || svc2.send(Cmd::Action2))
            .child(text("Action 2"))
    )
```
