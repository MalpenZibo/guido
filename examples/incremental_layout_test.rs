//! Benchmark: a small piece of a big UI changes, ten times a second.
//!
//! A header label updates while 100 static rows sit untouched. This is the
//! canonical incremental-invalidation case: layout must only re-run along the
//! dirty path, and paint must only redraw what layout actually touched — the
//! untouched rows should come out of the paint/flatten caches, and the damage
//! reported to the compositor should be the header's rectangle, not the whole
//! surface.
//!
//! ```bash
//! cargo run --release --example incremental_layout_test --features render-stats
//! ```
//!
//! Read `paint: cache_rate` (higher is better) and `damage:` (partial, not
//! full) in the per-second stats.

use guido::prelude::*;

#[tokio::main]
async fn main() {
    App::new().run(|app| {
        // A label that changes text — and therefore size — 10 times a second
        let ticks = create_signal(0u32);
        let ticks_w = ticks.writer();
        create_task(move |ctx| async move {
            while ctx.is_running() {
                ticks_w.update(|t| *t += 1);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        });

        app.add_surface(
            SurfaceConfig::new()
                .width(600)
                .height(800)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("guido-incremental-layout")
                .background_color(Color::rgb(0.10, 0.10, 0.14)),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .layout(Flex::column().spacing(8.0))
                    .padding(12.0)
                    .child(
                        container()
                            .width(fill())
                            .padding(12.0)
                            .corners(12.0)
                            .background(Color::rgb(0.18, 0.18, 0.24))
                            .child(text(move || format!("elapsed: {} ticks", ticks.get()))),
                    )
                    .children((0..100).map(static_row).collect::<Vec<_>>())
            },
        );
    });
}

/// A row that never changes: everything below it should stay cached.
fn static_row(i: usize) -> Container {
    container()
        .width(fill())
        .padding(8.0)
        .corners(8.0)
        .background(Color::rgb(0.14, 0.14, 0.19))
        .layout(Flex::row().spacing(8.0))
        .child(
            container()
                .width(16.0)
                .height(16.0)
                .corners(8.0)
                .background(Color::rgb(0.3, 0.5, 0.9)),
        )
        .child(container().child(text(format!("row {i}")).font_size(14.0).font_size(18.0)))
}
