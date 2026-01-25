//! Scrollable container functionality.

use crate::animation::{SpringConfig, Transition};
use crate::layout::Constraints;
use crate::reactive::request_animation_frame;
use crate::renderer::PaintContext;
use crate::transform::Transform;
use crate::transform_origin::TransformOrigin;
use crate::widgets::scroll::{ScrollAxis, ScrollbarAxis, ScrollbarVisibility};
use crate::widgets::widget::{Event, EventResponse, MouseButton, ScrollSource, Widget};

use super::animations::AnimationState;
use super::Container;

impl Container {
    /// Initialize scrollbar containers if scrolling is enabled and they don't exist yet
    pub(super) fn ensure_scrollbar_containers(&mut self) {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return;
        }

        // Create vertical scrollbar containers if needed
        if self.scroll_axis.allows_vertical() && self.v_scrollbar_track.is_none() {
            let (track, handle, scale_anim) =
                Self::create_scrollbar_components(&self.scrollbar_config);
            self.v_scrollbar_track = Some(Box::new(track));
            self.v_scrollbar_handle = Some(Box::new(handle));
            self.v_scrollbar_scale_anim = Some(scale_anim);
        }

        // Create horizontal scrollbar containers if needed
        if self.scroll_axis.allows_horizontal() && self.h_scrollbar_track.is_none() {
            let (track, handle, scale_anim) =
                Self::create_scrollbar_components(&self.scrollbar_config);
            self.h_scrollbar_track = Some(Box::new(track));
            self.h_scrollbar_handle = Some(Box::new(handle));
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
    pub(super) fn layout_scrollbar_containers(&mut self) {
        if self.scroll_axis == ScrollAxis::None
            || self.scrollbar_visibility == ScrollbarVisibility::Hidden
        {
            return;
        }

        let scale_factor = self.scrollbar_config.hover_width / self.scrollbar_config.width;
        let needs_vertical = self.scroll_state.needs_vertical_scrollbar();
        let needs_horizontal = self.scroll_state.needs_horizontal_scrollbar();

        // Layout vertical scrollbar
        if self.scroll_axis.allows_vertical() && needs_vertical {
            self.layout_scrollbar_axis(ScrollbarAxis::Vertical, scale_factor, needs_horizontal);
        }

        // Layout horizontal scrollbar
        if self.scroll_axis.allows_horizontal() && needs_horizontal {
            self.layout_scrollbar_axis(ScrollbarAxis::Horizontal, scale_factor, needs_vertical);
        }
    }

    fn layout_scrollbar_axis(
        &mut self,
        axis: ScrollbarAxis,
        scale_factor: f32,
        needs_other_scrollbar: bool,
    ) {
        let track_rect = self.scroll_state.scrollbar_track_rect(
            axis,
            self.bounds,
            &self.scrollbar_config,
            needs_other_scrollbar,
        );
        let handle_rect = self.scroll_state.scrollbar_handle_rect(
            axis,
            self.bounds,
            &self.scrollbar_config,
            needs_other_scrollbar,
        );

        // Update scale animation target based on hover state
        let is_hovered = self.scroll_state.is_track_hovered(axis)
            || self.scroll_state.is_handle_hovered(axis)
            || self.scroll_state.is_dragging(axis);
        let target_scale = if is_hovered { scale_factor } else { 1.0 };

        let (scale_anim, track_container, handle_container) = match axis {
            ScrollbarAxis::Vertical => (
                &mut self.v_scrollbar_scale_anim,
                &mut self.v_scrollbar_track,
                &mut self.v_scrollbar_handle,
            ),
            ScrollbarAxis::Horizontal => (
                &mut self.h_scrollbar_scale_anim,
                &mut self.h_scrollbar_track,
                &mut self.h_scrollbar_handle,
            ),
        };

        if let Some(ref mut anim) = scale_anim {
            anim.animate_to(target_scale);
            if anim.is_animating() {
                anim.advance();
                request_animation_frame();
            }
        }

        let current_scale = scale_anim.as_ref().map(|a| *a.current()).unwrap_or(1.0);

        // Layout and position track container
        if let Some(ref mut track) = track_container {
            let track_constraints = Constraints {
                min_width: track_rect.width,
                min_height: track_rect.height,
                max_width: track_rect.width,
                max_height: track_rect.height,
            };
            Widget::layout(track.as_mut(), track_constraints);
            track.set_origin(track_rect.x, track_rect.y);

            // Set scale transform
            let (transform, origin) = match axis {
                ScrollbarAxis::Vertical => (
                    Transform::scale_xy(current_scale, 1.0),
                    TransformOrigin::px(track_rect.width, track_rect.height / 2.0),
                ),
                ScrollbarAxis::Horizontal => (
                    Transform::scale_xy(1.0, current_scale),
                    TransformOrigin::px(track_rect.width / 2.0, track_rect.height),
                ),
            };
            track.set_transform(transform);
            track.set_transform_origin(origin);
        }

        // Layout and position handle container
        if let Some(ref mut handle) = handle_container {
            let handle_constraints = Constraints {
                min_width: handle_rect.width,
                min_height: handle_rect.height,
                max_width: handle_rect.width,
                max_height: handle_rect.height,
            };
            Widget::layout(handle.as_mut(), handle_constraints);
            handle.set_origin(handle_rect.x, handle_rect.y);

            // Set scale transform
            let (transform, origin) = match axis {
                ScrollbarAxis::Vertical => (
                    Transform::scale_xy(current_scale, 1.0),
                    TransformOrigin::px(handle_rect.width, handle_rect.height / 2.0),
                ),
                ScrollbarAxis::Horizontal => (
                    Transform::scale_xy(1.0, current_scale),
                    TransformOrigin::px(handle_rect.width / 2.0, handle_rect.height),
                ),
            };
            handle.set_transform(transform);
            handle.set_transform_origin(origin);
        }
    }

    /// Paint scrollbar container widgets
    pub(super) fn paint_scrollbar_containers(&self, ctx: &mut PaintContext) {
        if self.scrollbar_visibility == ScrollbarVisibility::Hidden {
            return;
        }

        // Vertical scrollbar
        if self.scroll_axis.allows_vertical() && self.scroll_state.needs_vertical_scrollbar() {
            if let Some(ref track) = self.v_scrollbar_track {
                track.paint(ctx);
            }
            if let Some(ref handle) = self.v_scrollbar_handle {
                handle.paint(ctx);
            }
        }

        // Horizontal scrollbar
        if self.scroll_axis.allows_horizontal() && self.scroll_state.needs_horizontal_scrollbar() {
            if let Some(ref track) = self.h_scrollbar_track {
                track.paint(ctx);
            }
            if let Some(ref handle) = self.h_scrollbar_handle {
                handle.paint(ctx);
            }
        }
    }

    /// Handle scrollbar-related events, returns EventResponse if handled
    pub(super) fn handle_scrollbar_event(&mut self, event: &Event) -> Option<EventResponse> {
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
                {
                    if let Some(response) =
                        self.handle_scrollbar_click(ScrollbarAxis::Vertical, *x, *y, event)
                    {
                        return Some(response);
                    }
                }

                // Check horizontal scrollbar
                if self.scroll_axis.allows_horizontal()
                    && self.scroll_state.needs_horizontal_scrollbar()
                {
                    if let Some(response) =
                        self.handle_scrollbar_click(ScrollbarAxis::Horizontal, *x, *y, event)
                    {
                        return Some(response);
                    }
                }
            }

            Event::MouseMove { x, y } => {
                // Handle dragging
                if self.scroll_state.scrollbar_dragging {
                    return Some(self.handle_scrollbar_drag(ScrollbarAxis::Vertical, *y));
                }
                if self.scroll_state.h_scrollbar_dragging {
                    return Some(self.handle_scrollbar_drag(ScrollbarAxis::Horizontal, *x));
                }

                // Update hover states
                let mut needs_repaint = false;

                if self.scroll_axis.allows_vertical()
                    && self.scroll_state.needs_vertical_scrollbar()
                {
                    needs_repaint |=
                        self.update_scrollbar_hover(ScrollbarAxis::Vertical, *x, *y, event);
                }

                if self.scroll_axis.allows_horizontal()
                    && self.scroll_state.needs_horizontal_scrollbar()
                {
                    needs_repaint |=
                        self.update_scrollbar_hover(ScrollbarAxis::Horizontal, *x, *y, event);
                }

                if needs_repaint {
                    request_animation_frame();
                }
            }

            Event::MouseUp { button, .. } if *button == MouseButton::Left => {
                if self.scroll_state.scrollbar_dragging {
                    self.scroll_state.scrollbar_dragging = false;
                    if let Some(ref mut handle) = self.v_scrollbar_handle {
                        handle.event(event);
                    }
                    request_animation_frame();
                    return Some(EventResponse::Handled);
                }
                if self.scroll_state.h_scrollbar_dragging {
                    self.scroll_state.h_scrollbar_dragging = false;
                    if let Some(ref mut handle) = self.h_scrollbar_handle {
                        handle.event(event);
                    }
                    request_animation_frame();
                    return Some(EventResponse::Handled);
                }
            }

            Event::MouseLeave => {
                // Clear scrollbar hover state
                if self.scroll_state.scrollbar_hovered || self.scroll_state.h_scrollbar_hovered {
                    self.scroll_state.scrollbar_hovered = false;
                    self.scroll_state.h_scrollbar_hovered = false;
                    if let Some(ref mut handle) = self.v_scrollbar_handle {
                        handle.event(event);
                    }
                    if let Some(ref mut handle) = self.h_scrollbar_handle {
                        handle.event(event);
                    }
                    request_animation_frame();
                }
                // Stop dragging
                if self.scroll_state.scrollbar_dragging || self.scroll_state.h_scrollbar_dragging {
                    self.scroll_state.scrollbar_dragging = false;
                    self.scroll_state.h_scrollbar_dragging = false;
                    request_animation_frame();
                }
            }

            _ => {}
        }

        None
    }

    fn handle_scrollbar_click(
        &mut self,
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
            self.bounds,
            &self.scrollbar_config,
            needs_other,
        );
        let hit_area = self.scroll_state.scrollbar_hit_area(
            axis,
            self.bounds,
            &self.scrollbar_config,
            needs_other,
        );

        if handle_rect.contains(x, y) {
            // Start dragging handle
            self.scroll_state.set_dragging(axis, true);
            let (pos, offset) = match axis {
                ScrollbarAxis::Vertical => (y, self.scroll_state.offset_y),
                ScrollbarAxis::Horizontal => (x, self.scroll_state.offset_x),
            };
            self.scroll_state.set_drag_start(axis, pos, offset);

            // Forward event to handle container for pressed state
            let handle_container = match axis {
                ScrollbarAxis::Vertical => &mut self.v_scrollbar_handle,
                ScrollbarAxis::Horizontal => &mut self.h_scrollbar_handle,
            };
            if let Some(ref mut handle) = handle_container {
                handle.event(event);
            }

            request_animation_frame();
            return Some(EventResponse::Handled);
        } else if hit_area.contains(x, y) {
            // Click on track - jump to position
            let track_rect = self.scroll_state.scrollbar_track_rect(
                axis,
                self.bounds,
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
                request_animation_frame();
            }
            return Some(EventResponse::Handled);
        }

        None
    }

    fn handle_scrollbar_drag(&mut self, axis: ScrollbarAxis, pos: f32) -> EventResponse {
        let needs_other = match axis {
            ScrollbarAxis::Vertical => self.scroll_state.needs_horizontal_scrollbar(),
            ScrollbarAxis::Horizontal => self.scroll_state.needs_vertical_scrollbar(),
        };
        let track = self.scroll_state.scrollbar_track_rect(
            axis,
            self.bounds,
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
            request_animation_frame();
        }

        EventResponse::Handled
    }

    fn update_scrollbar_hover(
        &mut self,
        axis: ScrollbarAxis,
        x: f32,
        y: f32,
        event: &Event,
    ) -> bool {
        let needs_other = match axis {
            ScrollbarAxis::Vertical => self.scroll_state.needs_horizontal_scrollbar(),
            ScrollbarAxis::Horizontal => self.scroll_state.needs_vertical_scrollbar(),
        };
        let hit_area = self.scroll_state.scrollbar_hit_area(
            axis,
            self.bounds,
            &self.scrollbar_config,
            needs_other,
        );
        let handle_rect = self.scroll_state.scrollbar_handle_rect(
            axis,
            self.bounds,
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
        let handle_container = match axis {
            ScrollbarAxis::Vertical => &mut self.v_scrollbar_handle,
            ScrollbarAxis::Horizontal => &mut self.h_scrollbar_handle,
        };
        if let Some(ref mut handle) = handle_container {
            handle.event(event);
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
