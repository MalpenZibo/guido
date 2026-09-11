//! Compositor-side background blur (`ext-background-effect-v1`).
//!
//! Containers opt in via [`backdrop_blur`](crate::widgets::Container::backdrop_blur),
//! which emits one `DrawCommand::BackdropBlur` carrying the sources it asked
//! for. The renderer filters the surface's own backdrop from that command; this
//! module reads the compositor's region off the same one, after the frame has
//! been flattened, through the tessellation in [`crate::region`] — `wl_region`
//! has no notion of curves.
//!
//! Nothing is remembered between frames: see [`regions_from_commands`] for why
//! that is the whole point. No-ops when the compositor does not support the
//! protocol or its blur capability.

use crate::backdrop::BackdropSources;
use crate::region::{RegionRect, rounded_rect_to_rects};
use crate::renderer::{DrawCommand, FlattenedCommand};

/// The region to hand `ext-background-effect-v1`, read off the frame that was
/// just drawn.
///
/// **Derived, never remembered.** An earlier version kept a registry that
/// containers wrote to during paint, and every way a container can *stop*
/// painting — hidden, culled, outside a scroller's window, satisfied from the
/// paint cache — was a way for that registry to disagree with the screen. Each
/// one had to be found and told, and one of them was always missing.
///
/// The flattened command list is the frame. A container that did not paint has
/// no command in it; one served from the paint cache carries its command along
/// unchanged; a culled one is absent. Ordering stops mattering too, since the
/// list only exists after the paint that built it.
pub(crate) fn regions_from_commands(commands: &[FlattenedCommand]) -> Vec<RegionRect> {
    let mut out = Vec::new();
    for cmd in commands {
        let DrawCommand::BackdropBlur {
            rect,
            sources,
            corner_radii,
            ..
        } = &*cmd.command
        else {
            continue;
        };
        if !sources.contains(BackdropSources::COMPOSITOR) {
            continue;
        }

        // World coordinates *and* the clip, by the same computation the renderer
        // uses for the surface half of this very command — so the two cannot
        // describe different shapes. A card half out of a viewport is filtered
        // only where it is on show, and a region published for the whole card
        // blurs the desktop beside a panel that is not there.
        let Some((world, world_radii)) = cmd.clipped_world_rounded_rect(*rect, *corner_radii)
        else {
            continue;
        };
        out.extend(rounded_rect_to_rects(world, world_radii));
    }
    out.sort_unstable_by_key(|r| (r.y, r.x, r.width, r.height));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::CornerRadii;
    use crate::widgets::Rect;

    #[test]
    fn a_compositor_region_does_not_need_a_radius() {
        let command = FlattenedCommand {
            command: std::rc::Rc::new(DrawCommand::BackdropBlur {
                rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                sources: BackdropSources::COMPOSITOR,
                radius: 0.0,
                corner_radii: CornerRadii::uniform(0.0),
                curvature: Default::default(),
            }),
            world_transform: Default::default(),
            world_transform_origin: None,
            layer: Default::default(),
            clip: None,
            clip_is_local: false,
        };
        assert_eq!(
            regions_from_commands(&[command]),
            vec![RegionRect {
                x: 0,
                y: 0,
                width: 100,
                height: 50
            }]
        );
    }
}
