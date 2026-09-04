//! Keeping animation state in step with the signals it mirrors.
//!
//! An [`AnimationState`] is a **cache of signal-derived state**: it holds a
//! target computed from a signal (plus whatever the active state layer
//! overrides) and interpolates toward it. A cache like that is only correct if
//! it is both
//!
//! 1. **subscribed from the moment it exists**, so no write can slip past
//!    before anyone is listening, and
//! 2. **reconciled against the live value at every use**, so a target reached
//!    through a branch nobody tracked still converges.
//!
//! Neither half is sufficient on its own, and both failure modes have been
//! shipped: a menu whose open-flip timer fired between its first layout and a
//! heavy first paint stayed collapsed forever (no subscription yet), and a
//! transform whose target moved through a conditional closure branch sat on
//! the stale value (subscribed, but to the branch not taken).
//!
//! The three passes below are where that invariant lives.
//!
//! - [`seed_animations`] is (1): at the first layout, every animated property
//!   is read under Animation tracking so its subscription starts *now*. Widget
//!   creation and first layout run in one synchronous block — for popups, the
//!   measure-before-spawn — so nothing can land in between.
//! - [`resync_animation_targets`] is (2): every paint re-reads the targets
//!   under tracking and asks for an Animation job when one has drifted, so
//!   `advance_animations` adopts whatever the subscription missed.
//! - [`update_size_targets`] is neither, and that is the point: width and
//!   height do not mirror a signal, they follow the measured content, so their
//!   targets are simply recomputed at every layout.
//!
//! [`seed_animations`]: Container::seed_animations
//! [`resync_animation_targets`]: Container::resync_animation_targets
//! [`update_size_targets`]: Container::update_size_targets

use super::box_model::BoxLengths;
use super::*;

impl Container {
    /// Point the size animations at the content just measured.
    ///
    /// A size target is content-driven, so unlike every other animated
    /// property it is recomputed from scratch at each layout and needs no
    /// subscription. Retargeting also invalidates the *parent*: a child whose
    /// width is moving makes its siblings move too.
    pub(super) fn update_size_targets(
        &mut self,
        tree: &Tree,
        id: WidgetId,
        lengths: &BoxLengths,
        content: Size,
    ) {
        let targets = [
            (
                lengths.width.exact,
                lengths.width.min,
                lengths.width.max,
                content.width + lengths.padding.horizontal_total(),
            ),
            (
                lengths.height.exact,
                lengths.height.min,
                lengths.height.max,
                content.height + lengths.padding.vertical_total(),
            ),
        ];
        let Some(ref mut anims) = self.anims else {
            return;
        };
        let mut retargeted = false;

        for (anim, (exact, min, max, content_extent)) in
            [anims.width.as_mut(), anims.height.as_mut()]
                .into_iter()
                .zip(targets)
        {
            let Some(anim) = anim else { continue };
            // The target is where the size is heading, so it obeys the same
            // bounds the size does. An unclamped one animates toward a value
            // `resolve_size` will cap anyway: frames spent going nowhere.
            let target = exact.unwrap_or_else(|| {
                let grown = content_extent.max(min.unwrap_or(0.0));
                match max {
                    Some(max) => grown.min(max),
                    None => grown,
                }
            });

            if anim.is_initial() {
                // Mark initialized at the first layout whatever the value, so
                // later changes animate instead of snapping.
                anim.set_immediate(target);
            } else if (target - *anim.target()).abs() > 0.001 {
                anim.animate_to(target, tree.frame_instant());
                retargeted = true;
            }
        }

        if retargeted {
            // A size animation moves layout, not just paint.
            request_job(id, JobRequest::Animation(RequiredJob::Layout));
            if let Some(parent_id) = tree.get_parent(id) {
                request_job(parent_id, JobRequest::Layout);
            }
        }
    }

    /// First layout: subscribe every animated property and start it from the
    /// value its signal actually holds, not the one captured at construction.
    ///
    /// See the module docs for why this cannot wait for the first paint.
    pub(super) fn seed_animations(&mut self, id: WidgetId) {
        // Written out one by one: each property animates a different type, so
        // there is no single accessor to loop over.
        let anims = self.anims.as_ref();
        let pd_init = anims.is_some_and(|a| a.padding.as_ref().is_some_and(|a| a.is_initial()));
        let bw_init =
            anims.is_some_and(|a| a.border_width.as_ref().is_some_and(|a| a.is_initial()));
        let bg_init = anims.is_some_and(|a| a.background.as_ref().is_some_and(|a| a.is_initial()));
        let cr_init = anims.is_some_and(|a| a.corners.as_ref().is_some_and(|a| a.is_initial()));
        let sh_init = anims.is_some_and(|a| a.shadow.as_ref().is_some_and(|a| a.is_initial()));
        let bc_init =
            anims.is_some_and(|a| a.border_color.as_ref().is_some_and(|a| a.is_initial()));
        let tr_init = anims.is_some_and(|a| a.translate.as_ref().is_some_and(|a| a.is_initial()));
        let ro_init = anims.is_some_and(|a| a.rotate.as_ref().is_some_and(|a| a.is_initial()));
        let sc_init = anims.is_some_and(|a| a.scale.as_ref().is_some_and(|a| a.is_initial()));

        if !(pd_init
            || bw_init
            || bg_init
            || cr_init
            || sh_init
            || bc_init
            || tr_init
            || ro_init
            || sc_init)
        {
            return;
        }

        // Targets are computed under `&self` first: the writes below need
        // `&mut self.anims`, and the effective_* readers need `&self`.
        let (bg_target, cr_target, sh_target, bc_target, tr_target, ro_target, sc_target) =
            with_signal_tracking(id, JobType::Animation, || {
                // Read for the subscription even where the value is unused.
                if pd_init {
                    let _ = self.padding.get_or(Padding::default());
                }
                if bw_init {
                    let _ = self.effective_border_width_target(id);
                }
                (
                    bg_init.then(|| self.effective_background_target(id)),
                    cr_init.then(|| self.effective_corners_target(id)),
                    sh_init.then(|| self.effective_shadow_target(id)),
                    bc_init.then(|| self.effective_border_color_target(id)),
                    tr_init.then(|| self.effective_translate_target(id)),
                    ro_init.then(|| self.effective_rotate_target(id)),
                    sc_init.then(|| self.effective_scale_target(id)),
                )
            });

        let Some(ref mut anims) = self.anims else {
            return;
        };
        // One line per property rather than a loop: each animates a
        // different type, so there is nothing to iterate over.
        seed(&mut anims.background, bg_target);
        seed(&mut anims.corners, cr_target);
        seed(&mut anims.shadow, sh_target);
        seed(&mut anims.border_color, bc_target);
        seed(&mut anims.translate, tr_target);
        seed(&mut anims.rotate, ro_target);
        seed(&mut anims.scale, sc_target);
    }

    /// Every paint: re-read the animated targets under tracking, and ask for an
    /// Animation job if any has drifted from what its animation is aiming at.
    ///
    /// This is the pull half of the invariant. It keeps the subscriptions
    /// current through conditional closure branches and state-layer overrides,
    /// and it converges any write that landed before the subscription existed.
    pub(super) fn resync_animation_targets(&self, id: WidgetId) {
        if !self.has_signal_animated_props() {
            return;
        }

        let drifted = with_signal_tracking(id, JobType::Animation, || {
            let anims = self.anims.as_ref().expect("checked by has_signal_*");
            // An animation still in its initial state has no target to drift
            // from — seed_animations has not run for it yet.
            let moved = |initial: bool, same: bool| !initial && !same;
            // Each property answers twice: has its target drifted, and has its
            // timeline been asked to play. Both reads are the subscription, so
            // every one runs whatever the last one answered — which is why
            // these are `|=` statements and not a `||` chain.
            let mut drift = false;

            if let Some(a) = &anims.padding {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.padding.get_or(Padding::default()),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.border_width {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_border_width_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.background {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_background_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.corners {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_corners_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.shadow {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_shadow_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.border_color {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_border_color_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.translate {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_translate_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.rotate {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_rotate_target(id),
                );
                drift |= a.wants_play();
            }
            if let Some(a) = &anims.scale {
                drift |= moved(
                    a.is_initial(),
                    *a.target() == self.effective_scale_target(id),
                );
                drift |= a.wants_play();
            }
            drift
        });

        if drifted {
            request_job(id, JobRequest::Animation(RequiredJob::None));
        }
    }
}

/// Start one animated property at the target just read for it.
///
/// Written as a call per property rather than a loop, because each animates a
/// different type and there is no single accessor to iterate.
fn seed<T: crate::animation::Animatable>(slot: &mut Option<AnimationState<T>>, target: Option<T>) {
    if let (Some(anim), Some(target)) = (slot, target) {
        anim.set_immediate(target);
    }
}
