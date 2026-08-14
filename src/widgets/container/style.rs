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
    ///
    /// Asks the focus path rather than walking the tree, so it can be called
    /// from a `create_derived` closure — which has no tree, and is where a
    /// container resolves the text colour it publishes to its descendants.
    pub(super) fn has_child_focus(id: WidgetId) -> bool {
        focus_path().contains(id)
    }

    // -----------------------------------------------------------------------
    // State layer: base value plus the override of the active state
    // -----------------------------------------------------------------------

    /// Apply the active state layer's override to `base`, if it declares one.
    /// Priority is pressed > focused > hovered.
    fn resolve_state_value<T: Clone>(
        &self,
        id: WidgetId,
        base: T,
        extractor: impl Fn(&StateStyle) -> Option<T>,
    ) -> T {
        let Some(ref ix) = self.interaction else {
            return base;
        };
        // Ask "does anything declare a state?" before touching the signals.
        // Reading them first would make every interactive container — a bare
        // `on_click`, say — subscribe to its own hover and start repainting on
        // every pointer move, for a state layer it does not have.
        if !ix.has_any_state() {
            return base;
        }

        let flags = ix.flags.get();
        if flags.contains(InteractionFlags::PRESSED)
            && let Some(ref state) = ix.pressed_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if ix.focused_state.is_some()
            && Self::has_child_focus(id)
            && let Some(ref state) = ix.focused_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        if flags.contains(InteractionFlags::HOVERED)
            && let Some(ref state) = ix.hover_state
            && let Some(value) = extractor(state)
        {
            return value;
        }
        base
    }

    /// The background a state layer resolves to: its own colour if it declares
    /// one, its own alpha if it declares one, otherwise the base.
    pub(super) fn effective_background_target(&self, id: WidgetId) -> Color {
        let base = self.background.get_or(Color::TRANSPARENT);
        self.resolve_state_value(id, base, |state| {
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

    pub(super) fn effective_border_width_target(&self, id: WidgetId) -> f32 {
        let base = self.border_width.get_or(0.0);
        self.resolve_state_value(id, base, |state| state.border_width)
    }

    pub(super) fn effective_border_color_target(&self, id: WidgetId) -> Color {
        let base = self.border_color.get_or(Color::TRANSPARENT);
        self.resolve_state_value(id, base, |state| state.border_color)
    }

    pub(super) fn effective_corner_radius_target(&self, id: WidgetId) -> f32 {
        let base = self.corner_radius.get_or(0.0);
        self.resolve_state_value(id, base, |state| state.corner_radius)
    }

    pub(super) fn effective_transform_target(&self, id: WidgetId) -> Transform {
        let base = self.transform.get_or(Transform::IDENTITY);
        self.resolve_state_value(id, base, |state| state.transform)
    }

    /// Elevation is never animated, so the target *is* the drawn value.
    pub(super) fn effective_elevation(&self, id: WidgetId) -> f32 {
        let base = self.elevation.get_or(0.0);
        self.resolve_state_value(id, base, |state| state.elevation)
    }

    // -----------------------------------------------------------------------
    // What descendants are told about text
    // -----------------------------------------------------------------------

    /// Whether any state layer declares a text colour.
    fn has_state_text_color(&self) -> bool {
        self.interaction.as_ref().is_some_and(|ix| {
            [&ix.hover_state, &ix.pressed_state, &ix.focused_state]
                .into_iter()
                .flatten()
                .any(|s| s.text_color.is_some())
        })
    }

    /// The text style this container publishes to its descendants.
    ///
    /// Usually exactly what the builder was handed. When a state layer
    /// declares a text colour, the *colour* published is instead a derived
    /// folding the base and the interaction flags together — so a descendant
    /// that reads it subscribes to this container's hover, and a flip reaches
    /// the glyphs instead of stopping at the box.
    ///
    /// Nothing is created when no state layer mentions text, which is almost
    /// always: the cost of the feature is paid only where it is used.
    pub(super) fn published_text_style(&mut self, tree: &Tree, id: WidgetId) -> Option<TextStyle> {
        let declared = self.text.as_deref().copied();
        if !self.has_state_text_color() {
            return declared;
        }

        let ix = self
            .interaction
            .as_ref()
            .expect("has_state_text_color implies interaction");
        let flags = ix.flags;
        let pressed = ix.pressed_state.as_ref().and_then(|s| s.text_color);
        let focused = ix.focused_state.as_ref().and_then(|s| s.text_color);
        let hovered = ix.hover_state.as_ref().and_then(|s| s.text_color);

        // What a descendant would have inherited without us — this container's
        // own declaration, or the nearest ancestor's. Walked once, here: a
        // derived closure has no tree, and which ancestor declares what cannot
        // change without the subtree being rebuilt, which re-registers and
        // re-walks.
        let base = declared
            .and_then(|s| s.color)
            .or_else(|| tree.inherited_text_style(id).color);

        let (color, owner) = with_owner(|| {
            create_derived(move || {
                let flags = flags.get();
                if flags.contains(InteractionFlags::PRESSED)
                    && let Some(color) = pressed
                {
                    return color;
                }
                // `focused` first: with no focused state there is nothing to
                // resolve, and no reason to subscribe to the focus path.
                if let Some(color) = focused
                    && Self::has_child_focus(id)
                {
                    return color;
                }
                if flags.contains(InteractionFlags::HOVERED)
                    && let Some(color) = hovered
                {
                    return color;
                }
                base.map(|base| base.get()).unwrap_or(Color::WHITE)
            })
        });

        // Re-registration replaces the previous derived; without this the old
        // one would outlive its container.
        self.dispose_text_owner();
        self.text_owner = Some(owner);

        let mut style = declared.unwrap_or_default();
        style.color = Some(color);
        Some(style)
    }

    /// Tear down the owner holding the published derived, if there is one.
    pub(super) fn dispose_text_owner(&mut self) {
        if let Some(owner) = self.text_owner.take() {
            dispose_owner_now(owner);
        }
    }

    // -----------------------------------------------------------------------
    // Animation: the in-flight value when one is running
    // -----------------------------------------------------------------------

    pub(super) fn animated_padding(&self) -> Padding {
        get_animated_value(self.anims.as_ref().and_then(|a| a.padding.as_ref()), || {
            self.padding.get_or(Padding::default())
        })
    }

    pub(super) fn animated_background(&self, id: WidgetId) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.background.as_ref()),
            || self.effective_background_target(id),
        )
    }

    pub(super) fn animated_corner_radius(&self, id: WidgetId) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.corner_radius.as_ref()),
            || self.effective_corner_radius_target(id),
        )
    }

    pub(super) fn animated_border_width(&self, id: WidgetId) -> f32 {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_width.as_ref()),
            || self.effective_border_width_target(id),
        )
    }

    pub(super) fn animated_border_color(&self, id: WidgetId) -> Color {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.border_color.as_ref()),
            || self.effective_border_color_target(id),
        )
    }

    pub(super) fn animated_transform(&self, id: WidgetId) -> Transform {
        get_animated_value(
            self.anims.as_ref().and_then(|a| a.transform.as_ref()),
            || self.effective_transform_target(id),
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
