use std::hash::{Hash, Hasher};

use glyphon::cosmic_text::Align;
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, ColorMode, FontSystem, Metrics, Resolution,
    Shaping, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use rustc_hash::FxHashMap;
use wgpu::{Device, MultisampleState, Queue};

use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::{Rect, TextAlign};

use super::types::TextEntry;

/// Compute a cache key for a text buffer based on content and styling.
/// Two entries with the same key produce identical glyphon Buffers.
fn text_buffer_key(entry: &TextEntry, scale_factor: f32) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    entry.text.hash(&mut hasher);
    (entry.font_size * scale_factor).to_bits().hash(&mut hasher);
    entry.font_weight.hash(&mut hasher);
    entry.font_family.hash(&mut hasher);
    entry.align.hash(&mut hasher);
    let (width, height) = shaping_buffer(entry.rect, scale_factor, entry.align);
    width.to_bits().hash(&mut hasher);
    height.to_bits().hash(&mut hasher);
    hasher.finish()
}

/// The buffer glyphon is given to shape a text of this layout box in, in
/// physical pixels.
///
/// The floors are glyphon's: a buffer narrower than the text it holds wraps it,
/// and a measured box can come back a hair narrower than the thing it measured.
///
/// An aligned text has no width floor: cosmic-text aligns each line across the
/// width it is shaped in, so a label narrower than the floor would be centred
/// in the floor rather than in its own box. A wrapped text's box is its widest
/// line as layout measured it, so that exact width breaks the lines where
/// layout did — `text_aligns_within_its_own_box_at_scale_1_5x` is the golden
/// that would see one break anywhere else.
///
/// It is a function and not four inline multiplications because a text's frost
/// is shaped separately from the text — see [`text_mask`](super::text_mask) —
/// and two shapings that disagree break their lines in different places, which
/// puts frost beside a letter that wrapped somewhere else. The sibling for the
/// transformed path is [`text_quad::shaping_buffer`](super::text_quad::shaping_buffer).
pub(super) fn shaping_buffer(rect: Rect, scale: f32, align: TextAlign) -> (f32, f32) {
    let width = match align {
        TextAlign::Start => rect.width.max(200.0),
        _ => rect.width,
    };
    (width * scale, rect.height.max(50.0) * scale)
}

/// Shape an untransformed text into a fresh buffer, as glyphon will draw it.
fn shape_entry(font_system: &mut FontSystem, entry: &TextEntry, scale_factor: f32) -> Buffer {
    shape(
        font_system,
        &entry.text,
        entry.font_size * scale_factor,
        entry.font_family,
        entry.font_weight,
        entry.align,
        shaping_buffer(entry.rect, scale_factor, entry.align),
    )
}

/// Shape `text` at `font_size` physical pixels into a buffer of `size`.
///
/// One function for the three paths that shape — glyphon's, the transformed
/// quad's and the frost's mask — because a frost has to break and align its
/// lines exactly where the letters over it do, and three copies of this are
/// three places for an option to reach two of.
pub(super) fn shape(
    font_system: &mut FontSystem,
    text: &str,
    font_size: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
    align: TextAlign,
    size: (f32, f32),
) -> Buffer {
    let (px, line_height) = crate::renderer::text_measurer::shapeable_metrics(font_size);
    let mut buffer = Buffer::new(font_system, Metrics::new(px, line_height));
    buffer.set_size(font_system, Some(size.0), Some(size.1));
    let weight = if font_weight == FontWeight::default() {
        FontWeight::NORMAL
    } else {
        font_weight
    };
    buffer.set_text(
        font_system,
        text,
        &Attrs::new()
            .family(font_family.to_cosmic())
            .weight(weight.to_cosmic()),
        Shaping::Advanced,
        cosmic_align(align),
    );
    buffer.shape_until_scroll(font_system, true);
    buffer
}

/// cosmic-text's word for an alignment. `Start` is no word at all, so the
/// text's own direction decides where its lines begin.
fn cosmic_align(align: TextAlign) -> Option<Align> {
    match align {
        TextAlign::Start => None,
        TextAlign::Center => Some(Align::Center),
        TextAlign::End => Some(Align::End),
        TextAlign::Justified => Some(Align::Justified),
    }
}

pub struct TextRenderState {
    font_system: FontSystem,
    swash_cache: SwashCache,
    #[allow(dead_code)] // Used for viewport and atlas construction
    cache: Cache,
    atlas: TextAtlas,
    /// One renderer per draw group that contains text.
    ///
    /// glyphon draws everything a `TextRenderer` prepared in a single call,
    /// so text that must appear between two other groups needs a renderer of
    /// its own. Grown on demand and reused across frames; a UI with one group
    /// keeps exactly one, as before.
    text_renderers: Vec<TextRenderer>,
    buffers: Vec<Buffer>,
    viewport: Viewport,
    /// Cache of shaped text buffers from the previous frame, keyed by
    /// content+style hash. Avoids expensive Unicode analysis and glyph
    /// shaping for unchanged text. Multi-valued: the same label can appear
    /// several times in a frame (e.g. repeated list items) — with a
    /// single-entry cache the first occurrence consumed the only buffer and
    /// every duplicate re-shaped from scratch, every frame, forever.
    buffer_cache: FxHashMap<u64, Vec<Buffer>>,
    /// Keys for current frame's buffers (parallel to `self.buffers`), used to
    /// repopulate `buffer_cache` at the start of the next frame.
    frame_keys: Vec<u64>,
    /// Positions in the group's texts that the textured-quad path takes, and
    /// what [`prepare_layer`](Self::prepare_layer) hands back.
    ///
    /// A field for the same reason `buffers` is one: the group it describes
    /// lasts a frame, the container it needs does not have to.
    transformed_indices: Vec<usize>,
    /// Positions in the group's texts that glyphon draws, parallel to the
    /// buffers this group pushed.
    ///
    /// Written in the one place a buffer is pushed, so the two lists cannot
    /// come apart: a text area reads the entry and the buffer that was shaped
    /// for it, and an early-out added later drops both or neither. The two
    /// hash sets this replaces said the same thing by saying who was *left
    /// out*, which every skipping branch then had to remember.
    kept: Vec<usize>,
}

/// Whether a text lies entirely off one side of its clip, and so is not drawn
/// at all.
///
/// A whole side, not a corner: two boxes that overlap on neither axis are
/// still both here, and this only says yes when one interval is wholly past
/// the other. The tolerance is for the boundary, where a text ending exactly
/// where its clip does should be kept rather than lost to the last bit of a
/// float.
fn falls_outside(text: Rect, clip: Rect) -> bool {
    const EPSILON: f32 = 0.1;
    text.x + text.width < clip.x - EPSILON
        || text.x > clip.x + clip.width + EPSILON
        || text.y + text.height < clip.y - EPSILON
        || text.y > clip.y + clip.height + EPSILON
}

/// A text's layout box, in world pixels.
///
/// What the culling check has always compared against a clip, lifted out so
/// the routing question below can ask it too. Not outset: widening it here
/// would widen what gets culled, and the two questions want different
/// margins — see the call that adds one.
fn laid_out_in(entry: &TextEntry) -> Rect {
    let (p1x, p1y) = entry.transform.transform_point(entry.rect.x, entry.rect.y);
    let (p2x, p2y) = entry.transform.transform_point(
        entry.rect.x + entry.rect.width,
        entry.rect.y + entry.rect.height,
    );
    Rect::new(
        p1x.min(p2x),
        p1y.min(p2y),
        (p2x - p1x).abs(),
        (p2y - p1y).abs(),
    )
}

impl TextRenderState {
    pub fn new(device: &Device, queue: &Queue, format: wgpu::TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        for data in crate::get_registered_fonts() {
            font_system
                .db_mut()
                .load_font_source(glyphon::fontdb::Source::Binary(data));
        }
        let swash_cache = SwashCache::new();
        let cache = Cache::new(device);
        let mut atlas = TextAtlas::with_color_mode(device, queue, &cache, format, ColorMode::Web);
        let text_renderer =
            TextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            cache,
            atlas,
            text_renderers: vec![text_renderer],
            buffers: Vec::new(),
            viewport,
            buffer_cache: FxHashMap::default(),
            frame_keys: Vec::new(),
            transformed_indices: Vec::new(),
            kept: Vec::new(),
        }
    }

    /// Start a frame: recycle last frame's shaped buffers into the cache.
    ///
    /// Split out of [`prepare_layer`](Self::prepare_layer) because a frame may
    /// prepare several groups and the recycling has to happen once for all.
    pub fn begin_frame(&mut self, queue: &Queue, screen: (u32, u32)) {
        for (key, buffer) in self.frame_keys.drain(..).zip(self.buffers.drain(..)) {
            self.buffer_cache.entry(key).or_default().push(buffer);
        }
        // Once per frame, not once per group: every group renders into the
        // same viewport.
        self.viewport.update(
            queue,
            Resolution {
                width: screen.0,
                height: screen.1,
            },
        );
    }

    /// Drop shaped buffers no group asked for. Call once every group has been
    /// prepared — whatever is still cached went unused this frame.
    pub fn end_frame(&mut self) {
        self.buffer_cache.clear();
    }

    /// Prepare one draw group's non-transformed text.
    ///
    /// `slot` selects the renderer, and with it when this text lands relative
    /// to the other groups; see [`render_slot`](Self::render_slot).
    /// Returns the indices, within `texts`, that need the textured-quad path —
    /// borrowed from the scratch they were sorted into, so the group's own
    /// prepare is the one that reads them.
    pub fn prepare_layer(
        &mut self,
        slot: usize,
        device: &Device,
        queue: &Queue,
        texts: &[TextEntry],
        screen: (u32, u32),
        scale_factor: f32,
    ) -> &[usize] {
        let (screen_width, screen_height) = screen;
        while self.text_renderers.len() <= slot {
            self.text_renderers.push(TextRenderer::new(
                &mut self.atlas,
                device,
                MultisampleState::default(),
                None,
            ));
        }
        // Buffers accumulate across the frame's groups; this is where this
        // group's own shaped buffers begin.
        let buffers_start = self.buffers.len();

        self.transformed_indices.clear();
        self.kept.clear();

        for (idx, entry) in texts.iter().enumerate() {
            // Text scaled to nothing is skipped before any work is done for
            // it. The scale comes from the row norms, not from the diagonal:
            // `a` and `d` are both `cos θ` for a rotation, so reading them as
            // the scale calls a quarter turn a collapse and throws the text
            // away at exactly 90° and 270° — and only there, which is why it
            // survived so long. `extract_scale_components` is what the rest of
            // the renderer already asks.
            if !entry.transform.is_identity() && !entry.transform.is_translation_only() {
                let (sx, sy) = entry.transform.extract_scale_components();
                if sx.abs() < 1e-3 || sy.abs() < 1e-3 {
                    continue;
                }
            }

            // Skip text that is completely outside its clip region (culling optimization)
            // Check this FIRST so culled texts don't get rendered via texture path either
            let laid_out = laid_out_in(entry);
            if entry
                .clip
                .is_some_and(|clip| falls_outside(laid_out, clip.world_aabb()))
            {
                // Text is completely outside clip - skip it entirely
                continue;
            }

            // Route all non-translation transforms (rotation, scale) to TextQuadRenderer.
            // This keeps the glyphon atlas stable — only identity/translation text goes through it.
            //
            // And anything glyphon cannot cut. `TextBounds` is four integers,
            // so the only clip it can honour is an upright rectangle: a turned
            // one, or a corner this text actually reaches into, has to be
            // tested in the clip's own space, and the quad is what does that
            // (#405). Asked of where the two answers *differ* rather than
            // whether they can, because a quad costs a rasterization and an
            // upload per text — a label in the middle of a rounded card is cut
            // the same by the box and stays with glyphon.
            let moved = !entry.transform.is_identity() && !entry.transform.is_translation_only();
            // Glyphs overshoot the box they were laid out in — descenders,
            // italics, accents — and ink that overshoots into a corner is ink
            // the box would not have cut. Half the font size is the slack
            // `command_to_text_backdrop` allows for the same reason, and it
            // errs the safe way here: too generous sends a text down the quad
            // path the box would have cut correctly, costing a rasterization;
            // too tight leaves ink outside its clip, which is the defect.
            let ink = laid_out.outset(entry.font_size * 0.5);
            let box_will_not_do = entry
                .clip
                .is_some_and(|clip| !clip.box_cuts_like_the_shape(ink));
            if moved || box_will_not_do {
                self.transformed_indices.push(idx);
                continue; // Skip transformed text in direct rendering
            }

            // Check buffer cache before expensive text shaping
            let key = text_buffer_key(entry, scale_factor);
            let cached = self
                .buffer_cache
                .get_mut(&key)
                .and_then(|buffers| buffers.pop());
            let buffer =
                cached.unwrap_or_else(|| shape_entry(&mut self.font_system, entry, scale_factor));
            self.kept.push(idx);
            self.frame_keys.push(key);
            self.buffers.push(buffer);
        }

        // Destructured so the text areas can borrow the buffers immutably
        // while `prepare` takes the font system and atlas mutably.
        let Self {
            font_system,
            swash_cache,
            atlas,
            viewport,
            buffers,
            text_renderers,
            kept,
            ..
        } = self;

        // Lazy rather than collected: glyphon takes an iterator, so the areas
        // are built as it reads them and the borrow of `buffers` each one
        // carries never has to outlive the call.
        let text_areas = kept
            .iter()
            .map(|&idx| &texts[idx])
            .zip(buffers[buffers_start..].iter())
            .map(|(entry, buffer)| {
                // Position text in world space using the full transform.
                // This is consistent with the clip rect (also in world space via
                // transform_clip_to_world), so TextBounds clipping works correctly.
                let (screen_x, screen_y) =
                    entry.transform.transform_point(entry.rect.x, entry.rect.y);

                // Scale positions for HiDPI rendering
                let scaled_left = screen_x * scale_factor;
                let scaled_top = screen_y * scale_factor;

                // Use clip rect if provided, otherwise use full screen
                // Clip bounds stay in screen space - don't apply transform translation
                // (text position is transformed, but clip region should remain fixed)
                // The box around the clip, because `TextBounds` is four
                // integers and glyphon has nowhere to put a shape. That is
                // exact here rather than approximate: a text this box would
                // cut differently from the shape never reaches this point —
                // `box_cuts_like_the_shape` sent it down the quad path
                // instead, which tests in the clip's own space (#405).
                let bounds = if let Some(clip_rect) = &entry.clip.map(|c| c.world_aabb()) {
                    TextBounds {
                        left: (clip_rect.x * scale_factor) as i32,
                        top: (clip_rect.y * scale_factor) as i32,
                        right: ((clip_rect.x + clip_rect.width) * scale_factor) as i32,
                        bottom: ((clip_rect.y + clip_rect.height) * scale_factor) as i32,
                    }
                } else {
                    TextBounds {
                        left: 0,
                        top: 0,
                        right: screen_width as i32,
                        bottom: screen_height as i32,
                    }
                };

                TextArea {
                    buffer,
                    left: scaled_left,
                    top: scaled_top,
                    scale: 1.0, // Buffer is already scaled, no additional scaling needed
                    bounds,
                    // The fade goes into the colour here: glyphon's atlas
                    // holds coverage and the colour rides each glyph's
                    // vertices, so a new alpha rasterises nothing.
                    default_color: {
                        let color = entry.color.scale_alpha(entry.opacity);
                        GlyphonColor::rgba(
                            (color.r * 255.0) as u8,
                            (color.g * 255.0) as u8,
                            (color.b * 255.0) as u8,
                            (color.a * 255.0) as u8,
                        )
                    },
                    custom_glyphs: &[],
                }
            });

        let result = text_renderers[slot].prepare(
            device,
            queue,
            font_system,
            atlas,
            viewport,
            text_areas,
            swash_cache,
        );

        if let Err(e) = result {
            log::error!("Text prepare failed: {:?}", e);
        }

        &self.transformed_indices
    }

    /// Draw the text prepared into `slot`.
    pub fn render_slot<'a>(&'a self, slot: usize, pass: &mut wgpu::RenderPass<'a>) {
        if let Some(renderer) = self.text_renderers.get(slot) {
            renderer
                .render(&self.atlas, &self.viewport, pass)
                .expect("Failed to render text");
        }
    }
}

/// A text entry with nothing but its box and its placement said.
///
/// Shared by every test module that needs one — `text_quad`'s sets its own
/// `text` on top — rather than written out per module:
/// `TextEntry` has nine public fields and no constructor, so a builder per
/// module is a field list per module to keep in step.
#[cfg(test)]
pub(super) fn test_entry(rect: Rect, transform: crate::transform::Transform) -> TextEntry {
    TextEntry {
        text: "x".into(),
        rect,
        color: crate::widgets::Color::WHITE,
        font_size: 16.0,
        font_family: crate::widgets::FontFamily::default(),
        font_weight: FontWeight::default(),
        align: Default::default(),
        opacity: 1.0,
        clip: None,
        transform,
    }
}

/// The per-group scratch is grown once and then kept.
///
/// Capacity and not an allocation count: counting allocations needs an
/// allocator this crate does not install (#461). What a capacity can say is
/// that the container outlived the frame boundary, which is the whole of this
/// change — and a local cannot be asked the question at all.
///
/// The second frame asks for *less* than the first, which is what makes the
/// answer mean something: only a container that was cleared rather than
/// dropped still has the first frame's room in it.
///
/// Five upright texts in that first frame, and not one or two, because a
/// `Vec<usize>` allocates room for four on its first push however few things
/// go into it. With fewer, `kept` reads 4 on a frame that keeps five and 4
/// again on a rebuilt frame that keeps one, and that half of the assertion
/// cannot fail. Five crosses the step to 8. `transformed_indices` needs no
/// such help — it goes from one entry to none, so a rebuilt one reads 0.
#[cfg(test)]
mod scratch_survives_the_frame {
    use super::{TextRenderState, test_entry};
    use crate::renderer::GpuContext;
    use crate::renderer::types::TextEntry;
    use crate::transform::Transform;
    use crate::widgets::Rect;

    #[test]
    fn a_smaller_second_frame_still_has_the_first_frame_s_room() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut state =
            TextRenderState::new(&gpu.device, &gpu.queue, wgpu::TextureFormat::Rgba8UnormSrgb);

        /// What a `Vec<usize>` allocates for its first push.
        const MIN_CAP: usize = 4;
        /// Enough upright texts to take `kept` past that floor.
        const UPRIGHT: usize = 5;

        // Texts glyphon draws, and then one the quad path takes, so both
        // lists are filled: `kept` holds the first five, `transformed_indices`
        // the last.
        let box_ = Rect::new(0.0, 0.0, 80.0, 20.0);
        let mut texts: Vec<TextEntry> = (0..UPRIGHT)
            .map(|_| test_entry(box_, Transform::default()))
            .collect();
        texts.push(test_entry(box_, Transform::scale(2.0)));

        let frame = |state: &mut TextRenderState, texts: &[TextEntry]| {
            state.begin_frame(&gpu.queue, (200, 100));
            let transformed =
                state.prepare_layer(0, &gpu.device, &gpu.queue, texts, (200, 100), 1.0);
            let quads = transformed.to_vec();
            state.end_frame();
            quads
        };

        assert_eq!(
            frame(&mut state, &texts),
            [UPRIGHT],
            "the scaled text goes to the quad path, and only it"
        );
        let first = (state.transformed_indices.capacity(), state.kept.capacity());
        assert!(
            first.0 > 0 && first.1 > MIN_CAP,
            "five upright and one scaled should fill one list and grow the \
             other past a fresh `Vec`'s floor: {first:?}"
        );

        // One upright text now, and nothing at all for the quad path.
        assert_eq!(frame(&mut state, &texts[..1]), []);
        assert_eq!(
            (state.transformed_indices.capacity(), state.kept.capacity()),
            first,
            "the second frame reused the first frame's scratch"
        );
    }
}

/// What the scale-collapse cull decides, at the boundary and either side.
///
/// The one branch in `prepare_layer` that throws a text away before anything
/// is done for it, and it has been wrong once already: it read the diagonal
/// rather than the row norms, so a quarter turn — where `a` and `d` are both
/// `cos θ` — was called a collapse and the label vanished at exactly 90° and
/// 270°. `tests/text_at_angles.rs` counts the pixels that came back, which is
/// what caught *that*, but pixels cannot see this branch invert: a text the
/// cull lets through at zero scale draws a quad of zero width, so nothing
/// appears either way and the only difference is the work done to produce
/// nothing.
///
/// So this asks the decision rather than the picture. `||` read as `&&` keeps
/// a text flattened on one axis, and the threshold read as `<=` or `==`
/// changes the answer for a text sitting exactly on it — `1e-3` survives
/// `scale_xy` and the row norm unchanged, so the boundary is a real value and
/// not a hair either side of one.
#[cfg(test)]
mod a_text_is_culled_only_when_an_axis_has_collapsed {
    use super::{TextRenderState, test_entry};
    use crate::renderer::GpuContext;
    use crate::transform::Transform;
    use crate::widgets::Rect;

    /// The scale at which a text stops being drawn, as `prepare_layer` spells
    /// it.
    const COLLAPSED: f32 = 1e-3;

    #[test]
    fn one_flattened_axis_is_enough_and_the_threshold_itself_is_not() {
        let Some(gpu) = crate::or_skip(GpuContext::try_new()) else {
            return;
        };
        let mut state =
            TextRenderState::new(&gpu.device, &gpu.queue, wgpu::TextureFormat::Rgba8UnormSrgb);
        let box_ = Rect::new(0.0, 0.0, 80.0, 20.0);

        // `true` where the text should be thrown away. Anything kept is a
        // transformed text, so it leaves by the quad path rather than through
        // glyphon.
        let cases = [
            (Transform::scale_xy(0.0, 1.0), true, "flat in x, whole in y"),
            (Transform::scale_xy(1.0, 0.0), true, "whole in x, flat in y"),
            (Transform::scale_xy(0.0, 0.0), true, "flat in both"),
            (
                Transform::scale_xy(COLLAPSED, 1.0),
                false,
                "x exactly at the threshold, which is not below it",
            ),
            (
                Transform::scale_xy(1.0, COLLAPSED),
                false,
                "y exactly at the threshold",
            ),
            (
                Transform::rotate(std::f32::consts::FRAC_PI_2),
                false,
                "a quarter turn, where the diagonal is zero and the scale is not",
            ),
        ];

        for (transform, culled, what) in cases {
            let texts = [test_entry(box_, transform)];
            state.begin_frame(&gpu.queue, (200, 100));
            let transformed =
                state.prepare_layer(0, &gpu.device, &gpu.queue, &texts, (200, 100), 1.0);
            let quads = transformed.to_vec();
            let kept = state.kept.len();
            state.end_frame();

            assert_eq!(
                quads.is_empty(),
                culled,
                "{what}: {quads:?} went to the quad path"
            );
            assert_eq!(kept, 0, "{what}: a transformed text never reaches glyphon");
        }
    }
}

#[cfg(test)]
mod laid_out_tests {
    use super::{laid_out_in, test_entry as entry};
    use crate::transform::Transform;
    use crate::widgets::Rect;

    /// The box a text was laid out in, carried through its placement.
    ///
    /// Two callers read it — what gets culled against its clip, and what gets
    /// routed away from glyphon — so every term here decides whether a text is
    /// drawn at all or drawn through the wrong pipeline. It is four
    /// arithmetic operations and each of them can be written backwards without
    /// the picture obviously changing.
    /// A text is dropped only when it is wholly past one side of its clip.
    ///
    /// The cheapest thing the text path does and the easiest to get backwards:
    /// too eager and a visible text is never drawn, with nothing to say why.
    #[test]
    fn a_text_is_dropped_only_when_it_is_wholly_outside_its_clip() {
        use super::falls_outside;
        let clip = Rect::new(100.0, 100.0, 200.0, 100.0);

        assert!(
            !falls_outside(Rect::new(150.0, 120.0, 40.0, 20.0), clip),
            "inside"
        );
        assert!(
            !falls_outside(Rect::new(0.0, 0.0, 400.0, 400.0), clip),
            "over all of it"
        );

        // A whole side past, on each of the four.
        assert!(
            falls_outside(Rect::new(10.0, 120.0, 80.0, 20.0), clip),
            "left of it"
        );
        assert!(
            falls_outside(Rect::new(310.0, 120.0, 80.0, 20.0), clip),
            "right of it"
        );
        assert!(
            falls_outside(Rect::new(150.0, 10.0, 40.0, 80.0), clip),
            "above it"
        );
        assert!(
            falls_outside(Rect::new(150.0, 210.0, 40.0, 80.0), clip),
            "below it"
        );

        // Just reaching in on each side is not outside — the far edge is what
        // decides, and reading the near one instead keeps a text that has
        // gone and drops one that is still here.
        assert!(
            !falls_outside(Rect::new(20.0, 120.0, 81.0, 20.0), clip),
            "reaching in from the left"
        );
        assert!(
            !falls_outside(Rect::new(299.0, 120.0, 80.0, 20.0), clip),
            "reaching in from the right"
        );
        assert!(
            !falls_outside(Rect::new(150.0, 20.0, 40.0, 81.0), clip),
            "reaching in from above"
        );
        assert!(
            !falls_outside(Rect::new(150.0, 199.0, 40.0, 80.0), clip),
            "reaching in from below"
        );

        // Overlapping on neither axis is still two boxes in the same frame,
        // and a corner-to-corner pair is outside by both.
        assert!(
            falls_outside(Rect::new(0.0, 0.0, 50.0, 50.0), clip),
            "off the top-left corner"
        );

        // The tolerance, which is the whole reason there is one. A text
        // ending exactly where its clip begins is kept, and so is one a
        // hundredth of a pixel short of it — which is where a float lands
        // after a transform. Half a pixel past is gone. The margin runs
        // outward on every side; inward on any of them drops a text that is
        // still touching its clip.
        for (label, text) in [
            (
                "ending exactly at the left edge",
                Rect::new(20.0, 120.0, 80.0, 20.0),
            ),
            (
                "a hundredth short of the left edge",
                Rect::new(20.0, 120.0, 79.99, 20.0),
            ),
            (
                "starting exactly at the right edge",
                Rect::new(300.0, 120.0, 80.0, 20.0),
            ),
            (
                "a hundredth past the right edge",
                Rect::new(300.01, 120.0, 80.0, 20.0),
            ),
            (
                "ending exactly at the top edge",
                Rect::new(150.0, 20.0, 40.0, 80.0),
            ),
            (
                "starting exactly at the bottom edge",
                Rect::new(150.0, 200.0, 40.0, 80.0),
            ),
        ] {
            assert!(
                !falls_outside(text, clip),
                "{label}: a text touching its clip is still drawn"
            );
        }
        for (label, text) in [
            (
                "half a pixel short of the left edge",
                Rect::new(20.0, 120.0, 79.5, 20.0),
            ),
            (
                "half a pixel past the right edge",
                Rect::new(300.5, 120.0, 80.0, 20.0),
            ),
            (
                "half a pixel short of the top edge",
                Rect::new(150.0, 20.0, 40.0, 79.5),
            ),
            (
                "half a pixel past the bottom edge",
                Rect::new(150.0, 200.5, 40.0, 80.0),
            ),
        ] {
            assert!(
                falls_outside(text, clip),
                "{label}: past the tolerance is past the clip"
            );
        }
    }

    #[test]
    fn a_text_is_laid_out_where_its_placement_puts_it() {
        let r = Rect::new(10.0, 20.0, 100.0, 40.0);

        let plain = laid_out_in(&entry(r, Transform::IDENTITY));
        assert_eq!((plain.x, plain.y), (10.0, 20.0));
        assert_eq!((plain.width, plain.height), (100.0, 40.0));

        let moved = laid_out_in(&entry(r, Transform::translate(5.0, -7.0)));
        assert_eq!((moved.x, moved.y), (15.0, 13.0));
        assert_eq!((moved.width, moved.height), (100.0, 40.0));

        let scaled = laid_out_in(&entry(r, Transform::scale_xy(2.0, 3.0)));
        assert_eq!((scaled.x, scaled.y), (20.0, 60.0));
        assert_eq!((scaled.width, scaled.height), (200.0, 120.0));

        // A mirror puts the far corner first, and the box is still a box with
        // a positive extent — which is why the corners are taken as a min and
        // an absolute difference rather than as first and second.
        let mirrored = laid_out_in(&entry(r, Transform::scale_xy(-1.0, 1.0)));
        assert_eq!((mirrored.x, mirrored.y), (-110.0, 20.0));
        assert_eq!((mirrored.width, mirrored.height), (100.0, 40.0));
    }
}

#[cfg(test)]
mod shaping_buffer_tests {
    use super::shaping_buffer;
    use crate::widgets::{Rect, TextAlign};

    /// The floors are in logical pixels and the scale is applied after them, so
    /// a small box on a HiDPI screen gets the floor at that screen's density
    /// rather than a floor that shrinks as the screen gets finer.
    #[test]
    fn the_floor_is_logical_and_the_scale_comes_after_it() {
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 50.0, 10.0), 1.0, TextAlign::Start),
            (200.0, 50.0)
        );
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 50.0, 10.0), 2.0, TextAlign::Start),
            (400.0, 100.0)
        );
    }

    /// Above the floors the buffer is the box, scaled — and scaled, not offset,
    /// which is the whole of what makes it agree with a glyph drawn at that
    /// scale.
    #[test]
    fn a_box_above_the_floor_is_carried_by_the_scale_alone() {
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 1.0, TextAlign::Start),
            (300.0, 80.0)
        );
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 2.0, TextAlign::Start),
            (600.0, 160.0)
        );
        // The floor is logical, so a scale below 1 can carry the buffer under
        // it: 80 clears the floor of 50 and then halves to 40.
        assert_eq!(
            shaping_buffer(Rect::new(0.0, 0.0, 300.0, 80.0), 0.5, TextAlign::Start),
            (150.0, 40.0)
        );
    }

    /// The two rules are not the same rule, which is why a coverage mask has to
    /// pick one rather than assume. If this ever passes, `text_mask` no longer
    /// has a choice to get wrong — and the comment saying it does is stale.
    #[test]
    fn the_transformed_shaper_asks_for_something_else() {
        let rect = Rect::new(0.0, 0.0, 72.0, 80.0);
        assert_ne!(
            shaping_buffer(rect, 4.0, TextAlign::Start),
            super::super::text_quad::shaping_buffer(rect, 4.0, TextAlign::Start),
            "a 72-point box: 400 texels through this one and 317 through the other"
        );
    }
}

/// A line is aligned across the text's own box, whatever the renderer's floor.
///
/// Shaped with the vendored font alone, so the widths are the font's and not
/// the machine's. The box is 120 wide — under the 200 the shaper is otherwise
/// given — and the line is far shorter than that, so a line centred in the
/// floor instead of the box starts 40 pixels late, and one never aligned at
/// all starts at nought.
#[cfg(test)]
mod a_line_is_aligned_in_its_own_box {
    use super::{shape_entry, test_entry, text_buffer_key};
    use crate::transform::Transform;
    use crate::widgets::{FontFamily, Rect, TextAlign};
    use glyphon::FontSystem;

    const FONT: &[u8] = include_bytes!("../../tests/assets/DejaVuSansMono.ttf");
    const BOX: f32 = 120.0;

    fn entry(align: TextAlign) -> super::TextEntry {
        let mut entry = test_entry(Rect::new(0.0, 0.0, BOX, 20.0), Transform::default());
        entry.text = "hi".into();
        entry.font_family = FontFamily::name("DejaVu Sans Mono");
        entry.align = align;
        entry
    }

    /// Where the first line's glyphs begin and how wide the line is, in
    /// physical pixels.
    fn first_line(align: TextAlign, scale: f32) -> (f32, f32) {
        let mut db = glyphon::fontdb::Database::new();
        db.load_font_data(FONT.to_vec());
        let mut font_system = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let buffer = shape_entry(&mut font_system, &entry(align), scale);
        let run = buffer.layout_runs().next().expect("a line");
        let start = run.glyphs.iter().map(|g| g.x).fold(f32::INFINITY, f32::min);
        (start, run.line_w)
    }

    #[test]
    fn a_centred_line_is_centred_in_its_box_and_not_in_the_floor() {
        for scale in [1.0, 2.0] {
            let (start, width) = first_line(TextAlign::Center, scale);
            let expected = (BOX * scale - width) / 2.0;
            assert!(
                (start - expected).abs() < 0.5,
                "at scale {scale} a {width}-pixel line in a {}-pixel box starts at \
                 {start}, not at {expected}",
                BOX * scale
            );
        }
    }

    #[test]
    fn an_end_aligned_line_ends_where_its_box_does() {
        let (start, width) = first_line(TextAlign::End, 1.0);
        assert!(
            (start + width - BOX).abs() < 0.5,
            "the line ends at {} in a box that ends at {BOX}",
            start + width
        );
    }

    #[test]
    fn a_start_aligned_line_starts_at_the_box() {
        let (start, _) = first_line(TextAlign::Start, 1.0);
        assert!(start.abs() < 0.5, "the line starts at {start}");
    }

    /// Two texts that differ only in alignment are two different buffers, so
    /// the cache must not hand one the other's.
    #[test]
    fn alignment_is_part_of_the_cache_key() {
        let keys: Vec<u64> = [
            TextAlign::Start,
            TextAlign::Center,
            TextAlign::End,
            TextAlign::Justified,
        ]
        .map(|align| text_buffer_key(&entry(align), 1.0))
        .into();
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b, "two alignments share a cached buffer");
            }
        }
    }
}
