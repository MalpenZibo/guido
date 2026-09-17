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

/// The table. Hands its rows to `$emit`, which decides what to build from
/// them.
///
/// A row is
///
/// ```text
/// name: AnimatedType = DeclaredType, target: <fn> | none, layout: yes | no,
///     test { value, declared_as, probe, from, to, then, base };
/// ```
///
/// - **`AnimatedType`** is what moves — what `AnimationState<T>` holds.
///   **`DeclaredType`** is what the setter takes. They differ for `width` and
///   `height`, which declare a `Length` and animate the `f32` in it.
/// - **`target`** names the `&self` method that reads where the property is
///   heading, or `none` for the sizes, whose target the layout supplies.
/// - **`layout`** says whether moving it re-measures the box.
/// - **`test`** is the property's own recipe, so the generated test can say
///   what it means to declare, watch and write *this* property: how one number
///   becomes a value of its type, a container built around the declaration, a
///   probe reading that number back off the painted frame, and the three
///   numbers it moves between — where it enters from, where it lands, and
///   where a later write sends it — and `base`, the value a timeline is
///   declared at, which is `from` everywhere but one.
macro_rules! animated_properties {
    ($emit:ident) => {
        $emit! {
            width: f32 = Length, target: none, layout: yes,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(v).height(100.0),
                    probe: |h: &mut H| h.tree.cached_size(h.root).unwrap().width,
                    from: 10.0, to: 150.0, then: 50.0, base: 10.0,
                };
            height: f32 = Length, target: none, layout: yes,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(200.0).height(v),
                    probe: |h: &mut H| h.tree.cached_size(h.root).unwrap().height,
                    from: 10.0, to: 150.0, then: 50.0, base: 10.0,
                };
            padding: Padding = Padding, target: effective_padding_target, layout: yes,
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
            background: Color = Color, target: effective_background_target, layout: no,
                test {
                    value: |k| Color::rgb(k, 0.0, 0.0),
                    declared_as: |v| container().width(200.0).height(100.0).background(v),
                    probe: |h: &mut H| rects(&h.paint())[0].1.r,
                    from: 0.0, to: 1.0, then: 0.5, base: 0.0,
                };
            corners: Corners = Corners, target: effective_corners_target, layout: no,
                test {
                    value: |k| crate::widgets::Corners::rounded(k),
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).corners(v),
                    probe: |h: &mut H| painted_style(&h.paint()).1.top_left,
                    from: 2.0, to: 20.0, then: 8.0, base: 2.0,
                };
            shadow: Shadow = Shadow, target: effective_shadow_target, layout: no,
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
            border_width: f32 = f32, target: effective_border_width_target, layout: no,
                test {
                    value: |k| k,
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).border(v, Color::WHITE),
                    probe: |h: &mut H| borders(&h.paint())[0].0,
                    from: 1.0, to: 10.0, then: 4.0, base: 1.0,
                };
            border_color: Color = Color, target: effective_border_color_target, layout: no,
                test {
                    value: |k| Color::rgb(k, 0.0, 0.0),
                    declared_as: |v| container().width(200.0).height(100.0)
                        .background(Color::RED).border(2.0, v),
                    probe: |h: &mut H| borders(&h.paint())[0].1.r,
                    from: 0.0, to: 1.0, then: 0.5, base: 0.0,
                };
            translate: Translate = Translate, target: effective_translate_target, layout: no,
                test {
                    value: |k| Translate::new(k, 0.0),
                    declared_as: |v| container().child(
                        container().width(200.0).height(100.0).translate(v)),
                    probe: |h: &mut H| h.paint().children[0].local_transform.tx(),
                    from: 5.0, to: 50.0, then: 20.0, base: 5.0,
                };
            rotate: f32 = f32, target: effective_rotate_target, layout: no,
                test {
                    value: |k| k,
                    declared_as: |v| container().child(
                        container().width(200.0).height(100.0).rotate(v)),
                    probe: |h: &mut H| turned_degrees(&h.paint().children[0]),
                    from: 4.0, to: 40.0, then: 16.0, base: 4.0,
                };
            scale: Scale = Scale, target: effective_scale_target, layout: no,
                test {
                    value: |k| Scale::uniform(k),
                    declared_as: |v| container().child(
                        container().width(200.0).height(100.0).scale(v)),
                    probe: |h: &mut H| h.paint().children[0].local_transform.data[0],
                    from: 0.5, to: 2.0, then: 1.25, base: 0.5,
                };
        }
    };
}

pub(crate) use animated_properties;
