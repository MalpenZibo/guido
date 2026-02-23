# Background Tasks

Guido signals (`Signal<T>`) live on the main thread and are `!Send` — they cannot be captured directly in background tasks. To update signals from a background task, call `.writer()` to obtain a `WriteSignal<T>`, which **is** `Send`. Writes through a `WriteSignal` are queued and applied on the main thread during the next frame.

The `create_service` API provides a convenient way to spawn async background tasks that are automatically cleaned up when the component unmounts. Services run as tokio tasks.

## Basic Pattern: Read-Only Service

For services that only push data to signals (no commands from UI):

```rust
use std::time::Duration;

let time = create_signal(String::new());
let time_w = time.writer(); // Get a Send-able write handle

// Spawn a read-only service - use () as command type
let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        time_w.set(chrono::Local::now().format("%H:%M:%S").to_string());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
});

// The service automatically stops when the component unmounts
```

## Bidirectional Service

For services that also receive commands from the UI, use `tokio::select!` for efficient async multiplexing:

```rust
enum Cmd {
    Refresh,
    SetInterval(u64),
}

let data = create_signal(String::new());
let data_w = data.writer(); // Get a Send-able write handle

let service = create_service(move |mut rx, ctx| async move {
    let mut interval = Duration::from_secs(1);

    loop {
        tokio::select! {
            Some(cmd) = rx.recv() => {
                match cmd {
                    Cmd::Refresh => {
                        data_w.set(fetch_data());
                    }
                    Cmd::SetInterval(secs) => {
                        interval = Duration::from_secs(secs);
                    }
                }
            }
            _ = tokio::time::sleep(interval) => {
                if !ctx.is_running() { break; }
                data_w.set(fetch_data());
            }
        }
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
    App::new().run(|app| {
        // Signals for system data
        let cpu_usage = create_signal(0.0f32);
        let memory_usage = create_signal(0.0f32);
        let time = create_signal(String::new());

        // Get Send-able write handles for the background task
        let cpu_w = cpu_usage.writer();
        let mem_w = memory_usage.writer();
        let time_w = time.writer();

        // Background monitoring service
        let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
            while ctx.is_running() {
                // Simulate system monitoring
                cpu_w.set(rand::random::<f32>() * 100.0);
                mem_w.set(rand::random::<f32>() * 100.0);
                time_w.set(chrono::Local::now().format("%H:%M:%S").to_string());

                tokio::time::sleep(Duration::from_secs(1)).await;
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

        app.add_surface(
            SurfaceConfig::new()
                .width(200)
                .height(100)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || view,
        );
    });
}
```

## Multiple Services

You can create multiple independent services:

```rust
let weather = create_signal(String::new());
let news = create_signal(String::new());

let weather_w = weather.writer();
let news_w = news.writer();

// Weather service
let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        weather_w.set(fetch_weather());
        tokio::time::sleep(Duration::from_secs(300)).await; // Every 5 minutes
    }
});

// News service
let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        news_w.set(fetch_news());
        tokio::time::sleep(Duration::from_secs(60)).await; // Every minute
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
let status_w = status.writer();

let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        match fetch_data() {
            Ok(data) => status_w.set(DataState::Success(data)),
            Err(e) => status_w.set(DataState::Error(e.to_string())),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
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
let time_w = time.writer();

let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        let now = chrono::Local::now();
        time_w.set(now.format("%H:%M:%S").to_string());
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
});

let view = container()
    .padding(20.0)
    .child(text(move || time.get()).font_size(48.0).color(Color::WHITE));
```

## Best Practices

### Use `tokio::select!` for Responsive Shutdown

```rust
// Good: select! wakes on either event
loop {
    tokio::select! {
        Some(cmd) = rx.recv() => {
            handle_command(cmd);
        }
        _ = tokio::time::sleep(Duration::from_secs(1)) => {
            if !ctx.is_running() { break; }
            // periodic work
        }
    }
}

// Also fine for simple loops
while ctx.is_running() {
    // do work
    tokio::time::sleep(Duration::from_millis(50)).await;
}
```

### Batch Signal Updates

If multiple signals update together, update them in sequence:

```rust
let cpu_w = cpu.writer();
let memory_w = memory.writer();
let disk_w = disk.writer();

let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
    while ctx.is_running() {
        let data = fetch_all_data();

        // All updates happen before next render
        cpu_w.set(data.cpu);
        memory_w.set(data.memory);
        disk_w.set(data.disk);

        tokio::time::sleep(Duration::from_secs(1)).await;
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
