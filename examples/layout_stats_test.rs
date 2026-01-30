//! Minimal test for layout stats - static UI with animation.
//! Tests layout skipping during animation frames.
//!
//! Run with:
//! ```bash
//! cargo run --example layout_stats_test --features layout-stats
//! ```
//!
//! The rotating container triggers continuous rendering,
//! but the static children should have their layouts skipped.

use guido::prelude::*;

fn main() {
    // Signal to drive rotation animation
    let rotation = create_signal(0.0f32);

    // Continuously update rotation to force frame rendering
    let _ = create_service::<(), _>(move |_rx, ctx| {
        while ctx.is_running() {
            rotation.update(|r| *r += 1.0);
            std::thread::sleep(std::time::Duration::from_millis(16)); // ~60fps
        }
    });

    let view = container()
        .width(400.0)
        .height(200.0)
        .padding(16.0)
        .background(Color::rgb(0.1, 0.1, 0.15))
        .layout(Flex::column().spacing(8.0))
        // Animated container (will re-layout due to transform animation)
        .child(
            container()
                .padding(8.0)
                .background(Color::rgb(0.2, 0.2, 0.3))
                .transform(move || Transform::rotate_degrees(rotation.get()))
                .child(text("Rotating").color(Color::WHITE)),
        )
        // Static containers (should have layout skipped after first frame)
        .child(
            container()
                .padding(8.0)
                .background(Color::rgb(0.2, 0.3, 0.2))
                .child(text("Static text 1").color(Color::WHITE)),
        )
        .child(
            container()
                .padding(8.0)
                .background(Color::rgb(0.3, 0.2, 0.2))
                .child(text("Static text 2").color(Color::WHITE)),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(400)
            .height(200)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .layer(Layer::Top)
            .namespace("layout-stats-test")
            .background_color(Color::rgb(0.08, 0.08, 0.12)),
        move || view,
    );
    app.run();
}
