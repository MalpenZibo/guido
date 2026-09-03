//! Performance stress test with a large scrollable list.
//!
//! This example creates 1000 items, each with:
//! - A toggle button
//! - An info section with reactive text
//! - A text input field
//! - A status indicator
//!
//! Run with: cargo run --example perf_stress_test
//! Run with render stats: cargo run --example perf_stress_test --features render-stats

use guido::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

const INITIAL_ITEM_COUNT: usize = 10000;

struct ItemData {
    enabled: RwSignal<bool>,
    input_value: RwSignal<String>,
}

fn main() {
    App::new().run(|app| {
        // Store item data in a Rc<RefCell<Vec>> since Signal requires Send and ItemData contains !Send signals
        let item_store: Rc<RefCell<Vec<ItemData>>> = Rc::new(RefCell::new(
            (0..INITIAL_ITEM_COUNT)
                .map(|_| ItemData {
                    enabled: create_signal(false),
                    input_value: create_signal(String::new()),
                })
                .collect(),
        ));

        // Signal that tracks the list of item IDs (u64 is Send)
        let item_ids: RwSignal<Vec<u64>> = create_signal((0..INITIAL_ITEM_COUNT as u64).collect());

        let store_for_children = item_store.clone();
        let store_for_rows = item_store.clone();
        let dyn_container_view = container()
            .layout(Flex::column().spacing(10.0))
            .children(keyed(
                move || {
                    let store = store_for_children.borrow();
                    // Read item_ids to track reactivity
                    item_ids
                        .get()
                        .into_iter()
                        .filter(|&id| store.get(id as usize).is_some())
                        .collect::<Vec<_>>()
                },
                |id| *id,
                move |id| {
                    let store = store_for_rows.borrow();
                    let item = store.get(id as usize).expect("row exists in store");
                    create_item_row(item.enabled, item.input_value, id as usize)
                },
            ));

        let store_for_button = item_store.clone();
        let view = container()
            .background(Color::rgb(0.12, 0.12, 0.18))
            .padding(20.0)
            .layout(Flex::column().spacing(20.0))
            .child(
                container().child(
                    text("Performance Stress Test")
                        .font_size(28.0)
                        .color(Color::WHITE),
                ),
            )
            .child(create_add_button(item_ids, store_for_button))
            .child(
                container()
                    .height(300.0)
                    .scroll(Scroll::vertical())
                    .child(dyn_container_view),
            );

        app.add_surface(
            SurfaceConfig::new()
                .width(750)
                .height(500)
                .anchor(Anchor::TOP | Anchor::LEFT)
                .layer(Layer::Top)
                .namespace("perf-stress-test")
                .background_color(Color::rgb(0.12, 0.12, 0.18)),
            move || view,
        );
    });
}

fn get_item_name(id: usize) -> String {
    format!("Item {}", id + 1)
}

fn get_item_description(id: usize) -> String {
    format!("Description for item {}", id + 1)
}

fn create_add_button(
    item_ids: RwSignal<Vec<u64>>,
    item_store: Rc<RefCell<Vec<ItemData>>>,
) -> Container {
    container()
        .padding([8.0, 16.0])
        .background(Color::rgb(0.2, 0.4, 0.6))
        .corners(6.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
        .on_click(move || {
            let id = item_store.borrow().len() as u64;
            item_store.borrow_mut().push(ItemData {
                enabled: create_signal(false),
                input_value: create_signal(String::new()),
            });
            item_ids.update(|ids| ids.push(id));
        })
        .child(text("Add Item").font_size(14.0).color(Color::WHITE))
}

fn create_item_row(
    enabled: RwSignal<bool>,
    input_value: RwSignal<String>,
    index: usize,
) -> Container {
    let name = get_item_name(index);
    let description = get_item_description(index);

    container()
        .padding(15.0)
        .background(Color::rgb(0.18, 0.18, 0.24))
        .corners(8.0)
        .layout(
            Flex::row()
                .spacing(20.0)
                .cross_alignment(CrossAlignment::Center),
        )
        .child(create_toggle_button(enabled))
        .child(create_info_section(name, description, input_value))
        .child(create_text_input_field(input_value))
    // .child(create_status_indicator(enabled))
}

fn create_toggle_button(enabled: RwSignal<bool>) -> Container {
    container()
        .width(100.0)
        .height(30.0) // Fixed dimensions make this a relayout boundary
        .padding([6.0, 12.0])
        .background(move || {
            if enabled.get() {
                Color::rgb(0.2, 0.5, 0.3)
            } else {
                Color::rgb(0.3, 0.3, 0.35)
            }
        })
        .corners(4.0)
        .when_hovered(|s| s.lighter(0.1))
        .when_pressed(|s| s.ripple())
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
            .font_size(12.0)
            .color(Color::WHITE),
        )
}

fn create_info_section(
    name: String,
    description: String,
    input_value: RwSignal<String>,
) -> Container {
    container()
        .width(at_least(200.0))
        .layout(Flex::column().spacing(4.0))
        .child(container().child(text(name).font_size(18.0).color(Color::WHITE)))
        .child(
            container().child(
                text(description)
                    .font_size(14.0)
                    .color(Color::rgb(0.7, 0.7, 0.8)),
            ),
        )
        .child(
            container().child(
                text(move || format!("Input: {}", input_value.get()))
                    .font_size(12.0)
                    .color(Color::rgb(0.6, 0.6, 0.7)),
            ),
        )
}

fn create_text_input_field(input_value: RwSignal<String>) -> Container {
    container()
        .width(200.0)
        .padding(8.0)
        .background(Color::rgb(0.15, 0.15, 0.2))
        .border(1.0, Color::rgb(0.3, 0.3, 0.4))
        .corners(6.0)
        .when_focused(|s| s.border(2.0, Color::rgb(0.4, 0.8, 1.0)))
        .child(
            text_input(input_value)
                .selection_color(Color::rgba(0.4, 0.6, 1.0, 0.4))
                .cursor_color(Color::rgb(0.4, 0.8, 1.0))
                .font_size(14.0)
                .color(Color::WHITE),
        )
}
