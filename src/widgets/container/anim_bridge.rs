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

use super::box_model::{BoxLengths, size_target};
use super::*;
use crate::tree::LayoutCtx;

impl Container {
    /// Point the size animations at the content just measured.
    ///
    /// A size target is content-driven, so unlike every other animated
    /// property it is recomputed from scratch at each layout and needs no
    /// subscription. Retargeting queues a layout of the container, which
    /// reaches its parent like any change to a widget's own size (see
    /// `Tree::mark_needs_layout`): a child whose width is moving makes its
    /// siblings move too.
    pub(super) fn update_size_targets(
        &mut self,
        ctx: &mut LayoutCtx,
        id: WidgetId,
        lengths: &BoxLengths,
        content: Size,
    ) {
        // Leaving, nothing follows the content: a size with an exit goes where
        // removal sent it, and one without stays where it was rather than
        // follow a measure of content whose item may be gone. Nor under a read
        // for a size alone, laid out in other room than the placement after
        // it: pointed by both, a size would be sent toward the read's content
        // and back on every layout, and enter from it the first time.
        if self.is_leaving() || ctx.sizing() {
            return;
        }
        let frame_instant = ctx.tree_ref().frame_instant();
        // A measure has already said where the content is heading this frame
        // — an ancestor's, at the box the ancestor is heading to — so a size
        // that follows it keeps that target, rather than one taken from the
        // content laid out in a box the ancestor is only passing through.
        // Taking that one restarted the transition on every frame.
        let measured = ctx.measured_this_frame();
        let Some(ref mut anims) = self.anims else {
            return;
        };
        let mut retargeted = false;
        // What a read before this placement answered, which it answered for a
        // size nothing had placed yet: see below.
        let read = ctx.read_answer(id);
        let inset = lengths.inset();

        // The two size slots, and whichever of them was declared.
        for slot in anims.slots_mut() {
            let (anim, length, extent, was_read) = match slot {
                AnimSlot::width(anim) => (
                    anim,
                    &lengths.width,
                    content.width + inset.horizontal_total(),
                    read.map(|size| size.width),
                ),
                AnimSlot::height(anim) => (
                    anim,
                    &lengths.height,
                    content.height + inset.vertical_total(),
                    read.map(|size| size.height),
                ),
                _ => continue,
            };
            let target = size_target(length, extent);

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
                    // A read before this placement answered where the
                    // placement would put the size in the read's room, and
                    // this one put it somewhere else without marking
                    // anything: laid out again, the read answers where it is.
                    retargeted |= !ctx.measuring()
                        && was_read.is_some_and(|read| (read - target).abs() > 0.001);
                }
            } else if !(measured && length.exact_size().is_none())
                && (target - *anim.target()).abs() > 0.001
            {
                anim.animate_to(target, frame_instant);
                retargeted = true;
            }
        }

        if retargeted {
            // A size animation moves layout, not just paint.
            request_job(id, JobRequest::Animation(RequiredJob::Layout));
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
