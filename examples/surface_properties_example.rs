//! Example demonstrating dynamic surface property modification.
//!
//! This example shows how to:
//! - Configure keyboard interactivity at surface creation
//! - Get a handle for surfaces added via `add_surface()`
//! - Dynamically modify surface properties (layer, keyboard interactivity, etc.)
//!
//! Click the buttons to change the surface's layer and keyboard mode.

use guido::prelude::*;

fn main() {
    App::new().run(|app| {
        // Store the surface ID so we can get a handle to it later
        let surface_id_signal = create_signal(None::<SurfaceId>);

        // Track current layer and keyboard mode for display
        let current_layer = create_signal("Top");
        let current_keyboard = create_signal("OnDemand");

        let surface_id = app.add_surface(
            SurfaceConfig::new()
                .width(500)
                .height(300)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .exclusive_zone(ExclusiveZone::Auto)
                .layer(Layer::Top)
                .keyboard_interactivity(KeyboardInteractivity::OnDemand)
                .namespace("surface-properties-example")
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                container()
                    .padding(24.0)
                    .layout(Flex::column().spacing(16.0))
                    .child(
                        container()
                            .font_size(20.0)
                            .text_color(Color::WHITE)
                            .child(text("Surface Properties Demo")),
                    )
                    // Current state display
                    .child(
                        container()
                            .padding(12.0)
                            .background(Color::rgb(0.15, 0.15, 0.2))
                            .corner_radius(8.0)
                            .layout(Flex::column().spacing(8.0))
                            .child(
                                container()
                                    .text_color(Color::rgb(0.7, 0.9, 0.7))
                                    .font_size(14.0)
                                    .child(text(move || {
                                        format!("Current Layer: {}", current_layer.get())
                                    })),
                            )
                            .child(
                                container()
                                    .text_color(Color::rgb(0.7, 0.9, 0.7))
                                    .font_size(14.0)
                                    .child(text(move || {
                                        format!(
                                            "Keyboard Interactivity: {}",
                                            current_keyboard.get()
                                        )
                                    })),
                            ),
                    )
                    // Layer controls
                    .child(
                        container()
                            .layout(Flex::column().spacing(8.0))
                            .child(
                                container()
                                    .text_color(Color::rgb(0.6, 0.6, 0.7))
                                    .font_size(12.0)
                                    .child(text("Change Layer:")),
                            )
                            .child(
                                container()
                                    .layout(Flex::row().spacing(8.0))
                                    .child(layer_button(
                                        "Background",
                                        Layer::Background,
                                        surface_id_signal,
                                        current_layer,
                                    ))
                                    .child(layer_button(
                                        "Bottom",
                                        Layer::Bottom,
                                        surface_id_signal,
                                        current_layer,
                                    ))
                                    .child(layer_button(
                                        "Top",
                                        Layer::Top,
                                        surface_id_signal,
                                        current_layer,
                                    ))
                                    .child(layer_button(
                                        "Overlay",
                                        Layer::Overlay,
                                        surface_id_signal,
                                        current_layer,
                                    )),
                            ),
                    )
                    // Keyboard interactivity controls
                    .child(
                        container()
                            .layout(Flex::column().spacing(8.0))
                            .child(
                                container()
                                    .text_color(Color::rgb(0.6, 0.6, 0.7))
                                    .font_size(12.0)
                                    .child(text("Change Keyboard Interactivity:")),
                            )
                            .child(
                                container()
                                    .layout(Flex::row().spacing(8.0))
                                    .child(keyboard_button(
                                        "None",
                                        KeyboardInteractivity::None,
                                        surface_id_signal,
                                        current_keyboard,
                                    ))
                                    .child(keyboard_button(
                                        "OnDemand",
                                        KeyboardInteractivity::OnDemand,
                                        surface_id_signal,
                                        current_keyboard,
                                    ))
                                    .child(keyboard_button(
                                        "Exclusive",
                                        KeyboardInteractivity::Exclusive,
                                        surface_id_signal,
                                        current_keyboard,
                                    )),
                            ),
                    )
                    // Instructions
                    .child(
                        container()
                            .padding(12.0)
                            .background(Color::rgb(0.12, 0.12, 0.16))
                            .corner_radius(6.0)
                            .layout(Flex::column().spacing(4.0))
                            .child(
                                container()
                                    .text_color(Color::rgb(0.5, 0.5, 0.6))
                                    .font_size(11.0)
                                    .child(text("Tips:")),
                            )
                            .child(
                                container()
                                    .text_color(Color::rgb(0.5, 0.5, 0.6))
                                    .font_size(11.0)
                                    .child(text("- Overlay layer appears above other windows")),
                            )
                            .child(
                                container()
                                    .text_color(Color::rgb(0.5, 0.5, 0.6))
                                    .font_size(11.0)
                                    .child(text("- Background layer appears below the desktop")),
                            )
                            .child(
                                container()
                                    .text_color(Color::rgb(0.5, 0.5, 0.6))
                                    .font_size(11.0)
                                    .child(text(
                                        "- Exclusive keyboard grabs focus from other apps",
                                    )),
                            ),
                    )
            },
        );

        // Store the surface ID so the buttons can access it
        surface_id_signal.set(Some(surface_id));
    });
}

fn layer_button(
    label: &'static str,
    layer: Layer,
    surface_id_signal: RwSignal<Option<SurfaceId>>,
    current_layer: RwSignal<&'static str>,
) -> Container {
    container()
        .padding([8.0, 12.0])
        .background(Color::rgb(0.25, 0.25, 0.35))
        .corner_radius(6.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
        .on_click(move || {
            if let Some(id) = surface_id_signal.get() {
                let handle = surface_handle(id);
                handle.set_layer(layer);
                current_layer.set(label);
            }
        })
        .text_color(Color::WHITE)
        .font_size(13.0)
        .child(text(label))
}

fn keyboard_button(
    label: &'static str,
    mode: KeyboardInteractivity,
    surface_id_signal: RwSignal<Option<SurfaceId>>,
    current_keyboard: RwSignal<&'static str>,
) -> Container {
    container()
        .padding([8.0, 12.0])
        .background(Color::rgb(0.25, 0.3, 0.35))
        .corner_radius(6.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
        .on_click(move || {
            if let Some(id) = surface_id_signal.get() {
                let handle = surface_handle(id);
                handle.set_keyboard_interactivity(mode);
                current_keyboard.set(label);
            }
        })
        .text_color(Color::WHITE)
        .font_size(13.0)
        .child(text(label))
}
