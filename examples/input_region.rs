//! Click-through overlay: only the centered pill accepts input.
//!
//! The surface spans the whole top edge without reserving space, but its
//! input region is glued to the pill's bounds via a `WidgetRef` — clicks
//! anywhere else pass through to the windows below.

use guido::prelude::*;

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let pill_ref = create_widget_ref();
        let count = create_signal(0);

        let id = app.add_surface(
            SurfaceConfig::new()
                .height(80)
                .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                .layer(Layer::Overlay)
                .exclusive_zone(Some(0))
                .background_color(Color::TRANSPARENT)
                // Start fully click-through; the effect below narrows the
                // region to the pill once it has been laid out.
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
                            .widget_ref(pill_ref)
                            .padding([10.0, 24.0])
                            .background(Color::rgba(0.15, 0.15, 0.25, 0.95))
                            .corner_radius(20.0)
                            .hover_state(|s| s.lighter(0.1))
                            .pressed_state(|s| s.ripple())
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

        // Keep the input region glued to the pill's bounds (re-runs whenever
        // layout moves or resizes it).
        create_effect(move || {
            let rect = pill_ref.rect().get();
            if rect.width > 0.0 {
                surface_handle(id).set_input_region(Some(vec![rect]));
            }
        });
    });
}
