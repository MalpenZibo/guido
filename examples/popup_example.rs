//! xdg popups anchored to a bar: real menu semantics.
//!
//! Clicking the button opens a grabbed popup under it — the compositor
//! positions it (flipping/sliding at screen edges) and dismisses it when
//! you click anywhere outside. No fullscreen overlay involved.

use std::cell::RefCell;
use std::rc::Rc;

use guido::prelude::*;

fn menu_entry(label: &str) -> Container {
    container()
        .padding([8.0, 12.0])
        .corner_radius(6.0)
        .when_hovered(|s| s.lighter(0.12))
        .font_size(13)
        .child(text(label))
}

fn main() {
    env_logger::init();

    App::new().run(|app| {
        let button_ref = create_widget_ref();
        let menu_open = create_signal(false);
        let popup_slot: Rc<RefCell<Option<PopupHandle>>> = Rc::new(RefCell::new(None));
        // The bar id is only known after add_surface returns; the click
        // handler runs later, so a slot is enough.
        let bar_id_slot: Rc<RefCell<Option<SurfaceId>>> = Rc::new(RefCell::new(None));
        let bar_id_for_click = bar_id_slot.clone();

        let bar_id = app.add_surface(
            SurfaceConfig::new()
                .height(36)
                .anchor(Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT)
                .background_color(Color::rgb(0.1, 0.1, 0.15)),
            move || {
                let popup_slot = popup_slot.clone();
                container()
                    .width(fill())
                    .layout(
                        Flex::row()
                            .spacing(12.0)
                            .main_alignment(MainAlignment::Center)
                            .cross_alignment(CrossAlignment::Center),
                    )
                    .child(
                        container()
                            .widget_ref(button_ref)
                            .padding([6.0, 16.0])
                            .background(move || {
                                if menu_open.get() {
                                    Color::rgb(0.35, 0.35, 0.5)
                                } else {
                                    Color::rgb(0.25, 0.25, 0.35)
                                }
                            })
                            .corner_radius(6.0)
                            .when_hovered(|s| s.lighter(0.1))
                            .on_click(move || {
                                if menu_open.get() {
                                    // Toggle closed
                                    if let Some(popup) = popup_slot.borrow_mut().take() {
                                        popup.close();
                                    }
                                    return;
                                }

                                let popup = spawn_popup(
                                    bar_id_for_click.borrow().unwrap(),
                                    PopupConfig::new(220)
                                        .anchor_rect(button_ref.rect().get())
                                        .anchor(PopupAnchor::Top)
                                        .gravity(PopupGravity::Top)
                                        .grab()
                                        .background_color(Color::TRANSPARENT),
                                    || {
                                        // Auto-height popup: the content
                                        // wraps, no fill-height here
                                        container()
                                            .width(fill())
                                            .background(Color::rgb(0.13, 0.13, 0.2))
                                            .corner_radius(10.0)
                                            .padding(8.0)
                                            .layout(Flex::column().spacing(2.0))
                                            .child(menu_entry("Profile"))
                                            .child(menu_entry("Settings"))
                                            .child(menu_entry("About"))
                                            .child(menu_entry("Quit"))
                                    },
                                );
                                menu_open.set(true);

                                // Reset state when the compositor dismisses
                                // the popup (outside click) or close() runs
                                create_effect(move || {
                                    if popup.dismissed() {
                                        menu_open.set(false);
                                    }
                                });

                                *popup_slot.borrow_mut() = Some(popup);
                            })
                            .child(text(move || {
                                if menu_open.get() {
                                    "Menu ▾ (click outside to dismiss)".to_string()
                                } else {
                                    "Menu ▸".to_string()
                                }
                            })),
                    )
            },
        );
        *bar_id_slot.borrow_mut() = Some(bar_id);
    });
}
