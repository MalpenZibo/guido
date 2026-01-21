//! Example demonstrating static and dynamic children.
//!
//! This example shows:
//! - Static children with .child() (not reactive)
//! - Conditional static children with .maybe_child() (NOT reactive - evaluated once)
//! - Dynamic children with .children_dyn() (fully reactive with state preservation)
//! - How to make truly reactive conditional children using .children_dyn()

use guido::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq)]
struct Item {
    id: u64,
    name: String,
    color: Color,
}

fn main() {
    // === Signals for reactive state ===
    let show_optional = create_signal(true);
    let show_optional2 = create_signal(true);
    let items = create_signal(vec![
        Item { id: 1, name: "Item 1".to_string(), color: Color::rgb(0.8, 0.3, 0.3) },
        Item { id: 2, name: "Item 2".to_string(), color: Color::rgb(0.3, 0.8, 0.3) },
        Item { id: 3, name: "Item 3".to_string(), color: Color::rgb(0.3, 0.3, 0.8) },
    ]);

    // Clone signals for closures
    let show_for_toggle = show_optional.clone();
    let show_for_toggle2 = show_optional2.clone();
    let show_for_maybe = show_optional.clone();
    let show_for_text = show_optional.clone();
    let show_for_child_dyn = show_optional.clone();
    let show_for_child_dyn_text = show_optional.clone();
    let show_for_children_dyn = show_optional2.clone();
    let show_for_children_dyn_text = show_optional2.clone();
    let items_for_add = items.clone();
    let items_for_remove = items.clone();
    let items_for_reverse = items.clone();

    let view = container()
        .layout(Flex::column().spacing(12.0))
        .padding(12.0)
        .child(
            // === SECTION 1: Static children example ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(text("1. Static Children (.child)").color(Color::rgb(0.9, 0.9, 1.0)))
                        .child(text("These children are fixed at creation time:").color(Color::WHITE))
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .child(text("Child A").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.2, 0.3, 0.4))
                                        .corner_radius(4.0)
                                        .child(text("Child B").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.4, 0.3, 0.2))
                                        .corner_radius(4.0)
                                        .child(text("Child C").color(Color::WHITE))
                                )
                        )
                )
        )
        .child(
            // === SECTION 2: Conditional static children (NOT REACTIVE) ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.2, 0.15, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("2. .maybe_child() - NOT REACTIVE!")
                                .color(Color::rgb(1.0, 0.9, 0.9))
                        )
                        .child(
                            text("LIMITATION: Evaluated ONCE at creation")
                                .color(Color::rgb(1.0, 0.7, 0.7))
                        )
                        .child(
                            text(move || format!("Signal: {} (but .maybe_child won't react!)", show_for_text.get()))
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(text("Fixed").color(Color::WHITE))
                                // This is evaluated ONCE at creation - won't update!
                                .maybe_child(
                                    if show_for_maybe.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.4, 0.2, 0.2))
                                                .corner_radius(4.0)
                                                .child(text("Frozen").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                )
                        )
                )
        )
        .child(
            // === SECTION 3: Dynamic children (FULLY REACTIVE) ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.2, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("3. Dynamic Children (.children_dyn) - REACTIVE!")
                                .color(Color::rgb(0.9, 1.0, 0.9))
                        )
                        .child(
                            text("These react to signal changes with state preservation")
                                .color(Color::WHITE)
                        )
                        .child(
                            // Control buttons
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.2, 0.4, 0.2))
                                        .corner_radius(4.0)
                                        .ripple()
                                        .on_click(move || {
                                            items_for_add.update(|list: &mut Vec<Item>| {
                                                let id = list.len() as u64 + 1;
                                                list.push(Item {
                                                    id,
                                                    name: format!("Item {}", id),
                                                    color: Color::rgb(
                                                        0.5 + (id as f32 * 0.3) % 0.5,
                                                        0.5 + (id as f32 * 0.5) % 0.5,
                                                        0.5 + (id as f32 * 0.7) % 0.5,
                                                    ),
                                                });
                                            });
                                        })
                                        .child(text("Add").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.4, 0.2, 0.2))
                                        .corner_radius(4.0)
                                        .ripple()
                                        .on_click(move || {
                                            items_for_remove.update(|list: &mut Vec<Item>| {
                                                if !list.is_empty() {
                                                    list.pop();
                                                }
                                            });
                                        })
                                        .child(text("Remove").color(Color::WHITE))
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.2, 0.2, 0.4))
                                        .corner_radius(4.0)
                                        .ripple()
                                        .on_click(move || {
                                            items_for_reverse.update(|list: &mut Vec<Item>| {
                                                list.reverse();
                                            });
                                        })
                                        .child(text("Reverse").color(Color::WHITE))
                                )
                        )
                        .child(
                            text("Notice: Reversing preserves widget state (ripple animations, etc.)")
                                .color(Color::rgb(0.8, 0.8, 0.8))
                        )
                        .child(
                            // Dynamic list with keyed reconciliation
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .children_dyn::<Item, Vec<Item>, (), _>(
                                    move || items.get(),
                                    |item| item.id,  // Key by ID - preserves widget state on reorder!
                                    |item| {
                                        container()
                                            .padding(8.0)
                                            .background(item.color)
                                            .corner_radius(4.0)
                                            .ripple()
                                            .child(text(item.name).color(Color::WHITE))
                                    },
                                )
                        )
                )
        )
        .child(
            // === SECTION 4: Reactive single child with .child_dyn() ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.2, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("4. Reactive Single Child (.child_dyn) - EASY!")
                                .color(Color::rgb(1.0, 0.9, 1.0))
                        )
                        .child(
                            text("Convenience method for single optional child:")
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .ripple()
                                .on_click(move || {
                                    show_for_toggle.update(|v| *v = !*v);
                                })
                                .child(
                                    text(move || {
                                        if show_for_child_dyn_text.get() {
                                            "Click to Hide".to_string()
                                        } else {
                                            "Click to Show".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Use child_dyn for clean reactive optional child
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.25, 0.2, 0.25))
                                        .corner_radius(4.0)
                                        .child(text("Before").color(Color::WHITE))
                                )
                                .child_dyn(move || {
                                    // Simply return Some(widget) or None!
                                    if show_for_child_dyn.get() {
                                        Some(
                                            container()
                                                .padding(8.0)
                                                .background(Color::rgb(0.5, 0.3, 0.5))
                                                .corner_radius(4.0)
                                                .ripple()
                                                .child(text("I'm reactive with .child_dyn()!").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.25, 0.2, 0.25))
                                        .corner_radius(4.0)
                                        .child(text("After").color(Color::WHITE))
                                )
                        )
                )
        )
        .child(
            // === SECTION 5: Truly reactive conditional children (verbose way) ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.18, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("5. Reactive Conditional (using .children_dyn) - Verbose")
                                .color(Color::rgb(0.9, 0.9, 1.0))
                        )
                        .child(
                            text("This is how to make conditional children REACTIVE (verbose):")
                                .color(Color::WHITE)
                        )
                        .child(
                            container()
                                .padding(6.0)
                                .background(Color::rgb(0.3, 0.2, 0.4))
                                .corner_radius(4.0)
                                .ripple()
                                .on_click(move || {
                                    show_for_toggle2.update(|v| *v = !*v);
                                })
                                .child(
                                    text(move || {
                                        if show_for_children_dyn_text.get() {
                                            "Click to Hide".to_string()
                                        } else {
                                            "Click to Show".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Use children_dyn with a list that's empty or has one item
                            container()
                                .layout(Flex::row().spacing(4.0))
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.25, 0.2, 0.25))
                                        .corner_radius(4.0)
                                        .child(text("Before").color(Color::WHITE))
                                )
                                .children_dyn::<&str, Vec<&str>, (), _>(
                                    move || {
                                        if show_for_children_dyn.get() {
                                            // Return a vec with one item
                                            vec!["Reactive! I appear/disappear based on signal!"]
                                        } else {
                                            // Return empty vec
                                            vec![]
                                        }
                                    },
                                    |content| {
                                        // Hash the string for a stable key
                                        let mut hasher = DefaultHasher::new();
                                        content.hash(&mut hasher);
                                        hasher.finish()
                                    },
                                    |content| {
                                        container()
                                            .padding(8.0)
                                            .background(Color::rgb(0.5, 0.3, 0.5))
                                            .corner_radius(4.0)
                                            .ripple()
                                            .child(text(content).color(Color::WHITE))
                                    },
                                )
                                .child(
                                    container()
                                        .padding(6.0)
                                        .background(Color::rgb(0.25, 0.2, 0.25))
                                        .corner_radius(4.0)
                                        .child(text("After").color(Color::WHITE))
                                )
                        )
                )
        );

    App::new()
        .width(900)
        .height(700)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
