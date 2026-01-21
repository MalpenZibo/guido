//! Example demonstrating static and dynamic children.
//!
//! This example shows:
//! 1. Static children with .child() - Fixed at creation
//! 2. Conditional static with .maybe_child() - NOT reactive (evaluated once)
//! 3. Dynamic list with .children_dyn() - Fully reactive with keyed reconciliation
//! 4. NEW: Mixing static and dynamic children - Now works in any order!
//! 5. Unified .child() API - Accepts both static widgets and closures

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
    let show_for_unified = show_optional.clone();
    let show_for_unified_text = show_optional.clone();
    let show_for_child_dyn = show_optional.clone();
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
            // === SECTION 4: NEW! Mixing static and dynamic children ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.15, 0.25, 0.15))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("4. NEW! Mixing Static and Dynamic - ANY ORDER!")
                                .color(Color::rgb(0.9, 1.0, 0.9))
                        )
                        .child(
                            text("You can now freely mix static and dynamic children!")
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
                                        if show_for_unified_text.get() {
                                            "Click to Hide Middle".to_string()
                                        } else {
                                            "Click to Show Middle".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Demonstrate mixing: static -> dynamic -> static
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.4, 0.3))
                                        .corner_radius(4.0)
                                        .child(text("Static Header").color(Color::WHITE))
                                )
                                .child(
                                    // Dynamic child in the middle!
                                    move || {
                                        if show_for_unified.get() {
                                            Some(
                                                container()
                                                    .padding(8.0)
                                                    .background(Color::rgb(0.5, 0.3, 0.5))
                                                    .corner_radius(4.0)
                                                    .ripple()
                                                    .child(text("Dynamic Middle!").color(Color::WHITE))
                                            )
                                        } else {
                                            None
                                        }
                                    }
                                )
                                .child(
                                    container()
                                        .padding(8.0)
                                        .background(Color::rgb(0.3, 0.4, 0.3))
                                        .corner_radius(4.0)
                                        .child(text("Static Footer").color(Color::WHITE))
                                )
                        )
                        .child(
                            text("This was IMPOSSIBLE before - would panic!")
                                .color(Color::rgb(0.8, 1.0, 0.8))
                        )
                )
        )
        .child(
            // === SECTION 5: Complex mixing example ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.18, 0.15, 0.25))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("5. Complex Mixing Example")
                                .color(Color::rgb(1.0, 0.9, 1.0))
                        )
                        .child(
                            text("Multiple static and dynamic children in any order:")
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
                                            "Click to Hide Dynamics".to_string()
                                        } else {
                                            "Click to Show Dynamics".to_string()
                                        }
                                    })
                                    .color(Color::WHITE)
                                )
                        )
                        .child(
                            // Complex pattern: S D S D S
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(text("Static 1").color(Color::WHITE))
                                .child(move || {
                                    if show_for_children_dyn.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.5, 0.2, 0.3))
                                                .corner_radius(4.0)
                                                .child(text("Dynamic 1").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .child(text("Static 2").color(Color::WHITE))
                                .child(move || {
                                    if show_for_child_dyn.get() {
                                        Some(
                                            container()
                                                .padding(6.0)
                                                .background(Color::rgb(0.3, 0.2, 0.5))
                                                .corner_radius(4.0)
                                                .child(text("Dynamic 2").color(Color::WHITE))
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .child(text("Static 3").color(Color::WHITE))
                        )
                )
        )
        .child(
            // === SECTION 6: .children_dyn() still works for keyed lists ===
            container()
                .padding(12.0)
                .background(Color::rgb(0.18, 0.15, 0.2))
                .corner_radius(8.0)
                .child(
                    container()
                        .layout(Flex::column().spacing(8.0))
                        .child(
                            text("6. .children_dyn() - For keyed lists")
                                .color(Color::rgb(0.9, 0.9, 1.0))
                        )
                        .child(
                            text("Use this for lists that need state preservation:")
                                .color(Color::WHITE)
                        )
                        .child(
                            // Can even mix static before keyed list!
                            container()
                                .layout(Flex::column().spacing(4.0))
                                .child(text("Static header before list").color(Color::rgb(0.8, 0.8, 0.8)))
                                .children_dyn::<&str, Vec<&str>, (), _>(
                                    move || vec!["Keyed Item 1", "Keyed Item 2"],
                                    |content| {
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
                                .child(text("Static footer after list").color(Color::rgb(0.8, 0.8, 0.8)))
                        )
                )
        );

    App::new()
        .width(900)
        .height(700)
        .background_color(Color::rgb(0.1, 0.1, 0.15))
        .run(view);
}
