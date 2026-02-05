//! Scrollable container functionality.

use crate::animation::{SpringConfig, Transition};
use crate::jobs::{JobRequest, RequiredJob, request_job};
use crate::layout::Constraints;
use crate::renderer::PaintContext;
use crate::tree::{Tree, WidgetId};
use crate::widgets::scroll::{ScrollAxis, ScrollbarAxis, ScrollbarVisibility};
use crate::widgets::widget::{Event, EventResponse, MouseButton, Rect, ScrollSource};

use super::Container;
use super::animations::AnimationState;

impl Container {
    /// Initialize scrollbar containers if scrolling is enabled and they don't exist yet.
    /// Scrollbar containers are registered in Tree as real widgets.
    pub(super) fn ensure_scrollbar_containers(&mut self, tree: &mut Tree, id: WidgetId) {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return;
        }

        // Create vertical scrollbar containers if needed
        if self.scroll_axis.allows_vertical() && self.v_scrollbar_track_id.is_none() {
            let (track, handle, scale_anim) =
                Self::create_scrollbar_components(&self.scrollbar_config);

            // Register track in Tree
            let track_id = tree.register(Box::new(track));
            tree.set_parent(track_id, id);
            self.v_scrollbar_track_id = Some(track_id);

            // Register handle in Tree
            let handle_id = tree.register(Box::new(handle));
            tree.set_parent(handle_id, id);
            self.v_scrollbar_handle_id = Some(handle_id);

            self.v_scrollbar_scale_anim = Some(scale_anim);
        }

        // Create horizontal scrollbar containers if needed
        if self.scroll_axis.allows_horizontal() && self.h_scrollbar_track_id.is_none() {
            let (track, handle, scale_anim) =
                Self::create_scrollbar_components(&self.scrollbar_config);

            // Register track in Tree
            let track_id = tree.register(Box::new(track));
            tree.set_parent(track_id, id);
            self.h_scrollbar_track_id = Some(track_id);

            // Register handle in Tree
            let handle_id = tree.register(Box::new(handle));
            tree.set_parent(handle_id, id);
            self.h_scrollbar_handle_id = Some(handle_id);

            self.h_scrollbar_scale_anim = Some(scale_anim);
        }
    }

    fn create_scrollbar_components(
        config: &crate::widgets::scroll::ScrollbarConfig,
    ) -> (Container, Container, AnimationState<f32>) {
        use crate::widgets::state_layer::StateStyle;

        let track_color = config.track_color;
        let track_corner_radius = config.track_corner_radius;
        let track_corner_curvature = config.track_corner_curvature;
        let handle_color = config.handle_color;
        let handle_corner_radius = config.handle_corner_radius;
        let handle_corner_curvature = config.handle_corner_curvature;
        let handle_hover_color = config.handle_hover_color;
        let handle_pressed_color = config.handle_pressed_color;

        // Track container
        let track = Container::new()
            .background(track_color)
            .corner_radius(track_corner_radius)
            .corner_curvature(track_corner_curvature);

        // Handle container with hover state for color change and ripple on press
        let handle = Container::new()
            .background(handle_color)
            .corner_radius(handle_corner_radius)
            .corner_curvature(handle_corner_curvature)
            .hover_state(move |s: StateStyle| s.background(handle_hover_color))
            .pressed_state(move |s: StateStyle| s.background(handle_pressed_color).ripple());

        // Scale animation for hover expansion (starts at 1.0 = normal size)
        // Custom spring: high stiffness (fast) + low damping (bouncy)
        let scrollbar_spring = SpringConfig {
            mass: 1.0,
            stiffness: 400.0, // Higher = faster
            damping: 15.0,    // Lower = more bounce
        };
        let scale_anim = AnimationState::new(1.0, Transition::spring(scrollbar_spring));

        (track, handle, scale_anim)
    }

    /// Layout and position scrollbar container widgets
    pub(super) fn layout_scrollbar_containers(
        &mut self,
        tree: &mut Tree,
        id: WidgetId,
        size: crate::layout::Size,
    ) {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return;
        }

        let scale_factor = self.scrollbar_config.hover_width / self.scrollbar_config.width;
        let needs_vertical = self.scroll_state.needs_vertical_scrollbar();
        let needs_horizontal = self.scroll_state.needs_horizontal_scrollbar();

        // Use LOCAL bounds (0,0 origin) for scrollbar positioning.
        // Scrollbars are positioned relative to container origin, and paint
        // handles the translation to screen coordinates via parent transforms.
        let local_bounds = Rect::new(0.0, 0.0, size.width, size.height);

        // Layout vertical scrollbar
        if self.scroll_axis.allows_vertical() && needs_vertical {
            self.layout_scrollbar_axis(
                tree,
                id,
                ScrollbarAxis::Vertical,
                scale_factor,
                needs_horizontal,
                local_bounds,
            );
        }

        // Layout horizontal scrollbar
        if self.scroll_axis.allows_horizontal() && needs_horizontal {
            self.layout_scrollbar_axis(
                tree,
                id,
                ScrollbarAxis::Horizontal,
                scale_factor,
                needs_vertical,
                local_bounds,
            );
        }
    }

    fn layout_scrollbar_axis(
        &mut self,
        tree: &mut Tree,
        id: WidgetId,
        axis: ScrollbarAxis,
        scale_factor: f32,
        needs_other_scrollbar: bool,
        local_bounds: Rect,
    ) {
        let track_rect = self.scroll_state.scrollbar_track_rect(
            axis,
            local_bounds,
            &self.scrollbar_config,
            needs_other_scrollbar,
        );
        let handle_rect = self.scroll_state.scrollbar_handle_rect(
            axis,
            local_bounds,
            &self.scrollbar_config,
            needs_other_scrollbar,
        );

        // Update scale animation target based on hover state
        let is_hovered = self.scroll_state.is_track_hovered(axis)
            || self.scroll_state.is_handle_hovered(axis)
            || self.scroll_state.is_dragging(axis);
        let target_scale = if is_hovered { scale_factor } else { 1.0 };

        let (scale_anim, track_id, handle_id) = match axis {
            ScrollbarAxis::Vertical => (
                &mut self.v_scrollbar_scale_anim,
                self.v_scrollbar_track_id,
                self.v_scrollbar_handle_id,
            ),
            ScrollbarAxis::Horizontal => (
                &mut self.h_scrollbar_scale_anim,
                self.h_scrollbar_track_id,
                self.h_scrollbar_handle_id,
            ),
        };

        if let Some(anim) = scale_anim {
            anim.animate_to(target_scale);
            if anim.is_animating() {
                let _ = anim.advance(); // Paint-only, ignore result
                request_job(id, JobRequest::Animation(RequiredJob::Paint));
            }
        }

        // Layout and position track container via Tree
        // Note: Scale transform is applied during paint, not stored on widget
        if let Some(track_id) = track_id {
            let track_constraints = Constraints {
                min_width: track_rect.width,
                min_height: track_rect.height,
                max_width: track_rect.width,
                max_height: track_rect.height,
            };
            tree.with_widget_mut(track_id, |widget, widget_id, tree| {
                widget.layout(tree, widget_id, track_constraints);
            });
            tree.set_origin(track_id, track_rect.x, track_rect.y);
        }

        // Layout and position handle container via Tree
        if let Some(handle_id) = handle_id {
            let handle_constraints = Constraints {
                min_width: handle_rect.width,
                min_height: handle_rect.height,
                max_width: handle_rect.width,
                max_height: handle_rect.height,
            };
            tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                widget.layout(tree, widget_id, handle_constraints);
            });
            tree.set_origin(handle_id, handle_rect.x, handle_rect.y);
        }
    }

    /// Advance scrollbar scale animations and apply transforms.
    /// Called from advance_animations since scroll is paint-only and layout
    /// may not run during hover events.
    pub(super) fn advance_scrollbar_scale_animations_internal(&mut self, _id: WidgetId) -> bool {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return false;
        }

        let scale_factor = self.scrollbar_config.hover_width / self.scrollbar_config.width;
        let needs_vertical = self.scroll_state.needs_vertical_scrollbar();
        let needs_horizontal = self.scroll_state.needs_horizontal_scrollbar();
        let mut any_animating = false;

        // Advance vertical scrollbar scale animation
        if self.scroll_axis.allows_vertical() && needs_vertical {
            any_animating |=
                self.advance_scrollbar_scale_axis(ScrollbarAxis::Vertical, scale_factor);
        }

        // Advance horizontal scrollbar scale animation
        if self.scroll_axis.allows_horizontal() && needs_horizontal {
            any_animating |=
                self.advance_scrollbar_scale_axis(ScrollbarAxis::Horizontal, scale_factor);
        }

        any_animating
    }

    fn advance_scrollbar_scale_axis(&mut self, axis: ScrollbarAxis, scale_factor: f32) -> bool {
        // Determine target scale based on hover state
        let is_hovered = self.scroll_state.is_track_hovered(axis)
            || self.scroll_state.is_handle_hovered(axis)
            || self.scroll_state.is_dragging(axis);
        let target_scale = if is_hovered { scale_factor } else { 1.0 };

        let scale_anim = match axis {
            ScrollbarAxis::Vertical => &mut self.v_scrollbar_scale_anim,
            ScrollbarAxis::Horizontal => &mut self.h_scrollbar_scale_anim,
        };

        // Advance the animation
        // Scale transforms are applied during paint, not stored on widgets
        let mut animating = false;
        if let Some(anim) = scale_anim {
            anim.animate_to(target_scale);
            if anim.is_animating() {
                let _ = anim.advance(); // Paint-only, ignore result
                animating = true;
            }
        }

        animating
    }

    /// Update scrollbar handle positions based on current scroll offset.
    /// Called from advance_animations to ensure handles are positioned correctly
    /// even when layout doesn't run (scroll is paint-only).
    pub(super) fn update_scrollbar_handle_positions(&mut self, tree: &mut Tree, id: WidgetId) {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return;
        }

        let needs_vertical = self.scroll_state.needs_vertical_scrollbar();
        let needs_horizontal = self.scroll_state.needs_horizontal_scrollbar();

        // Get bounds from Tree (single source of truth)
        let bounds = tree.get_bounds(id).unwrap_or_default();
        // Use local bounds (0,0 origin) for scrollbar positioning.
        // Scrollbars are positioned relative to container origin.
        let local_bounds = Rect::new(0.0, 0.0, bounds.width, bounds.height);

        // Update vertical scrollbar handle position
        if self.scroll_axis.allows_vertical()
            && needs_vertical
            && let Some(handle_id) = self.v_scrollbar_handle_id
        {
            let handle_rect = self.scroll_state.scrollbar_handle_rect(
                ScrollbarAxis::Vertical,
                local_bounds,
                &self.scrollbar_config,
                needs_horizontal,
            );
            tree.set_origin(handle_id, handle_rect.x, handle_rect.y);
        }

        // Update horizontal scrollbar handle position
        if self.scroll_axis.allows_horizontal()
            && needs_horizontal
            && let Some(handle_id) = self.h_scrollbar_handle_id
        {
            let handle_rect = self.scroll_state.scrollbar_handle_rect(
                ScrollbarAxis::Horizontal,
                local_bounds,
                &self.scrollbar_config,
                needs_vertical,
            );
            tree.set_origin(handle_id, handle_rect.x, handle_rect.y);
        }
    }

    /// Paint scrollbar container widgets.
    /// Scrollbar containers are registered in Tree with real WidgetIds.
    /// Scrollbar bounds are in local coordinates (relative to container origin 0,0).
    /// Scale transforms are applied during paint (not stored on widgets).
    pub(super) fn paint_scrollbar_containers(
        &self,
        tree: &Tree,
        _id: WidgetId,
        ctx: &mut PaintContext,
    ) {
        use crate::transform::Transform;

        if self.scrollbar_visibility == ScrollbarVisibility::Hidden {
            return;
        }

        // Get current scale for vertical scrollbar
        let v_scale = self
            .v_scrollbar_scale_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or(1.0);

        // Get current scale for horizontal scrollbar
        let h_scale = self
            .h_scrollbar_scale_anim
            .as_ref()
            .map(|a| *a.current())
            .unwrap_or(1.0);

        // Vertical scrollbar
        if self.scroll_axis.allows_vertical() && self.scroll_state.needs_vertical_scrollbar() {
            // Vertical scrollbar scales horizontally (expands width on hover)
            let scale_transform = Transform::scale_xy(v_scale, 1.0);

            if let Some(track_id) = self.v_scrollbar_track_id
                && let Some(track_bounds) = tree.get_bounds(track_id)
            {
                // Scrollbar bounds are already in local coordinates (relative to 0,0)
                let track_local = Rect::new(0.0, 0.0, track_bounds.width, track_bounds.height);

                let mut track_ctx = ctx.add_child(track_id.as_u64(), track_local);
                // Scale from right edge (transform origin at right center)
                // First translate to position, then apply scale centered at right edge
                let position = Transform::translate(track_bounds.x, track_bounds.y);
                let scale_origin_x = track_bounds.width;
                let scale_origin_y = track_bounds.height / 2.0;
                let combined = position
                    .then(&Transform::translate(scale_origin_x, scale_origin_y))
                    .then(&scale_transform)
                    .then(&Transform::translate(-scale_origin_x, -scale_origin_y));
                track_ctx.set_transform(combined);
                tree.with_widget(track_id, |widget| {
                    widget.paint(tree, track_id, &mut track_ctx);
                });
            }
            if let Some(handle_id) = self.v_scrollbar_handle_id
                && let Some(handle_bounds) = tree.get_bounds(handle_id)
            {
                let handle_local = Rect::new(0.0, 0.0, handle_bounds.width, handle_bounds.height);

                let mut handle_ctx = ctx.add_child(handle_id.as_u64(), handle_local);
                // Scale from right edge (transform origin at right center)
                let position = Transform::translate(handle_bounds.x, handle_bounds.y);
                let scale_origin_x = handle_bounds.width;
                let scale_origin_y = handle_bounds.height / 2.0;
                let combined = position
                    .then(&Transform::translate(scale_origin_x, scale_origin_y))
                    .then(&scale_transform)
                    .then(&Transform::translate(-scale_origin_x, -scale_origin_y));
                handle_ctx.set_transform(combined);
                tree.with_widget(handle_id, |widget| {
                    widget.paint(tree, handle_id, &mut handle_ctx);
                });
            }
        }

        // Horizontal scrollbar
        if self.scroll_axis.allows_horizontal() && self.scroll_state.needs_horizontal_scrollbar() {
            // Horizontal scrollbar scales vertically (expands height on hover)
            let scale_transform = Transform::scale_xy(1.0, h_scale);

            if let Some(track_id) = self.h_scrollbar_track_id
                && let Some(track_bounds) = tree.get_bounds(track_id)
            {
                let track_local = Rect::new(0.0, 0.0, track_bounds.width, track_bounds.height);

                let mut track_ctx = ctx.add_child(track_id.as_u64(), track_local);
                // Scale from bottom edge (transform origin at bottom center)
                let position = Transform::translate(track_bounds.x, track_bounds.y);
                let scale_origin_x = track_bounds.width / 2.0;
                let scale_origin_y = track_bounds.height;
                let combined = position
                    .then(&Transform::translate(scale_origin_x, scale_origin_y))
                    .then(&scale_transform)
                    .then(&Transform::translate(-scale_origin_x, -scale_origin_y));
                track_ctx.set_transform(combined);
                tree.with_widget(track_id, |widget| {
                    widget.paint(tree, track_id, &mut track_ctx);
                });
            }
            if let Some(handle_id) = self.h_scrollbar_handle_id
                && let Some(handle_bounds) = tree.get_bounds(handle_id)
            {
                let handle_local = Rect::new(0.0, 0.0, handle_bounds.width, handle_bounds.height);

                let mut handle_ctx = ctx.add_child(handle_id.as_u64(), handle_local);
                // Scale from bottom edge (transform origin at bottom center)
                let position = Transform::translate(handle_bounds.x, handle_bounds.y);
                let scale_origin_x = handle_bounds.width / 2.0;
                let scale_origin_y = handle_bounds.height;
                let combined = position
                    .then(&Transform::translate(scale_origin_x, scale_origin_y))
                    .then(&scale_transform)
                    .then(&Transform::translate(-scale_origin_x, -scale_origin_y));
                handle_ctx.set_transform(combined);
                tree.with_widget(handle_id, |widget| {
                    widget.paint(tree, handle_id, &mut handle_ctx);
                });
            }
        }
    }

    /// Handle scrollbar-related events, returns EventResponse if handled
    pub(super) fn handle_scrollbar_event(
        &mut self,
        tree: &mut Tree,
        id: WidgetId,
        bounds: Rect,
        event: &Event,
    ) -> Option<EventResponse> {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return None;
        }

        match event {
            Event::MouseDown { x, y, button } if *button == MouseButton::Left => {
                // Check vertical scrollbar
                if self.scroll_axis.allows_vertical()
                    && self.scroll_state.needs_vertical_scrollbar()
                    && let Some(response) = self.handle_scrollbar_click(
                        tree,
                        id,
                        bounds,
                        ScrollbarAxis::Vertical,
                        *x,
                        *y,
                        event,
                    )
                {
                    return Some(response);
                }

                // Check horizontal scrollbar
                if self.scroll_axis.allows_horizontal()
                    && self.scroll_state.needs_horizontal_scrollbar()
                    && let Some(response) = self.handle_scrollbar_click(
                        tree,
                        id,
                        bounds,
                        ScrollbarAxis::Horizontal,
                        *x,
                        *y,
                        event,
                    )
                {
                    return Some(response);
                }
            }

            Event::MouseMove { x, y } => {
                // Handle dragging
                if self.scroll_state.scrollbar_dragging {
                    return Some(self.handle_scrollbar_drag(
                        id,
                        bounds,
                        ScrollbarAxis::Vertical,
                        *y,
                    ));
                }
                if self.scroll_state.h_scrollbar_dragging {
                    return Some(self.handle_scrollbar_drag(
                        id,
                        bounds,
                        ScrollbarAxis::Horizontal,
                        *x,
                    ));
                }

                // Update hover states
                let mut needs_repaint = false;

                if self.scroll_axis.allows_vertical()
                    && self.scroll_state.needs_vertical_scrollbar()
                {
                    needs_repaint |= self.update_scrollbar_hover(
                        tree,
                        id,
                        bounds,
                        ScrollbarAxis::Vertical,
                        *x,
                        *y,
                        event,
                    );
                }

                if self.scroll_axis.allows_horizontal()
                    && self.scroll_state.needs_horizontal_scrollbar()
                {
                    needs_repaint |= self.update_scrollbar_hover(
                        tree,
                        id,
                        bounds,
                        ScrollbarAxis::Horizontal,
                        *x,
                        *y,
                        event,
                    );
                }

                if needs_repaint {
                    request_job(id, JobRequest::Animation(RequiredJob::Paint));
                }
            }

            Event::MouseUp { button, .. } if *button == MouseButton::Left => {
                if self.scroll_state.scrollbar_dragging {
                    self.scroll_state.scrollbar_dragging = false;
                    if let Some(handle_id) = self.v_scrollbar_handle_id {
                        tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                            widget.event(tree, widget_id, event);
                        });
                    }
                    request_job(id, JobRequest::Paint);
                    return Some(EventResponse::Handled);
                }
                if self.scroll_state.h_scrollbar_dragging {
                    self.scroll_state.h_scrollbar_dragging = false;
                    if let Some(handle_id) = self.h_scrollbar_handle_id {
                        tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                            widget.event(tree, widget_id, event);
                        });
                    }
                    request_job(id, JobRequest::Paint);
                    return Some(EventResponse::Handled);
                }
            }

            Event::MouseLeave => {
                // Clear scrollbar hover state
                if self.scroll_state.scrollbar_hovered || self.scroll_state.h_scrollbar_hovered {
                    self.scroll_state.scrollbar_hovered = false;
                    self.scroll_state.h_scrollbar_hovered = false;
                    if let Some(handle_id) = self.v_scrollbar_handle_id {
                        tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                            widget.event(tree, widget_id, event);
                        });
                    }
                    if let Some(handle_id) = self.h_scrollbar_handle_id {
                        tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                            widget.event(tree, widget_id, event);
                        });
                    }
                    request_job(id, JobRequest::Paint);
                }
                // Stop dragging
                if self.scroll_state.scrollbar_dragging || self.scroll_state.h_scrollbar_dragging {
                    self.scroll_state.scrollbar_dragging = false;
                    self.scroll_state.h_scrollbar_dragging = false;
                    request_job(id, JobRequest::Paint);
                }
            }

            _ => {}
        }

        None
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_scrollbar_click(
        &mut self,
        tree: &mut Tree,
        id: WidgetId,
        bounds: Rect,
        axis: ScrollbarAxis,
        x: f32,
        y: f32,
        event: &Event,
    ) -> Option<EventResponse> {
        let needs_other = match axis {
            ScrollbarAxis::Vertical => self.scroll_state.needs_horizontal_scrollbar(),
            ScrollbarAxis::Horizontal => self.scroll_state.needs_vertical_scrollbar(),
        };
        let handle_rect = self.scroll_state.scrollbar_handle_rect(
            axis,
            bounds,
            &self.scrollbar_config,
            needs_other,
        );
        let hit_area =
            self.scroll_state
                .scrollbar_hit_area(axis, bounds, &self.scrollbar_config, needs_other);

        if handle_rect.contains(x, y) {
            // Start dragging handle
            self.scroll_state.set_dragging(axis, true);
            let (pos, offset) = match axis {
                ScrollbarAxis::Vertical => (y, self.scroll_state.offset_y),
                ScrollbarAxis::Horizontal => (x, self.scroll_state.offset_x),
            };
            self.scroll_state.set_drag_start(axis, pos, offset);

            // Forward event to handle container for pressed state
            let handle_id = match axis {
                ScrollbarAxis::Vertical => self.v_scrollbar_handle_id,
                ScrollbarAxis::Horizontal => self.h_scrollbar_handle_id,
            };
            if let Some(handle_id) = handle_id {
                tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                    widget.event(tree, widget_id, event);
                });
            }

            request_job(id, JobRequest::Paint);
            return Some(EventResponse::Handled);
        } else if hit_area.contains(x, y) {
            // Click on track - jump to position
            let track_rect = self.scroll_state.scrollbar_track_rect(
                axis,
                bounds,
                &self.scrollbar_config,
                needs_other,
            );
            let (track_size, handle_size, click_pos) = match axis {
                ScrollbarAxis::Vertical => {
                    let handle_size = self.scroll_state.scrollbar_handle_size(
                        axis,
                        track_rect.height,
                        &self.scrollbar_config,
                    );
                    (
                        track_rect.height,
                        handle_size,
                        y - track_rect.y - handle_size / 2.0,
                    )
                }
                ScrollbarAxis::Horizontal => {
                    let handle_size = self.scroll_state.scrollbar_handle_size(
                        axis,
                        track_rect.width,
                        &self.scrollbar_config,
                    );
                    (
                        track_rect.width,
                        handle_size,
                        x - track_rect.x - handle_size / 2.0,
                    )
                }
            };
            let available = track_size - handle_size;
            if available > 0.0 {
                let ratio = (click_pos / available).clamp(0.0, 1.0);
                let offset = ratio * self.scroll_state.max_scroll(axis);
                self.scroll_state.set_offset(axis, offset);
                request_job(id, JobRequest::Paint);
            }
            return Some(EventResponse::Handled);
        }

        None
    }

    fn handle_scrollbar_drag(
        &mut self,
        id: WidgetId,
        bounds: Rect,
        axis: ScrollbarAxis,
        pos: f32,
    ) -> EventResponse {
        let needs_other = match axis {
            ScrollbarAxis::Vertical => self.scroll_state.needs_horizontal_scrollbar(),
            ScrollbarAxis::Horizontal => self.scroll_state.needs_vertical_scrollbar(),
        };
        let track = self.scroll_state.scrollbar_track_rect(
            axis,
            bounds,
            &self.scrollbar_config,
            needs_other,
        );
        let track_size = match axis {
            ScrollbarAxis::Vertical => track.height,
            ScrollbarAxis::Horizontal => track.width,
        };
        let handle_size =
            self.scroll_state
                .scrollbar_handle_size(axis, track_size, &self.scrollbar_config);
        let available = track_size - handle_size;

        if available > 0.0 {
            let (drag_start, start_offset) = self.scroll_state.drag_start(axis);
            let delta = pos - drag_start;
            let scroll_delta = (delta / available) * self.scroll_state.max_scroll(axis);
            let new_offset =
                (start_offset + scroll_delta).clamp(0.0, self.scroll_state.max_scroll(axis));
            self.scroll_state.set_offset(axis, new_offset);
            // Scrollbar dragging needs Animation + Paint for smooth updates
            request_job(id, JobRequest::Animation(RequiredJob::Paint));
        }

        EventResponse::Handled
    }

    #[allow(clippy::too_many_arguments)]
    fn update_scrollbar_hover(
        &mut self,
        tree: &mut Tree,
        _id: WidgetId,
        bounds: Rect,
        axis: ScrollbarAxis,
        x: f32,
        y: f32,
        event: &Event,
    ) -> bool {
        let needs_other = match axis {
            ScrollbarAxis::Vertical => self.scroll_state.needs_horizontal_scrollbar(),
            ScrollbarAxis::Horizontal => self.scroll_state.needs_vertical_scrollbar(),
        };
        let hit_area =
            self.scroll_state
                .scrollbar_hit_area(axis, bounds, &self.scrollbar_config, needs_other);
        let handle_rect = self.scroll_state.scrollbar_handle_rect(
            axis,
            bounds,
            &self.scrollbar_config,
            needs_other,
        );

        let mut needs_repaint = false;

        // Track area hover (for expansion effect)
        let was_track_hovered = self.scroll_state.is_track_hovered(axis);
        let is_track_hovered = hit_area.contains(x, y);
        self.scroll_state.set_track_hovered(axis, is_track_hovered);
        if was_track_hovered != is_track_hovered {
            needs_repaint = true;
        }

        // Handle hover (for color change)
        let was_hovered = self.scroll_state.is_handle_hovered(axis);
        let is_hovered = handle_rect.contains(x, y);
        self.scroll_state.set_handle_hovered(axis, is_hovered);
        if was_hovered != is_hovered {
            needs_repaint = true;
        }

        // Forward event to handle container for state layer hover
        let handle_id = match axis {
            ScrollbarAxis::Vertical => self.v_scrollbar_handle_id,
            ScrollbarAxis::Horizontal => self.h_scrollbar_handle_id,
        };
        if let Some(handle_id) = handle_id {
            tree.with_widget_mut(handle_id, |widget, widget_id, tree| {
                widget.event(tree, widget_id, event);
            });
        }

        needs_repaint
    }

    /// Apply scroll delta and return true if any scrolling occurred
    pub(super) fn apply_scroll(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        source: ScrollSource,
    ) -> bool {
        let old_x = self.scroll_state.offset_x;
        let old_y = self.scroll_state.offset_y;

        match self.scroll_axis {
            ScrollAxis::Vertical => {
                self.scroll_state.offset_y = (self.scroll_state.offset_y + delta_y)
                    .clamp(0.0, self.scroll_state.max_scroll_y());
            }
            ScrollAxis::Horizontal => {
                self.scroll_state.offset_x = (self.scroll_state.offset_x + delta_x)
                    .clamp(0.0, self.scroll_state.max_scroll_x());
            }
            ScrollAxis::Both => {
                self.scroll_state.offset_x = (self.scroll_state.offset_x + delta_x)
                    .clamp(0.0, self.scroll_state.max_scroll_x());
                self.scroll_state.offset_y = (self.scroll_state.offset_y + delta_y)
                    .clamp(0.0, self.scroll_state.max_scroll_y());
            }
            ScrollAxis::None => return false,
        }

        // Track velocity for kinetic scrolling (touchpad/finger input only)
        if source == ScrollSource::Finger {
            match self.scroll_axis {
                ScrollAxis::Vertical => {
                    self.scroll_state.velocity_y = delta_y;
                }
                ScrollAxis::Horizontal => {
                    self.scroll_state.velocity_x = delta_x;
                }
                ScrollAxis::Both => {
                    self.scroll_state.velocity_x = delta_x;
                    self.scroll_state.velocity_y = delta_y;
                }
                ScrollAxis::None => {}
            }
            self.scroll_state.last_scroll_time = Some(std::time::Instant::now());
        }

        old_x != self.scroll_state.offset_x || old_y != self.scroll_state.offset_y
    }
}
