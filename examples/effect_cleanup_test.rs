//! Test example to verify that signals and effects are properly cleaned up
//! when children are dynamically added/removed using automatic ownership.
//!
//! This example demonstrates:
//! 1. Dynamic children with add/remove buttons
//! 2. Each child returns (key, closure) where the closure calls a function
//!    that creates signals, effects, and cleanup callbacks
//! 3. When children are removed:
//!    - The `OwnedWidget` wrapper is dropped
//!    - This automatically calls `dispose_owner()`
//!    - All signals and effects are cleaned up
//!    - The `on_cleanup` callback stops the background thread
//!
//! Key API pattern:
//! ```ignore
//! fn create_child(id: u64) -> impl Widget {
//!     let signal = create_signal(0);  // OWNED!
//!     on_cleanup(|| println!("Cleaned up"));
//!     container().child(text(move || signal.get().to_string()))
//! }
//!
//! .children(move || {
//!     items.get().into_iter().map(|id| (id, move || create_child(id)))
//! })
//! ```
//!
//! Expected console output:
//! - "[Child N Effect] Signal value: X" - from the effect
//! - "[Child N Thread] Tick #X" - from the background thread
//! - When removed, you should see:
//!   - "[Child N Cleanup] Stopping thread..."
//!   - "[Child N Thread] STOPPED"
//!   - The effect logs should STOP (effect was disposed)
//!
//! To run: cargo run --example effect_cleanup_test
//! Watch the console for logging output.

use guido::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// Global counter for unique child IDs
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Creates a child widget with its own signal, effect, and background thread.
///
/// Everything created inside this function is automatically owned by the
/// child's owner scope because it's called from within the `.children()`
/// closure wrapper.
fn create_child_widget(id: u64) -> impl Widget {
    // Create a signal for this child's tick count - AUTOMATICALLY OWNED!
    let tick_signal = create_signal(0i32);

    // Flag to stop the background thread
    let running = Arc::new(AtomicBool::new(true));
    let running_for_cleanup = running.clone();

    log::info!(
        "[Child {}] Created - signal and effect automatically owned",
        id
    );

    // Create an effect that tracks the tick signal
    // This effect will be disposed when the child is removed
    create_effect(move || {
        let value = tick_signal.get();
        log::info!("[Child {} Effect] Signal value: {}", id, value);
    });

    // Register cleanup callback - runs when the child is removed
    on_cleanup(move || {
        log::info!("[Child {} Cleanup] Stopping thread...", id);
        running_for_cleanup.store(false, Ordering::SeqCst);
    });

    // Spawn a background thread for this child
    let running_clone = running.clone();
    thread::spawn(move || {
        let mut count = 0;
        while running_clone.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(2));
            if running_clone.load(Ordering::SeqCst) {
                count += 1;
                log::info!("[Child {} Thread] Tick #{}", id, count);
                // Update the signal from background thread
                tick_signal.set(count);
            }
        }
        log::info!("[Child {} Thread] STOPPED", id);
    });

    // Return the widget
    container()
        .layout(Flex::row().spacing(8.0))
        .padding(12.0)
        .background(Color::rgb(0.2, 0.2, 0.3))
        .corner_radius(8.0)
        .child(text(format!("Child {}", id)).color(Color::WHITE))
        .child(
            text(move || format!("Ticks: {}", tick_signal.get())).color(Color::rgb(0.6, 0.8, 1.0)),
        )
}

fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();

    log::info!("=== Effect Cleanup Test (Automatic Ownership) ===");
    log::info!("This test verifies automatic cleanup via dynamic children ownership.");
    log::info!("Child creation is extracted into a function - signals are still owned!");
    log::info!("When a child is removed, its OwnedWidget drops and calls dispose_owner().\n");

    // Signal holding the list of child IDs
    let children_ids = create_signal(Vec::<u64>::new());

    let view = container()
        .layout(Flex::column().spacing(12.0))
        .padding(16.0)
        .child(
            // Title and instructions
            container()
                .layout(Flex::column().spacing(4.0))
                .child(text("Effect Cleanup Test (Automatic Ownership)").color(Color::WHITE))
                .child(
                    text("Watch the console: each child logs every 2 seconds.")
                        .color(Color::rgb(0.7, 0.7, 0.7)),
                )
                .child(
                    text("When removed, on_cleanup runs and effect logs STOP.")
                        .color(Color::rgb(0.7, 0.7, 0.7)),
                ),
        )
        .child(
            // Control buttons
            container()
                .layout(Flex::row().spacing(8.0))
                .child(
                    container()
                        .padding(10.0)
                        .background(Color::rgb(0.2, 0.5, 0.3))
                        .corner_radius(6.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || {
                            let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
                            log::info!("[Child {}] Adding to list", id);
                            children_ids.update(|list| list.push(id));
                        })
                        .child(text("Add Child").color(Color::WHITE)),
                )
                .child(
                    container()
                        .padding(10.0)
                        .background(Color::rgb(0.5, 0.2, 0.2))
                        .corner_radius(6.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || {
                            children_ids.update(|list| {
                                if let Some(id) = list.pop() {
                                    log::info!(
                                        "[Child {}] Removing from list (automatic cleanup will happen)",
                                        id
                                    );
                                }
                            });
                        })
                        .child(text("Remove Last").color(Color::WHITE)),
                )
                .child(
                    container()
                        .padding(10.0)
                        .background(Color::rgb(0.5, 0.3, 0.2))
                        .corner_radius(6.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || {
                            children_ids.update(|list| {
                                log::info!(
                                    "Removing all {} children (automatic cleanup for each)",
                                    list.len()
                                );
                                list.clear();
                            });
                        })
                        .child(text("Remove All").color(Color::WHITE)),
                ),
        )
        .child(
            // Status display
            text(move || {
                let count = children_ids.get().len();
                format!("Active children: {}", count)
            })
            .color(Color::WHITE),
        )
        .child(
            // Children container with dynamic children
            // The closure wrapper ensures create_child_widget runs inside owner scope
            container()
                .layout(Flex::column().spacing(8.0))
                .children(move || {
                    children_ids
                        .get()
                        .into_iter()
                        .map(|id| (id, move || create_child_widget(id)))
                }),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(500)
            .height(400)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || view,
    );
    app.run();
}
