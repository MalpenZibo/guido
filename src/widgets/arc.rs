//! A ring, or part of one: a progress ring, a timer, a gauge.

use crate::animation::IntoAnimated;
use crate::clock::FrameInstant;
use crate::jobs::{JobRequest, RequiredJob, request_job};
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
    /// The motions `color`, `track` and `progress` were declared with, if
    /// any. Absent unless one was, as a text's are.
    anims: Option<Box<ArcAnims>>,
}

/// Where a declared arc property keeps its motion.
#[derive(Default)]
struct ArcAnims {
    color: Option<AnimationState<Color>>,
    track: Option<AnimationState<Color>>,
    progress: Option<AnimationState<f32>>,
}

impl ArcAnims {
    /// Whether any motion is still on its way.
    fn is_animating(&self) -> bool {
        self.color
            .as_ref()
            .is_some_and(AnimationState::is_animating)
            || self
                .track
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

    /// The colour of the whole line, drawn under the covered part, and how it
    /// moves. None when nothing says.
    pub fn track<M>(mut self, color: impl IntoAnimated<Color, M>) -> Self {
        self.track = declare(&mut self.anims, color, |a: &mut ArcAnims| &mut a.track);
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
        let ArcAnims {
            color,
            track,
            progress,
        } = &mut *anims;
        if let Some(a) = color {
            a.retarget(ctx, self.color.get_or(Color::WHITE), now);
        }
        if let Some(a) = track {
            a.retarget(ctx, self.track.get_or(Color::TRANSPARENT), now);
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
        let ArcAnims {
            color,
            track,
            progress,
        } = &mut *anims;
        let moved = color.as_mut().is_some_and(|a| a.advance(now).is_changed())
            | track.as_mut().is_some_and(|a| a.advance(now).is_changed())
            | progress
                .as_mut()
                .is_some_and(|a| a.advance(now).is_changed());
        (moved, anims.is_animating())
    }
}

impl Widget for Arc {
    fn advance_animations(&mut self, tree: &mut Tree, id: WidgetId) -> bool {
        let (moved, running) = self.advance(tree.frame_instant());
        if moved {
            request_job(id, JobRequest::Paint);
        }
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

        let anims = self.anims.as_deref();
        let track = get_animated_value(anims.and_then(|a| a.track.as_ref()), || {
            self.track.get_or(Color::TRANSPARENT)
        });
        if track.a > 0.0 {
            ctx.draw_arc(
                center,
                radius,
                start,
                sweep,
                Stroke::new(width, track).caps(cap),
            );
        }
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

    /// Every arc colour a frame at a named instant drew, in paint order.
    fn colours_at(tree: &mut Tree, root: WidgetId, at: std::time::Instant) -> Vec<Color> {
        tree.set_frame_instant(Some(at));
        jobs::pump_and_layout(tree, root, Constraints::new(0.0, 0.0, 100.0, 100.0))
            .expect("the root is registered");
        let mut node = RenderNode::new(root.as_u64());
        tree.paint_widget(root, &mut node, 1.0);
        tree.set_frame_instant(None);

        fn walk(node: &RenderNode, out: &mut Vec<Color>) {
            out.extend(node.commands.iter().filter_map(|cmd| match **cmd {
                DrawCommand::Arc { color, .. } => Some(color),
                _ => None,
            }));
            for child in &node.children {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        walk(&node, &mut out);
        out
    }

    /// A track declared with a motion eases into a new colour, as the covered
    /// part does, where one declared without it switches (#951).
    ///
    /// The pair differs in nothing but the transition and follows the same
    /// signal, so the eased track being between the two colours a frame later
    /// is the claim, and the plain one having arrived is what says the test is
    /// not watching a slow machine. Neither has any progress, so each draws its
    /// track and nothing else.
    #[test]
    fn a_declared_transition_eases_the_track() {
        use crate::animation::Animate;

        const FROM: Color = Color::rgb(0.0, 0.0, 0.0);
        const TO: Color = Color::rgb(1.0, 1.0, 1.0);
        let palette = create_signal(FROM);

        // One tree, so the two arcs do not share ids and the job queue.
        let mut tree = Tree::new();
        let root = tree.register(Box::new(
            crate::widgets::container()
                .child(
                    arc()
                        .size(20.0)
                        .track((move || palette.get()).transition(400.0)),
                )
                .child(arc().size(20.0).track(palette)),
        ));
        tree.with_widget_mut(root, |w, id, t| w.register_children(t, id));

        let t0 = std::time::Instant::now();
        colours_at(&mut tree, root, t0);

        palette.set(TO);

        // The frame the write lands on is where the ease begins.
        colours_at(&mut tree, root, t0 + std::time::Duration::from_millis(1));
        let drawn = colours_at(&mut tree, root, t0 + std::time::Duration::from_millis(100));
        let [eased, plain] = drawn[..] else {
            panic!("each arc draws its track and nothing else, got {drawn:?}");
        };
        assert!(
            plain.r > 0.99,
            "a track declared without a transition switches, got {plain:?}"
        );
        assert!(
            eased.r > 0.01 && eased.r < 0.99,
            "a quarter of the way through a 400ms ease the track has to be \
             between the two, got {eased:?}"
        );

        let settled = colours_at(&mut tree, root, t0 + std::time::Duration::from_millis(600));
        assert!(
            settled[0].r > 0.99,
            "the ease never finished, got {:?}",
            settled[0]
        );
    }
}
