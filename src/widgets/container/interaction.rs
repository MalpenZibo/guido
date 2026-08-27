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
    pub(super) fn contains(&self, x: f32, y: f32) -> bool {
        self.bounds.contains_shape(x, y, self.corners)
    }

    /// A surface-space point expressed relative to the container's own origin.
    pub(super) fn local(&self, x: f32, y: f32) -> (f32, f32) {
        local_point(&self.transform, self.pivot, self.bounds, x, y)
    }

    /// The same, for a point that has already had the transform undone.
    fn rebase(&self, x: f32, y: f32) -> (f32, f32) {
        (x - self.bounds.x, y - self.bounds.y)
    }
}

/// Map a point from surface space into the container's untransformed space.
///
/// A container's own transform is applied around its origin at paint time, so
/// hit testing has to undo it before comparing against the laid-out bounds.
/// An identity transform returns the point unchanged.
///
/// A transform that has collapsed the container onto a line or a point cannot
/// be undone, and the point comes back unchanged — so an invisible subtree
/// answers for the whole area it occupies when it is open. That is #227, and
/// it is older than the components API: it needs the *event* to be able to say
/// it has no position, which no coordinate can say on its behalf.
pub(super) fn untransform_point(
    transform: &Transform,
    origin: Pivot,
    bounds: Rect,
    x: f32,
    y: f32,
) -> (f32, f32) {
    if transform.is_identity() {
        return (x, y);
    }
    let (origin_x, origin_y) = origin.resolve(bounds);
    transform
        .center_at(origin_x, origin_y)
        .inverse()
        .transform_point(x, y)
}

/// [`untransform_point`] expressed relative to the container's own origin —
/// the space ripples and pointer callbacks work in.
pub(super) fn local_point(
    transform: &Transform,
    origin: Pivot,
    bounds: Rect,
    x: f32,
    y: f32,
) -> (f32, f32) {
    let (ux, uy) = untransform_point(transform, origin, bounds, x, y);
    (ux - bounds.x, uy - bounds.y)
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
        at: Instant,
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
            Event::MouseEnter { x, y } if hit.contains(*x, *y) && !ix.is_hovered() => {
                ix.set_flag(InteractionFlags::HOVERED, true);
                if ix.declares(|w| matches!(w, StateWhen::Hovered)) {
                    request_repaint(id);
                }
                if let Some(ref callback) = ix.on_hover {
                    callback(true);
                }
            }
            Event::MouseMove { x, y } => {
                // A pressed container keeps receiving moves that leave it —
                // that implicit capture is what makes dragging work.
                if let Some(ref callback) = ix.on_pointer_move
                    && (hit.contains(*x, *y) || ix.is_pressed())
                {
                    let (lx, ly) = hit.rebase(*x, *y);
                    callback(lx, ly);
                }

                let was_hovered = ix.is_hovered();
                ix.set_flag(InteractionFlags::HOVERED, hit.contains(*x, *y));

                // Dragging off the container abandons the press. Only leaving
                // the whole surface used to say so, which left a ripple
                // growing at full brightness on a button the pointer had left
                // several hundred pixels ago.
                if was_hovered
                    && !ix.is_hovered()
                    && ix.ripple.is_active()
                    && let Some(ref config) = ripple_config
                {
                    ix.ripple.cancel(config, at);
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
        at: Instant,
    ) -> EventResponse {
        match local {
            // Hover was tracked before the children ran. Returning Ignored
            // here is deliberate: a hover change must not stop sibling
            // containers from tracking their own.
            Event::MouseEnter { .. } | Event::MouseMove { .. } => {}

            Event::MouseDown { x, y, button } if *button != MouseButton::Left => {
                if hit.contains(*x, *y)
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

            Event::MouseDown { x, y, button } => {
                if hit.contains(*x, *y)
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
                        let (screen_x, screen_y) = event.coords().unwrap_or((*x, *y));
                        let (local_x, local_y) = hit.local(screen_x, screen_y);
                        ix.ripple.start(local_x, local_y, at);
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if !was_pressed && ix.declares(|w| matches!(w, StateWhen::Pressed)) {
                        self.request_state_change_repaint(id);
                    }
                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_down
                    {
                        let (lx, ly) = hit.rebase(*x, *y);
                        callback(lx, ly);
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

            Event::MouseUp { x, y, button } => {
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
                        if hit.contains(*x, *y) {
                            ix.ripple.release(&config, at);
                        } else {
                            ix.ripple.cancel(&config, at);
                        }
                        request_job(id, JobRequest::Animation(RequiredJob::Paint));
                    }

                    if was_pressed && ix.declares(|w| matches!(w, StateWhen::Pressed)) {
                        self.request_state_change_repaint(id);
                    }

                    let mut handled = false;
                    if let Some(ref ix) = self.interaction
                        && let Some(ref callback) = ix.on_mouse_up
                    {
                        let (lx, ly) = hit.rebase(*x, *y);
                        callback(lx, ly);
                        handled = true;
                    }
                    // A click is a release *inside* the shape; a release that
                    // wandered off only ends the press.
                    if let Some(ref ix) = self.interaction
                        && hit.contains(*x, *y)
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
                        ix.ripple.cancel(&config, at);
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
                x,
                y,
                delta_x,
                delta_y,
                source,
            } => {
                if hit.contains(*x, *y) {
                    // Our own scrolling consumes the event first; the callback
                    // only sees what scrolling did not take.
                    if self.scroll_axis != ScrollAxis::None
                        && self.apply_scroll(*delta_x, *delta_y, *source, at)
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
            Event::ScrollEnd { x, y } => {
                if self.scroll_axis != ScrollAxis::None && hit.contains(*x, *y) {
                    let sd = self.scroll_mut();
                    sd.scroll_state.end_gesture(at);
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
