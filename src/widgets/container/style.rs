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
use crate::finite::{AllFinite, FiniteOr};
use crate::tree::LayoutCtx;
use crate::widgets::BackgroundOverride;

impl Container {
    // -----------------------------------------------------------------------
    // State layer: base value plus the override of the active state
    // -----------------------------------------------------------------------

    /// Apply the active state layer's override to `base`, if it declares one.
    /// Priority is pressed > focused > hovered.
    fn resolve_state_value<T: AllFinite + Clone>(
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

        // Built once for the whole walk rather than per layer: every
        // overridable property resolves per paint, each walking these layers.
        let control = ix.as_control(id);

        // Backwards: the last layer declared wins. A layer that says nothing
        // about *this* property is passed over rather than ending the search,
        // so `when_hovered(|s| s.lighter(0.1))` still lightens under a pressed
        // layer that only scales.
        for (when, state) in ix.states.iter().rev() {
            // A layer whose value is not finite is passed over exactly as one
            // that says nothing about this property is — the base it falls
            // through to was resolved the same way, so what comes out of here
            // is finite whatever the application computed.
            let Some(value) = extractor(state).filter(|v| v.all_finite()) else {
                continue;
            };
            if control.is_active(when) {
                return value;
            }
        }
        base
    }

    /// The background a state layer resolves to: its own colour if it declares
    /// one, its own alpha if it declares one, otherwise the base.
    pub(super) fn effective_background_target(&self, id: WidgetId) -> Color {
        let base = self
            .background
            .get_finite_or(Color::TRANSPARENT, id, "background");
        self.resolve_state_value(id, base, |state| state_colour(base, state))
    }

    /// The gradient a state layer resolves to: each end resolved the way
    /// [`effective_background_target`] resolves a solid fill, or no gradient at
    /// all while an exact background replaces it.
    ///
    /// [`effective_background_target`]: Self::effective_background_target
    pub(super) fn effective_gradient(&self, id: WidgetId) -> Option<LinearGradient> {
        let base = self.gradient_prop().get().flatten()?;
        self.resolve_state_value(id, Some(base), |state| {
            if let Some(BackgroundOverride::Exact(_)) = state.background {
                // Passed over, as the solid fill passes it over, when the colour
                // it would replace the gradient with is not a number.
                return state_colour(base.start_color, state)
                    .filter(AllFinite::all_finite)
                    .map(|_| None);
            }
            let (start_color, end_color) =
                state_colour(base.start_color, state).zip(state_colour(base.end_color, state))?;
            Some(Some(LinearGradient {
                start_color,
                end_color,
                ..base
            }))
        })
    }

    pub(super) fn effective_border_width_target(&self, id: WidgetId) -> f32 {
        let base = self.border_width.get_finite_or(0.0, id, "border_width");
        self.resolve_state_value(id, base, |state| state.border.map(|b| b.width.get()))
    }

    pub(super) fn effective_border_color_target(&self, id: WidgetId) -> Color {
        let base = self
            .border_color
            .get_finite_or(Color::TRANSPARENT, id, "border_color");
        self.resolve_state_value(id, base, |state| state.border.map(|b| b.color.get()))
    }

    pub(super) fn effective_outline_width_target(&self, id: WidgetId) -> f32 {
        let base = self
            .outline_width_prop()
            .get_finite_or(0.0, id, "outline_width");
        self.resolve_state_value(id, base, |state| state.outline.map(|o| o.width.get()))
    }

    pub(super) fn effective_outline_color_target(&self, id: WidgetId) -> Color {
        let base = self
            .outline_color_prop()
            .get_finite_or(Color::TRANSPARENT, id, "outline_color");
        self.resolve_state_value(id, base, |state| state.outline.map(|o| o.color.get()))
    }

    pub(super) fn effective_outline_offset_target(&self, id: WidgetId) -> f32 {
        let base = self
            .outline_offset_prop()
            .get_finite_or(0.0, id, "outline_offset");
        self.resolve_state_value(id, base, |state| state.outline.map(|o| o.offset.get()))
    }

    pub(super) fn effective_corners_target(&self, id: WidgetId) -> crate::widgets::Corners {
        let base = self
            .corners
            .get_finite_or(crate::widgets::Corners::SQUARE, id, "corners");
        self.resolve_state_value(id, base, |state| state.corners.get())
    }

    pub(super) fn effective_translate_target(&self, id: WidgetId) -> Translate {
        let base = self
            .translate_prop()
            .get_finite_or(Translate::NONE, id, "translate");
        self.resolve_state_value(id, base, |state| state.translate.get())
    }

    pub(super) fn effective_rotate_target(&self, id: WidgetId) -> f32 {
        let base = self.rotate_prop().get_finite_or(0.0, id, "rotate");
        self.resolve_state_value(id, base, |state| state.rotate.get())
    }

    pub(super) fn effective_scale_target(&self, id: WidgetId) -> Scale {
        let base = self.scale_prop().get_finite_or(Scale::NONE, id, "scale");
        self.resolve_state_value(id, base, |state| state.scale.get())
    }

    pub(super) fn effective_opacity_target(&self, id: WidgetId) -> f32 {
        let base = self.opacity_prop().get_finite_or(1.0, id, "opacity");
        self.resolve_state_value(id, base, |state| state.opacity.get())
    }

    /// The declared blur radius. No state layer overrides one; a negative
    /// one is clamped where it is painted.
    pub(super) fn effective_blur_target(&self, id: WidgetId) -> f32 {
        self.blur_prop().get_finite_or(0.0, id, "blur")
    }

    /// How far past everything else it draws this container's blur can carry
    /// a pixel, in logical pixels: the bound on the reach of the larger of
    /// the declared radius and the one in flight, with the overshoot a spring
    /// still has to come. Zero for a container that declares no blur.
    pub(super) fn max_blur_reach(&self) -> f32 {
        if !self.blur_prop().is_set() {
            return 0.0;
        }
        let mut radius = self.blur_prop().get_finite_or_quietly(0.0).max(0.0);
        if let Some(anim) = self.anims.as_ref().and_then(|a| a.blur()) {
            radius = radius.max(*anim.current()) * (1.0 + anim.peak_overshoot());
        }
        crate::renderer::layer_blur::blur_reach_bound(radius)
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
    /// - **and every stop of a declared timeline**, which no signal mentions
    ///   either. A sequence's keyframes are values the shadow will hold, and
    ///   `animated_shadow` clamps whatever is in flight to this number — so
    ///   leaving them out meant a timeline that grows the shadow was scaled
    ///   straight back down to where it started. The animation ran, `advance`
    ///   moved it, and the painted blur was the declared value on every frame
    ///   of it (#384). `transition` was never affected: it moves the declared
    ///   signal, which this reads, so the reach follows it.
    ///
    /// Reading every state layer under layout tracking is what the comment on
    /// [`max_transform_reach`](Self::max_transform_reach) is about: every shadow
    /// *any* state layer declares is a layout dependency, so a container whose
    /// hover shadow is a constant re-lays out only when the declaration changes.
    /// `.shadow(none).when_hovered(|s| s.shadow(lifted))` — both constants — is
    /// laid out once; `when_hovered(|s| s.shadow(lift))` with `lift` a signal
    /// re-lays out the container when it moves.
    ///
    /// Side by side: a shadow cast downward reaches further below the box than
    /// above it, and each side is bounded on its own — the overshoot of each
    /// measured across that side's own range.
    pub(super) fn max_shadow_extent(&self, id: WidgetId) -> Outsets {
        let base = self
            .shadow_prop()
            .get_finite_or(Shadows::none(), id, "shadow");
        let anim = self.anims.as_ref().and_then(|a| a.shadow());
        // Signed, because a step between two shadows pulled inside the box can
        // overshoot past its edge though neither end reaches it. The low end
        // starts at zero, so a shadow rising from none, or from any positive
        // declaration, keeps the bound it had: a fraction of its own depth.
        let resting = base.signed_outsets();
        let (floor, deepest) = self
            .interaction
            .iter()
            .flat_map(|ix| ix.states.iter())
            .filter_map(|(_, state)| state.shadow.as_deref()?.get())
            .filter(AllFinite::all_finite)
            .map(|shadow| shadow.signed_outsets())
            .fold(
                (resting.min(Outsets::ZERO), resting),
                |(low, high), extent| (low.min(extent), high.max(extent)),
            );

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
            Some(anim) => {
                let sequence =
                    |side: fn(Outsets) -> f32| anim.sequence_reach(|s| side(s.signed_outsets()));
                let sequence = Outsets {
                    left: sequence(|o| o.left),
                    top: sequence(|o| o.top),
                    right: sequence(|o| o.right),
                    bottom: sequence(|o| o.bottom),
                };
                (deepest + (deepest - floor) * anim.peak_overshoot())
                    .max(anim.current().outsets())
                    .max(sequence)
            }
            None => deepest.max(Outsets::ZERO),
        }
    }

    /// How far past its own bounds this container's outline can reach — its
    /// offset and its width — and the number its paint reach grows by.
    ///
    /// Paint's, not layout's: an outline never reaches layout, so the reach
    /// is published from `refresh_paint_bounds`, which runs before each Paint
    /// job, as a transform's is. The answer is a bound over what is declared,
    /// for the reason [`Self::max_transform_reach`] gives — the declaration,
    /// every state layer's, inflated by the overshoot a spring still has to
    /// come, whatever is in flight and every stop of a timeline — so a ring
    /// that a hover brings up is inside the rect its parent culls against
    /// before it shows.
    ///
    /// The declaration is read tracked, inside the Paint scope both callers
    /// open; the state layers are snapshots, as the transform's are.
    pub(super) fn max_outline_reach(&self) -> f32 {
        if !self.declares_outline() {
            return 0.0;
        }
        let reach = |width: f32, offset: f32| (width.max(0.0) + offset).max(0.0);
        let width = self.outline_width_prop().get_finite_or_quietly(0.0);
        let offset = self.outline_offset_prop().get_finite_or_quietly(0.0);
        let mut declared = reach(width, offset);
        if let Some(ref ix) = self.interaction {
            for outline in ix.states.iter().filter_map(|(_, state)| state.outline) {
                let (w, o) = (
                    outline.width.get_untracked(),
                    outline.offset.get_untracked(),
                );
                if w.is_finite() && o.is_finite() {
                    declared = declared.max(reach(w, o));
                }
            }
        }

        let anims = self.anims.as_ref();
        let (width_anim, offset_anim) = (
            anims.and_then(|a| a.outline_width()),
            anims.and_then(|a| a.outline_offset()),
        );
        let overshoot = [width_anim, offset_anim]
            .into_iter()
            .flatten()
            .map(|a| a.peak_overshoot())
            .fold(0.0f32, f32::max);
        let flying = reach(
            width_anim.map_or(width, |a| *a.current()),
            offset_anim.map_or(offset, |a| *a.current()),
        );
        // A timeline's stops are values no declaration mentions.
        let sequenced = reach(
            width_anim.map_or(width, |a| a.sequence_reach(|w| *w).max(width)),
            offset_anim.map_or(offset, |a| a.sequence_reach(|o| *o).max(offset)),
        );
        (declared * (1.0 + overshoot)).max(flying).max(sequenced)
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
                || self.translate_prop().is_set()
                || anims.is_some_and(|a| a.translate().is_some()),
            rotate: declared.rotate
                || self.rotate_prop().is_set()
                || anims.is_some_and(|a| a.rotate().is_some()),
            scale: declared.scale
                || self.scale_prop().is_set()
                || anims.is_some_and(|a| a.scale().is_some()),
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
    /// The declared transform and the pivot are read *tracked*, and both
    /// callers of
    /// `Container::refresh_paint_bounds` open this container's own *Paint*
    /// scope around it — the Paint job in `jobs.rs`, and `Tree::layout_widget`
    /// after the Layout scope has closed. So a write to a transform marks the
    /// container that declared it for repaint, and marks nothing for layout.
    ///
    /// They were `_untracked` until #319, and the reason given was that layout
    /// ran a child inside its *parent's* scope, so a tracked read would
    /// register against an ancestor and reflow it. #387 ended that: a child is
    /// laid out through one call that opens the child's own scope. What was
    /// left was a widget culled before it ever painted, which had read nothing
    /// and so could never be told to come back —
    /// `a_row_that_never_painted_can_still_lift_itself_into_view`.
    ///
    /// The schedule still carries the ordinary case: the publish runs in the
    /// pass before this frame paints, as Blink resolves its transforms in the
    /// same gap. The subscription is what covers the case the schedule cannot
    /// reach. `a_transform_does_not_reflow_the_parent_either` is still what
    /// says the marking lands on the right widget and the right pass.
    ///
    /// The state-layer transforms below stay snapshots. A hover or a press
    /// cannot arrive at a widget nobody can see, so subscribing to them would
    /// buy nothing the schedule does not already cover — and would put every
    /// container that declares `when_hovered` on its own hover signal from
    /// the reach path, which is a wider fan-out than the case needs.
    pub(super) fn max_transform_reach(&self, bounds: Rect, shadow_extent: Outsets) -> Outsets {
        let moves = self.moving_components();
        if !moves.any() {
            return shadow_extent;
        }

        // The box the transform actually carries: the container plus the
        // shadow already standing outside it. Measured back against `bounds`,
        // because that is what every consumer grows by the answer — an outset
        // taken against `painted` would report the excursion beyond the shadow
        // and lose its width.
        let painted = bounds.outset_by(shadow_extent);
        let pivot = self.resolved_pivot_quietly();
        let anims = self.anims.as_ref();

        // Each translate resolved against `bounds` as it is composed, since
        // that is where `animated_transform` resolves the one it paints.
        let base_translate = self.translate_prop().get_finite_or_quietly(Translate::NONE);
        let base_rotate = self.rotate_prop().get_finite_or_quietly(0.0);
        let base_scale = self.scale_prop().get_finite_or_quietly(Scale::NONE);
        let base = Transform::compose(base_translate.resolve(bounds), base_rotate, base_scale);
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
                if !state.translate.is_set() && !state.rotate.is_set() && !state.scale.is_set() {
                    continue;
                }
                let candidate = Transform::compose(
                    state
                        .translate
                        .get_or_untracked(base_translate)
                        .resolve(bounds),
                    state.rotate.get_or_untracked(base_rotate),
                    state.scale.get_or_untracked(base_scale),
                );
                reach = reach.max(outset_of(candidate, painted, bounds, pivot));
            }
        }

        // A spring passes its target before it settles, so the declared reach
        // is inflated by the overshoot still to come — the allowance
        // `max_shadow_extent` makes for a shadow rising from none — and the
        // value in flight, which is already
        // past whatever overshoot it had, is folded in flat.
        if let Some(anims) = anims {
            let overshoot = [
                anims.translate().map(|a| a.peak_overshoot()),
                anims.rotate().map(|a| a.peak_overshoot()),
                anims.scale().map(|a| a.peak_overshoot()),
            ]
            .into_iter()
            .flatten()
            .fold(0.0f32, f32::max);
            reach = reach * (1.0 + overshoot);

            let flying = Transform::compose(
                anims
                    .translate()
                    .map_or(base_translate, |a| *a.current())
                    .resolve(bounds),
                anims.rotate().map_or(base_rotate, |a| *a.current()),
                anims.scale().map_or(base_scale, |a| *a.current()),
            );
            reach = reach.max(outset_of(flying, painted, bounds, pivot));
        }

        reach
    }

    pub(super) fn effective_shadow_target(&self, id: WidgetId) -> Shadows {
        let base = self
            .shadow_prop()
            .get_finite_or(Shadows::none(), id, "shadow");
        self.resolve_state_value(id, base, |state| state.shadow.as_deref()?.get())
    }

    // -----------------------------------------------------------------------
    // Animation: the in-flight value when one is running
    // -----------------------------------------------------------------------

    /// The declared pivot, with one nobody can compute reading as undeclared —
    /// the same rule every other property is resolved by, spelled here because
    /// a pivot has no state layer to be resolved through.
    pub(super) fn resolved_pivot(&self, id: WidgetId) -> Pivot {
        self.pivot_prop().get_finite_or(Pivot::CENTER, id, "pivot")
    }

    /// The same, for the reach calculation: it subscribes like the paint
    /// beside it, and says nothing about a value that is not a number, because
    /// that paint is reading the same signal in the same frame and one warning
    /// is the point.
    pub(super) fn resolved_pivot_quietly(&self) -> Pivot {
        self.pivot_prop().get_finite_or_quietly(Pivot::CENTER)
    }

    /// The declared padding, coerced like every other property.
    ///
    /// Its own accessor because four places read it — the seeding pass, the
    /// drift check, layout and the animated reader — and a property resolved
    /// four ways is a property three of them will eventually resolve wrongly.
    pub(super) fn effective_padding_target(&self, id: WidgetId) -> Padding {
        self.padding
            .get_finite_or(Padding::default(), id, "padding")
    }

    pub(super) fn animated_padding(&self, ctx: &mut LayoutCtx, id: WidgetId) -> Padding {
        match self.anims.as_ref().and_then(|a| a.padding()) {
            Some(anim) => anim.displayed_in(ctx),
            None => self.effective_padding_target(id),
        }
    }

    pub(super) fn animated_background(&self, id: WidgetId) -> Color {
        get_animated_value(self.anims.as_ref().and_then(|a| a.background()), || {
            self.effective_background_target(id)
        })
    }

    pub(super) fn animated_corners(&self, id: WidgetId) -> crate::widgets::Corners {
        get_animated_value(self.anims.as_ref().and_then(|a| a.corners()), || {
            self.effective_corners_target(id)
        })
    }

    /// The border width a layout keeps room for: where its animation is, for
    /// the pass that asked, as the padding beside it is read — never below
    /// zero, because a border cannot give room back.
    ///
    /// Without an animation, the state is asked only where a layer's width
    /// differs from the declared one. A layer that recolours the border at
    /// its own width moves nothing, and asking would put this layout on the
    /// hover of every bordered button. The widths are read tracked, so one
    /// that comes to differ lays the container out and is asked then.
    pub(super) fn laid_out_border_width(&self, ctx: &mut LayoutCtx, id: WidgetId) -> f32 {
        let width = match self.anims.as_ref().and_then(|a| a.border_width()) {
            Some(anim) => anim.displayed_in(ctx),
            None => {
                let base = self.border_width.get_finite_or(0.0, id, "border_width");
                let layers_move_it = self.interaction.as_ref().is_some_and(|ix| {
                    ix.states
                        .iter()
                        .filter_map(|(_, state)| state.border)
                        .any(|border| border.width.get() != base)
                });
                if layers_move_it {
                    self.effective_border_width_target(id)
                } else {
                    base
                }
            }
        };
        width.max(0.0)
    }

    /// Never below zero, as the layout reads it: a spring overshooting past
    /// none must not turn the clip and the hit test inside the border into an
    /// outset.
    pub(super) fn animated_border_width(&self, id: WidgetId) -> f32 {
        get_animated_value(self.anims.as_ref().and_then(|a| a.border_width()), || {
            self.effective_border_width_target(id)
        })
        .max(0.0)
    }

    pub(super) fn animated_blur(&self, id: WidgetId) -> f32 {
        get_animated_value(self.anims.as_ref().and_then(|a| a.blur()), || {
            self.effective_blur_target(id)
        })
    }

    /// Whether anything can draw an outline — the declaration, which an
    /// animation of one comes with, or a state layer. Asked first, so that a
    /// container without one, which is nearly all of them, walks no state
    /// layer three times over to find that out.
    fn declares_outline(&self) -> bool {
        self.outline_width_prop().is_set()
            || self
                .interaction
                .as_ref()
                .is_some_and(|ix| ix.states.iter().any(|(_, s)| s.outline.is_some()))
    }

    pub(super) fn animated_outline(&self, id: WidgetId) -> Option<Outline> {
        if !self.declares_outline() {
            return None;
        }
        let anims = self.anims.as_ref();
        Some(Outline {
            width: get_animated_value(anims.and_then(|a| a.outline_width()), || {
                self.effective_outline_width_target(id)
            }),
            color: get_animated_value(anims.and_then(|a| a.outline_color()), || {
                self.effective_outline_color_target(id)
            }),
            offset: get_animated_value(anims.and_then(|a| a.outline_offset()), || {
                self.effective_outline_offset_target(id)
            }),
        })
    }

    pub(super) fn animated_opacity(&self, id: WidgetId) -> f32 {
        get_animated_value(self.anims.as_ref().and_then(|a| a.opacity()), || {
            self.effective_opacity_target(id)
        })
    }

    pub(super) fn animated_border_color(&self, id: WidgetId) -> Color {
        get_animated_value(self.anims.as_ref().and_then(|a| a.border_color()), || {
            self.effective_border_color_target(id)
        })
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
    /// Uniformly — see [`Shadow::shrunk_to`] — by the factor that brings the
    /// side furthest past its reserved outset inside it, so the shape survives.
    ///
    /// A clamp at all, rather than a wider reach, because the reach that would
    /// make one unnecessary is not a small one. A spring keeps its momentum
    /// across a retarget, so hover flicker *pumps* it: driven at its damped
    /// natural frequency the steady-state excursion is the resonant gain,
    /// `1 / (2ζ√(1-ζ²))` — 1.03x the step response for `BOUNCY`, but 3.7x at
    /// ζ = 0.05, and sizing every damage rect for that is not a trade worth
    /// making for the tip of a bounce nobody asked for. See
    /// `hover_flicker_cannot_push_a_shadow_outside_its_damage_rect`.
    pub(super) fn animated_shadow(&self, id: WidgetId) -> Shadows {
        let anim = self.anims.as_ref().and_then(|a| a.shadow());
        let shadow = get_animated_value(anim, || self.effective_shadow_target(id));
        match anim {
            Some(_) => shadow.shrunk_to(self.shadow_reach.get()),
            // Only where something is in flight, because `shadow_reach` is the
            // number *layout* recorded and layout has not run yet on the frame
            // a container is first painted — it is zero until then, and
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
    ///
    /// `bounds` is what a relative translate is a fraction of, resolved here
    /// and not before, for the reason `Pivot` is resolved at flatten: what
    /// animates is the fraction, so a width change moves the box at once
    /// rather than starting a slide the declaration never asked for.
    pub(super) fn animated_transform(&self, id: WidgetId, bounds: Rect) -> Transform {
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
            get_animated_value(anims.and_then(|a| a.translate()), || {
                self.effective_translate_target(id)
            })
            .resolve(bounds)
        } else {
            (0.0, 0.0)
        };
        let rotate = if has_rotate {
            get_animated_value(anims.and_then(|a| a.rotate()), || {
                self.effective_rotate_target(id)
            })
        } else {
            0.0
        };
        let scale = if has_scale {
            get_animated_value(anims.and_then(|a| a.scale()), || {
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
    /// A border belongs here now that a state layer declares it as a pair:
    /// `when_hovered(|s| s.border(14.0, GREEN))` moves the width as well as the
    /// colour, and without the width the enter queued no Animation job at all.
    /// The spring then started a frame late, and only if something else had
    /// forced that paint. Both halves are in by construction now: a property
    /// counts here when its row says a state layer can override it.
    ///
    /// Not when it follows a signal and does not move the box, which is what
    /// this asked until the border's width took room (#767): the width moves
    /// the box now, and a hover still moves it.
    pub(super) fn has_animated_state_properties(&self) -> bool {
        self.anims
            .as_deref()
            .is_some_and(|declared| declared.slots().any(AnimSlot::a_state_overrides))
    }
}

/// What a clipping container cuts its children to: its box inside the border,
/// as CSS's overflow clip edge is the padding box (#764). Each radius loses the
/// border width, floored at zero, and the curvature is kept.
///
/// Paint cuts to it and the event path tests against it, so a point on the
/// border reaches no child — what is drawn and what is clicked are one shape.
pub(super) fn inside_border(
    bounds: Rect,
    corners: crate::widgets::Corners,
    border_width: f32,
) -> (Rect, crate::widgets::Corners) {
    let less = |radius: f32| (radius - border_width).max(0.0);
    let inner = crate::widgets::Corners {
        radii: corners.radii.to_array().map(less).into(),
        ..corners
    };
    (bounds.inset(border_width), inner)
}

/// A container's outline, resolved for one frame.
#[derive(Clone, Copy)]
pub(super) struct Outline {
    pub width: f32,
    pub color: Color,
    pub offset: f32,
}

impl Outline {
    /// The ring this outline draws around `bounds`, as a border frame: its
    /// outer edge `offset + width` past the box, its inner edge `offset` past
    /// it, each rounded corner grown by as much, as CSS grows an outline's —
    /// a square one stays square — and the curvature kept. `None` where it
    /// draws nothing.
    pub(super) fn ring(
        &self,
        bounds: Rect,
        corners: crate::widgets::Corners,
    ) -> Option<(Rect, crate::widgets::Corners)> {
        if !border_shows(self.width, self.color) {
            return None;
        }
        let out = self.offset + self.width;
        let grow = |radius: f32| {
            if radius > 0.0 {
                (radius + out).max(0.0)
            } else {
                0.0
            }
        };
        let rect = bounds.outset(out);
        (rect.width > 0.0 && rect.height > 0.0).then(|| {
            let ring = crate::widgets::Corners {
                radii: corners.radii.to_array().map(grow).into(),
                ..corners
            };
            (rect, ring)
        })
    }
}

/// The container's own surface, resolved for one frame.
#[derive(Clone, Copy)]
pub(super) struct Decoration {
    pub background: Color,
    pub gradient: Option<LinearGradient>,
    pub corner_radii: crate::renderer::CornerRadii,
    pub corner_curvature: f32,
    pub shadow: Shadows,
    pub border_width: f32,
    pub border_color: Color,
}

impl Decoration {
    /// The shadows and the border a merged shape takes from the container
    /// that merges, by the gates `paint_decoration` puts them through: every
    /// outer shadow that shows, and the border. Its inset shadows stay on its
    /// own box.
    pub(super) fn for_merged_shape(&self) -> (Shadows, Option<crate::renderer::Border>) {
        let mut outer = Shadows::none();
        for shadow in self.shadow.as_slice() {
            if !shadow.inset && shadow.color.a > SHADOW_ALPHA_FLOOR {
                outer.push(*shadow);
            }
        }
        (
            outer,
            border_shows(self.border_width, self.border_color)
                .then(|| crate::renderer::Border::new(self.border_width, self.border_color)),
        )
    }

    /// This decoration without the outer shadows and the border a merged
    /// shape took.
    pub(super) fn without_its_own_edge(&self) -> Self {
        let mut inset = Shadows::none();
        for shadow in self.shadow.as_slice().iter().filter(|s| s.inset) {
            inset.push(*shadow);
        }
        Self {
            shadow: inset,
            border_width: 0.0,
            ..*self
        }
    }
}

impl Container {
    /// What `merge_children` resolves to, if it was declared: a distance no
    /// smaller than zero.
    pub(super) fn merge_distance(&self, id: WidgetId) -> Option<f32> {
        let distance = self.merge_children_prop().get()?;
        if !distance.is_finite() {
            crate::reactive::diagnostics::non_finite_value(id, "merge_children");
            return Some(0.0);
        }
        Some(distance.max(0.0))
    }

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
        let shows = |s: &&Shadow| s.color.a > SHADOW_ALPHA_FLOOR;
        let shadows = d.shadow.as_slice();

        // Gated like the fill, the border and the shadow: a gradient between two
        // fully transparent colours draws nothing, and with both endpoints
        // reactive one animating out through transparent would push a rect per
        // frame to do it. One end still visible is still a gradient.
        let gradient = d.gradient.filter(gradient_shows);
        let fills = gradient.is_some() || d.background.a > 0.0;

        // The first outer shadow rides the fill's own instance, as the only
        // shadow always did, so a box with one costs what it did. Every other
        // one is a rect of its own that draws nothing but its shadow, under the
        // fill and the last of them first, so the first in the list ends on top
        // as CSS stacks them — the order a box per shadow, each on the last,
        // would draw them in.
        let mut shadow = None;
        if fills {
            let mut outer = shadows.iter().filter(|s| !s.inset).filter(shows);
            shadow = outer.next().copied();
            for under in outer.rev() {
                ctx.draw_rounded_rect_with_shadow(
                    bounds,
                    Color::TRANSPARENT,
                    d.corner_radii,
                    d.corner_curvature,
                    *under,
                );
            }
        }

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

        // Inset shadows go over the fill and under the border, a rect each,
        // the last first so the first is on top. They need no fill to be seen
        // — a glow inside an empty box is a glow — so unlike the outer ones
        // they are not gated on one.
        //
        // Each is cast inside the border, as CSS casts one inside the padding
        // box. **On these rects the `border` field is not a border to draw: it
        // carries the padding-box inset**, the container's border width in a
        // colour that draws nothing, and the shader casts the shadow inside
        // `dist + border_width` — the same edge the border frame drawn next is
        // cut along, so the two meet exactly. Anything that drops or culls a
        // transparent border has to leave these alone, or an inset shadow
        // slides under the border.
        let inside_border = (d.border_width > 0.0)
            .then(|| crate::renderer::Border::new(d.border_width, Color::TRANSPARENT));
        for inset in shadows.iter().filter(|s| s.inset).filter(shows).rev() {
            ctx.draw_rounded_rect_full(
                bounds,
                Color::TRANSPARENT,
                d.corner_radii,
                d.corner_curvature,
                inside_border,
                Some(*inset),
                None,
            );
        }

        // A border that resolves to nothing visible must not reach the instance
        // buffer every frame. Both halves are always declared together, so this
        // is only reached deliberately — `border(2.0, TRANSPARENT)`,
        // `border(0.0, RED)` — or in passing, while an animated colour crosses
        // transparent.
        if border_shows(d.border_width, d.border_color) {
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

impl Container {
    /// Whether this container draws a surface of its own: a fill, a border
    /// frame or a backdrop blur, by the gates `paint` puts each through. A
    /// shadow does not count — it rides the fill, and draws nothing without
    /// one.
    ///
    /// What makes a point inside its shape a point *on* it, which covers the
    /// siblings painted beneath it (#636).
    pub(super) fn draws_its_own_surface(&self, id: WidgetId) -> bool {
        // The plain fill first: it is the common surface, and the gradient the
        // dearest to resolve.
        self.animated_background(id).a > 0.0
            || self
                .effective_gradient(id)
                .is_some_and(|g| gradient_shows(&g))
            || border_shows(
                self.animated_border_width(id),
                self.animated_border_color(id),
            )
            || self
                .backdrop_blur
                .get()
                .is_some_and(|blur| blur.asks_for_anything())
    }
}

/// A gradient between two fully transparent colours draws nothing; one end
/// still visible is still a gradient.
fn gradient_shows(gradient: &LinearGradient) -> bool {
    gradient.start_color.a > 0.0 || gradient.end_color.a > 0.0
}

/// A border draws only with both a width and a colour that shows.
fn border_shows(width: f32, color: Color) -> bool {
    width > 0.0 && color.a > 0.0
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
/// Side by side, as the transform leaves the mapped rect: a card that lifts
/// reaches above its box and nowhere else, and a reach that is too generous
/// costs paint and damage while one that is too tight loses a widget.
fn outset_of(transform: Transform, painted: Rect, bounds: Rect, pivot: Pivot) -> Outsets {
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

/// A colour as a state layer resolves it: its background override, then its
/// alpha, or `None` where the layer declares neither.
fn state_colour(base: Color, state: &StateStyle) -> Option<Color> {
    let alpha = state.background_alpha.get();
    if state.background.is_none() && alpha.is_none() {
        return None;
    }
    let colour = state
        .background
        .as_ref()
        .map_or(base, |bg| resolve_background(base, bg));
    Some(alpha.map_or(colour, |a| colour.with_alpha(a)))
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
            let spin = Transform::compose((0.0, 0.0), degrees as f32, Scale::NONE);
            worst = worst.max(outset_of(spin, BOX, BOX, Pivot::TOP_LEFT).widest());
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
            Outsets::all(10.0)
        );
    }

    /// The shadow and the transform compose. This is the case the review
    /// found: measured against the shadow-inflated box the answer came out 50,
    /// and the shadow really reaches 60.
    ///
    /// And only upward. The lift carries the box away from its bottom edge, so
    /// nothing is drawn below it at all, and its sides keep the shadow's 10.
    /// One number for the four reported 60 on every side — 50 pixels of
    /// damage either side of a card that only ever moved up.
    #[test]
    fn a_shadow_carried_by_a_translate_reaches_past_both() {
        let painted = BOX.outset(10.0);
        let lift = Transform::compose((0.0, -50.0), 0.0, Scale::NONE);
        assert_eq!(
            outset_of(lift, painted, BOX, Pivot::CENTER),
            Outsets {
                left: 10.0,
                top: 60.0,
                right: 10.0,
                bottom: 0.0,
            }
        );
    }

    /// And a consumer growing the box by the answer contains what is drawn,
    /// which is the whole promise: a widget on screen is never culled.
    #[test]
    fn the_grown_box_contains_what_is_drawn() {
        let painted = BOX.outset(10.0);
        let lift = Transform::compose((0.0, -50.0), 0.0, Scale::NONE);
        let reach = outset_of(lift, painted, BOX, Pivot::CENTER);

        let drawn = lift.about(Pivot::CENTER, painted).map_rect(painted);
        let grown = BOX.outset_by(reach);
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
        let grow = Transform::compose((0.0, 0.0), 0.0, Scale::uniform(2.0));
        // The 120-wide painted box doubles about the centre it shares with
        // bounds, reaching 120 either side of it; 70 of that stands outside the
        // 100-wide bounds. A shadow that did not scale would reach 60.
        assert_eq!(
            outset_of(grow, painted, BOX, Pivot::CENTER),
            Outsets::all(70.0)
        );
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
            let bound = turning.max_transform_reach(BOX, Outsets::all(shadow));

            let worst = (0..360)
                .map(|deg| {
                    let spin = Transform::compose((0.0, 0.0), deg as f32, Scale::NONE);
                    outset_of(spin, painted, BOX, pivot)
                })
                .fold(Outsets::ZERO, Outsets::max);

            // Side by side: a corner pivot sweeps far past the two edges it
            // sits away from and less past the two it sits on.
            assert!(
                bound.covers(worst - Outsets::all(0.01)),
                "{pivot:?}: the bound is {bound:?} and the widest angles reach \
                 {worst:?}, so a container mid-rotation would be culled"
            );
            // And tight. A bound that is merely large is satisfied by any
            // arithmetic that errs upward, which leaves the whole computation
            // unwatched — and every pixel of slack is painted and damaged.
            assert!(
                (worst + Outsets::all(0.01)).covers(bound),
                "{pivot:?}: the bound is {bound:?} where the widest angles reach \
                 only {worst:?}, so every rotation pays for reach it cannot use"
            );
        }
    }

    /// And it is a bound, not a shrug: a box that cannot rotate is not given
    /// the reach of one that can.
    #[test]
    fn a_box_that_cannot_rotate_is_not_given_a_rotation_s_reach() {
        let still = container().width(100.0).height(100.0);
        assert_eq!(still.max_transform_reach(BOX, Outsets::ZERO), Outsets::ZERO);
        assert_eq!(
            still.max_transform_reach(BOX, Outsets::all(6.0)),
            Outsets::all(6.0),
            "only its shadow"
        );
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

        let reach = sprung.max_transform_reach(BOX, Outsets::ZERO);
        assert!(
            reach.top > 100.0,
            "a spring to -100 was given a reach of {reach:?}, so it is culled at \
             the far end of its own overshoot"
        );

        let eased = container()
            .width(100.0)
            .height(100.0)
            .translate(Translate::new(0.0, -100.0).transition(200.0));
        assert_eq!(
            eased.max_transform_reach(BOX, Outsets::ZERO),
            Outsets {
                top: 100.0,
                ..Outsets::ZERO
            },
            "an eased translate overshoots nothing and should pay for nothing"
        );
    }

    /// A relative translate reaches as far as the box it is a fraction of: one
    /// width left is `BOX.width` outside it, in the declaration and in a state
    /// layer. Read as pixels it reaches nothing, and a slide that starts
    /// off-screen is culled for as long as it is sliding.
    #[test]
    fn a_relative_translate_reaches_by_the_box_it_is_a_fraction_of() {
        let declared = container()
            .width(100.0)
            .height(100.0)
            .translate(Translate::relative(-1.0, 0.0));
        assert_eq!(
            declared.max_transform_reach(BOX, Outsets::ZERO),
            Outsets {
                left: BOX.width,
                ..Outsets::ZERO
            }
        );

        let layered = container()
            .width(100.0)
            .height(100.0)
            .when_hovered(|s| s.translate(Translate::relative(0.0, 0.5)));
        assert_eq!(
            layered.max_transform_reach(BOX, Outsets::ZERO),
            Outsets {
                bottom: BOX.height / 2.0,
                ..Outsets::ZERO
            }
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
            scaled.max_transform_reach(BOX, Outsets::ZERO),
            Outsets::all(50.0),
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
            lifted.max_transform_reach(BOX, Outsets::ZERO),
            Outsets {
                top: 25.0,
                ..Outsets::ZERO
            },
            "a layer that lifts this box on hover was not counted"
        );

        let turned = container()
            .width(100.0)
            .height(100.0)
            .when_hovered(|s| s.rotate(45.0));
        assert!(
            turned.max_transform_reach(BOX, Outsets::ZERO).widest() > 0.0,
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
                Transform::compose((0.0, 0.0), deg, Scale::NONE),
                BOX,
                BOX,
                Pivot::CENTER,
            )
            .widest()
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
