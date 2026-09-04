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
        let base = self.translate_signal().get_or(Translate::NONE);
        self.resolve_state_value(id, base, |state| state.translate.map(|s| s.get()))
    }

    pub(super) fn effective_rotate_target(&self, id: WidgetId) -> f32 {
        let base = self.rotate_signal().get_or(0.0);
        self.resolve_state_value(id, base, |state| state.rotate.map(|s| s.get()))
    }

    pub(super) fn effective_scale_target(&self, id: WidgetId) -> Scale {
        let base = self.scale_signal().get_or(Scale::NONE);
        self.resolve_state_value(id, base, |state| state.scale.map(|s| s.get()))
    }

    /// How far past its own bounds the deepest shadow this container can cast
    /// reaches, and the number its damage rect is sized by.
    ///
    /// Layout's, not paint's. A shadow animates paint-only, so a hover that
    /// lifts a card never re-runs this layout, and layout is where the shadow's
    /// reach is recorded. Sizing the reach to whatever is showing would leave
    /// the ring outside every damage rect.
    ///
    /// So the answer is a bound over everything the shadow can be:
    ///
    /// - the declared shadow, and every state layer's,
    /// - inflated by the overshoot a spring still has to come,
    /// - **and whatever is in flight**, because a *shrinking* shadow is drawn
    ///   from a value the declarations no longer mention. `.shadow(move ||
    ///   pick.get())` written from deep to none re-runs this layout on the
    ///   write, at which point every declaration reads as nothing while the
    ///   shadow on screen is still 8 deep.
    ///
    /// Reading every state layer under layout tracking is what the comment on
    /// [`max_transform_reach`](Self::max_transform_reach) is about: every shadow
    /// *any* state layer declares is a layout dependency, so a container whose
    /// hover shadow is a constant re-lays out only when the declaration changes.
    /// `.shadow(none).when_hovered(|s| s.shadow(lifted))` — both constants — is
    /// laid out once; `when_hovered(|s| s.shadow(lift))` with `lift` a signal
    /// re-lays out the container when it moves.
    pub(super) fn max_shadow_extent(&self) -> f32 {
        let base = self.shadow.get_or(Shadow::none());
        let anim = self.anims.as_ref().and_then(|a| a.shadow.as_ref());
        let declared = self
            .interaction
            .iter()
            .flat_map(|ix| ix.states.iter())
            .filter_map(|(_, state)| state.shadow.map(|s| s.get().extent()))
            .fold(base.extent(), f32::max);

        match anim {
            // The value in flight is already past its overshoot, so it is folded
            // in flat; the declarations have theirs still to come.
            //
            // Scaling the extent by the overshoot is exact only while every
            // channel keeps its sign, which a level guaranteed and a shadow does
            // not: `(0.0, -100.0)` to `(0.0, 100.0)` are both 100 deep, and at
            // the peak of a bounce the offset is 134. `animated_shadow` shrinks
            // whatever is in flight to this number, so that is a visibly damped
            // peak on a sign-crossing bounce rather than a ring outside the
            // damage rect — the same one-sided error the clamp already trades
            // for not sizing every rect to a resonant gain.
            Some(anim) => (declared * (1.0 + anim.peak_overshoot())).max(anim.current().extent()),
            None => declared,
        }
    }

    /// Which of the three components anything can move — the declaration, a
    /// state layer's override of it, or an animation holding one.
    ///
    /// One computation, read by `animated_transform`'s gate and by
    /// `max_transform_reach`'s. Two copies of this drifting apart would make a
    /// container that transforms report a reach of zero and be culled while it
    /// is moved, which is the defect this all exists to stop.
    pub(super) fn moving_components(&self) -> Moves {
        let anims = self.anims.as_ref();
        let declared = self
            .interaction
            .as_ref()
            .map(|ix| ix.declares_transform)
            .unwrap_or_default();
        Moves {
            translate: declared.translate
                || self.translate_signal().is_some()
                || anims.is_some_and(|a| a.translate.is_some()),
            rotate: declared.rotate
                || self.rotate_signal().is_some()
                || anims.is_some_and(|a| a.rotate.is_some()),
            scale: declared.scale
                || self.scale_signal().is_some()
                || anims.is_some_and(|a| a.scale.is_some()),
        }
    }

    /// The furthest this container's paint can land outside `bounds`, in
    /// logical pixels — its shadow, and then whatever its transform can do to
    /// the box that shadow surrounds.
    ///
    /// The largest it can reach *from what is declared* — the value, the state
    /// layers, and whatever an animation is holding as this is asked. A
    /// `timeline` is not bounded here: its keyframes are the animation's, not a
    /// declaration, so the answer for one is the frame it is on rather than the
    /// whole play. That is sound because a playing animation queues a Paint job
    /// per changed frame and this is recomputed from it, which
    /// `a_declared_transform_change_invalidates_the_reach_without_a_layout`
    /// holds in place; it is not a bound taken once and trusted.
    ///
    /// For everything that *is* declared it is the largest, not the one
    /// showing, for the reason [`Self::max_shadow_extent`] gives: a transform
    /// animates paint-only, so a hover that lifts a card never re-runs this
    /// layout. A reach sized to the resting value would leave the lifted card
    /// outside the rect its parent culls against, and it would vanish for
    /// exactly as long as it was moved.
    ///
    /// A bound rather than a value, so the error is one-sided. Everything
    /// downstream grows a laid-out rect by this and asks whether the result is
    /// visible; a child's drawn rect is inside its grown rect by construction,
    /// so a child on screen is never culled. Sometimes one is painted that has
    /// not moved into view yet.
    ///
    /// The two outsets compose rather than alternate: a shadow is drawn in the
    /// container's own space and then carried by the transform, so an elevated
    /// card that lifts reaches further than either alone. Taking the transform
    /// over the shadow-inflated box is also what makes `scale` scale the
    /// shadow.
    ///
    /// Every read here is `_untracked`, and it has to be. Layout runs a child
    /// inside its *parent's* scope, so a tracked read registers against
    /// whichever ancestor is innermost and reflows that one — which is why
    /// `a_transform_does_not_reflow_the_parent_either` asks the parent rather
    /// than the container that declared the transform, and why being outside a
    /// scope of one's own is not enough. `snapshot_zone` is not enough either:
    /// it silences the non-reactive-read diagnostic and leaves the read
    /// tracked.
    ///
    /// What keeps the value current is not a subscription but the schedule:
    /// `Container::refresh_paint_bounds` runs from the Paint job that the
    /// declaring write already queues, in the pass before this frame paints.
    /// Blink resolves its transforms in the same gap, for the same reason.
    pub(super) fn max_transform_reach(&self, bounds: Rect, shadow_extent: f32) -> f32 {
        let moves = self.moving_components();
        if !moves.any() {
            return shadow_extent;
        }

        // The box the transform actually carries: the container plus the
        // shadow already standing outside it. Measured back against `bounds`,
        // because that is what every consumer grows by the answer — an outset
        // taken against `painted` would report the excursion beyond the shadow
        // and lose its width.
        let painted = bounds.outset(shadow_extent);
        let pivot = self.pivot_signal().get_or_untracked(Pivot::CENTER);
        let anims = self.anims.as_ref();

        let base_translate = self.translate_signal().get_or_untracked(Translate::NONE);
        let base_rotate = self.rotate_signal().get_or_untracked(0.0);
        let base_scale = self.scale_signal().get_or_untracked(Scale::NONE);
        let base = Transform::compose(base_translate, base_rotate, base_scale);
        let mut reach = outset_of(base, painted, bounds, pivot);

        // A rotation's outset is not monotone in its angle: a box turned 0 or
        // 180 degrees sits back where it started and somewhere between stands
        // furthest outside. Endpoints bound a shadow, which grows monotonically
        // with its extent; they bound nothing here. So a container that can
        // rotate at all is given the worst angle outright.
        //
        // Turning about a pivot keeps every point at its own distance from it,
        // so the whole swept shape fits the circle of radius `far` about the
        // pivot — which bounds every angle, and every pivot, without caring
        // which. A corner pivot sweeps a much larger circle than a centre one,
        // and half the diagonal only ever describes the centre.
        if moves.rotate {
            // About `bounds`, which is where `outset_of` and `flatten` resolve
            // it. Resolving against the shadow-inflated box instead puts the
            // centre of the swept circle somewhere the content never turns
            // about, and the bound comes out looser than the widest angle.
            let (px, py) = pivot.resolve(bounds);
            let far = [
                (painted.x, painted.y),
                (painted.x + painted.width, painted.y),
                (painted.x, painted.y + painted.height),
                (painted.x + painted.width, painted.y + painted.height),
            ]
            .into_iter()
            .fold(0.0f32, |most, (x, y)| most.max((x - px).hypot(y - py)));
            let swept = Rect::new(px - far, py - far, far * 2.0, far * 2.0);
            reach = reach.max(swept.outset_beyond(bounds));
        }

        // Each layer is a transform this container can be in, whether or not it
        // is in it now. Resolving only the active one would make a hover re-run
        // layout, which is the whole thing this avoids.
        if let Some(ref ix) = self.interaction {
            for (_, state) in ix.states.iter() {
                if state.translate.is_none() && state.rotate.is_none() && state.scale.is_none() {
                    continue;
                }
                let candidate = Transform::compose(
                    state
                        .translate
                        .map_or(base_translate, |s| s.get_untracked()),
                    state.rotate.map_or(base_rotate, |s| s.get_untracked()),
                    state.scale.map_or(base_scale, |s| s.get_untracked()),
                );
                reach = reach.max(outset_of(candidate, painted, bounds, pivot));
            }
        }

        // A spring passes its target before it settles, so the declared reach
        // is inflated by the overshoot still to come — the same allowance
        // `max_shadow_extent` makes — and the value in flight, which is already
        // past whatever overshoot it had, is folded in flat.
        if let Some(anims) = anims {
            let overshoot = [
                anims.translate.as_ref().map(|a| a.peak_overshoot()),
                anims.rotate.as_ref().map(|a| a.peak_overshoot()),
                anims.scale.as_ref().map(|a| a.peak_overshoot()),
            ]
            .into_iter()
            .flatten()
            .fold(0.0f32, f32::max);
            reach *= 1.0 + overshoot;

            let flying = Transform::compose(
                anims
                    .translate
                    .as_ref()
                    .map_or(base_translate, |a| *a.current()),
                anims.rotate.as_ref().map_or(base_rotate, |a| *a.current()),
                anims.scale.as_ref().map_or(base_scale, |a| *a.current()),
            );
            reach = reach.max(outset_of(flying, painted, bounds, pivot));
        }

        reach
    }

    pub(super) fn effective_shadow_target(&self, id: WidgetId) -> Shadow {
        let base = self.shadow.get_or(Shadow::none());
        self.resolve_state_value(id, base, |state| state.shadow.map(|s| s.get()))
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

    /// The shadow to draw, never reaching further than the rect the layout
    /// reserved.
    ///
    /// Clamped, and clamped to [`shadow_reach`](super::Container::shadow_reach)
    /// — the number `layout` recorded — rather than to a fresh
    /// [`max_shadow_extent`](Self::max_shadow_extent). Recomputing it here reads
    /// the signals at *paint* time, which is a different frame's answer: on a
    /// shrinking shadow the declared maximum has already reached nothing and the
    /// shadow was cut away while the animation was still playing.
    ///
    /// Uniformly — see [`Shadow::shrunk_to`] — because a shadow is four numbers
    /// and the reserved rect is one.
    ///
    /// A clamp at all, rather than a wider reach, because the reach that would
    /// make one unnecessary is not a small one. A spring keeps its momentum
    /// across a retarget, so hover flicker *pumps* it: driven at its damped
    /// natural frequency the steady-state excursion is the resonant gain,
    /// `1 / (2ζ√(1-ζ²))` — 1.03x the step response for `BOUNCY`, but 3.7x at
    /// ζ = 0.05, and sizing every damage rect for that is not a trade worth
    /// making for the tip of a bounce nobody asked for. See
    /// `hover_flicker_cannot_push_a_shadow_outside_its_damage_rect`.
    pub(super) fn animated_shadow(&self, id: WidgetId) -> Shadow {
        let anim = self.anims.as_ref().and_then(|a| a.shadow.as_ref());
        let shadow = get_animated_value(anim, || self.effective_shadow_target(id));
        match anim {
            Some(_) => shadow.shrunk_to(self.shadow_reach.get()),
            // Only where something is in flight, because `shadow_reach` is the
            // number *layout* recorded and layout has not run yet on the frame
            // a container is first painted — it is `0.0` until then, and
            // clamping to it would draw no shadow at all. Everything a
            // declaration or a state layer can resolve to is already folded
            // into `max_shadow_extent`, so an unanimated shadow is inside the
            // rect by construction and has nothing to be clamped to.
            None => shadow,
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

        // What could move each component: its own declaration, its own
        // animation, or a state layer that overrides it. Per component and not
        // "does any layer move anything", because `when_pressed(|s|
        // s.scale(0.98))` is on every button and one bit for all three would
        // have made it resolve a translate and a rotate nothing declares,
        // twice per pointer move.
        //
        // Computed once and shared with the early-out below, so the two cannot
        // disagree: a component wired into the gates and forgotten in the
        // early-out would return IDENTITY and silently stop transforming.
        let Moves {
            translate: has_translate,
            rotate: has_rotate,
            scale: has_scale,
        } = self.moving_components();

        // A plain layout box is none of the three, and most containers are one.
        if !(has_translate || has_rotate || has_scale) {
            return Transform::IDENTITY;
        }

        let translate = if has_translate {
            get_animated_value(anims.and_then(|a| a.translate.as_ref()), || {
                self.effective_translate_target(id)
            })
        } else {
            Translate::NONE
        };
        let rotate = if has_rotate {
            get_animated_value(anims.and_then(|a| a.rotate.as_ref()), || {
                self.effective_rotate_target(id)
            })
        } else {
            0.0
        };
        let scale = if has_scale {
            get_animated_value(anims.and_then(|a| a.scale.as_ref()), || {
                self.effective_scale_target(id)
            })
        } else {
            Scale::NONE
        };
        Transform::compose(translate, rotate, scale)
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
                || a.shadow.is_some()
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
                || a.shadow.is_some()
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
    pub shadow: Shadow,
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
        // change between frames; a gradient that dropped the shadow meant a
        // shadow animation stopped drawing halfway through while still asking
        // for a frame at every step.
        //
        // To the box *it draws*, though: the shadow rides whichever fill runs,
        // so a container with neither — no gradient, and a background that is
        // transparent or has animated out — casts none, and `paint_overflow`
        // goes on reserving the room. A shadow with nothing above it is a smear
        // rather than a lift, so that is the behaviour; it is not a
        // free-standing command waiting for a box to belong to.
        // On the alpha rather than on whether a shadow was declared: the first
        // frames of a lift out of `Shadow::none()` carry an alpha of ~0.001,
        // where the shadow rounds to nothing and every frame would still push a
        // rect carrying it. The same gate the border gets.
        let shadow = (d.shadow.color.a > SHADOW_ALPHA_FLOOR).then_some(d.shadow);

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
/// carrying it is a rect drawn for no reason, once per frame for as long as a
/// shadow animation is leaving transparent.
pub(super) const SHADOW_ALPHA_FLOOR: f32 = 0.004;

/// How far `painted`, once `transform` has carried it, stands outside `bounds`.
///
/// Two rects because they are two questions: `painted` is everything the widget
/// draws, `bounds` is the box a parent knows it by, and the answer has to be in
/// terms of the second — every consumer grows a laid-out rect by it. With no
/// transform at all the answer is the difference between them, which is the
/// shadow, so the shadow needs no separate accounting.
///
/// One number for four sides, because everything reading it grows a rect
/// equally in every direction: a scalar cannot say a card lifts upward only,
/// and a reach that is too generous costs paint while one that is too tight
/// loses a widget.
fn outset_of(transform: Transform, painted: Rect, bounds: Rect, pivot: Pivot) -> f32 {
    let moved = if transform.is_identity() {
        painted
    } else {
        // About `bounds`, which is what `flatten` resolves the pivot against
        // when it draws. Resolving against the shadow-inflated box instead
        // would put a corner pivot in a different place here than on screen.
        transform.about(pivot, bounds).map_rect(painted)
    };
    moved.outset_beyond(bounds)
}

/// `outset_of` is the arithmetic every cull and every damage rect is grown by,
/// and it had no test until a review worked an example by hand and found it ten
/// pixels short.
#[cfg(test)]
mod reach_tests {
    use super::*;
    use crate::animation::{Animate, SpringConfig, Transition};

    /// Turning about a corner carries the far corner around a circle of the
    /// whole diagonal, so the box sweeps well past what a centre rotation
    /// reaches. A bound written for the centre reports 20.7 here, and the
    /// answer is 70.7.
    #[test]
    fn a_rotation_about_a_corner_sweeps_further_than_one_about_the_centre() {
        let mut worst: f32 = 0.0;
        for degrees in 0..360 {
            let spin = Transform::compose(Translate::NONE, degrees as f32, Scale::NONE);
            worst = worst.max(outset_of(spin, BOX, BOX, Pivot::TOP_LEFT));
        }
        // What a bound written for the centre would have said.
        let centre_shaped = (100.0f32.hypot(100.0) - 100.0) / 2.0;
        assert!(
            worst > centre_shaped,
            "a corner rotation reaches {worst}; a centre-shaped bound says \
             {centre_shaped} and would cull a widget that is on screen"
        );

        // What the code says: every point stays its own distance from the
        // pivot, so the swept shape fits the circle of the furthest corner.
        let swept = 100.0f32.hypot(100.0);
        assert!(
            swept >= worst,
            "the swept-circle bound {swept} does not cover the worst angle {worst}"
        );
    }

    const BOX: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };

    /// With nothing moving, what a widget paints outside its box is its shadow.
    #[test]
    fn an_untransformed_box_reaches_exactly_its_shadow() {
        let painted = BOX.outset(10.0);
        assert_eq!(
            outset_of(Transform::IDENTITY, painted, BOX, Pivot::CENTER),
            10.0
        );
    }

    /// The shadow and the transform compose. This is the case the review
    /// found: measured against the shadow-inflated box the answer came out 50,
    /// and the shadow really reaches 60.
    #[test]
    fn a_shadow_carried_by_a_translate_reaches_past_both() {
        let painted = BOX.outset(10.0);
        let lift = Transform::compose(Translate::new(0.0, -50.0), 0.0, Scale::NONE);
        assert_eq!(outset_of(lift, painted, BOX, Pivot::CENTER), 60.0);
    }

    /// And a consumer growing the box by the answer contains what is drawn,
    /// which is the whole promise: a widget on screen is never culled.
    #[test]
    fn the_grown_box_contains_what_is_drawn() {
        let painted = BOX.outset(10.0);
        let lift = Transform::compose(Translate::new(0.0, -50.0), 0.0, Scale::NONE);
        let reach = outset_of(lift, painted, BOX, Pivot::CENTER);

        let drawn = lift.about(Pivot::CENTER, painted).map_rect(painted);
        let grown = BOX.outset(reach);
        assert!(
            grown.x <= drawn.x
                && grown.y <= drawn.y
                && grown.x + grown.width >= drawn.x + drawn.width
                && grown.y + grown.height >= drawn.y + drawn.height,
            "grown {grown:?} does not contain drawn {drawn:?}"
        );
    }

    /// A scale carries the shadow with it rather than leaving it behind.
    #[test]
    fn a_scale_scales_the_shadow_it_surrounds() {
        let painted = BOX.outset(10.0);
        let grow = Transform::compose(Translate::NONE, 0.0, Scale::uniform(2.0));
        // The 120-wide painted box doubles about the centre it shares with
        // bounds, reaching 120 either side of it; 70 of that stands outside the
        // 100-wide bounds. A shadow that did not scale would reach 60.
        assert_eq!(outset_of(grow, painted, BOX, Pivot::CENTER), 70.0);
    }

    /// The rotation bound, asked of the container rather than of the
    /// arithmetic under it.
    ///
    /// `max_transform_reach` has its own branch for rotation — the swept circle
    /// about the pivot — and the tests below reach `outset_of` directly, so
    /// that branch was watched by nothing. It has to cover the worst angle for
    /// the pivot it is given, and a corner pivot sweeps far wider than a centre
    /// one.
    #[test]
    fn the_rotation_bound_covers_every_angle_for_the_pivot_it_is_given() {
        // BOTTOM_RIGHT included deliberately: it is the pivot whose swept
        // circle reaches furthest past the *far* edges rather than the near
        // ones, so it is the only one where the circle's width and height are
        // load-bearing rather than masked by its origin.
        for pivot in [
            Pivot::CENTER,
            Pivot::TOP_LEFT,
            Pivot::TOP,
            Pivot::BOTTOM_RIGHT,
        ] {
            let turning = container()
                .width(100.0)
                .height(100.0)
                .rotate(0.0)
                .pivot(pivot);
            // With a shadow, so the box the rotation carries does not start at
            // the origin. At the origin `x` is zero and half the arithmetic
            // that reads it is indistinguishable from arithmetic that does not.
            let shadow = 6.0;
            let painted = BOX.outset(shadow);
            let bound = turning.max_transform_reach(BOX, shadow);

            let worst = (0..360)
                .map(|deg| {
                    let spin = Transform::compose(Translate::NONE, deg as f32, Scale::NONE);
                    outset_of(spin, painted, BOX, pivot)
                })
                .fold(0.0f32, f32::max);

            assert!(
                bound >= worst - 0.01,
                "{pivot:?}: the bound is {bound} and the widest angle reaches \
                 {worst}, so a container mid-rotation would be culled"
            );
            // And tight. A bound that is merely large is satisfied by any
            // arithmetic that errs upward, which leaves the whole computation
            // unwatched — and every pixel of slack is painted and damaged.
            assert!(
                bound <= worst + 0.01,
                "{pivot:?}: the bound is {bound} where the widest angle reaches \
                 only {worst}, so every rotation pays for reach it cannot use"
            );
        }
    }

    /// And it is a bound, not a shrug: a box that cannot rotate is not given
    /// the reach of one that can.
    #[test]
    fn a_box_that_cannot_rotate_is_not_given_a_rotation_s_reach() {
        let still = container().width(100.0).height(100.0);
        assert_eq!(still.max_transform_reach(BOX, 0.0), 0.0);
        assert_eq!(still.max_transform_reach(BOX, 6.0), 6.0, "only its shadow");
    }

    /// A spring passes its target before it settles, and the reach has to
    /// cover where it goes, not where it is going.
    ///
    /// This is the one part of the bound that is not about geometry: a
    /// `SNAPPY` translate to -100 reaches past -100 on the way, and a reach of
    /// exactly 100 culls the widget at the top of its bounce.
    #[test]
    fn a_spring_is_given_room_for_the_overshoot_it_has_not_taken_yet() {
        let sprung = container().width(100.0).height(100.0).translate(
            Translate::new(0.0, -100.0).transition(Transition::spring(SpringConfig::BOUNCY)),
        );

        let reach = sprung.max_transform_reach(BOX, 0.0);
        assert!(
            reach > 100.0,
            "a spring to -100 was given a reach of {reach}, so it is culled at \
             the far end of its own overshoot"
        );

        let eased = container()
            .width(100.0)
            .height(100.0)
            .translate(Translate::new(0.0, -100.0).transition(200.0));
        assert_eq!(
            eased.max_transform_reach(BOX, 0.0),
            100.0,
            "an eased translate overshoots nothing and should pay for nothing"
        );
    }

    /// A state layer that moves only one component is still a transform this
    /// container can be in.
    ///
    /// The loop skips a layer declaring none of the three, and the skip has to
    /// mean *none* — a layer that scales but does not translate still carries
    /// the container somewhere its resting declaration does not, and treating
    /// "not all three" as "none" reports a reach it does not have.
    #[test]
    fn a_state_layer_moving_one_component_still_counts() {
        let scaled = container()
            .width(100.0)
            .height(100.0)
            .when_pressed(|s| s.scale(Scale::uniform(2.0)));
        assert_eq!(
            scaled.max_transform_reach(BOX, 0.0),
            50.0,
            "a layer that doubles this box on press was not counted, so it is \
             culled at its resting size while it is pressed"
        );

        // Each of the three on its own, because the skip is a conjunction and a
        // layer declaring only the first of them is where a loosened one shows.
        let lifted = container()
            .width(100.0)
            .height(100.0)
            .when_hovered(|s| s.translate(Translate::new(0.0, -25.0)));
        assert_eq!(
            lifted.max_transform_reach(BOX, 0.0),
            25.0,
            "a layer that lifts this box on hover was not counted"
        );

        let turned = container()
            .width(100.0)
            .height(100.0)
            .when_hovered(|s| s.rotate(45.0));
        assert!(
            turned.max_transform_reach(BOX, 0.0) > 0.0,
            "a layer that turns this box on hover was not counted"
        );
    }

    /// The angle that puts a square furthest outside its box is 45 degrees,
    /// where neither endpoint of a 0-to-90 rotation is. Nothing may report a
    /// reach smaller than this while a rotation is declared.
    ///
    /// About the centre. A corner pivot sweeps a far bigger circle, which is
    /// why the bound is taken from the distance to the furthest corner rather
    /// than from half the diagonal — see the neighbour below.
    #[test]
    fn a_square_reaches_furthest_halfway_through_its_rotation() {
        let at = |deg: f32| {
            outset_of(
                Transform::compose(Translate::NONE, deg, Scale::NONE),
                BOX,
                BOX,
                Pivot::CENTER,
            )
        };
        assert_eq!(at(0.0), 0.0);
        assert!(
            at(90.0) < 0.01,
            "a square turned 90 degrees is back in its box"
        );

        let corner = (100.0f32.hypot(100.0) - 100.0) / 2.0;
        assert!(
            (at(45.0) - corner).abs() < 0.01,
            "at 45 degrees a square stands {corner} outside its box, got {}",
            at(45.0)
        );
    }
}
