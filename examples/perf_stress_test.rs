//! Performance stress test with a large scrollable list.
//!
//! This example creates 1000 items, each with:
//! - A toggle button
//! - An info section with reactive text
//! - A text input field
//! - A status indicator
//!
//! Run with: cargo run --example perf_stress_test
//! Run with layout stats: cargo run --example perf_stress_test --features layout-stats

use guido::prelude::*;

const INITIAL_ITEM_COUNT: usize = 1000;

fn main() {
    let items: Signal<Vec<ItemData>> = create_signal(
        (0..INITIAL_ITEM_COUNT)
            .map(|i| ItemData {
                id: i as u64,
                enabled: create_signal(false),
                input_value: create_signal(String::new()),
            })
            .collect(),
    );

    let dyn_container_view = container()
        .layout(Flex::column().spacing(10.0))
        .children(move || {
            items
                .get()
                .iter()
                .map(|item| {
                    let id = item.id;
                    let enabled = item.enabled;
                    let input_value = item.input_value;

                    // Signals are stored in ItemData, so no effects needed.
                    // Changing item.enabled doesn't mark the Vec as changed.
                    (id, move || {
                        create_item_row(enabled, input_value, id as usize)
                    })
                })
                .collect::<Vec<_>>()
        });

    let view = container()
        .background(Color::rgb(0.12, 0.12, 0.18))
        .padding(20.0)
        .layout(Flex::column().spacing(20.0))
        .child(
            text("Performance Stress Test")
                .color(Color::WHITE)
                .font_size(28.0),
        )
        .child(create_add_button(items))
        .child(
            container()
                .height(300.0)
                .scrollable(ScrollAxis::Vertical)
                .child(dyn_container_view),
        );

    let (app, _) = App::new().add_surface(
        SurfaceConfig::new()
            .width(750)
            .height(500)
            .anchor(Anchor::TOP | Anchor::LEFT)
            .layer(Layer::Top)
            .namespace("perf-stress-test")
            .background_color(Color::rgb(0.12, 0.12, 0.18)),
        move |_| view,
    );
    app.run();
}

#[derive(Clone, Copy, PartialEq)]
struct ItemData {
    id: u64,
    enabled: Signal<bool>,
    input_value: Signal<String>,
}

fn get_item_name(id: usize) -> String {
    format!("Item {}", id + 1)
}

fn get_item_description(id: usize) -> String {
    format!("Description for item {}", id + 1)
}

fn create_add_button(items: Signal<Vec<ItemData>>) -> Container {
    container()
        .padding_xy(16.0, 8.0)
        .background(Color::rgb(0.2, 0.4, 0.6))
        .corner_radius(6.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .on_click(move || {
            items.update(|list| {
                let id = list.len() as u64;
                list.push(ItemData {
                    id,
                    enabled: create_signal(false),
                    input_value: create_signal(String::new()),
                });
            });
        })
        .child(text("Add Item").color(Color::WHITE).font_size(14.0))
}

fn create_item_row(enabled: Signal<bool>, input_value: Signal<String>, index: usize) -> Container {
    let name = get_item_name(index);
    let description = get_item_description(index);

    container()
        .padding(15.0)
        .background(Color::rgb(0.18, 0.18, 0.24))
        .corner_radius(8.0)
        .layout(
            Flex::row()
                .spacing(20.0)
                .cross_axis_alignment(CrossAxisAlignment::Center),
        )
        .child(create_toggle_button(enabled))
        .child(create_info_section(name, description, input_value))
        .child(create_text_input_field(input_value))
    // .child(create_status_indicator(enabled))
}

fn create_toggle_button(enabled: Signal<bool>) -> Container {
    container()
        .width(100.0)
        .height(30.0) // Fixed dimensions make this a relayout boundary
        .padding_xy(12.0, 6.0)
        .background(move || {
            if enabled.get() {
                Color::rgb(0.2, 0.5, 0.3)
            } else {
                Color::rgb(0.3, 0.3, 0.35)
            }
        })
        .corner_radius(4.0)
        .hover_state(|s| s.lighter(0.1))
        .pressed_state(|s| s.ripple())
        .on_click(move || {
            enabled.update(|e| *e = !*e);
        })
        .child(
            text(move || {
                if enabled.get() {
                    "Enabled".to_string()
                } else {
                    "Disabled".to_string()
                }
            })
            .color(Color::WHITE)
            .font_size(12.0),
        )
}

fn create_info_section(
    name: String,
    description: String,
    input_value: Signal<String>,
) -> Container {
    container()
        .width(at_least(200.0))
        .layout(Flex::column().spacing(4.0))
        .child(text(name).color(Color::WHITE).font_size(18.0))
        .child(
            text(description)
                .color(Color::rgb(0.7, 0.7, 0.8))
                .font_size(14.0),
        )
        .child(
            text(move || format!("Input: {}", input_value.get()))
                .color(Color::rgb(0.6, 0.6, 0.7))
                .font_size(12.0),
        )
}

fn create_text_input_field(input_value: Signal<String>) -> Container {
    container()
        .width(200.0)
        .padding(8.0)
        .background(Color::rgb(0.15, 0.15, 0.2))
        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
        .corner_radius(6.0)
        .focused_state(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
        .child(
            text_input(input_value)
                .text_color(Color::WHITE)
                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                .font_size(14.0),
        )
}
