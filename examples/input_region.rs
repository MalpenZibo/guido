//! Click-through overlay: only the centered pill accepts input.
//!
//! The surface spans the whole top edge without reserving space and lets
//! everything through; the pill declares that input reaches it. The region the
//! compositor is given is read off the frame, so it is the pill's rounded
//! shape, wherever the layout put it, and it is gone the moment the pill is.

use guido::prelude::*;

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let count = create_signal(0);

        app.add_surface(
            SurfaceConfig::new()
                .height(80)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Overlay)
                .exclusive_zone(ExclusiveZone::None)
                .background_color(Color::TRANSPARENT)
                // Everything passes through, except what says otherwise.
                .click_through(),
            move || {
                container()
                    .width(fill())
                    .height(fill())
                    .layout(
                        Flex::row()
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        container()
                            .takes_input(true)
                            .padding([10.0, 24.0])
                            .background(Color::rgba(0.15, 0.15, 0.25, 0.95))
                            .corners(20.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .when_pressed(|s| s.ripple())
                            .on_click(move || count.update(|c| *c += 1))
                            .child(text(move || {
                                format!(
                                    "Clicks: {} — everything around me passes through",
                                    count.get()
                                )
                            })),
                    )
            },
        );
    });
}
