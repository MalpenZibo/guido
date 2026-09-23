//! The eleven animated properties of `Container`, declared in one table.
//!
//! An animated property used to be written out by name in nine places that
//! nothing kept in step — the field, the seed, the target, the drift check,
//! the timeline start, the advance, two predicates in `style.rs` and the
//! subset `box_model.rs` asks about. Nothing failed to build when one was
//! missed, and a property could be seeded but never advanced, or advanced but
//! never resynced.
//!
//! Those nine lists differ from each other by two facts per property, not
//! nine:
//!
//! - **Where its target comes from.** All but `width` and `height` read it
//!   from the signal they were declared with, which is what
//!   [`Container::effective_background_target`] and its siblings do, under a
//!   tracking scope. A size's target is whatever the layout just measured, so
//!   it is recomputed from scratch each time and needs no subscription — see
//!   `update_size_targets`.
//! - **Whether it moves the box.** `width`, `height` and `padding` do, which
//!   is what `is_relayout_boundary_for` asks and what splits the advance into
//!   a Layout half and a Paint half.
//!
//! So one row states a property and every list is emitted from it. The table
//! is an *X-macro*: [`animated_properties!`] holds the rows and takes the name
//! of the macro to hand them to, so each site — the store, the passes, the
//! tests — reads the same list without a second copy of it existing.
//!
//! Adding an animated property is adding a row. Forgetting one of the nine
//! sites is no longer possible, because there are no longer nine sites to
//! forget.

use super::animations::AnimationState;
use super::{Color, Padding, Scale, Shadow, Translate};
use crate::clock::FrameInstant;
use crate::jobs::{JobRequest, JobType, RequiredJob, request_job};
use crate::reactive::with_signal_tracking;
use crate::tree::{LayoutCtx, WidgetId};
use crate::widgets::Corners;

/// The table. Hands its rows to `$emit`, which decides what to build from
/// them.
///
/// A row is
///
/// ```text
/// name: AnimatedType as DeclaredType, target: <fn> | none, layout: yes | no,
///     timeline: yes | no,
///     test { value, declared_as, probe, from, to, then, base };
/// ```
///
/// - **`AnimatedType`** is what moves — what `AnimationState<T>` holds.
///   **`DeclaredType`** is what the setter takes. They differ for `width` and
///   `height`, which declare a `Length` and animate the `f32` in it.
/// - **`target`** names the `&self` method that reads where the property is
///   heading, or `none` for the sizes, whose target the layout supplies.
/// - **`layout`** says whether moving it re-measures the box.
/// - **`timeline`** says whether a sequence can be declared on it. `no` for
///   the sizes and only for them: `Keyframes<Length>` has no constructor, so
///   `width(w.timeline(..))` does not compile. Stated rather than inferred
///   from `target`, because they are two different facts that happen to
///   divide the same two rows off today.
/// - **`test`** is the property's own recipe, so the generated test can say
///   what it means to declare, watch and write *this* property: how one number
///   becomes a value of its type, a container built around the declaration, a
///   probe reading that number back off the painted frame, and the three
///   numbers it moves between — where it enters from, where it lands, and
///   where a later write sends it; `from` is where it leaves to as well, when
///   the container is removed from the one holding it — and `base`, the value
///   a timeline is
///   declared at, which is `from` for every row but `shadow`'s, for the reason
///   written there.
///
/// The test half names `H`, `container()`, `box_of`, `rects`, `borders` and
/// `painted_style`: it is written here and resolved in
/// `characterization.rs`, which is the only module that asks for those rows.
macro_rules! animated_properties {
    ($emit:ident) => {
        $emit! {
            width: f32 as Length, target: none, layout: yes,
                timeline: no,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(v).height(100.0),
                    probe: |h: &mut H| h.tree.cached_size(h.root).unwrap().width,
                    from: 10.0, to: 150.0, then: 50.0, base: 10.0,
                };
            height: f32 as Length, target: none, layout: yes,
                timeline: no,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(200.0).height(v),
                    probe: |h: &mut H| h.tree.cached_size(h.root).unwrap().height,
                    from: 10.0, to: 150.0, then: 50.0, base: 10.0,
                };
            padding: Padding as Padding, target: effective_padding_target, layout: yes,
                timeline: yes,
                test {
                    value: |k| Padding::all(k),
                    declared_as: |v| container().padding(v).child(box_of(10.0, 10.0)),
                    probe: |h: &mut H| {
                        // Painted like every other probe, because a paint is
                        // where a target is resynced and a trigger is read —
                        // and then measured off the box it grows rather than
                        // the child it pushes, whose origin belongs to the
                        // layout that placed it a frame earlier.
                        h.paint();
                        (h.tree.cached_size(h.root).unwrap().width - 10.0) / 2.0
                    },
                    from: 4.0, to: 40.0, then: 16.0, base: 4.0,
                };
            background: Color as Color, target: effective_background_target, layout: no,
                timeline: yes,
                test {
                    value: |k| Color::rgb(k, 0.0, 0.0),
                    declared_as: |v| container().width(200.0).height(100.0).background(v),
                    probe: |h: &mut H| rects(&h.paint())[0].1.r,
                    from: 0.0, to: 1.0, then: 0.5, base: 0.0,
                };
            corners: Corners as Corners, target: effective_corners_target, layout: no,
                timeline: yes,
                test {
                    value: |k| crate::widgets::Corners::rounded(k),
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).corners(v),
                    probe: |h: &mut H| painted_style(&h.paint()).1.top_left,
                    from: 2.0, to: 20.0, then: 8.0, base: 2.0,
                };
            shadow: Shadow as Shadow, target: effective_shadow_target, layout: no,
                timeline: yes,
                test {
                    value: |k| Shadow::new((0.0, 0.0), k, 0.0, Color::BLACK),
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).shadow(v),
                    probe: |h: &mut H| painted_style(&h.paint()).3.unwrap().blur,
                    from: 2.0, to: 20.0, then: 8.0,
                    // A shadow's sequence is declared at the *deepest* value it
                    // reaches, not the first: paint clamps a shadow to the reach
                    // `layout` recorded from the declared one, and a timeline is
                    // not part of that reach — declared at 2 it would play 2 to
                    // 20 and draw 2 throughout. See #384.
                    base: 20.0,
                };
            border_width: f32 as f32, target: effective_border_width_target, layout: no,
                timeline: yes,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).border(v, Color::WHITE),
                    probe: |h: &mut H| borders(&h.paint())[0].0,
                    from: 1.0, to: 10.0, then: 4.0, base: 1.0,
                };
            border_color: Color as Color, target: effective_border_color_target, layout: no,
                timeline: yes,
                test {
                    value: |k| Color::rgb(k, 0.0, 0.0),
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).border(2.0, v),
                    probe: |h: &mut H| borders(&h.paint())[0].1.r,
                    from: 0.0, to: 1.0, then: 0.5, base: 0.0,
                };
            translate: Translate as Translate, target: effective_translate_target, layout: no,
                timeline: yes,
                test {
                    value: |k| Translate::new(k, 0.0),
                    declared_as: |v| container().width(200.0).height(100.0).translate(v),
                    probe: |h: &mut H| h.paint().local_transform.tx(),
                    from: 5.0, to: 50.0, then: 20.0, base: 5.0,
                };
            rotate: f32 as f32, target: effective_rotate_target, layout: no,
                timeline: yes,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(200.0).height(100.0).rotate(v),
                    probe: |h: &mut H| turned_degrees(&h.paint()),
                    from: 4.0, to: 40.0, then: 16.0, base: 4.0,
                };
            scale: Scale as Scale, target: effective_scale_target, layout: no,
                timeline: yes,
                test {
                    value: |k| Scale::uniform(k),
                    declared_as: |v| container().width(200.0).height(100.0).scale(v),
                    probe: |h: &mut H| h.paint().local_transform.data[0],
                    from: 0.5, to: 2.0, then: 1.25, base: 0.5,
                };
        }
    };
}

// The emitters below reach the rows where they are written; this is what lets
// `characterization.rs` reach them by path, to build a test out of each.
#[cfg(test)]
pub(crate) use animated_properties;

/// The store: one slot per property that was *declared* with a motion, rather
/// than a field per property whether or not anybody asked for one.
macro_rules! emit_store {
    ($($name:ident: $anim:ty as $decl:ty, target: $target:tt, layout: $layout:ident,
        timeline: $timeline:ident,
        test { $($test:tt)* };)*) => {
        /// One declared animation, and which property declared it.
        ///
        /// The variants are named exactly as the properties are, so a row of
        /// the table and the slot it makes cannot be spelled apart.
        #[allow(non_camel_case_types)]
        pub(crate) enum AnimSlot {
            $($name(AnimationState<$anim>),)*
        }

        /// Which property a slot belongs to, with nothing in it — what a
        /// declaration names when it takes an animation *away*.
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(crate) enum AnimKind {
            $($name,)*
        }

        impl AnimSlot {
            pub(crate) fn kind(&self) -> AnimKind {
                match self {
                    $(AnimSlot::$name(_) => AnimKind::$name,)*
                }
            }
        }

        /// The animations a container declared, in the order it declared them.
        ///
        /// Two inline. One would be smaller — a `SmallVec` of one slot is 320
        /// bytes against 624 for two, because the inline buffer and the heap
        /// pointer share their space — but declaring two is ordinary: a border
        /// is a width and a colour, a press is a scale over a background. The
        /// second slot buys those the allocation they would otherwise make,
        /// and both numbers are far below the 2464 bytes every animated
        /// container used to carry.
        #[derive(Default)]
        pub(crate) struct ContainerAnims {
            slots: smallvec::SmallVec<[AnimSlot; 2]>,
        }

        impl ContainerAnims {
            /// Put a declared animation where the property keeps it, replacing
            /// whatever that property declared before.
            pub(crate) fn set(&mut self, slot: AnimSlot) {
                match self.slots.iter_mut().find(|s| s.kind() == slot.kind()) {
                    Some(existing) => *existing = slot,
                    None => self.slots.push(slot),
                }
            }

            /// Take one away: a declaration restated without a motion.
            pub(crate) fn clear(&mut self, kind: AnimKind) {
                self.slots.retain(|slot| slot.kind() != kind);
            }

            /// How many animations were declared. What a container pays for,
            /// which is what the test of that pays attention to.
            #[cfg(test)]
            pub(crate) fn declared_count(&self) -> usize {
                self.slots.len()
            }

            /// What is declared, in declaration order.
            pub(crate) fn slots(&self) -> impl Iterator<Item = &AnimSlot> {
                self.slots.iter()
            }

            /// The same, to advance or to seed.
            pub(crate) fn slots_mut(&mut self) -> impl Iterator<Item = &mut AnimSlot> {
                self.slots.iter_mut()
            }

            $(
                /// The animation declared for this property, if one was — a
                /// walk over what this container declared, not a field read.
                pub(crate) fn $name(
                    &self,
                ) -> Option<&AnimationState<$anim>> {
                    self.slots.iter().find_map(|slot| match slot {
                        AnimSlot::$name(anim) => Some(anim),
                        _ => None,
                    })
                }
            )*
        }
    };
}

animated_properties!(emit_store);

/// A row's two facts, asked in expression position — which is how an arm
/// generated for every property can still do the one thing that property's row
/// says.
macro_rules! follows_a_signal {
    (none) => {
        false
    };
    ($target:ident) => {
        true
    };
}
macro_rules! moves_the_box {
    (yes) => {
        true
    };
    (no) => {
        false
    };
}

/// What a slot knows about itself, so the three predicates that used to be
/// three lists are now three questions asked of whatever is declared.
macro_rules! emit_slot_facts {
    ($($name:ident: $anim:ty as $decl:ty, target: $target:tt, layout: $layout:ident,
        timeline: $timeline:ident,
        test { $($test:tt)* };)*) => {
        impl AnimSlot {
            /// Whether this property reads its own target from the signal it
            /// was declared with. The sizes do not: the layout hands them one.
            pub(crate) fn follows_a_signal(&self) -> bool {
                match self {
                    $(AnimSlot::$name(_) => follows_a_signal!($target),)*
                }
            }

            /// Whether moving it re-measures the box.
            pub(crate) fn moves_the_box(&self) -> bool {
                match self {
                    $(AnimSlot::$name(_) => moves_the_box!($layout),)*
                }
            }

            /// Whether it has yet to be placed by a layout — the question
            /// `seed_animations` asks before it does anything at all.
            pub(crate) fn is_initial(&self) -> bool {
                match self {
                    $(AnimSlot::$name(anim) => anim.is_initial(),)*
                }
            }

            /// Whether it is moving now.
            pub(crate) fn is_animating(&self) -> bool {
                match self {
                    $(AnimSlot::$name(anim) => anim.is_animating(),)*
                }
            }

            /// Whether it declared somewhere to go when the widget is
            /// removed.
            pub(crate) fn declares_exit(&self) -> bool {
                match self {
                    $(AnimSlot::$name(anim) => anim.declares_exit(),)*
                }
            }

            /// Begin the exit it declared, if it declared one.
            pub(crate) fn begin_exit(&mut self, now: FrameInstant) -> bool {
                match self {
                    $(AnimSlot::$name(anim) => anim.begin_exit(now),)*
                }
            }

            /// Whether an exit has begun on it, settled or not.
            pub(crate) fn is_leaving(&self) -> bool {
                match self {
                    $(AnimSlot::$name(anim) => anim.is_leaving(),)*
                }
            }

            /// Stop leaving; the next advance sends it home.
            pub(crate) fn cancel_exit(&mut self) {
                match self {
                    $(AnimSlot::$name(anim) => anim.cancel_exit(),)*
                }
            }
        }
    };
}

animated_properties!(emit_slot_facts);

/// The per-property halves of a pass: what a row does that the row next to it
/// does not.
macro_rules! retarget {
    (none, $anim:expr, $container:expr, $id:expr, $now:expr) => {};
    ($target:ident, $anim:expr, $container:expr, $id:expr, $now:expr) => {
        // A sequence that a trigger has asked for, started before the frame
        // that will show its first value. The read is a snapshot: the
        // subscription belongs to `resync_animation_targets`, which asks the
        // same question inside its tracking scope.
        if $anim.take_play() {
            $anim.play($now);
        }
        $anim.animate_to($container.$target($id), $now);
    };
}
macro_rules! seed_from_target {
    (none, $anim:expr, $container:expr, $id:expr, $now:expr, $entered:expr,
        $ctx:expr) => {
        // A size is seeded by the layout that measured it — see
        // `update_size_targets`, which is where the formula for one lives.
        let _ = $anim;
    };
    ($target:ident, $anim:expr, $container:expr, $id:expr, $now:expr, $entered:expr,
        $ctx:expr) => {
        if $anim.is_initial() {
            $entered |=
                super::anim_bridge::seed_or_enter($ctx, $anim, $container.$target($id), $now);
        }
    };
}
macro_rules! drift_from_target {
    (none, $anim:expr, $container:expr, $id:expr, $drift:expr) => {
        // A size has no signal-backed target to have drifted from: the layout
        // recomputes it, every time, from the content it just measured.
        let _ = $anim;
    };
    ($target:ident, $anim:expr, $container:expr, $id:expr, $drift:expr) => {
        // An animation still in its initial state has no target to drift from
        // — `seed_animations` has not run for it yet. Both reads happen
        // whatever the answer, because each of them is the subscription.
        $drift |= !$anim.is_initial() && *$anim.target() != $container.$target($id);
        $drift |= $anim.wants_play();
    };
}

/// The three passes every declared animation goes through, emitted from the
/// rows rather than written out property by property.
///
/// Each walks what a container *declared*, so a container that animates one
/// property does one property's work.
macro_rules! emit_passes {
    ($($name:ident: $anim:ty as $decl:ty, target: $target:tt, layout: $layout:ident,
        timeline: $timeline:ident,
        test { $($test:tt)* };)*) => {
        impl super::Container {
            /// First layout: subscribe every declared animation and start it
            /// from the value its signal actually holds, not the one captured
            /// at construction.
            ///
            /// See `anim_bridge`'s module docs for why this cannot wait for the
            /// first paint.
            pub(super) fn seed_animations(&mut self, ctx: &mut LayoutCtx, id: WidgetId) {
                // Before anything else, and before a tracking scope: a
                // property is initial until the pass that places it has run,
                // so once every declared animation has been placed there is
                // nothing here to do, and every later layout would otherwise
                // open a scope to find that out.
                if !self
                    .anims
                    .as_deref()
                    .is_some_and(|declared| declared.slots().any(AnimSlot::is_initial))
                {
                    return;
                }

                // Moved out and put back, so the targets can be read from
                // `&self` while the animations are held by `&mut`.
                let now = ctx.tree_ref().frame_instant();
                let mut anims = self.anims.take();
                let mut entered = false;
                if let Some(declared) = anims.as_deref_mut() {
                    with_signal_tracking(id, JobType::Animation, || {
                        for slot in declared.slots_mut() {
                            match slot {
                                $(AnimSlot::$name(anim) => {
                                    seed_from_target!(
                                        $target, anim, self, id,
                                        || now, entered, ctx
                                    );
                                })*
                            }
                        }
                    });
                }
                self.anims = anims;

                // Something is under way from this frame — an enter, or a
                // sequence owed the play it gets for existing — so there has to
                // be another one: nothing else will ask, because no signal
                // changed.
                if entered {
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }
            }

            /// Every paint: re-read the declared targets under tracking, and
            /// ask for an Animation job if any has drifted from what its
            /// animation is aiming at.
            ///
            /// This is the pull half of the invariant. It keeps the
            /// subscriptions current through conditional closure branches and
            /// state-layer overrides, and it converges any write that landed
            /// before the subscription existed.
            pub(super) fn resync_animation_targets(&self, id: WidgetId) {
                let Some(declared) = self.anims.as_deref() else {
                    return;
                };
                // A container that is leaving has nowhere to drift to: its
                // exits are heading where removal sent them, and it reads
                // nothing that could wake it.
                if !declared.slots().any(AnimSlot::follows_a_signal) || self.is_leaving() {
                    return;
                }

                let drifted = with_signal_tracking(id, JobType::Animation, || {
                    let mut drift = false;
                    for slot in declared.slots() {
                        match slot {
                            $(AnimSlot::$name(anim) => {
                                drift_from_target!($target, anim, self, id, drift);
                            })*
                        }
                    }
                    drift
                });

                if drifted {
                    request_job(id, JobRequest::Animation(RequiredJob::None));
                }
            }

            /// One frame of every declared animation: play what a trigger
            /// asked for, adopt the target it is aiming at, and move it.
            ///
            /// The target each one aims at is read where its slot is walked, so
            /// a container that animates its background does not compute a
            /// shadow's target to throw away.
            ///
            /// Those reads are snapshots on purpose: this pass consumes a
            /// target, it does not subscribe to one. The subscriptions belong
            /// to `seed_animations` at the first layout and to
            /// `resync_animation_targets` at every paint, both under an
            /// Animation tracking scope — which is what `snapshot_zone` says
            /// here. Without it the debug diagnostic reports each of these
            /// reads as a value that "will not update", which is the opposite
            /// of true and trains the reader to ignore a warning that is
            /// usually right.
            ///
            /// Returns whether anything is still moving.
            pub(super) fn advance_declared_animations(
                &mut self,
                id: WidgetId,
                now: FrameInstant,
            ) -> bool {
                let mut anims = self.anims.take();
                let mut any_animating = false;
                if let Some(declared) = anims.as_deref_mut() {
                    // Leaving, nothing is retargeted: the exits are going where
                    // removal sent them, and the rest stay where they were
                    // rather than read a target whose item may be gone.
                    let leaving = declared.slots().any(AnimSlot::is_leaving);
                    crate::reactive::diagnostics::snapshot_zone(|| {
                        for slot in declared.slots_mut() {
                            match slot {
                                $(AnimSlot::$name(anim) => {
                                    if !leaving {
                                        retarget!($target, anim, self, id, now);
                                    }
                                    if anim.is_animating() {
                                        any_animating = true;
                                        let required =
                                            match (anim.advance(now).is_changed(), moves_the_box!($layout)) {
                                                (false, _) => RequiredJob::None,
                                                (true, true) => RequiredJob::Layout,
                                                (true, false) => RequiredJob::Paint,
                                            };
                                        request_job(id, JobRequest::Animation(required));
                                    }
                                })*
                            }
                        }
                    });
                }
                self.anims = anims;
                any_animating
            }
        }
    };
}

animated_properties!(emit_passes);

/// A declaration, for a property whose target it reads (`declare_anim`) or for
/// a size, whose declared type is not the type it animates (`declare_size`).
macro_rules! emit_declare {
    (none, $name:ident, $decl:ty) => {
        pub(in crate::widgets::container) fn $name<M>(
            anims: &mut Option<Box<ContainerAnims>>,
            value: impl crate::animation::IntoAnimated<$decl, M>,
        ) -> crate::reactive::Prop<$decl> {
            declare_size(anims, value, AnimKind::$name, AnimSlot::$name)
        }
    };
    ($target:ident, $name:ident, $decl:ty) => {
        pub(in crate::widgets::container) fn $name<M>(
            anims: &mut Option<Box<ContainerAnims>>,
            value: impl crate::animation::IntoAnimated<$decl, M>,
        ) -> crate::reactive::Prop<$decl> {
            declare_anim(anims, value, AnimKind::$name, AnimSlot::$name)
        }
    };
}

/// One function per property to declare it with, so a setter names its
/// property once.
///
/// It named it twice before this: the kind to take an animation away and the
/// variant to put one in, passed side by side and unchecked — `AnimKind` is a
/// unit enum, so naming the wrong one compiled and emptied somebody else's
/// slot. That is the defect this whole change is about, in a new spelling.
macro_rules! emit_declarers {
    ($($name:ident: $anim:ty as $decl:ty, target: $target:tt, layout: $layout:ident,
        timeline: $timeline:ident,
        test { $($test:tt)* };)*) => {
        pub(in crate::widgets::container) mod declare {
            use super::super::{declare_anim, declare_size};
            use super::{AnimKind, AnimSlot, ContainerAnims};
            use crate::layout::Length;
            use crate::widgets::container::{Color, Padding, Scale, Shadow, Translate};
            use crate::widgets::Corners;

            $(emit_declare!($target, $name, $decl);)*
        }
    };
}

animated_properties!(emit_declarers);
