//! One bar per connected monitor, reacting to hotplug.
//!
//! The app starts with zero surfaces: an effect reads the reactive
//! `outputs()` list and spawns a bar pinned to each monitor via
//! `SurfaceConfig::output`. Plugging a monitor in creates its bar,
//! unplugging it removes it (the compositor closes the surface; the
//! effect also drops its handle).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use guido::prelude::*;

fn main() {
    env_logger::init();

    App::new().run(|_app| {
        let bars: Rc<RefCell<HashMap<OutputId, SurfaceHandle>>> =
            Rc::new(RefCell::new(HashMap::new()));

        create_effect(move || {
            let current = outputs().get();
            let mut bars = bars.borrow_mut();

            // Close bars whose output disappeared (usually already closed by
            // the compositor — this just keeps the map tidy)
            bars.retain(|id, handle| {
                let alive = current.iter().any(|o| o.id == *id);
                if !alive {
                    handle.close();
                }
                alive
            });

            // Spawn a bar on every newly connected output
            for info in current {
                if bars.contains_key(&info.id) {
                    continue;
                }

                let label = info
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("output-{}", info.id.raw()));
                let model = info.model.clone();
                let surface_id = Rc::new(RefCell::new(None::<SurfaceId>));
                let surface_id_for_widget = surface_id.clone();

                let handle = spawn_surface(
                    SurfaceConfig::new()
                        .height(32)
                        .anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT)
                        .exclusive_zone(ExclusiveZone::Auto)
                        .layer(Layer::Top)
                        .namespace("guido-multi-output")
                        .output(info.id),
                    move || {
                        let my_id = surface_id_for_widget.borrow().unwrap();
                        container()
                            .layout(
                                Flex::row()
                                    .spacing(8.0)
                                    .cross_alignment(CrossAlignment::Center)
                                    .main_alignment(MainAlignment::SpaceBetween),
                            )
                            .padding(8.0)
                            .child(text(format!("{label} — {model}")))
                            .child(text(move || {
                                // Reactive reads: re-renders on hotplug and
                                // when the compositor maps the surface
                                let total = outputs().get().len();
                                match surface_output(my_id) {
                                    Some(out) => {
                                        format!("on output {} of {total}", out.raw())
                                    }
                                    None => format!("{total} output(s)"),
                                }
                            }))
                    },
                );
                *surface_id.borrow_mut() = Some(handle.id());
                bars.insert(info.id, handle);
            }
        });
    });
}
