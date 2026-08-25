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

        // Backwards: the last layer declared wins. A layer that says nothing
        // about *this* property is passed over rather than ending the search,
        // so `when_hovered(|s| s.lighter(0.1))` still lightens under a pressed
        // layer that only scales.
        for (when, state) in ix.states.iter().rev() {
            let Some(value) = extractor(state) else {
                continue;
            };
            if ix.is_active(id, when) {
                return value;
            }
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
            match (bg_color, state.alpha.map(|s| s.get())) {
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
        self.resolve_state_value(id, base, |state| state.border.map(|b| b.width.get()))
    }

    pub(super) fn effective_border_color_target(&self, id: WidgetId) -> Color {
        let base = self.border_color.get_or(Color::TRANSPARENT);
        self.resolve_state_value(id, base, |state| state.border.map(|b| b.color.get()))
    }

    pub(super) fn effective_corners_target(&self, id: WidgetId) -> crate::widgets::Corners {
        let base = self.corners.get_or(crate::widgets::Corners::SQUARE);
        self.resolve_state_value(id, base, |state| state.corners.map(|s| s.get()))
    }

    pub(super) fn effective_translate_target(&self, id: WidgetId) -> Translate {
        let base = self.translate.get_or(Translate::NONE);
        self.resolve_state_value(id, base, |state| state.translate.map(|s| s.get()))
    }

    pub(super) fn effective_rotate_target(&self, id: WidgetId) -> f32 {
        let base = self.rotate.get_or(0.0);
        self.resolve_state_value(id, base, |state| state.rotate.map(|s| s.get()))
    }

    pub(super) fn effective_scale_target(&self, id: WidgetId) -> Scale {
        let base = self.scale.get_or(Scale::NONE);
        self.resolve_state_value(id, base, |state| state.scale.map(|s| s.get()))
    }

    /// The largest elevation this container can reach, and the number its damage
    /// rect is sized from.
    ///
    /// Damage bounds want the worst case, not the current value. Elevation is a
    /// paint-only animation, so a hover that lifts a card never re-runs layout —
    /// and layout is where the shadow's reach is recorded. Sizing the reach to
    /// what is possible rather than to what is showing keeps the damage rect
    /// correct without asking a colour change to re-run a layout.
    ///
    /// Three things can be showing, so all three are folded:
    ///
    /// - the declared elevation, and every state layer's,
    /// - the overshoot, because a spring does not stop at its target,
    /// - **and whatever is in flight**, because a *falling* elevation is drawn
    ///   from a value the declarations no longer mention. `.elevation(move ||
    ///   if lifted.get() { 8.0 } else { 0.0 })` re-runs this layout on the write
    ///   that starts the descent, and at that instant the declared maximum is
    ///   already 0 while the shadow on screen is still 8 deep.
    ///
    /// The whole fold is read under layout tracking, which is the cost to know
    /// about: every elevation *any* state layer declares is a layout dependency,
    /// active or not, because the maximum genuinely moves when one of them
    /// changes. `.elevation(0.0).when_hovered(|s| s.elevation(8.0))` — both
    /// constants — subscribes to nothing and hovering moves only the paint;
    /// `when_hovered(|s| s.elevation(lift))` with `lift` a signal re-lays out the
    /// subtree whenever `lift` is written, pressed or not.
    pub(super) fn max_elevation(&self) -> f32 {
        let base = self.elevation.get_or(0.0);
        let anim = self.anims.as_ref().and_then(|a| a.elevation.as_ref());
        let declared = match self.interaction {
            Some(ref ix) => ix.states.iter().fold(base, |most, (_, state)| {
                match state.elevation.map(|s| s.get()) {
                    Some(level) => most.max(level),
                    None => most,
                }
            }),
            None => base,
        };

        match anim {
            // The value in flight is already past its overshoot, so it is folded
            // in flat; the declarations have theirs still to come.
            Some(anim) => (declared * (1.0 + anim.peak_overshoot())).max(*anim.current()),
            None => declared,
        }
    }

    pub(super) fn effective_elevation_target(&self, id: WidgetId) -> f32 {
        let base = self.elevation.get_or(0.0);
        self.resolve_state_value(id, base, |state| state.elevation.map(|s| s.get()))
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

    pub(super) fn animated_corners(&self, id: WidgetId) -> crate::widgets::Corners {
        get_animated_value(self.anims.as_ref().and_then(|a| a.corners.as_ref()), || {
            self.effective_corners_target(id)
        })
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

    /// The elevation to draw, never deeper than the rect the layout reserved.
    ///
    /// Clamped, and clamped to [`elevation_reach`](super::Container::elevation_reach)
    /// — the number `layout` recorded — rather than to a fresh
    /// [`max_elevation`](Self::max_elevation). Recomputing it here reads the
    /// signals at *paint* time, which is a different frame's answer: on a falling
    /// elevation the declared maximum has already reached 0 and the shadow was
    /// cut to nothing while the animation was still playing.
    ///
    /// A clamp at all, rather than a wider reach, because the reach that would
    /// make one unnecessary is not a small one. A spring keeps its momentum
    /// across a retarget, so hover flicker *pumps* it: driven at its damped
    /// natural frequency the steady-state excursion is the resonant gain,
    /// `1 / (2ζ√(1-ζ²))` — 1.03x the step response for `BOUNCY`, but 3.7x at
    /// ζ = 0.05, and sizing every damage rect for that is not a trade worth
    /// making for the tip of a bounce nobody asked for. See
    /// `hover_flicker_cannot_push_a_shadow_outside_its_damage_rect`.
    pub(super) fn animated_elevation(&self, id: WidgetId) -> f32 {
        let anim = self.anims.as_ref().and_then(|a| a.elevation.as_ref());
        let level = get_animated_value(anim, || self.effective_elevation_target(id));
        match anim {
            Some(_) => level.min(self.elevation_reach.get()),
            None => level,
        }
    }

    /// The three declared components, each at whatever its own animation has
    /// reached, composed into the matrix the renderer and the hit test share.
    ///
    /// This is the only place the two forms meet. Everything above it says
    /// `translate`, `rotate`, `scale`; everything below it takes a matrix and
    /// never has to ask how the matrix was arrived at.
    pub(super) fn animated_transform(&self, id: WidgetId) -> Transform {
        let anims = self.anims.as_ref();
        Transform::compose(
            get_animated_value(anims.and_then(|a| a.translate.as_ref()), || {
                self.effective_translate_target(id)
            }),
            get_animated_value(anims.and_then(|a| a.rotate.as_ref()), || {
                self.effective_rotate_target(id)
            }),
            get_animated_value(anims.and_then(|a| a.scale.as_ref()), || {
                self.effective_scale_target(id)
            }),
        )
    }

    /// Whether a state-layer change can move an animated property — i.e.
    /// whether hovering or pressing needs an Animation job rather than a plain
    /// repaint.
    ///
    /// `border_width` belongs here now that a state layer's border is declared
    /// as a pair: `when_hovered(|s| s.border(14.0, GREEN))` moves the width as
    /// well as the colour, and without it the enter queued no Animation job at
    /// all. The spring then started a frame late, and only if something else had
    /// forced that paint.
    pub(super) fn has_animated_state_properties(&self) -> bool {
        self.anims.as_ref().is_some_and(|a| {
            a.background.is_some()
                || a.corners.is_some()
                || a.elevation.is_some()
                || a.border_width.is_some()
                || a.border_color.is_some()
                || a.translate.is_some()
                || a.rotate.is_some()
                || a.scale.is_some()
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
                || a.corners.is_some()
                || a.elevation.is_some()
                || a.border_color.is_some()
                || a.translate.is_some()
                || a.rotate.is_some()
                || a.scale.is_some()
        })
    }
}

/// The container's own surface, resolved for one frame.
pub(super) struct Decoration {
    pub background: Color,
    pub gradient: Option<LinearGradient>,
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
        // A gradient replaces the solid fill rather than layering over it — but
        // not the shadow, which belongs to the box rather than to either fill.
        // Both are reactive, so which of the two branches a container takes can
        // change between frames; a gradient that dropped the shadow meant an
        // elevation animation stopped drawing halfway through while still asking
        // for a frame at every step.
        //
        // To the box *it draws*, though: the shadow rides whichever fill runs,
        // so a container with neither — no gradient, and a background that is
        // transparent or has animated out — casts none, and `paint_overflow`
        // goes on reserving the room. A shadow with nothing above it is a smear
        // rather than a lift, so that is the behaviour; it is not a
        // free-standing command waiting for a box to belong to.
        // Not `elevation > 0.0`: the first frames of a lift are at ~0.001, where
        // the shadow's alpha rounds to nothing and every frame would still push a
        // rect carrying it. The same gate the border gets, on the thing that is
        // actually drawn rather than on the level that asked for it.
        let shadow = elevation_to_shadow(d.elevation);
        let shadow = (shadow.color.a > SHADOW_ALPHA_FLOOR).then_some(shadow);

        // Gated like the fill, the border and the shadow: a gradient between two
        // fully transparent colours draws nothing, and with both endpoints
        // reactive one animating out through transparent would push a rect per
        // frame to do it. One end still visible is still a gradient.
        let gradient = d
            .gradient
            .filter(|g| g.start_color.a > 0.0 || g.end_color.a > 0.0);
        if let Some(ref gradient) = gradient {
            ctx.draw_rounded_rect_full(
                bounds,
                gradient.start_color,
                d.corner_radii,
                d.corner_curvature,
                None,
                shadow,
                Some(crate::renderer::Gradient {
                    start_color: gradient.start_color,
                    end_color: gradient.end_color,
                    direction: gradient.direction.into(),
                }),
            );
        } else if d.background.a > 0.0 {
            match shadow {
                Some(shadow) => ctx.draw_rounded_rect_with_shadow(
                    bounds,
                    d.background,
                    d.corner_radii,
                    d.corner_curvature,
                    shadow,
                ),
                None => ctx.draw_rounded_rect_with_curvature(
                    bounds,
                    d.background,
                    d.corner_radii,
                    d.corner_curvature,
                ),
            }
        }

        // A border that resolves to nothing visible must not reach the instance
        // buffer every frame. Both halves are always declared together, so this
        // is only reached deliberately — `border(2.0, TRANSPARENT)`,
        // `border(0.0, RED)` — or in passing, while an animated colour crosses
        // transparent.
        if d.border_width > 0.0 && d.border_color.a > 0.0 {
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

/// Below this the shadow is not a faint shadow, it is nothing — and a rect
/// carrying it is a rect drawn for no reason, once per frame for as long as an
/// elevation animation is leaving zero.
const SHADOW_ALPHA_FLOOR: f32 = 0.004;

/// The tabulated Material steps, `level => (offset_y, blur, alpha)`.
///
/// Level 0 is in the table so the interpolation below has somewhere to come
/// from, and the last entry meets the formula that continues past it exactly:
/// at 5, `level * 1.2`, `level * 2.0` and `0.12 + level * 0.02` are 6.0, 10.0
/// and 0.22.
const ELEVATION_STEPS: [(f32, f32, f32); 6] = [
    (0.0, 0.0, 0.0),
    (1.0, 3.0, 0.12),
    (2.0, 4.0, 0.16),
    (3.0, 6.0, 0.19),
    (4.0, 8.0, 0.20),
    (6.0, 10.0, 0.22),
];

/// Convert an elevation level to the shadow that expresses it.
///
/// Material-style: the higher the surface sits, the further the shadow falls
/// and the softer it gets. Levels 0–5 are tabulated; above that the numbers
/// keep growing on the same curve, up to a ceiling.
///
/// A fractional level interpolates between the two steps around it. Reading the
/// table with `level as i32` was fine while elevation could not be animated —
/// it always arrived as one of the six integers. Now that it can, truncation
/// made the shadow a staircase between 1 and 5, and worse, discontinuous where
/// the table met the formula: 0.999 fell through to the formula for
/// (1.199, 1.998, 0.140) while 1.0 read the table for (1.0, 3.0, 0.12), so
/// crossing 1 dropped the offset and the alpha while jumping the blur.
///
/// A level that is not a number is no shadow. `.elevation(f32::NAN)` is
/// writable, and NaN fails every comparison on the way in, so it reached the
/// interpolating branch and came back out as a NaN extent — into
/// `set_paint_overflow`, where it disables every `min` and `max` downstream
/// without anything failing. The same guard `SpringConfig::peak_overshoot` has,
/// for the same reason: a number that sizes a rect has to be one.
pub(super) fn elevation_to_shadow(level: f32) -> Shadow {
    if level.is_nan() || level <= 0.0 {
        return Shadow::none();
    }

    let last = (ELEVATION_STEPS.len() - 1) as f32;
    let (offset_y, blur, alpha) = if level >= last {
        (
            (level * 1.2).min(12.0),
            (level * 2.0).min(24.0),
            (0.12 + level * 0.02).min(0.25),
        )
    } else {
        let lower = level.floor();
        let t = level - lower;
        let (o0, b0, a0) = ELEVATION_STEPS[lower as usize];
        let (o1, b1, a1) = ELEVATION_STEPS[lower as usize + 1];
        (o0 + (o1 - o0) * t, b0 + (b1 - b0) * t, a0 + (a1 - a0) * t)
    };

    Shadow::new(
        (0.0, offset_y),
        blur,
        0.0,
        Color::rgba(0.0, 0.0, 0.0, alpha),
    )
}

#[cfg(test)]
mod elevation_tests {
    use super::*;

    fn parts(level: f32) -> (f32, f32, f32) {
        let s = elevation_to_shadow(level);
        (s.offset.1, s.blur, s.color.a)
    }

    /// Whole levels keep the Material numbers they always had, which is every
    /// elevation the examples and snapshots use.
    ///
    /// A *fractional* static elevation does move: `0.5` now interpolates from
    /// nothing towards level 1 rather than falling through to the formula, so
    /// its shadow is lighter and tighter than before. That is the point of
    /// interpolating, and it is what makes an animation pass through the
    /// fractions smoothly, but it is a change and this says so rather than
    /// claiming nothing moved.
    #[test]
    fn the_tabulated_levels_are_unchanged() {
        assert_eq!(parts(1.0), (1.0, 3.0, 0.12));
        assert_eq!(parts(2.0), (2.0, 4.0, 0.16));
        assert_eq!(parts(3.0), (3.0, 6.0, 0.19));
        assert_eq!(parts(4.0), (4.0, 8.0, 0.20));
        assert_eq!(parts(5.0), (6.0, 10.0, 0.22));
    }

    /// An animated elevation sweeps through the fractions, so every component
    /// has to grow without ever going backwards. Reading the table with
    /// `level as i32` failed this twice over: flat between whole levels, and
    /// non-monotonic across 1, where the formula's (1.199, 1.998, 0.140) met
    /// the table's (1.0, 3.0, 0.12).
    #[test]
    fn a_fractional_level_never_goes_backwards() {
        let mut previous = parts(0.001);
        let mut moved = 0;
        for step in 2..=8000 {
            let level = step as f32 * 0.001;
            let current = parts(level);
            assert!(
                current.0 >= previous.0 - 1e-4
                    && current.1 >= previous.1 - 1e-4
                    && current.2 >= previous.2 - 1e-4,
                "level {level} went backwards: {previous:?} -> {current:?}"
            );
            if current != previous {
                moved += 1;
            }
            previous = current;
        }
        assert!(
            moved > 7000,
            "the shadow has to actually move with the level, moved on {moved} of 7999 steps"
        );
    }

    /// The table hands over to the formula without a step.
    #[test]
    fn the_table_meets_the_formula_at_five() {
        let below = parts(5.0 - 1e-4);
        let above = parts(5.0 + 1e-4);
        assert!((below.0 - above.0).abs() < 1e-2);
        assert!((below.1 - above.1).abs() < 1e-2);
        assert!((below.2 - above.2).abs() < 1e-3);
    }

    /// A level that is not a number is no shadow either. NaN fails every
    /// comparison on the way in, so it used to reach the interpolating branch and
    /// come back out as a NaN extent — into `set_paint_overflow`, where it
    /// disables every `min` and `max` downstream without anything failing.
    #[test]
    fn a_level_that_is_not_a_number_is_no_shadow() {
        let s = elevation_to_shadow(f32::NAN);
        assert_eq!(s.color, Color::TRANSPARENT);
        assert!(!s.extent().is_nan(), "and nothing downstream is poisoned");
    }

    /// Zero is no shadow at all, not a shadow of size zero with a colour.
    #[test]
    fn zero_is_no_shadow() {
        let s = elevation_to_shadow(0.0);
        assert_eq!(s.color, Color::TRANSPARENT);
        assert_eq!(s.blur, 0.0);
    }
}
