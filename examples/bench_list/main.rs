//! Benchmark: a long scrollable list of interactive rows, on a real surface.
//!
//! Intended for comparing performance and memory footprint against equivalent
//! apps in other toolkits (see the iced twin of this example). The scrolling is
//! done by hand, which is fine for a comparison a person watches and no use at
//! all for a number — `benches/scroll_list` plays a scripted gesture over the
//! same list instead.
//!
//! Row count is the first CLI argument (default 1000):
//!
//! ```bash
//! cargo run --release --example bench_list -- 1000
//! ```

mod list;

use guido::prelude::*;

fn main() {
    let row_count: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1000);

    App::new().run(move |app| {
        app.add_surface(
            SurfaceConfig::new()
                .width(list::VIEWPORT.0)
                .height(list::VIEWPORT.1)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .namespace("guido-bench-list")
                .background_color(list::BACKGROUND),
            move || list::list(row_count),
        );
    });
}
