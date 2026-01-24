# Background Threads

Guido signals are thread-safe and can be updated from background threads. This enables live data updates, async operations, and real-time applications.

## Basic Pattern

```rust
use std::sync::mpsc;
use std::thread;

let data = create_signal(String::new());
let (tx, rx) = mpsc::channel();

// Background thread
thread::spawn(move || {
    loop {
        let result = fetch_data(); // Your async operation
        tx.send(result).ok();
        thread::sleep(Duration::from_secs(1));
    }
});

// Poll for updates in the render loop
App::new()
    .on_update(move || {
        while let Ok(msg) = rx.try_recv() {
            data.set(msg);
        }
    })
    .run(view);
```

## Why Channels?

Signals are thread-safe, but effects only run on the main thread. The channel pattern:

1. Background thread does work and sends results
2. Main thread polls the channel in `on_update`
3. Signal updates trigger reactive UI changes

This keeps the main thread responsive while safely integrating async work.

## Complete Example: System Monitor

```rust
use guido::prelude::*;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn main() {
    // Signals for system data
    let cpu_usage = create_signal(0.0f32);
    let memory_usage = create_signal(0.0f32);
    let time = create_signal(String::new());

    // Channel for updates
    let (tx, rx) = mpsc::channel::<SystemUpdate>();

    // Background monitoring thread
    thread::spawn(move || {
        loop {
            // Simulate system monitoring
            tx.send(SystemUpdate {
                cpu: rand::random::<f32>() * 100.0,
                memory: rand::random::<f32>() * 100.0,
                time: chrono::Local::now().format("%H:%M:%S").to_string(),
            }).ok();

            thread::sleep(Duration::from_secs(1));
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

    // Run with update callback
    App::new()
        .width(200)
        .height(100)
        .on_update(move || {
            while let Ok(update) = rx.try_recv() {
                cpu_usage.set(update.cpu);
                memory_usage.set(update.memory);
                time.set(update.time);
            }
        })
        .run(view);
}

struct SystemUpdate {
    cpu: f32,
    memory: f32,
    time: String,
}
```

## Multiple Data Sources

```rust
let (weather_tx, weather_rx) = mpsc::channel();
let (news_tx, news_rx) = mpsc::channel();

// Spawn multiple background threads
thread::spawn(move || { /* fetch weather */ });
thread::spawn(move || { /* fetch news */ });

App::new()
    .on_update(move || {
        // Poll all channels
        while let Ok(weather) = weather_rx.try_recv() {
            weather_signal.set(weather);
        }
        while let Ok(news) = news_rx.try_recv() {
            news_signal.set(news);
        }
    })
    .run(view);
```

## Error Handling

Handle errors from background threads:

```rust
enum DataUpdate {
    Success(String),
    Error(String),
}

let status = create_signal(DataUpdate::Success(String::new()));

thread::spawn(move || {
    match fetch_data() {
        Ok(data) => tx.send(DataUpdate::Success(data)),
        Err(e) => tx.send(DataUpdate::Error(e.to_string())),
    }.ok();
});

// In UI
text(move || match status.get() {
    DataUpdate::Success(s) => s,
    DataUpdate::Error(e) => format!("Error: {}", e),
})
```

## Timer Example

Simple clock using background thread:

```rust
let time = create_signal(String::new());
let (tx, rx) = mpsc::channel();

thread::spawn(move || {
    loop {
        let now = chrono::Local::now();
        tx.send(now.format("%H:%M:%S").to_string()).ok();
        thread::sleep(Duration::from_millis(100));
    }
});

let view = container()
    .padding(20.0)
    .child(text(move || time.get()).font_size(48.0).color(Color::WHITE));

App::new()
    .on_update(move || {
        if let Ok(t) = rx.try_recv() {
            time.set(t);
        }
    })
    .run(view);
```

## Best Practices

### Use Non-Blocking Receives

```rust
// Good: Non-blocking, processes all pending messages
while let Ok(msg) = rx.try_recv() {
    signal.set(msg);
}

// Bad: Would block the main thread
let msg = rx.recv().unwrap();
```

### Batch Updates

If multiple signals update together, batch them:

```rust
struct Update {
    cpu: f32,
    memory: f32,
    disk: f32,
}

// Single channel for all related data
while let Ok(update) = rx.try_recv() {
    cpu.set(update.cpu);
    memory.set(update.memory);
    disk.set(update.disk);
}
```

### Handle Thread Termination

For clean shutdown:

```rust
let (tx, rx) = mpsc::channel();
let running = Arc::new(AtomicBool::new(true));
let running_clone = running.clone();

let handle = thread::spawn(move || {
    while running_clone.load(Ordering::Relaxed) {
        // Work...
        thread::sleep(Duration::from_secs(1));
    }
});

// On app shutdown
running.store(false, Ordering::Relaxed);
handle.join().ok();
```
