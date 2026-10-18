//! A layer's texture, from one frame to the next.
//!
//! Flatten decides which subtrees are layers and hands their content over as
//! [`LayerPass`]es; this keeps the texture each one is drawn into, and decides
//! per frame whether the texture still holds that content. It does as long
//! as the content is the same — the same commands, drawn the same way, at the
//! same scale — which is the whole of what makes a fade cheap: a container
//! whose opacity animates repaints itself and nothing under it, and the
//! texture it composites is the one the first frame drew.
//!
//! A blur is kept the same way, one step further on: the blurred texture
//! holds the content blurred by one radius, and a new radius runs the blur
//! again over the content already drawn rather than drawing it again.
//!
//! The textures come from a pool and go back to it, as the backdrop target
//! does: kept across a run of frames that do not need them, released after
//! [`IDLE_FRAMES_BEFORE_RELEASE`] of them.

use std::rc::Rc;

use rustc_hash::FxHashMap;
use wgpu::{BindGroup, Device, TextureFormat, TextureView};

use super::backdrop_pass::{Scene, Working, make_target};
use super::clip::{ClipChains, ResolvedClip};
use super::commands::DrawCommand;
use super::flatten::{FlatFrame, FlattenedCommand, LayerPass, RenderLayer};
use super::image_quad::{Composite, ImageQuadRenderer};
use super::layer_blur::{BlurPlan, IDLE_FRAMES_BEFORE_RELEASE};
use super::tree::NodeId;
use crate::render_stats;
use crate::shape::PlacedShape;
use crate::transform::Transform;

/// How many times its own surface's patience a texture is kept for while no
/// surface at all names it — long enough for a surface that paints rarely,
/// short enough that one which has gone does not hold it for good.
const ANY_SURFACE: u32 = 30;

/// Textures are allocated in steps of this many texels a side, so a layer
/// whose size animates draws into the texture it already has.
const TEXTURE_STEP: u32 = 64;

/// A texture a layer is drawn into or blurred into, with the view the passes
/// write through and the bind group the composite reads through.
struct LayerTexture {
    // Held for the view and the bind group, which address it.
    _texture: wgpu::Texture,
    view: TextureView,
    bind_group: Rc<BindGroup>,
    size: (u32, u32),
}

impl LayerTexture {
    fn fits(&self, size: (u32, u32)) -> bool {
        self.size.0 >= size.0 && self.size.1 >= size.1
    }

    /// Not more than four times what is asked for, so a small layer does not
    /// keep a surface-sized texture it inherited from the pool.
    fn suits(&self, size: (u32, u32)) -> bool {
        self.fits(size)
            && (self.size.0 as u64 * self.size.1 as u64)
                <= 4 * (stepped(size.0) as u64 * stepped(size.1) as u64)
    }
}

fn stepped(texels: u32) -> u32 {
    texels.max(1).div_ceil(TEXTURE_STEP) * TEXTURE_STEP
}

/// One command of a layer's content as it was drawn into the texture.
struct Drawn {
    command: Rc<DrawCommand>,
    transform: Transform,
    opacity: f32,
    layer: RenderLayer,
    clip: Option<DrawnClip>,
}

/// A clip as it cut the content: every shape of its chain, since a chain index
/// means something in one frame only.
#[derive(PartialEq)]
struct DrawnClip {
    open: crate::widgets::Rect,
    shapes: Vec<PlacedShape>,
}

impl DrawnClip {
    fn of(clip: &ResolvedClip, chains: &ClipChains) -> Self {
        Self {
            open: clip.open,
            shapes: chains.shapes(clip).copied().collect(),
        }
    }

    fn is(&self, clip: &ResolvedClip, chains: &ClipChains) -> bool {
        self.open == clip.open && self.shapes.iter().eq(chains.shapes(clip))
    }
}

impl Drawn {
    fn of(cmd: &FlattenedCommand, chains: &ClipChains) -> Self {
        Self {
            command: Rc::clone(&cmd.command),
            transform: cmd.world_transform,
            opacity: cmd.opacity,
            layer: cmd.layer,
            clip: cmd.clip().map(|clip| DrawnClip::of(&clip, chains)),
        }
    }

    fn is(&self, cmd: &FlattenedCommand, chains: &ClipChains) -> bool {
        (Rc::ptr_eq(&self.command, &cmd.command) || *self.command == *cmd.command)
            && self.transform == cmd.world_transform
            && self.opacity == cmd.opacity
            && self.layer == cmd.layer
            && match (&self.clip, cmd.clip()) {
                (None, None) => true,
                (Some(drawn), Some(clip)) => drawn.is(&clip, chains),
                _ => false,
            }
    }
}

/// A layer's blurred texture, and what it was blurred from.
struct Blurred {
    /// The two the blur ping-pongs between, one size — see
    /// [`LayerTextures::take_pair`] — so if the first fits, both do.
    work: [LayerTexture; 2],
    plan: BlurPlan,
    /// The working texels the result covers.
    region: (u32, u32),
    /// Whether it was blurred from the content the texture holds now.
    current: bool,
}

/// What is kept for one layer.
struct Entry {
    content: LayerTexture,
    /// What the texture holds, command by command.
    drawn: Vec<Drawn>,
    /// The scale and size it was drawn at.
    scale: f32,
    size: (u32, u32),
    /// `false` when it was drawn from something still on its way — an image
    /// whose pixels had not landed — and has to be drawn again.
    settled: bool,
    blurred: Option<Blurred>,
    /// The surface it was last drawn for, whose frames it idles by.
    surface: NodeId,
    /// That surface's frames since one last named it.
    idle: u32,
    /// Every surface's frames since one last named it — what releases the
    /// texture of a layer whose surface has gone.
    idle_anywhere: u32,
    /// Whether the frame being drawn names it.
    named: bool,
}

/// What this frame does for one pass.
pub(super) struct PassPlan {
    /// The texture its content is drawn into this frame, as the scene a
    /// backdrop inside it filters — `None` when the texture is kept.
    pub(super) draw: Option<Scene>,
    /// The blur to run over it this frame, if one is due.
    pub(super) blur: Option<BlurPlan>,
}

/// A texture unused by any layer, waiting for one.
struct Spare {
    texture: LayerTexture,
    idle: u32,
}

/// Every layer's texture, and the pool they come from.
pub(super) struct LayerTextures {
    format: TextureFormat,
    entries: FxHashMap<NodeId, Entry>,
    spare: Vec<Spare>,
    /// This frame's passes, by their index in the frame.
    plans: Vec<PassPlan>,
}

impl LayerTextures {
    pub(super) fn new(format: TextureFormat) -> Self {
        Self {
            format,
            entries: FxHashMap::default(),
            spare: Vec::new(),
            plans: Vec::new(),
        }
    }

    /// Whether any of `kept` has no texture to be laid down from at `scale`:
    /// none at all, one drawn at another scale, or one drawn from pixels still
    /// on their way.
    pub(super) fn lacks(&self, kept: &[NodeId], scale: f32) -> bool {
        kept.iter().any(|id| {
            !self
                .entries
                .get(id)
                .is_some_and(|entry| entry.settled && entry.scale == scale)
        })
    }

    /// Name `id`, and every layer inside it, as in use by this frame.
    fn name(&mut self, id: NodeId, surface: NodeId) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        entry.named = true;
        entry.surface = surface;
        let inner: smallvec::SmallVec<[NodeId; 4]> = entry
            .drawn
            .iter()
            .filter_map(|drawn| match &*drawn.command {
                DrawCommand::Layer { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        for id in inner {
            self.name(id, surface);
        }
    }

    /// This frame's plan for each of its passes, in order.
    pub(super) fn plans(&self) -> &[PassPlan] {
        &self.plans
    }

    /// Decide, for every layer of `frame`, whether its texture still holds
    /// its content and its blur — and give every one a texture large enough.
    pub(super) fn begin_frame(
        &mut self,
        device: &Device,
        images: &ImageQuadRenderer,
        frame: &FlatFrame<'_>,
        scale: f32,
    ) {
        self.plans.clear();
        for pass in frame.passes {
            let content = &frame.commands[pass.commands.clone()];
            // A layer inside it that is drawn again, or blurred again, is a
            // different picture in it.
            let inner_moved = self.plans[pass.inner.clone()]
                .iter()
                .any(|plan| plan.draw.is_some() || plan.blur.is_some());

            let mut entry = match self.entries.remove(&pass.id) {
                Some(entry) if entry.content.fits(pass.size) => entry,
                other => {
                    let texture = self.take(device, images, pass.size);
                    if let Some(old) = other {
                        self.give(old.content);
                        if let Some(blurred) = old.blurred {
                            self.give_blurred(blurred);
                        }
                    }
                    Entry {
                        content: texture,
                        drawn: Vec::new(),
                        scale: 0.0,
                        size: (0, 0),
                        settled: false,
                        blurred: None,
                        surface: frame.surface,
                        idle: 0,
                        idle_anywhere: 0,
                        named: false,
                    }
                }
            };
            entry.named = true;
            entry.surface = frame.surface;

            let kept = entry.settled
                && !inner_moved
                && entry.scale == scale
                && entry.size == pass.size
                && entry.drawn.len() == content.len()
                && entry
                    .drawn
                    .iter()
                    .zip(content)
                    .all(|(drawn, cmd)| drawn.is(cmd, frame.clips));
            if !kept {
                entry.drawn.clear();
                entry
                    .drawn
                    .extend(content.iter().map(|cmd| Drawn::of(cmd, frame.clips)));
                entry.scale = scale;
                entry.size = pass.size;
                entry.settled = true;
                if let Some(blurred) = &mut entry.blurred {
                    blurred.current = false;
                }
            }
            render_stats::record_layer(if kept {
                render_stats::LayerEvent::Kept
            } else {
                render_stats::LayerEvent::Drawn
            });

            let blur = match BlurPlan::new(pass.blur, scale) {
                Some(plan) => {
                    let region = blurred_region(pass, plan);
                    let blurred = match entry.blurred.take() {
                        Some(blurred) if blurred.work[0].fits(region) => blurred,
                        other => {
                            if let Some(old) = other {
                                self.give_blurred(old);
                            }
                            Blurred {
                                work: self.take_pair(device, images, region),
                                plan,
                                region,
                                current: false,
                            }
                        }
                    };
                    let due = !blurred.current || blurred.plan != plan || blurred.region != region;
                    entry.blurred = Some(Blurred {
                        plan,
                        region,
                        current: true,
                        ..blurred
                    });
                    due.then_some(plan)
                }
                None => {
                    if let Some(old) = entry.blurred.take() {
                        self.give_blurred(old);
                    }
                    None
                }
            };
            if blur.is_some() {
                render_stats::record_layer(render_stats::LayerEvent::Blurred);
            }

            let draw = (!kept).then(|| Scene {
                view: entry.content.view.clone(),
                width: entry.content.size.0,
                height: entry.content.size.1,
                layer: true,
            });
            self.entries.insert(pass.id, entry);
            self.plans.push(PassPlan { draw, blur });
        }

        // The layers a cached subtree lays down again with no pass: their
        // textures are what is drawn, so they are in use — and a texture that
        // is not there, or not settled, needs a frame that draws its content.
        for &id in frame.kept {
            if !self.lacks(&[id], scale) {
                render_stats::record_layer(render_stats::LayerEvent::Kept);
            }
            self.name(id, frame.surface);
        }
    }

    /// The content drawn into a layer this frame came out of something still
    /// on its way: draw it again next frame, whatever its commands say.
    pub(super) fn unsettle(&mut self, id: NodeId) {
        if let Some(entry) = self.entries.get_mut(&id) {
            entry.settled = false;
        }
    }

    /// The textures a pass's blur reads and writes.
    pub(super) fn blur_targets(&self, id: NodeId) -> Option<BlurTargets> {
        let entry = self.entries.get(&id)?;
        let blurred = entry.blurred.as_ref()?;
        Some(BlurTargets {
            source: entry.content.view.clone(),
            source_size: entry.content.size,
            work: [blurred.work[0].view.clone(), blurred.work[1].view.clone()],
            work_size: blurred.work[0].size,
            region: blurred.region,
        })
    }

    /// What a layer's composite samples, and over what.
    pub(super) fn composite(&self, cmd: &FlattenedCommand, scale: f32) -> Option<Composite> {
        let DrawCommand::Layer { id, frame, .. } = &*cmd.command else {
            return None;
        };
        let entry = self.entries.get(id)?;
        match &entry.blurred {
            Some(blurred) => {
                let reach = blurred.plan.reach() as f32 / scale;
                let shrink = blurred.plan.shrink as f32;
                let (width, height) = blurred.work[0].size;
                Some(Composite {
                    bind_group: blurred.work[0].bind_group.clone(),
                    rect: crate::widgets::Rect::new(
                        frame.x - reach,
                        frame.y - reach,
                        blurred.region.0 as f32 * shrink / scale,
                        blurred.region.1 as f32 * shrink / scale,
                    ),
                    uv: (
                        0.0,
                        0.0,
                        blurred.region.0 as f32 / width as f32,
                        blurred.region.1 as f32 / height as f32,
                    ),
                })
            }
            None => {
                let (width, height) = entry.content.size;
                Some(Composite {
                    bind_group: entry.content.bind_group.clone(),
                    rect: *frame,
                    uv: (
                        0.0,
                        0.0,
                        entry.size.0 as f32 / width as f32,
                        entry.size.1 as f32 / height as f32,
                    ),
                })
            }
        }
    }

    /// Age everything this frame did not name, and release what has waited
    /// long enough.
    ///
    /// By the frames of the surface a layer is drawn on, not every surface's:
    /// a still panel on one surface beside another that animates would
    /// otherwise lose its texture in two seconds and draw it again on its next
    /// frame. A texture no surface has named for many times as long is let go
    /// anyway, which is what releases one whose surface is gone.
    pub(super) fn end_frame(&mut self, surface: NodeId) {
        let mut released = Vec::new();
        for (id, entry) in &mut self.entries {
            if std::mem::take(&mut entry.named) {
                entry.idle = 0;
                entry.idle_anywhere = 0;
                continue;
            }
            entry.idle_anywhere += 1;
            if entry.surface == surface {
                entry.idle += 1;
            }
            if entry.idle > IDLE_FRAMES_BEFORE_RELEASE
                || entry.idle_anywhere > IDLE_FRAMES_BEFORE_RELEASE * ANY_SURFACE
            {
                released.push(*id);
            }
        }
        for id in released {
            if let Some(entry) = self.entries.remove(&id) {
                self.give(entry.content);
                if let Some(blurred) = entry.blurred {
                    self.give_blurred(blurred);
                }
            }
        }
        for spare in &mut self.spare {
            spare.idle += 1;
        }
        self.spare
            .retain(|spare| spare.idle <= IDLE_FRAMES_BEFORE_RELEASE);
    }

    /// Drop every texture, in use or waiting.
    #[cfg(feature = "testing")]
    pub(super) fn forget(&mut self) {
        self.entries.clear();
        self.spare.clear();
    }

    /// How many textures are held, in use or waiting.
    #[cfg(feature = "testing")]
    pub(super) fn textures(&self) -> usize {
        self.spare.len()
            + self
                .entries
                .values()
                .map(|entry| 1 + entry.blurred.as_ref().map_or(0, |_| 2))
                .sum::<usize>()
    }

    /// A texture at least `size`, from the pool if one there suits.
    fn take(
        &mut self,
        device: &Device,
        images: &ImageQuadRenderer,
        size: (u32, u32),
    ) -> LayerTexture {
        if let Some(at) = self
            .spare
            .iter()
            .position(|spare| spare.texture.suits(size))
        {
            return self.spare.swap_remove(at).texture;
        }
        self.allocate(device, images, (stepped(size.0), stepped(size.1)))
    }

    /// A new texture of exactly `size`.
    fn allocate(
        &mut self,
        device: &Device,
        images: &ImageQuadRenderer,
        size: (u32, u32),
    ) -> LayerTexture {
        render_stats::record_layer(render_stats::LayerEvent::TextureAllocated);
        let texture = make_target(device, self.format, "Layer", size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = images.bind_texture(device, &view);
        LayerTexture {
            _texture: texture,
            view,
            bind_group,
            size,
        }
    }

    /// Two textures of one size, at least `size` — what a blur ping-pongs
    /// between.
    ///
    /// One size because the blur reads either through the first one's: its
    /// passes are sized and stepped by [`Working::size`], and a second texture
    /// of another size is read stretched by the ratio between them. Two calls
    /// to [`take`](Self::take) can be handed different ones — a texture
    /// another layer left in the pool suits the first, and nothing there
    /// suits the second (#808).
    fn take_pair(
        &mut self,
        device: &Device,
        images: &ImageQuadRenderer,
        size: (u32, u32),
    ) -> [LayerTexture; 2] {
        let first = self.take(device, images, size);
        let second = match self
            .spare
            .iter()
            .position(|spare| spare.texture.size == first.size)
        {
            Some(at) => self.spare.swap_remove(at).texture,
            None => self.allocate(device, images, first.size),
        };
        [first, second]
    }

    fn give(&mut self, texture: LayerTexture) {
        self.spare.push(Spare { texture, idle: 0 });
    }

    fn give_blurred(&mut self, blurred: Blurred) {
        let [a, b] = blurred.work;
        self.give(a);
        self.give(b);
    }
}

/// What a pass's blur reads and writes: the content's texture and its size,
/// the two the blur ping-pongs between and theirs, and how many working texels
/// the result covers.
pub(super) struct BlurTargets {
    pub(super) source: TextureView,
    pub(super) source_size: (u32, u32),
    pub(super) work: [TextureView; 2],
    pub(super) work_size: (u32, u32),
    pub(super) region: (u32, u32),
}

impl BlurTargets {
    /// The two working textures, as the backdrop's passes take them.
    pub(super) fn working(&self) -> Working<'_> {
        Working {
            views: [&self.work[0], &self.work[1]],
            size: self.work_size,
        }
    }
}

/// How many working texels a pass's blur covers: its content and the blur's
/// reach on every side, shrunk.
fn blurred_region(pass: &LayerPass, plan: BlurPlan) -> (u32, u32) {
    let reach = 2 * plan.reach();
    (
        (pass.size.0 + reach).div_ceil(plan.shrink),
        (pass.size.1 + reach).div_ceil(plan.shrink),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A texture is allocated in steps, so a layer that grows by a pixel draws
    /// into the one it has.
    #[test]
    fn textures_come_in_steps() {
        assert_eq!(stepped(1), TEXTURE_STEP);
        assert_eq!(stepped(0), TEXTURE_STEP);
        assert_eq!(stepped(TEXTURE_STEP), TEXTURE_STEP);
        assert_eq!(stepped(TEXTURE_STEP + 1), 2 * TEXTURE_STEP);
    }

    /// A blur's two textures are one size, even when the pool holds one that
    /// suits the first and nothing that suits the second (#808).
    #[test]
    fn a_blur_s_two_textures_are_one_size() {
        let Some(gpu) = crate::or_skip(crate::renderer::GpuContext::try_new()) else {
            return;
        };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let images = ImageQuadRenderer::new(
            &gpu.device,
            format,
            super::super::clip_records::ClipRecords::new(&gpu.device).layout(),
        );
        let mut layers = LayerTextures::new(format);
        // What a larger layer leaves behind: twice the size a small blur
        // would be allocated each way, and within what `suits` lets it take.
        let left = layers.allocate(&gpu.device, &images, (256, 128));
        layers.give(left);

        let [first, second] = layers.take_pair(&gpu.device, &images, (73, 39));
        assert_eq!(first.size, (256, 128), "the first is the one left behind");
        assert_eq!(second.size, first.size, "and the second is its size");
    }
}
