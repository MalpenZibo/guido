//! Minimal test for dynamic children
//!
//! This example automatically adds a child after startup to test the dynamic children system.

use guido::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static ADD_TRIGGERED: AtomicBool = AtomicBool::new(false);

fn main() {
    let items = create_signal(vec![1u64, 2, 3]);

    // Create a service that adds an item after 2 seconds
    let items_for_service = items;
    let _ = create_service::<(), _>(move |_rx, ctx| {
        std::thread::sleep(Duration::from_secs(2));
        if ctx.is_running() && !ADD_TRIGGERED.swap(true, Ordering::SeqCst) {
            items_for_service.update(|list| {
                list.push(4);
            });
        }
    });

    let view = container()
        .layout(Flex::column().spacing(8.0))
        .padding(20.0)
        .background(Color::rgb(0.1, 0.1, 0.15))
        .child(text("Dynamic Children Test").color(Color::WHITE))
        .child(text("An item will be added after 2 seconds...").color(Color::rgb(0.7, 0.7, 0.7)))
        .child(
            container()
                .layout(Flex::row().spacing(4.0))
                .children(move || {
                    let list = items.get();
                    list.into_iter().map(|id| {
                        (id, move || {
                            container()
                                .padding(8.0)
                                .background(Color::rgb(0.3 + id as f32 * 0.1, 0.3, 0.4))
                                .corner_radius(4.0)
                                .child(text(format!("Item {}", id)).color(Color::WHITE))
                        })
                    })
                }),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(600)
            .height(200)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .background_color(Color::rgb(0.1, 0.1, 0.15)),
        move |_| view,
    );
    app.run();
}
