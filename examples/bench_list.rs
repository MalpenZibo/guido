//! Benchmark: a long scrollable list of interactive rows.
//!
//! Each row has a checkbox (composed from a container — guido has no
//! dedicated checkbox widget yet) and a text input. Intended for comparing
//! performance and memory footprint against equivalent apps in other
//! toolkits (see the iced twin of this example).
//!
//! Row count is the first CLI argument (default 1000):
//!
//! ```bash
//! cargo run --release --example bench_list -- 1000
//! ```

use guido::prelude::*;

fn main() {
    let row_count: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1000);

    App::new().run(move |app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(600)
                .height(800)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .namespace("guido-bench-list")
                .background_color(Color::rgb(0.10, 0.10, 0.14)),
            move || {
                container()
                    .background(Color::rgb(0.10, 0.10, 0.14))
                    .scrollable(ScrollAxis::Vertical)
                    .child(
                        container()
                            .layout(Flex::column().spacing(4.0))
                            .padding(8.0)
                            .children((0..row_count).map(bench_row).collect::<Vec<_>>()),
                    )
            },
        );
    });
}

fn bench_row(i: usize) -> Container {
    let value = create_signal(format!("Item {i}"));
    let checked = create_signal(i.is_multiple_of(5));

    container()
        .layout(Flex::row().spacing(8.0))
        .padding(4.0)
        .background(Color::rgb(0.14, 0.14, 0.19))
        .corners(6.0)
        .child(
            // Checkbox composed from a container
            container()
                .width(18.0)
                .height(18.0)
                .corners(4.0)
                .border(1.5, Color::rgb(0.45, 0.45, 0.55))
                .background(move || {
                    if checked.get() {
                        Color::rgb(0.30, 0.55, 0.95)
                    } else {
                        Color::rgb(0.18, 0.18, 0.24)
                    }
                })
                .when_hovered(|s| s.lighter(0.08))
                .on_click(move || checked.update(|c| *c = !*c))
                .child(
                    text(move || (if checked.get() { "x" } else { "" }).to_string())
                        .color(Color::WHITE),
                ),
        )
        .child(
            container()
                .width(fill())
                .padding(6.0)
                .background(Color::rgb(0.18, 0.18, 0.24))
                .corners(4.0)
                .when_focused(|s| s.border(1.5, Color::rgb(0.4, 0.8, 1.0)))
                .child(
                    text_input(value)
                        .font_size(13.0)
                        .font_size(12.0)
                        .color(Color::WHITE),
                ),
        )
}
