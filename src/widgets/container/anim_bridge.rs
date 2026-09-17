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
//! Three passes are where that invariant lives, and two of them are emitted
//! from the table in `animated_properties.rs`, a row at a time:
//!
//! - [`seed_animations`] is (1): at the first layout, every declared animation
//!   is read under Animation tracking so its subscription starts *now*. Widget
//!   creation and first layout run in one synchronous block — for popups, the
//!   measure-before-spawn — so nothing can land in between. It runs *before*
//!   the measurement, not after: padding is a property a layout reads, and one
//!   placed after `read_box_lengths` is a value that layout never saw. On the
//!   popup path there is no later pass to repair that — the size is a return
//!   value.
//! - [`resync_animation_targets`] is (2): every paint re-reads the targets
//!   under tracking and asks for an Animation job when one has drifted, so
//!   `advance_animations` adopts whatever the subscription missed.
//!
//! What is left here is the third, which is neither, and that is the point:
//! [`update_size_targets`] follows the measured content rather than a signal,
//! so a size's target is simply recomputed at every layout — by the one
//! formula that is the sizes' own. [`seed_or_enter`] is beside it, called from
//! the arm the table generates for each property, where the type it animates
//! is known.
//!
//! [`seed_animations`]: Container::seed_animations
//! [`resync_animation_targets`]: Container::resync_animation_targets
//! [`update_size_targets`]: Container::update_size_targets

use super::box_model::BoxLengths;
use super::*;
use crate::tree::LayoutCtx;

impl Container {
    /// Point the size animations at the content just measured.
    ///
    /// A size target is content-driven, so unlike every other animated
    /// property it is recomputed from scratch at each layout and needs no
    /// subscription. Retargeting also invalidates the *parent*: a child whose
    /// width is moving makes its siblings move too.
    pub(super) fn update_size_targets(
        &mut self,
        ctx: &mut LayoutCtx,
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
        let parent = ctx.tree_ref().get_parent(id);
        let frame_instant = ctx.tree_ref().frame_instant();
        let Some(ref mut anims) = self.anims else {
            return;
        };
        let mut retargeted = false;

        // The two size slots, in the order the targets above are written, and
        // whichever of them was declared.
        for slot in anims.slots_mut() {
            let (anim, (exact, min, max, content_extent)) = match slot {
                AnimSlot::width(anim) => (anim, targets[0]),
                AnimSlot::height(anim) => (anim, targets[1]),
                _ => continue,
            };
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
                // Mark initialized at the first *real* layout whatever the
                // value, so later changes animate instead of snapping — unless
                // an enter was declared, in which case this is the appearance it
                // was waiting for and the size animates from there. A measure
                // pass places without marking, so the appearance is still to
                // come: see `set_immediate`.
                if anim.begin_enter(ctx, target, || frame_instant) {
                    retargeted = true;
                } else {
                    anim.set_immediate(ctx, target);
                }
            } else if (target - *anim.target()).abs() > 0.001 {
                anim.animate_to(target, frame_instant);
                retargeted = true;
            }
        }

        if retargeted {
            // A size animation moves layout, not just paint.
            request_job(id, JobRequest::Animation(RequiredJob::Layout));
            if let Some(parent_id) = parent {
                request_job(parent_id, JobRequest::Layout);
            }
        }
    }
}

/// Start one animated property at the target just read for it, or begin the
/// enter it declared. Returns whether this frame started something, which is
/// what needs another frame after it.
///
/// Called from the arm the table generates for each property, which is where
/// the type it animates is known.
pub(super) fn seed_or_enter<T: crate::animation::Animatable>(
    ctx: &mut LayoutCtx,
    anim: &mut AnimationState<T>,
    target: T,
    now: impl FnOnce() -> crate::clock::FrameInstant,
) -> bool {
    let entered = anim.begin_enter(ctx, target, now);
    if !entered {
        anim.set_immediate(ctx, target);
    }
    // A sequence with no trigger is owed a play, and only this pass can ask for
    // the frame that gives it one: a triggered sequence wakes its container
    // through the signal it reads, and one that plays because the widget exists
    // has no signal to read. Starting it stays where every other play starts,
    // in the animate pass — asked for here, played there.
    entered || anim.owes_its_first_play()
}
