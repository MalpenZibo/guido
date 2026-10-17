//! A ring, or part of one: a progress ring, a timer, a gauge.

use crate::animation::IntoAnimated;
use crate::clock::FrameInstant;
use crate::jobs::{JobRequest, RequiredJob, request_animation_frame, request_job};
use crate::layout::{Constraints, Size};
use crate::reactive::{IntoSignal, Prop};
use crate::renderer::{Cap, PaintContext, Stroke};
use crate::tree::{LayoutCtx, Tree, WidgetId};

use super::container::{AnimationState, declare, get_animated_value};
use super::widget::{Color, Widget};

/// How wide an arc is when nothing says, in logical pixels: an icon's size.
const DEFAULT_SIZE: f32 = 24.0;
/// How thick its line is when nothing says, in logical pixels.
const DEFAULT_STROKE: f32 = 2.0;

/// An arc drawn round the middle of a square box — see [`arc`].
pub struct Arc {
    size: Prop<f32>,
    stroke: Prop<f32>,
    color: Prop<Color>,
    track: Prop<Color>,
    cap: Prop<Cap>,
    start: Prop<f32>,
    sweep: Prop<f32>,
    progress: Prop<f32>,
    /// The motions `color` and `progress` were declared with, if any. Absent
    /// unless one was, as a text's are.
    anims: Option<Box<ArcAnims>>,
}

/// Where a declared arc property keeps its motion.
#[derive(Default)]
struct ArcAnims {
    color: Option<AnimationState<Color>>,
    progress: Option<AnimationState<f32>>,
}

impl ArcAnims {
    fn states(
        &mut self,
    ) -> (
        Option<&mut AnimationState<Color>>,
        Option<&mut AnimationState<f32>>,
    ) {
        (self.color.as_mut(), self.progress.as_mut())
    }

    /// Whether either motion is still on its way.
    fn is_animating(&self) -> bool {
        self.color
            .as_ref()
            .is_some_and(AnimationState::is_animating)
            || self
                .progress
                .as_ref()
                .is_some_and(AnimationState::is_animating)
    }
}

impl Arc {
    fn new() -> Self {
        Self {
            size: Prop::Unset,
            stroke: Prop::Unset,
            color: Prop::Unset,
            track: Prop::Unset,
            cap: Prop::Unset,
            start: Prop::Unset,
            sweep: Prop::Unset,
            progress: Prop::Unset,
            anims: None,
        }
    }

    /// The side of the square box the arc is drawn in, in logical pixels.
    /// 24 when nothing says.
    pub fn size<M>(mut self, size: impl IntoSignal<f32, M>) -> Self {
        self.size = size.into_prop();
        self
    }

    /// How thick the line is, in logical pixels. 2 when nothing says. The
    /// line runs inside the box: its outer edge touches the box's sides.
    pub fn stroke<M>(mut self, width: impl IntoSignal<f32, M>) -> Self {
        self.stroke = width.into_prop();
        self
    }

    /// The colour of the part of the line [`progress`](Self::progress) has
    /// covered, and how it moves. White when nothing says.
    pub fn color<M>(mut self, color: impl IntoAnimated<Color, M>) -> Self {
        self.color = declare(&mut self.anims, color, |a: &mut ArcAnims| &mut a.color);
        self
    }

    /// The colour of the whole line, drawn under the covered part. None when
    /// nothing says.
    pub fn track<M>(mut self, color: impl IntoSignal<Color, M>) -> Self {
        self.track = color.into_prop();
        self
    }

    /// How the ends of the line are drawn, the track's included. Butt when
    /// nothing says.
    pub fn caps<M>(mut self, cap: impl IntoSignal<Cap, M>) -> Self {
        self.cap = cap.into_prop();
        self
    }

    /// Where the line begins, in degrees clockwise from twelve o'clock. 0
    /// when nothing says.
    pub fn start<M>(mut self, degrees: impl IntoSignal<f32, M>) -> Self {
        self.start = degrees.into_prop();
        self
    }

    /// How far the whole line runs from [`start`](Self::start), in degrees —
    /// 270 is a gauge open at the bottom when it starts at -135. A full turn
    /// when nothing says; a negative sweep runs anticlockwise.
    pub fn sweep<M>(mut self, degrees: impl IntoSignal<f32, M>) -> Self {
        self.sweep = degrees.into_prop();
        self
    }

    /// How much of the line is covered, from 0 to 1, and how it moves. Held
    /// in that range; 0 when nothing says, which draws the track alone.
    pub fn progress<M>(mut self, fraction: impl IntoAnimated<f32, M>) -> Self {
        self.progress = declare(&mut self.anims, fraction, |a: &mut ArcAnims| {
            &mut a.progress
        });
        self
    }

    /// Point the declared motions at the values now declared, and ask for the
    /// frame that moves them.
    ///
    /// During layout, as a text's colour is retargeted: the declared value has
    /// to be read under tracking for a change to reach the motion, and only an
    /// animated property pays for that read — the rest are read at paint.
    fn retarget(&mut self, ctx: &mut LayoutCtx, id: WidgetId) {
        let Some(anims) = self.anims.as_deref_mut() else {
            return;
        };
        let now = ctx.tree_ref().frame_instant();
        let (color, progress) = anims.states();
        if let Some(a) = color {
            a.retarget(ctx, self.color.get_or(Color::WHITE), now);
        }
        if let Some(a) = progress {
            a.retarget(ctx, self.progress.get_or(0.0), now);
        }
        if anims.is_animating() {
            request_job(id, JobRequest::Animation(RequiredJob::Paint));
        }
    }

    fn advance(&mut self, now: FrameInstant) -> (bool, bool) {
        let Some(anims) = self.anims.as_deref_mut() else {
            return (false, false);
        };
        let (color, progress) = anims.states();
        let moved = color.is_some_and(|a| a.advance(now).is_changed())
            | progress.is_some_and(|a| a.advance(now).is_changed());
        (moved, anims.is_animating())
    }
}

impl Widget for Arc {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let (moved, running) = self.advance(tree.frame_instant());
        request_animation_frame(id, moved.then_some(RequiredJob::Paint), running);
        running
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, constraints: Constraints) -> Size {
        let id = ctx.id();
        ctx.tree().set_relayout_boundary(id, false);
        self.retarget(ctx, id);
        let side = self.size.get_or(DEFAULT_SIZE).max(0.0);
        constraints.constrain(Size::new(side, side))
    }

    fn paint(&self, ctx: &mut PaintContext) {
        let size = ctx.tree().cached_size(ctx.id()).unwrap_or_default();
        let side = size.width.min(size.height);
        // A line no wider than the box is wide is the most that fits round
        // it; `draw_arc` draws nothing for one of no width.
        let width = self.stroke.get_or(DEFAULT_STROKE).clamp(0.0, side * 0.5);
        let center = (size.width * 0.5, size.height * 0.5);
        let radius = (side - width) * 0.5;
        let cap = self.cap.get_or(Cap::Butt);
        let start = self.start.get_or(0.0);
        let sweep = self.sweep.get_or(360.0);

        if let Some(track) = self.track.get().filter(|c| c.a > 0.0) {
            ctx.draw_arc(
                center,
                radius,
                start,
                sweep,
                Stroke::new(width, track).caps(cap),
            );
        }
        let anims = self.anims.as_deref();
        let progress = get_animated_value(anims.and_then(|a| a.progress.as_ref()), || {
            self.progress.get_or(0.0)
        })
        .clamp(0.0, 1.0);
        let color = get_animated_value(anims.and_then(|a| a.color.as_ref()), || {
            self.color.get_or(Color::WHITE)
        });
        ctx.draw_arc(
            center,
            radius,
            start,
            sweep * progress,
            Stroke::new(width, color).caps(cap),
        );
    }
}

/// An arc of a circle, centred in a square box: a progress ring, a countdown,
/// a gauge.
///
/// The line runs inside the box, [`stroke`](Arc::stroke) thick, from
/// [`start`](Arc::start) round [`sweep`](Arc::sweep) degrees; a
/// [`track`](Arc::track) draws all of it and [`progress`](Arc::progress) says
/// how much of it is covered in [`color`](Arc::color). It is drawn exactly, as
/// one shape, so a progress that animates every frame costs nothing to
/// re-tessellate.
///
/// ```
/// # use guido::prelude::*;
/// let battery = create_signal(64.0_f32);
/// arc()
///     .size(18.0)
///     .stroke(2.6)
///     .track(Color::WHITE.with_alpha(0.16))
///     .color(Color::rgb(0.4, 0.85, 0.5))
///     .caps(Cap::Round)
///     .progress((move || battery.get() / 100.0).transition(1000.0));
/// ```
pub fn arc() -> Arc {
    Arc::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs;
    use crate::reactive::create_signal;
    use crate::renderer::{DrawCommand, RenderNode};
    use crate::widgets::Point;

    fn painted(widget: Arc) -> Vec<(f32, f32, Color)> {
        let mut tree = Tree::new();
        let root = tree.register(Box::new(widget));
        jobs::pump_and_layout(&mut tree, root, Constraints::new(0.0, 0.0, 100.0, 100.0))
            .expect("the root is registered");
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node, 1.0);
        node.commands
            .iter()
            .filter_map(|cmd| match **cmd {
                DrawCommand::Arc {
                    sweep,
                    radius,
                    color,
                    center,
                    ..
                } => {
                    assert_eq!(center, Point::new(10.0, 10.0));
                    Some((sweep, radius, color))
                }
                _ => None,
            })
            .collect()
    }

    /// The track runs the whole sweep, the covered part its fraction of it,
    /// and both sit on the circle whose line touches the box.
    #[test]
    fn the_track_and_the_covered_part_share_one_circle() {
        let drawn = painted(
            arc()
                .size(20.0)
                .stroke(4.0)
                .track(Color::BLACK)
                .color(Color::RED)
                .sweep(270.0)
                .progress(0.5),
        );
        assert_eq!(
            drawn,
            vec![(270.0, 8.0, Color::BLACK), (135.0, 8.0, Color::RED)]
        );
    }

    /// No progress draws no covered part, and a progress past one is a full
    /// sweep rather than more than one.
    #[test]
    fn progress_is_held_between_none_and_all() {
        assert!(painted(arc().size(20.0)).is_empty());
        assert_eq!(painted(arc().size(20.0).progress(3.0)).len(), 1);
        assert_eq!(painted(arc().size(20.0).progress(3.0))[0].0, 360.0);
    }

    /// The progress is reactive: a new value repaints with the new sweep.
    #[test]
    fn a_new_progress_is_a_new_sweep() {
        let level = create_signal(0.25_f32);
        let mut tree = Tree::new();
        let root = tree.register(Box::new(arc().size(20.0).progress(level)));
        let constraints = Constraints::new(0.0, 0.0, 100.0, 100.0);
        jobs::pump_and_layout(&mut tree, root, constraints).expect("registered");
        let sweep = |tree: &mut Tree| {
            let mut node = RenderNode::new(root.as_u64());
            tree.paint_widget(root, &mut node, 1.0);
            node.commands.iter().find_map(|cmd| match **cmd {
                DrawCommand::Arc { sweep, .. } => Some(sweep),
                _ => None,
            })
        };
        assert_eq!(sweep(&mut tree), Some(90.0));
        level.set(0.75);
        jobs::pump_and_layout(&mut tree, root, constraints).expect("registered");
        assert_eq!(sweep(&mut tree), Some(270.0));
    }
}
