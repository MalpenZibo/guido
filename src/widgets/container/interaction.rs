//! Turning pointer and key events into container state and callbacks.
//!
//! A container resolves every event against its [`HitContext`] — the geometry
//! it was laid out and transformed into. Hit testing therefore happens in two
//! steps: undo the container's own transform, then test the point against the
//! laid-out bounds *and* their corner radius, so a rounded container does not
//! answer for the pixels outside its shape.
//!
//! Hover is tracked **before** children see the event ([`track_pointer`]),
//! because a child handling a `MouseMove` must not stop its ancestors from
//! noticing the pointer passed over them. Everything else is handled after the
//! children have had their turn ([`handle_own_event`]), so the innermost
//! interested widget wins.
//!
//! [`track_pointer`]: Container::track_pointer
//! [`handle_own_event`]: Container::handle_own_event

use std::time::Instant;

use super::*;

/// The geometry an event is resolved against: where the container ended up,
/// what shape it is, and the transform standing between the two.
pub(super) struct HitContext {
    pub bounds: Rect,
    pub corners: crate::widgets::Corners,
    pub transform: Transform,
    pub pivot: Pivot,
}

impl HitContext {
    /// Whether a point falls inside the container's *shape* — its bounds
    /// narrowed by the corner radius, not the bounding box.
    ///
    /// An event with no position is inside nothing. That is the whole of
    /// #227: a container collapsed to a line has no inverse to undo, so the
    /// point that reaches it is `None`, and every bounds test below it answers
    /// no without any of them having to know why.
    #[inline]
    pub(super) fn contains(&self, at: Option<Point>) -> bool {
        self.bounds.contains_shape_at(at, self.corners)
    }

    /// A surface-space point expressed relative to the container's own origin
    /// — the space ripples and pointer callbacks work in.
    pub(super) fn local(&self, at: Point) -> Option<Point> {
        Some(
            untransform_point(&self.transform, self.pivot, self.bounds, at)?
                .offset(-self.bounds.x, -self.bounds.y),
        )
    }

    /// The same, for a point that has already had the transform undone.
    pub(super) fn rebase(&self, at: Point) -> Point {
        at.offset(-self.bounds.x, -self.bounds.y)
    }
}

/// Map a point from surface space into the container's untransformed space,
/// or `None` where the container occupies no space to be pointed at.
///
/// A container's own transform is applied around its origin at paint time, so
/// hit testing has to undo it before comparing against the laid-out bounds.
/// An identity transform returns the point unchanged.
///
/// A transform that has collapsed the container onto a line or a point has no
/// inverse, and there is no coordinate that means "nowhere" — every number is
/// somewhere, and a descendant that rotates or mirrors would carry a far-away
/// sentinel back into the visible half-plane. So the absence is the answer.
pub(super) fn untransform_point(
    transform: &Transform,
    origin: Pivot,
    bounds: Rect,
    at: Point,
) -> Option<Point> {
    if transform.is_identity() {
        return Some(at);
    }
    let (origin_x, origin_y) = origin.resolve(bounds);
    let (ux, uy) = transform
        .center_at(origin_x, origin_y)
        .inverse()?
        .transform_point(at.x, at.y);
    Some(Point::new(ux, uy))
}

impl Container {
    /// Let a state-layer animation pick up its new target after a hover or
    /// press change.
    ///
    /// The *repaint* needs no request: the flag write is a signal write, and
    /// whatever resolved a state layer — this container, and any descendant
    /// text whose colour resolves through it — subscribed to that signal and
    /// is marked by it. What a signal write cannot do is drive the clock, so a
    /// container with animated state properties still asks for its Animation
    /// job here.
    pub(super) fn request_state_change_repaint(&self, id: WidgetId) {
        if self.has_animated_state_properties() {
            request_job(id, JobRequest::Animation(RequiredJob::Paint));
        }
    }

    /// Update hover state and fire the pointer-move callback, before children
    /// get the event: a child that handles a `MouseMove` must not stop its
    /// ancestors from tracking their own hover.
    pub(super) fn track_pointer(
        &mut self,
        id: WidgetId,
        hit: &HitContext,
        event: &Event,
        now: Instant,
    ) {
        let has_animated = self.has_animated_state_properties();
        // Read before the mutable borrow: cancelling a ripple below needs it.
        let ripple_config = self.interaction.as_ref().and_then(|ix| ix.ripple_config());
        let Some(ref mut ix) = self.interaction else {
            return;
        };
        // See `request_state_change_repaint`: the flag write invalidates, this
        // only advances an animation.
        let request_repaint = |id: WidgetId| {
            if has_animated {
                request_job(id, JobRequest::Animation(RequiredJob::Paint));
            }
        };

        match event {
            Event::MouseEnter { at } if hit.contains(*at) && !ix.is_hovered() => {
                ix.set_flag(InteractionFlags::HOVERED, true);
                if ix.declares(|w| matches!(w, StateWhen::Hovered)) {
                    request_repaint(id);
                }
                if let Some(ref callback) = ix.on_hover {
                    callback(true);
                }
            }
            Event::MouseMove { at } => {
                // A pressed container keeps receiving moves that leave it —
                // that implicit capture is what makes dragging work. A move
                // with no position is not one of those: there is nowhere to
                // report, so the callback is not called and the hover below
                // simply falls.
                // Asked once: `contains_shape` evaluates the corner
                // superellipse, which is three `powf`s for a squircle, and
                // this is the coalesced-pointer-move path.
                let inside = hit.contains(*at);

                if let Some(ref callback) = ix.on_pointer_move
                    && (inside || ix.is_pressed())
                    && let Some(at) = at
                {
                    let local = hit.rebase(*at);
                    callback(local.x, local.y);
                }

                let was_hovered = ix.is_hovered();
                ix.set_flag(InteractionFlags::HOVERED, inside);

                // Dragging off the container abandons the press. Only leaving
                // the whole surface used to say so, which left a ripple
                // growing at full brightness on a button the pointer had left
                // several hundred pixels ago.
                if was_hovered
                    && !ix.is_hovered()
                    && ix.ripple.is_active()
                    && let Some(ref config) = ripple_config
                {
                    ix.ripple.cancel(config, now);
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }

                if was_hovered != ix.is_hovered() {
                    if ix.declares(|w| matches!(w, StateWhen::Hovered)) {
                        request_repaint(id);
                    }
                    if let Some(ref callback) = ix.on_hover {
                        callback(ix.is_hovered());
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle an event the children did not take.
    ///
    /// `event` is the original, still in surface space: the ripple needs it to
    /// place its origin under the actual cursor. `local` is the same event with
    /// this container's transform undone, which is what everything else tests
    /// against.
    pub(super) fn handle_own_event(
        &mut self,
        id: WidgetId,
        hit: &HitContext,
        event: &Event,
        local: &Event,
        now: Instant,
    ) -> EventResponse {
        match local {
            // Hover was tracked before the children ran. Returning Ignored
            // here is deliberate: a hover change must not stop sibling
            // containers from tracking their own.
            Event::MouseEnter { .. } | Event::MouseMove { .. } => {}

            Event::MouseDown { at, button } if *button != MouseButton::Left => {
                if hit.contains(*at)
                    && let Some(ref ix) = self.interaction
                {
                    let callback = match button {
                        MouseButton::Right => ix.on_right_click.as_ref(),
                        MouseButton::Middle => ix.on_middle_click.as_ref(),
                        MouseButton::Left => None,
                    };
                    if let Some(callback) = callback {
                        callback();
                        return EventResponse::Handled;
                    }
                }
            }

            Event::MouseDown { at, button } => {
                if hit.contains(*at)
                    && let Some(at) = at
                    && *button == MouseButton::Left
                    && let Some(ref mut ix) = self.interaction
                {
                    let was_pressed = ix.is_pressed();
                    ix.set_flag(InteractionFlags::PRESSED, true);

                    let has_ripple = ix
                        .states
                        .iter()
                        .any(|(w, s)| matches!(w, StateWhen::Pressed) && s.ripple.is_some());
                    if has_ripple {
                        // The ripple starts under the finger, so it wants the
                        // point as the surface gave it, not the one already
                        // rebased for this container.
                        let on_screen = event.coords().unwrap_or(*at);
                        if let Some(local) = hit.local(on_screen) {
                            ix.ripple.start(local.x, local.y, now);
                        }
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if !was_pressed && ix.declares(|w| matches!(w, StateWhen::Pressed)) {
                        self.request_state_change_repaint(id);
                    }
                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_down
                    {
                        let local = hit.rebase(*at);
                        callback(local.x, local.y);
                        return EventResponse::Handled;
                    }
                    // A press is claimed even without a down handler, so the
                    // matching release reaches the click handler.
                    if let Some(ref ix) = self.interaction
                        && (ix.on_click.is_some() || ix.on_mouse_up.is_some())
                    {
                        return EventResponse::Handled;
                    }
                }
            }

            Event::MouseUp { at, button } => {
                if let Some(ref mut ix) = self.interaction
                    && ix.is_pressed()
                    && *button == MouseButton::Left
                {
                    let was_pressed = ix.is_pressed();
                    ix.set_flag(InteractionFlags::PRESSED, false);

                    // The ripple has to say the same thing `on_click` below
                    // says: a release inside activated something and finishes
                    // its expansion, a release that wandered off did not and
                    // simply goes.
                    if ix.ripple.is_active()
                        && let Some(config) = ix.ripple_config()
                    {
                        if hit.contains(*at) {
                            ix.ripple.release(&config, now);
                        } else {
                            ix.ripple.cancel(&config, now);
                        }
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if was_pressed && ix.declares(|w| matches!(w, StateWhen::Pressed)) {
                        self.request_state_change_repaint(id);
                    }

                    let mut handled = false;
                    if let Some(at) = at
                        && let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_up
                    {
                        let local = hit.rebase(*at);
                        callback(local.x, local.y);
                        handled = true;
                    }
                    // A click is a release *inside* the shape; a release that
                    // wandered off only ends the press.
                    if let Some(ref ix) = self.interaction
                        && hit.contains(*at)
                        && let Some(ref callback) = ix.on_click
                    {
                        callback();
                        return EventResponse::Handled;
                    }
                    if handled {
                        return EventResponse::Handled;
                    }
                }
            }

            Event::MouseLeave => {
                if let Some(ref mut ix) = self.interaction {
                    let was_hovered = ix.is_hovered();
                    let was_pressed = ix.is_pressed();
                    if ix.is_hovered() {
                        ix.set_flag(InteractionFlags::HOVERED, false);
                        if let Some(ref callback) = ix.on_hover {
                            callback(false);
                        }
                    }
                    ix.set_flag(InteractionFlags::PRESSED, false);

                    // The pointer left without releasing, so nothing was
                    // activated and there is nothing to complete: the ripple
                    // just goes.
                    if ix.ripple.is_active()
                        && let Some(config) = ix.ripple_config()
                    {
                        ix.ripple.cancel(&config, now);
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if (was_hovered && ix.declares(|w| matches!(w, StateWhen::Hovered)))
                        || (was_pressed && ix.declares(|w| matches!(w, StateWhen::Pressed)))
                    {
                        self.request_state_change_repaint(id);
                    }
                }
            }

            Event::Scroll {
                at,
                delta_x,
                delta_y,
                source,
            } => {
                if hit.contains(*at) {
                    // Our own scrolling consumes the event first; the callback
                    // only sees what scrolling did not take.
                    if self.scroll_axis != ScrollAxis::None
                        && self.apply_scroll(*delta_x, *delta_y, *source, now)
                    {
                        // A paint, and only a paint. A sample means the finger
                        // is still down, so there is no momentum for an
                        // animation pass to advance — it begins on the
                        // end-of-gesture, which asks for its own frames.
                        request_job(id, JobRequest::Paint);
                        return EventResponse::Handled;
                    }

                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_scroll
                    {
                        callback(*delta_x, *delta_y, *source);
                        return EventResponse::Handled;
                    }
                }
            }

            Event::KeyDown { key, modifiers } => {
                if let Some(ref ix) = self.interaction
                    && let Some(ref callback) = ix.on_key_down
                {
                    callback(*key, *modifiers);
                    return EventResponse::Handled;
                }
            }

            // The finger lifted. Momentum starts here or not at all — and it
            // starts now, on the event, rather than becoming due for whatever
            // frame happens along next.
            Event::ScrollEnd { at } => {
                if self.scroll_axis != ScrollAxis::None && hit.contains(*at) {
                    let sd = self.scroll_mut();
                    sd.scroll_state.end_gesture(now);
                    if sd.scroll_state.should_apply_momentum() {
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }
                    return EventResponse::Handled;
                }
            }

            Event::KeyUp { .. } | Event::FocusIn | Event::FocusOut => {}
        }

        EventResponse::Ignored
    }
}
