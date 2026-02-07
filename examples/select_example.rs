//! Example demonstrating Signal::select() for field-level reactivity.
//!
//! This example shows:
//! - Using select() to derive signals from specific fields of a struct
//! - Chained selects for nested fields
//! - Only the selected field triggers updates (not changes to other fields)
//! - Background thread updates via create_service with select()

use std::time::Duration;

use guido::prelude::*;

/// Top-level application state
#[derive(Clone, Debug, PartialEq)]
struct AppState {
    user: String,
    count: i32,
    stats: Stats,
}

/// Nested struct to demonstrate chained selects
#[derive(Clone, Debug, PartialEq)]
struct Stats {
    cpu: f32,
    mem: f32,
}

fn main() {
    // A single signal holding the entire app state
    let state = create_signal(AppState {
        user: "Alice".into(),
        count: 0,
        stats: Stats { cpu: 0.0, mem: 0.0 },
    });

    // Derive field-level signals — each only updates when its field changes
    let user = state.select(|s| &s.user);
    let count = state.select(|s| &s.count);
    let stats = state.select(|s| &s.stats);

    // Chained select: state -> stats -> cpu
    let cpu = stats.select(|s| &s.cpu);
    let mem = stats.select(|s| &s.mem);

    // Background service that simulates CPU/mem changes every second
    let _ = create_service::<(), _>(move |_rx, ctx| {
        let mut tick = 0u32;
        while ctx.is_running() {
            std::thread::sleep(Duration::from_secs(1));
            tick += 1;

            state.update(|s| {
                // CPU/mem change every tick
                s.stats.cpu = 10.0 + (tick as f32 * 0.7).sin() * 40.0;
                s.stats.mem = 50.0 + (tick as f32 * 0.3).cos() * 20.0;
            });
        }
    });

    let users = ["Alice", "Bob", "Charlie", "Diana"];

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(80)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            container()
                .layout(
                    Flex::row()
                        .spacing(12.0)
                        .main_axis_alignment(MainAxisAlignment::Center)
                        .cross_axis_alignment(CrossAxisAlignment::Center),
                )
                .padding(8.0)
                // User display — only updates when user field changes
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.2, 0.2, 0.35))
                        .corner_radius(8.0)
                        .child(text(move || format!("User: {}", user.get())).color(Color::WHITE)),
                )
                // Count display — only updates when count changes
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.2, 0.3, 0.2))
                        .corner_radius(8.0)
                        .child(text(move || format!("Count: {}", count.get())).color(Color::WHITE)),
                )
                // CPU display — updates via chained select (state -> stats -> cpu)
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.3, 0.2, 0.2))
                        .corner_radius(8.0)
                        .child(text(move || format!("CPU: {:.1}%", cpu.get())).color(Color::WHITE)),
                )
                // Mem display — updates via chained select (state -> stats -> mem)
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.25, 0.2, 0.3))
                        .corner_radius(8.0)
                        .child(text(move || format!("Mem: {:.1}%", mem.get())).color(Color::WHITE)),
                )
                // Button: cycle user name (only user field changes)
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.2, 0.3, 0.4))
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || {
                            state.update(|s| {
                                let idx = users
                                    .iter()
                                    .position(|u| *u == s.user)
                                    .map(|i| (i + 1) % users.len())
                                    .unwrap_or(0);
                                s.user = users[idx].into();
                            });
                        })
                        .child(text("Cycle User").color(Color::WHITE)),
                )
                // Button: increment count (only count field changes)
                .child(
                    container()
                        .padding(12.0)
                        .background(Color::rgb(0.3, 0.35, 0.2))
                        .corner_radius(8.0)
                        .hover_state(|s| s.lighter(0.1))
                        .pressed_state(|s| s.ripple())
                        .on_click(move || {
                            state.update(|s| s.count += 1);
                        })
                        .child(text("+1 Count").color(Color::WHITE)),
                )
        },
    );
    app.run();
}
