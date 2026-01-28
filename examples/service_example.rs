//! Example demonstrating the create_service API for background tasks.
//!
//! This example shows:
//! - Creating a bidirectional service that handles commands
//! - Creating a read-only service for periodic updates
//! - Automatic cleanup when components unmount

use std::time::Duration;

use guido::prelude::*;

/// Commands that can be sent to the workspace service
#[derive(Clone)]
enum WorkspaceCmd {
    Switch(i32),
}

fn main() {
    // Clock signal - updated by a read-only service
    let time = create_signal(get_current_time());

    // Clock service - read-only (ignore the receiver)
    let _ = create_service::<(), _>(move |_rx, ctx| {
        log::info!("Clock service started");

        while ctx.is_running() {
            time.set(get_current_time());
            std::thread::sleep(Duration::from_secs(1));
        }

        log::info!("Clock service stopped");
    });

    // Workspace signals
    let active = create_signal(1i32);
    let workspaces: Vec<i32> = vec![1, 2, 3, 4, 5];

    // Workspace service - bidirectional
    let service = create_service(move |rx, ctx| {
        log::info!("Workspace service started");

        while ctx.is_running() {
            // Handle commands from UI
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    WorkspaceCmd::Switch(id) => {
                        log::info!("Switching to workspace {}", id);
                        active.set(id);
                    }
                }
            }

            std::thread::sleep(Duration::from_millis(16));
        }

        log::info!("Workspace service stopped");
    });

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .height(40)
            .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move || {
            let svc = service.clone();

            container()
                .layout(
                    Flex::row()
                        .spacing(16.0)
                        .main_axis_alignment(MainAxisAlignment::SpaceBetween),
                )
                .padding(4.0)
                // Workspace buttons
                .child(container().layout(Flex::row().spacing(4.0)).children(
                    workspaces.clone().into_iter().map(move |id| {
                        let svc = svc.clone();
                        container()
                            .width(32.0)
                            .height(32.0)
                            .background(move || {
                                if active.get() == id {
                                    Color::rgb(0.3, 0.5, 0.8)
                                } else {
                                    Color::rgb(0.2, 0.2, 0.3)
                                }
                            })
                            .corner_radius(4.0)
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
                            .on_click(move || svc.send(WorkspaceCmd::Switch(id)))
                            .child(
                                container()
                                    .layout(
                                        Flex::row()
                                            .main_axis_alignment(MainAxisAlignment::Center)
                                            .cross_axis_alignment(CrossAxisAlignment::Center),
                                    )
                                    .child(text(format!("{}", id)).color(Color::WHITE)),
                            )
                    }),
                ))
                // Clock display
                .child(
                    container()
                        .padding(8.0)
                        .background(Color::rgb(0.2, 0.2, 0.3))
                        .corner_radius(4.0)
                        .child(text(move || time.get()).color(Color::WHITE)),
                )
        },
    );
    app.run();
}

fn get_current_time() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Simple time formatting (HH:MM:SS)
    let hours = (now % 86400) / 3600;
    let minutes = (now % 3600) / 60;
    let seconds = now % 60;

    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
