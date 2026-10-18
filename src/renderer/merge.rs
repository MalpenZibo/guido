//! Shapes merged into one: the backgrounds of a container's direct children,
//! drawn as a single smooth union — see
//! [`Container::merge_children`](crate::widgets::Container::merge_children).
//!
//! A container that merges says so on its render node. Flatten, where every
//! child's world transform is already being composed, collects each direct
//! child's background — the commands the child marked as its own box — and
//! emits one [`DrawCommand::MergedShape`] in their place, at the container's
//! own place in paint order. The renderer turns that into one instance whose
//! members are records in a storage buffer, and the fragment shader folds
//! their distances with a polynomial smooth minimum.

use std::rc::Rc;

use super::commands::{Border, CornerRadii, DrawCommand};
use super::flatten::union_of;
use super::tree::RenderNode;
use super::types::Shadows;
use crate::shape::PlacedShape;
use crate::transform::Transform;
use crate::widgets::{Color, Rect};

/// The most members one merged shape takes. Children past it are drawn on
/// their own: a fragment walks every member, so the cost of a merged shape
/// grows with each one, and a bound keeps that cost something a layout can
/// see.
pub const MAX_MEMBERS: usize = 16;

/// What a container that merges its children declares on its render node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergeChildren {
    /// How close two members come before their outlines reach for each other,
    /// in logical pixels; a neck joins them once the gap is half of it. `0.0`
    /// is a plain union: overlapping members become one shape, and members
    /// apart stay apart.
    pub distance: f32,
    /// The container's own border, drawn along the merged outline in place
    /// of the members' borders.
    pub border: Option<Border>,
    /// The container's own outer shadows, cast by the merged shape. Its
    /// inset ones stay on its own box.
    pub shadows: Shadows,
}

/// One child's background, as a merged shape holds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MergeMember {
    /// The box, in the member's own space.
    pub rect: Rect,
    pub radius: CornerRadii,
    pub curvature: f32,
    /// The fill. Transparent for a member that draws only a border.
    pub color: Color,
    pub border: Option<Border>,
    /// The member's own space to the merged command's.
    pub placement: Transform,
    /// The member's own opacity, under the one the command is drawn at.
    pub opacity: f32,
}

/// What collecting a container's members produced: the merged command, the
/// shadows the members cast on their own, and which of each child's commands
/// the merged shape stands in for.
pub(crate) struct Gathered {
    pub(crate) command: Rc<DrawCommand>,
    /// A member's own shadow, which is not the merged shape's: drawn beneath
    /// it as the member would have drawn it, as `(child, shadow command)`.
    pub(crate) shadows: Vec<(usize, Rc<DrawCommand>)>,
    /// Per child, the commands the merged shape replaces, by address — the
    /// same `Rc` the child's node holds, which a replay of the child's own
    /// cache hands back too.
    pub(crate) taken: Vec<Taken>,
    /// The child each member is the background of, in member order.
    members_of: Vec<usize>,
    merge: MergeChildren,
}

impl Gathered {
    /// The merged command once `children` are no longer members, or `None`
    /// when no member is left.
    pub(crate) fn without(&self, children: &[usize]) -> Option<Rc<DrawCommand>> {
        let DrawCommand::MergedShape { members, .. } = &*self.command else {
            unreachable!("gather builds a merged shape")
        };
        let kept = members
            .iter()
            .zip(&self.members_of)
            .filter(|(_, child)| !children.contains(child))
            .map(|(member, _)| *member);
        merged_shape(kept.collect(), &self.merge)
    }
}

/// One child's fill and border frame, at most.
pub(crate) type Taken = smallvec::SmallVec<[*const DrawCommand; 2]>;

/// Why a child with a background was drawn on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeftOut {
    /// Its fill is a gradient, which the merged shape cannot blend.
    Gradient,
    /// [`MAX_MEMBERS`] came before it.
    TooMany,
    /// It was composited as a layer, which holds its background, blurred or
    /// faded with the rest of it.
    Layer,
}

impl LeftOut {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::Gradient => "its fill is a gradient, which a merged shape does not blend",
            Self::TooMany => "a merged shape takes no more members than `MAX_MEMBERS`",
            Self::Layer => "it is drawn as a layer, which holds its background",
        }
    }
}

/// Collect the backgrounds of `node`'s direct children into one merged
/// command, or `None` when no child draws one.
///
/// A child's background is every `RoundedRect` it marked as one — its fill,
/// and the border frame a container draws as a second command. A child whose
/// fill is a gradient keeps its background, as does every child past
/// [`MAX_MEMBERS`]; `left_out` hears about each. A child that turns out to be
/// drawn as a layer is only known once it is flattened, and is left out then
/// — see [`Gathered::without`].
pub(crate) fn gather(
    node: &RenderNode,
    merge: &MergeChildren,
    mut left_out: impl FnMut(LeftOut),
) -> Option<Gathered> {
    let mut members = Vec::new();
    let mut members_of = Vec::new();
    let mut shadows = Vec::new();
    let mut taken = Vec::with_capacity(node.children.len());

    for (index, child) in node.children.iter().enumerate() {
        taken.push(Taken::new());
        let mut member: Option<MergeMember> = None;
        let mut riding = None;
        let mut own_shadows = Vec::new();
        let mut gradient = false;
        let mut replaced = Taken::new();
        for command in &child.commands {
            let DrawCommand::RoundedRect {
                rect,
                color,
                radius,
                curvature,
                border,
                shadow: cast,
                gradient: fill,
                background: true,
                shadow_only,
            } = &**command
            else {
                continue;
            };
            // An inset shadow is cast over the fill, inside the member's own
            // box: it stays the member's, drawn over the merged shape with
            // what the member holds.
            if cast.is_some_and(|s| s.inset) {
                continue;
            }
            gradient |= fill.is_some();
            let into = member.get_or_insert(MergeMember {
                rect: *rect,
                radius: *radius,
                curvature: *curvature,
                color: Color::TRANSPARENT,
                border: None,
                placement: child.placement(),
                opacity: child.opacity,
            });
            match (border, cast) {
                // The frame a container draws over its fill.
                (Some(border), _) => into.border = Some(*border),
                // A further shadow, which a box draws as its fill again
                // beneath it: moved beneath the merged shape whole.
                (None, Some(_)) if *shadow_only => {
                    own_shadows.push((index, Rc::clone(command)));
                }
                (None, cast) => {
                    into.color = *color;
                    riding = *cast;
                }
            }
            replaced.push(Rc::as_ptr(command));
        }
        // A member nobody can see draws nothing, and must not pull its
        // neighbours' outline towards where it would be.
        let Some(member) = member.filter(|member| member.opacity > 0.0) else {
            continue;
        };
        let why = if gradient {
            Some(LeftOut::Gradient)
        } else if members.len() == MAX_MEMBERS {
            Some(LeftOut::TooMany)
        } else {
            None
        };
        if let Some(why) = why {
            left_out(why);
            continue;
        }
        taken[index] = replaced;
        shadows.append(&mut own_shadows);
        // The first shadow rides the fill's own rect, and has to be drawn on
        // its own now the fill is the merged shape's: beneath it, as a further
        // shadow is.
        if let Some(cast) = riding {
            shadows.push((
                index,
                Rc::new(DrawCommand::shadow_beneath(
                    member.rect,
                    member.color,
                    member.radius,
                    member.curvature,
                    cast,
                    None,
                )),
            ));
        }
        members.push(member);
        members_of.push(index);
    }

    Some(Gathered {
        command: merged_shape(members, merge)?,
        shadows,
        taken,
        members_of,
        merge: *merge,
    })
}

/// One merged command of `members`, or `None` for none.
fn merged_shape(members: Vec<MergeMember>, merge: &MergeChildren) -> Option<Rc<DrawCommand>> {
    let reach = |member: &MergeMember| member.placement.map_rect(member.rect);
    let bounds = members.iter().map(reach).reduce(union_of)?;
    // Where a smooth minimum of `k` lies at or below zero, `min(a, b)` is at
    // most `k / 4` — the most it ever takes off — so the neck never reaches
    // further than a quarter of the distance past the members' boxes.
    let bounds = bounds.outset(merge.distance * 0.25);
    Some(Rc::new(DrawCommand::MergedShape {
        members,
        distance: merge.distance,
        border: merge.border,
        shadows: merge.shadows,
        bounds,
    }))
}

/// The record the members of a merged shape continue at, or the end of them.
pub const NO_MERGE: u32 = u32::MAX;

/// One member as the shape shader reads it. Mirrors `MergeRecord` in
/// `shader.wgsl`, checked field for field by the test beside it.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MergeRecord {
    /// `[x, y, width, height]` in the member's own space, logical pixels.
    pub rect: [f32; 4],
    /// `[top_left, top_right, bottom_right, bottom_left]`, in that space.
    pub radii: [f32; 4],
    /// Physical world pixels to that space, `[a, b, tx, c, d, ty]`.
    pub inverse: [f32; 6],
    pub curvature: f32,
    /// One unit of the member's own space, in physical pixels — so the
    /// members' distances are measured in one unit and can be compared. The
    /// geometric mean of the two axes, so under a scale that differs between
    /// them the distance is an approximation, as a lone shape's antialiasing
    /// already is.
    pub to_pixels: f32,
    /// Premultiplied, so a transparent member blends towards nothing rather
    /// than towards black.
    pub fill: [f32; 4],
    /// Premultiplied too.
    pub border_color: [f32; 4],
    /// In physical pixels; `0.0` for a member with no border.
    pub border_width: f32,
    /// The smooth minimum's reach, in physical pixels: the merge distance.
    pub smoothing: f32,
    /// The next member, or [`NO_MERGE`].
    pub next: u32,
    pub _pad: u32,
}

impl MergeRecord {
    /// `member`, placed by `world` (the merged command's world transform),
    /// at `opacity` and the surface's `scale`. `border` is the container's
    /// own, which stands in for the member's when there is one.
    ///
    /// `None` for a member whose placement collapsed: it has no inside, and
    /// no distance to be compared with the others' in.
    pub(crate) fn new(
        member: &MergeMember,
        world: &Transform,
        border: Option<Border>,
        opacity: f32,
        distance: f32,
        scale: f32,
    ) -> Option<Self> {
        let placement = world.then(&member.placement);
        let shape = PlacedShape::placed(member.rect, member.radius, member.curvature, placement);
        let inverse = shape.to_local(scale)?;
        let opacity = opacity * member.opacity;
        let premultiplied = |c: Color| {
            let a = c.a * opacity;
            [c.r * a, c.g * a, c.b * a, a]
        };
        let border = border.or(member.border);
        let r = member.rect;
        Some(Self {
            rect: [r.x, r.y, r.width, r.height],
            radii: member.radius.to_array(),
            inverse: inverse.data,
            curvature: member.curvature,
            to_pixels: scale * placement.extract_scale(),
            fill: premultiplied(member.color),
            border_color: border.map_or([0.0; 4], |b| premultiplied(b.color)),
            border_width: border.map_or(0.0, |b| b.width * scale),
            smoothing: distance.max(0.0) * scale,
            next: NO_MERGE,
            _pad: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// The record is one layout written twice, here and in the shape shader,
    /// and a field read from the wrong bytes blends the wrong colour.
    #[test]
    fn the_shader_and_the_record_agree_field_for_field() {
        let rust = [
            ("rect", offset_of!(MergeRecord, rect)),
            ("radii", offset_of!(MergeRecord, radii)),
            ("inverse", offset_of!(MergeRecord, inverse)),
            ("curvature", offset_of!(MergeRecord, curvature)),
            ("to_pixels", offset_of!(MergeRecord, to_pixels)),
            ("fill", offset_of!(MergeRecord, fill)),
            ("border_color", offset_of!(MergeRecord, border_color)),
            ("border_width", offset_of!(MergeRecord, border_width)),
            ("smoothing", offset_of!(MergeRecord, smoothing)),
            ("next", offset_of!(MergeRecord, next)),
        ];
        let (wgsl, size) = super::super::gpu::instance_layout::wgsl_fields(
            "src/renderer/shader.wgsl",
            "MergeRecord",
        );
        assert_eq!(
            wgsl.iter()
                .map(|(name, offset)| (name.as_str(), *offset))
                .collect::<Vec<_>>(),
            rust
        );
        assert_eq!(size, size_of::<MergeRecord>());
    }

    /// The shader walks no more members than gather hands it; fewer, and it
    /// would drop the last of them without a word.
    #[test]
    fn the_shader_walks_every_member_gather_keeps() {
        let shader = include_str!("shader.wgsl");
        assert!(
            shader.contains(&format!("const MAX_MERGED: u32 = {MAX_MEMBERS}u;")),
            "`MAX_MERGED` in shader.wgsl is not `MAX_MEMBERS`"
        );
    }

    fn background(rect: Rect, color: Color, border: Option<Border>) -> Rc<DrawCommand> {
        Rc::new(DrawCommand::RoundedRect {
            rect,
            color,
            radius: CornerRadii::uniform(4.0),
            curvature: 1.0,
            border,
            shadow: None,
            gradient: None,
            background: true,
            shadow_only: false,
        })
    }

    fn child(at: (f32, f32), commands: &[Rc<DrawCommand>]) -> Rc<RenderNode> {
        let mut node = RenderNode::with_bounds(1, Rect::new(0.0, 0.0, 20.0, 20.0));
        node.local_transform = Transform::translate(at.0, at.1);
        node.commands.extend(commands.iter().cloned());
        Rc::new(node)
    }

    const MERGE: MergeChildren = MergeChildren {
        distance: 8.0,
        border: None,
        shadows: Shadows::none(),
    };

    fn members(gathered: &Gathered) -> &[MergeMember] {
        let DrawCommand::MergedShape { members, .. } = &*gathered.command else {
            panic!("gather builds a merged shape");
        };
        members
    }

    /// A container's fill and its border frame are two commands and one
    /// member, and the merged shape stands in for both.
    #[test]
    fn a_fill_and_its_frame_are_one_member() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let fill = background(rect, Color::RED, None);
        let frame = background(
            rect,
            Color::TRANSPARENT,
            Some(Border::new(2.0, Color::BLUE)),
        );
        let mut group = RenderNode::new(0);
        group
            .children
            .push(child((10.0, 0.0), &[fill.clone(), frame.clone()]));

        let gathered = gather(&group, &MERGE, |_| panic!("nothing is left out")).unwrap();
        let [member] = members(&gathered) else {
            panic!("one child, one member");
        };
        assert_eq!(member.color, Color::RED);
        assert_eq!(member.border.map(|b| b.width), Some(2.0));
        assert_eq!(member.placement, Transform::translate(10.0, 0.0));
        assert_eq!(
            gathered.taken[0].to_vec(),
            vec![Rc::as_ptr(&fill), Rc::as_ptr(&frame)]
        );
    }

    /// A command a child did not mark as its background is not a member's —
    /// a selection band, a caret — and a child that marked nothing is not a
    /// member at all.
    #[test]
    fn only_what_a_child_marked_is_merged() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let mut group = RenderNode::new(0);
        group.children.push(child(
            (0.0, 0.0),
            &[Rc::new(DrawCommand::rounded_rect(rect, Color::RED, 0.0))],
        ));
        assert!(gather(&group, &MERGE, |_| {}).is_none());
    }

    /// A gradient member keeps its background and is said so; the rest
    /// merge.
    #[test]
    fn a_gradient_member_is_left_out_and_reported() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let gradient = Rc::new(DrawCommand::RoundedRect {
            rect,
            color: Color::RED,
            radius: CornerRadii::uniform(0.0),
            curvature: 1.0,
            border: None,
            shadow: None,
            gradient: Some(crate::renderer::Gradient {
                start_color: Color::RED,
                end_color: Color::BLUE,
                direction: crate::renderer::GradientDir::Horizontal,
            }),
            background: true,
            shadow_only: false,
        });
        let mut group = RenderNode::new(0);
        group.children.push(child((0.0, 0.0), &[gradient]));
        group
            .children
            .push(child((30.0, 0.0), &[background(rect, Color::GREEN, None)]));

        let mut told = Vec::new();
        let gathered = gather(&group, &MERGE, |why| told.push(why)).unwrap();
        assert_eq!(told, [LeftOut::Gradient]);
        assert_eq!(members(&gathered).len(), 1);
        assert_eq!(gathered.taken[0].len(), 0, "the gradient stays drawn");
        assert_eq!(gathered.taken[1].len(), 1);
    }

    /// A member at no opacity is not a member: it draws nothing, so it must
    /// not pull its neighbour's outline towards where it would be.
    #[test]
    fn an_invisible_child_does_not_shape_the_outline() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let mut group = RenderNode::new(0);
        group
            .children
            .push(child((0.0, 0.0), &[background(rect, Color::GREEN, None)]));
        let mut faded = RenderNode::with_bounds(2, rect);
        faded.local_transform = Transform::translate(24.0, 0.0);
        faded.opacity = 0.0;
        faded.commands.push(background(rect, Color::GREEN, None));
        group.children.push(Rc::new(faded));

        let gathered = gather(&group, &MERGE, |_| panic!("nothing is reported")).unwrap();
        assert_eq!(members(&gathered).len(), 1);
        assert!(
            gathered.taken[1].is_empty(),
            "it is drawn as it was, at nothing"
        );
    }

    /// A merged shape without some of its members holds the rest, over their
    /// own bounds, and is nothing once none is left. Members are named by
    /// child, which a child that is no member at all does not upset.
    #[test]
    fn a_merged_shape_lets_members_go() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let mut group = RenderNode::new(0);
        group.children.push(child((0.0, 0.0), &[]));
        for at in [0.0, 30.0, 60.0] {
            group
                .children
                .push(child((at, 0.0), &[background(rect, Color::GREEN, None)]));
        }
        let gathered = gather(&group, &MERGE, |_| panic!("nothing is left out")).unwrap();

        let rest = gathered.without(&[2]).expect("two members are left");
        let DrawCommand::MergedShape {
            members, bounds, ..
        } = &*rest
        else {
            panic!("a merged shape");
        };
        let at: Vec<_> = members.iter().map(|m| m.placement.tx()).collect();
        assert_eq!(at, [0.0, 60.0]);
        assert_eq!(*bounds, Rect::new(-2.0, -2.0, 84.0, 24.0));
        assert!(gathered.without(&[1, 2, 3]).is_none());
    }

    /// Sixteen members merge and the seventeenth is drawn on its own.
    #[test]
    fn members_past_the_limit_are_drawn_on_their_own() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let mut group = RenderNode::new(0);
        for i in 0..=MAX_MEMBERS {
            group.children.push(child(
                (i as f32 * 30.0, 0.0),
                &[background(rect, Color::GREEN, None)],
            ));
        }
        let mut told = Vec::new();
        let gathered = gather(&group, &MERGE, |why| told.push(why)).unwrap();
        assert_eq!(members(&gathered).len(), MAX_MEMBERS);
        assert_eq!(told, [LeftOut::TooMany]);
        assert_eq!(gathered.taken[MAX_MEMBERS].len(), 0);
    }

    /// The box a merged shape is drawn over covers its members and the
    /// quarter of the distance a neck can reach past them.
    #[test]
    fn the_bounds_cover_the_members_and_their_neck() {
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        let mut group = RenderNode::new(0);
        group
            .children
            .push(child((0.0, 0.0), &[background(rect, Color::GREEN, None)]));
        group
            .children
            .push(child((40.0, 10.0), &[background(rect, Color::GREEN, None)]));
        let gathered = gather(&group, &MERGE, |_| {}).unwrap();
        let DrawCommand::MergedShape { bounds, .. } = &*gathered.command else {
            unreachable!()
        };
        assert_eq!(*bounds, Rect::new(-2.0, -2.0, 64.0, 34.0));
    }

    /// A record carries the member's colours premultiplied by its opacity and
    /// the command's, and its lengths in physical pixels.
    #[test]
    fn a_record_is_premultiplied_and_scaled() {
        let member = MergeMember {
            rect: Rect::new(0.0, 0.0, 20.0, 10.0),
            radius: CornerRadii::uniform(5.0),
            curvature: 1.0,
            color: Color::rgba(1.0, 0.5, 0.0, 0.5),
            border: Some(Border::new(1.5, Color::BLUE)),
            placement: Transform::translate(4.0, 0.0),
            opacity: 0.5,
        };
        let record =
            MergeRecord::new(&member, &Transform::scale(2.0), None, 0.5, 12.0, 2.0).unwrap();
        assert_eq!(record.fill, [0.125, 0.0625, 0.0, 0.125]);
        assert_eq!(record.border_color, [0.0, 0.0, 0.25, 0.25]);
        assert_eq!(record.border_width, 3.0);
        assert_eq!(record.smoothing, 24.0);
        assert_eq!(record.to_pixels, 4.0, "the surface's scale and the world's");

        let framed = MergeRecord::new(
            &member,
            &Transform::IDENTITY,
            Some(Border::new(4.0, Color::RED)),
            1.0,
            0.0,
            1.0,
        )
        .unwrap();
        assert_eq!(framed.border_width, 4.0, "the container's border wins");

        let collapsed = MergeRecord::new(&member, &Transform::scale(0.0), None, 1.0, 0.0, 1.0);
        assert!(collapsed.is_none());
    }
}
