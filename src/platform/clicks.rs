//! Counting clicks: which press of a run a press is.
//!
//! Wayland says nothing about double clicks — `wl_pointer.button` is a press
//! and a timestamp — so the count is the toolkit's, as it is in GTK's
//! `GestureClick` and as winit leaves it to whoever sits above it. Both
//! platforms pass what they queue through one [`ClickCounter`], so the run the
//! recorder counts is the run a compositor's presses are counted into.
//!
//! A press continues the run when it is the same button, within
//! [`DOUBLE_CLICK_TIME`] and [`DOUBLE_CLICK_DISTANCE`] of the press before it,
//! on the same surface, with no drag between. The last two rules are bugs
//! others fixed: a right click after a left one is not a double click
//! (wxWidgets), and neither is a press that dragged before the next one landed
//! (Mozilla).

use std::time::{Duration, Instant};

use crate::surface::SurfaceId;
use crate::widgets::{Event, MouseButton, Point, PointerKind};

/// The longest a press may follow the one before it and still be the next of
/// its run. Fixed for now; the desktop's own setting is a later change.
pub(crate) const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(400);

/// The farthest a press may land from the one before it and still be the next
/// of its run, in logical pixels — and the farthest the pointer may move while
/// held before the press is a drag.
pub(crate) const DOUBLE_CLICK_DISTANCE: f32 = 5.0;

/// The press a run has reached, and whether it has since dragged.
#[derive(Debug, Clone, Copy)]
struct LastPress {
    surface: SurfaceId,
    button: MouseButton,
    at: Point,
    when: Instant,
    count: u32,
    /// The button is still down. Only a move while it is counts as a drag.
    held: bool,
    dragged: bool,
}

/// The run of presses a seat is in.
#[derive(Debug, Default)]
pub(crate) struct ClickCounter {
    last: Option<LastPress>,
}

impl ClickCounter {
    /// Read one event on its way to `surface`, and write the count into a
    /// press. Anything already in the press's `count` is replaced: the
    /// compositor never sends one.
    pub(crate) fn observe(&mut self, surface: SurfaceId, when: Instant, event: &mut Event) {
        match event {
            Event::MouseDown {
                at,
                button,
                pointer,
                count,
            } => {
                *count = match (*pointer, *at) {
                    (PointerKind::Mouse, Some(at)) => self.press(surface, *button, at, when),
                    // Touch double tap is not counted, and a finger breaks the
                    // mouse's run as any other press would.
                    _ => {
                        self.last = None;
                        1
                    }
                };
            }
            Event::MouseMove { at: Some(at), .. } => {
                if let Some(last) = &mut self.last
                    && last.held
                    && !near(last.at, *at)
                {
                    last.dragged = true;
                }
            }
            Event::MouseUp { button, .. } => {
                if let Some(last) = &mut self.last
                    && last.button == *button
                {
                    last.held = false;
                }
            }
            _ => {}
        }
    }

    fn press(&mut self, surface: SurfaceId, button: MouseButton, at: Point, when: Instant) -> u32 {
        let count = match self.last {
            Some(last)
                if last.surface == surface
                    && last.button == button
                    && !last.dragged
                    && when.saturating_duration_since(last.when) <= DOUBLE_CLICK_TIME
                    && near(last.at, at) =>
            {
                last.count.saturating_add(1)
            }
            _ => 1,
        };
        self.last = Some(LastPress {
            surface,
            button,
            at,
            when,
            count,
            held: true,
            dragged: false,
        });
        count
    }
}

/// Whether two points are within [`DOUBLE_CLICK_DISTANCE`] of each other.
fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).hypot(a.y - b.y) <= DOUBLE_CLICK_DISTANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counts a sequence of events gives its presses, each event `ms`
    /// milliseconds after the start, on the surface it names.
    fn counts_on(events: impl IntoIterator<Item = (u64, SurfaceId, Event)>) -> Vec<u32> {
        let start = Instant::now();
        let mut counter = ClickCounter::default();
        let mut counts = Vec::new();
        for (ms, surface, mut event) in events {
            counter.observe(surface, start + Duration::from_millis(ms), &mut event);
            if let Event::MouseDown { count, .. } = event {
                counts.push(count);
            }
        }
        counts
    }

    /// The same, with every event on one surface.
    fn counts(events: impl IntoIterator<Item = (u64, Event)>) -> Vec<u32> {
        let surface = SurfaceId::next();
        counts_on(events.into_iter().map(|(ms, event)| (ms, surface, event)))
    }

    fn click(ms: u64, x: f32, y: f32, button: MouseButton) -> [(u64, Event); 2] {
        [
            (ms, Event::mouse_down(x, y, button)),
            (ms + 20, Event::mouse_up(x, y, button)),
        ]
    }

    fn left(ms: u64, x: f32) -> [(u64, Event); 2] {
        click(ms, x, 10.0, MouseButton::Left)
    }

    #[test]
    fn three_quick_presses_in_one_place_count_one_two_three() {
        assert_eq!(
            counts(
                left(0, 10.0)
                    .into_iter()
                    .chain(left(150, 10.0))
                    .chain(left(300, 10.0))
            ),
            [1, 2, 3]
        );
    }

    #[test]
    fn a_press_after_the_time_or_beyond_the_distance_starts_again() {
        assert_eq!(
            counts(left(0, 10.0).into_iter().chain(left(401, 10.0))),
            [1, 1]
        );
        assert_eq!(
            counts(left(0, 10.0).into_iter().chain(left(400, 10.0))),
            [1, 2]
        );
        assert_eq!(
            counts(left(0, 10.0).into_iter().chain(left(100, 15.1))),
            [1, 1]
        );
        assert_eq!(
            counts(left(0, 10.0).into_iter().chain(left(100, 15.0))),
            [1, 2]
        );
    }

    #[test]
    fn another_button_starts_again() {
        let right = click(100, 10.0, 10.0, MouseButton::Right);
        assert_eq!(counts(left(0, 10.0).into_iter().chain(right)), [1, 1]);
    }

    /// Pressed, dragged away and back while held, released where it began:
    /// the next press lands on the first one's spot in time, and is still a
    /// first press.
    #[test]
    fn a_drag_between_the_presses_starts_again() {
        let dragged = [
            (0, Event::mouse_down(10.0, 10.0, MouseButton::Left)),
            (20, Event::mouse_move(40.0, 10.0)),
            (40, Event::mouse_move(10.0, 10.0)),
            (60, Event::mouse_up(10.0, 10.0, MouseButton::Left)),
        ];
        assert_eq!(counts(dragged.into_iter().chain(left(100, 10.0))), [1, 1]);
    }

    /// A move with no button held is not a drag: the pointer may wander and
    /// come back between two clicks of a double click.
    #[test]
    fn a_move_with_the_button_up_or_within_the_distance_is_not_a_drag() {
        let wandered = left(0, 10.0)
            .into_iter()
            .chain([
                (30, Event::mouse_move(40.0, 10.0)),
                (60, Event::mouse_move(10.0, 10.0)),
            ])
            .chain(left(100, 10.0));
        assert_eq!(counts(wandered), [1, 2]);

        let trembled = [
            (0, Event::mouse_down(10.0, 10.0, MouseButton::Left)),
            (10, Event::mouse_move(13.0, 10.0)),
            (20, Event::mouse_up(13.0, 10.0, MouseButton::Left)),
        ];
        assert_eq!(counts(trembled.into_iter().chain(left(100, 10.0))), [1, 2]);
    }

    #[test]
    fn a_finger_always_presses_one_and_breaks_the_run() {
        let tapped = [
            (0, Event::finger_down(10.0, 10.0)),
            (20, Event::mouse_up(10.0, 10.0, MouseButton::Left)),
            (100, Event::finger_down(10.0, 10.0)),
        ];
        assert_eq!(counts(tapped), [1, 1]);

        let between = left(0, 10.0)
            .into_iter()
            .chain([(50, Event::finger_down(10.0, 10.0))])
            .chain(left(100, 10.0));
        assert_eq!(counts(between), [1, 1, 1]);
    }

    #[test]
    fn a_press_on_another_surface_starts_again() {
        let (first, second) = (SurfaceId::next(), SurfaceId::next());
        let press = || Event::mouse_down(10.0, 10.0, MouseButton::Left);
        assert_eq!(
            counts_on([(0, first, press()), (100, second, press())]),
            [1, 1]
        );
    }
}
