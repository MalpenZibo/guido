//! Compositor-side background blur behind translucent containers.
//!
//! Two floating cards with different corner radii blur whatever is behind
//! the surface (`ext-background-effect-v1`). On compositors without the
//! protocol or its blur capability this renders plain translucent cards.

use guido::prelude::*;

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let radius = create_signal(24.0f32);

        app.add_surface(
            SurfaceConfig::new()
                .width(420)
                .height(260)
                .anchor(Anchor::TOP)
                .margin(120, 0, 0, 0)
                .layer(Layer::Overlay)
                .exclusive_zone(ExclusiveZone::None)
                .background_color(Color::TRANSPARENT),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .layout(
                        Flex::column()
                            .spacing(16.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        // Blurred card: translucent background + blur behind.
                        // Restricted to the compositor's backdrop, since this
                        // example is about the desktop showing through a
                        // translucent surface, not about blurring the surface's
                        // own content.
                        container()
                            .backdrop_blur(
                                BackdropBlur::new(0.0).sources(BackdropSources::COMPOSITOR),
                            )
                            .background(Color::rgba(0.12, 0.12, 0.18, 0.55))
                            .corner_radius(radius)
                            .padding(24.0)
                            .when_hovered(|s| s.lighter(0.05))
                            .on_click(move || {
                                // Cycle the radius to show the region follows it
                                radius.update(|r| *r = if *r >= 48.0 { 0.0 } else { *r + 12.0 });
                            })
                            .child(text("Blurred — click to change the corner radius")),
                    )
                    .child(
                        // Control card without blur, for comparison
                        container()
                            .background(Color::rgba(0.12, 0.12, 0.18, 0.55))
                            .corner_radius(16.0)
                            .padding(24.0)
                            .child(text("Same background, no blur")),
                    )
            },
        );
    });
}
