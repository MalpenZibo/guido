//! Resolving a container's styled values.
//!
//! Every visual property of a container exists at three levels, and this
//! module is the only place that knows how they combine:
//!
//! 1. the **base** value the builder was given (a signal, possibly a closure);
//! 2. the **state layer** override that applies while the container is
//!    pressed, focused or hovered — pressed wins, then focused, then hovered;
//! 3. the **animation** currently interpolating toward that value.
//!
//! The two families below correspond to the last two steps. `effective_*`
//! answers "what should this property be right now" — the *target*, base plus
//! state layer, which is what an animation is asked to animate toward.
//! `animated_*` answers "what should be drawn right now" — the in-flight value
//! when an animation exists, the target otherwise.
//!
//! Layout and paint both read through these, which is why they live apart from
//! either: a state-layer change has to move the same value whether it lands in
//! a size or in a colour.

use super::*;

impl Container {
    // -----------------------------------------------------------------------
    // Focus, as seen by the focused state layer
    // -----------------------------------------------------------------------

    /// Whether the focused widget is this container or one of its descendants.
    pub(super) fn has_child_focus(&self, tree: &Tree) -> bool {
        match focused_widget() {
            Some(focused_id) => self.widget_has_focus(tree, focused_id),
            None => false,
        }
    }

    pub(super) fn widget_has_focus(&self, tree: &Tree, focused_id: WidgetId) -> bool {
        for &child_id in self.children_source.get() {
            if child_id == focused_id {
                return true;
            }
            if tree.with_widget(child_id, |child| {
                child.has_focus_descendant(tree, focused_id)
            }) == Some(true)
            {
                return true;
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // State layer: base value plus the override of the active state
    // -----------------------------------------------------------------------

    /// Apply the active state layer's override to `base`, if it declares one.
    /// Priority is pressed > focused > hovered.
    fn resolve_state_value<T: Clone>(
        &self,
        tree: &Tree,
        base: T,
        extractor: impl Fn(&StateStyle) -> Option<T>,
    ) -> T {
        let Some(ref ix) = self.interaction else {
            return base;
        };
        if ix.is_pressed
            && let Some(ref state) = ix.pressed_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if ix.focused_state.is_some()
            && self.has_child_focus(tree)
            && let Some(ref state) = ix.focused_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if ix.is_hovered
            && let Some(ref state) = ix.hover_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        base
    }

    /// The background a state layer resolves to: its own colour if it declares
    /// one, its own alpha if it declares one, otherwise the base.
    pub(super) fn effective_background_target(&self, tree: &Tree) -> Color {
        let base = self.background.get_or(Color::TRANSPARENT);
        self.resolve_state_value(tree, base, |state| {
            let bg_color = state
                .background
                .as_ref()
                .map(|bg| resolve_background(base, bg));
            match (bg_color, state.alpha) {
                (Some(mut c), Some(a)) => {
                    c.a = a;
                    Some(c)
                }
                (Some(c), None) => Some(c),
                (None, Some(a)) => {
                    let mut c = base;
                    c.a = a;
                    Some(c)
                }
                (None, None) => None,
            }
        })
    }

    pub(super) fn effective_border_width_target(&self, tree: &Tree) -> f32 {
        let base = self.border_width.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.border_width)
    }

    pub(super) fn effective_border_color_target(&self, tree: &Tree) -> Color {
        let base = self.border_color.get_or(Color::TRANSPARENT);
        self.resolve_state_value(tree, base, |state| state.border_color)
    }

    pub(super) fn effective_corner_radius_target(&self, tree: &Tree) -> f32 {
        let base = self.corner_radius.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.corner_radius)
    }

    pub(super) fn effective_transform_target(&self, tree: &Tree) -> Transform {
        let base = self.transform.get_or(Transform::IDENTITY);
        self.resolve_state_value(tree, base, |state| state.transform)
    }

    /// Elevation is never animated, so the target *is* the drawn value.
    pub(super) fn effective_elevation(&self, tree: &Tree) -> f32 {
        let base = self.elevation.get_or(0.0);
        self.resolve_state_value(tree, base, |state| state.elevation)
    }

    // -----------------------------------------------------------------------
    // Animation: the in-flight value when one is running
    // -----------------------------------------------------------------------

    pub(super) fn animated_padding(&self) -> Padding {
        get_animated_value(self.anims.as_ref().and_then(|a| a.padding.as_ref()), || {
            self.padding.get_or(Padding::default())
        })
    }

    pub(super) fn animated_background(&self, tree: &Tree) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.background.as_ref()),
            || self.effective_background_target(tree),
        )
    }

    pub(super) fn animated_corner_radius(&self, tree: &Tree) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.corner_radius.as_ref()),
            || self.effective_corner_radius_target(tree),
        )
    }

    pub(super) fn animated_border_width(&self, tree: &Tree) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_width.as_ref()),
            || self.effective_border_width_target(tree),
        )
    }

    pub(super) fn animated_border_color(&self, tree: &Tree) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_color.as_ref()),
            || self.effective_border_color_target(tree),
        )
    }

    pub(super) fn animated_transform(&self, tree: &Tree) -> Transform {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.transform.as_ref()),
            || self.effective_transform_target(tree),
        )
    }

    /// Whether a state-layer change can move an animated property — i.e.
    /// whether hovering or pressing needs an Animation job rather than a plain
    /// repaint.
    pub(super) fn has_animated_state_properties(&self) -> bool {
        self.anims.as_ref().is_some_and(|a| {
            a.background.is_some()
                || a.corner_radius.is_some()
                || a.border_color.is_some()
                || a.transform.is_some()
        })
    }

    /// Whether any animation follows a signal-backed target, as opposed to
    /// width/height whose targets are content-driven and recomputed at every
    /// layout. These are the ones holding a copy of signal state, so they are
    /// the ones needing the paint-time target re-sync.
    pub(super) fn has_signal_animated_props(&self) -> bool {
        self.anims.as_ref().is_some_and(|a| {
            a.padding.is_some()
                || a.border_width.is_some()
                || a.background.is_some()
                || a.corner_radius.is_some()
                || a.border_color.is_some()
                || a.transform.is_some()
        })
    }
}

/// The container's own surface, resolved for one frame.
pub(super) struct Decoration {
    pub background: Color,
    pub corner_radii: crate::renderer::CornerRadii,
    pub corner_curvature: f32,
    pub elevation: f32,
    pub border_width: f32,
    pub border_color: Color,
}

impl Container {
    /// Draw the container's own surface: background or gradient, its shadow,
    /// and the border frame. Children are painted separately, after this.
    ///
    /// `bounds` is local — the origin is the container itself, and the parent
    /// has already positioned the node.
    pub(super) fn paint_decoration(&self, ctx: &mut PaintContext, bounds: Rect, d: &Decoration) {
        // A gradient replaces the solid fill rather than layering over it.
        if let Some(ref gradient) = self.gradient {
            ctx.draw_gradient_rect(
                bounds,
                crate::renderer::Gradient {
                    start_color: gradient.start_color,
                    end_color: gradient.end_color,
                    direction: gradient.direction.into(),
                },
                d.corner_radii,
                d.corner_curvature,
            );
        } else if d.background.a > 0.0 {
            if d.elevation > 0.0 {
                ctx.draw_rounded_rect_with_shadow(
                    bounds,
                    d.background,
                    d.corner_radii,
                    d.corner_curvature,
                    elevation_to_shadow(d.elevation),
                );
            } else {
                ctx.draw_rounded_rect_with_curvature(
                    bounds,
                    d.background,
                    d.corner_radii,
                    d.corner_curvature,
                );
            }
        }

        if d.border_width > 0.0 {
            ctx.draw_border_frame_with_curvature(
                bounds,
                d.border_color,
                d.corner_radii,
                d.border_width,
                d.corner_curvature,
            );
        }
    }
}

/// Convert an elevation level to the shadow that expresses it.
///
/// Material-style: the higher the surface sits, the further the shadow falls
/// and the softer it gets. Levels 1–5 are tabulated; above that the numbers
/// keep growing on the same curve, up to a ceiling.
pub(super) fn elevation_to_shadow(level: f32) -> Shadow {
    if level <= 0.0 {
        return Shadow::none();
    }

    let (offset_y, blur, alpha) = match level as i32 {
        1 => (1.0, 3.0, 0.12),
        2 => (2.0, 4.0, 0.16),
        3 => (3.0, 6.0, 0.19),
        4 => (4.0, 8.0, 0.20),
        5 => (6.0, 10.0, 0.22),
        _ => {
            let offset = (level * 1.2).min(12.0);
            let blur = (level * 2.0).min(24.0);
            let alpha = (0.12 + level * 0.02).min(0.25);
            (offset, blur, alpha)
        }
    };

    Shadow::new(
        (0.0, offset_y),
        blur,
        0.0,
        Color::rgba(0.0, 0.0, 0.0, alpha),
    )
}
