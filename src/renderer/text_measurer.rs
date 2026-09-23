use crate::app_state::with_app_state;
use crate::layout::Size;
use crate::widgets::font::{FontFamily, FontWeight};
use crate::widgets::{TextAlign, TextOverflow};
use cosmic_text::{Buffer, FontSystem};
use rustc_hash::FxHashMap;

/// The smallest font size guido will hand to the shaper.
///
/// Not a taste decision: `Metrics` built from a non-positive size does not
/// terminate inside cosmic-text, so a `font_size(-1.0)` — or any size computed
/// from a width that went negative — hangs the process with nothing drawn and
/// nothing said (#349). Well below a pixel, so a size that was merely very
/// small still measures as itself.
const SMALLEST_SHAPEABLE: f32 = 0.01;

/// A size the shaper can actually work with, and the line height that goes
/// with it.
///
/// One door, because there are four places that build `Metrics` from a font
/// size — this measurer and the three draw paths — and each would otherwise
/// have to know that a non-finite or non-positive one is the single input that
/// does not come back.
///
/// A clamped size draws a glyph far too small to see, which is what a caller
/// who asked for nothing drawable should get.
pub(crate) fn shapeable_metrics(font_size: f32) -> (f32, f32) {
    let size = if font_size.is_finite() && font_size >= SMALLEST_SHAPEABLE {
        font_size
    } else {
        SMALLEST_SHAPEABLE
    };
    (size, size * LINE_HEIGHT_RATIO)
}

/// The line height guido asks for, as a multiple of the font size.
const LINE_HEIGHT_RATIO: f32 = 1.2;
use std::hash::{Hash, Hasher};

/// How a text is cut to the lines it may take, decided by layout and carried to
/// every path that shapes it.
///
/// Carried rather than re-derived, because the measurer and the three draw
/// paths each shape the text for themselves, and a text that one of them cuts
/// at another width or line is measured one line high and drawn two. The width
/// is the one it was laid out in, not the box it came back as: a wrapped text
/// is measured at the width it was offered, and shaped anywhere narrower it
/// breaks its lines somewhere else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFit {
    /// The width the lines are laid out in, in logical pixels, or `None` for
    /// as wide as they run.
    pub width: Option<f32>,
    /// The most lines drawn; at least one.
    pub max_lines: u32,
    /// What marks the cut.
    pub overflow: TextOverflow,
    /// Whether the text wraps at `width` or runs on as one line per paragraph.
    pub wrap: bool,
}

/// A [`LineFit`] as something a cache can key by: the width by its bits.
pub(crate) type LineFitKey = (Option<u32>, u32, TextOverflow, bool);

impl LineFit {
    /// The fit as something a cache can key by.
    pub(crate) fn key(&self) -> LineFitKey {
        (
            self.width.map(f32::to_bits),
            self.max_lines,
            self.overflow,
            self.wrap,
        )
    }
}

/// Cache key for measurement results.
///
/// The text and font family are folded into a 64-bit hash (plus the text
/// length as a discriminator) instead of storing owned Strings: the previous
/// key allocated a String on every lookup, hit or miss, and cursor
/// positioning performs many lookups per keystroke.
#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct MeasureCacheKey {
    /// FxHash of (text, font_family)
    content_hash: u64,
    /// Text byte length — cheap extra collision discriminator
    text_len: u32,
    font_size_bits: u32,
    font_weight: FontWeight,
    max_width_bits: Option<u32>,
    /// The cut, when there is one.
    fit: Option<LineFitKey>,
}

impl MeasureCacheKey {
    fn new(
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        fit: Option<LineFit>,
    ) -> Self {
        let mut hasher = rustc_hash::FxHasher::default();
        text.hash(&mut hasher);
        font_family.hash(&mut hasher);
        Self {
            content_hash: hasher.finish(),
            text_len: text.len() as u32,
            font_size_bits: font_size.to_bits(),
            font_weight,
            max_width_bits: max_width.map(|w| w.to_bits()),
            fit: fit.map(|f| f.key()),
        }
    }
}

/// Wholesale-eviction bound for the measurement cache. Entries are tiny
/// (~40 bytes), but the cache previously grew without bound for the process
/// lifetime (e.g. one entry per text-input prefix ever measured).
const MEASURE_CACHE_CAP: usize = 8192;

/// What one shaping pass tells us about a piece of text.
#[derive(Clone, Copy, Debug)]
pub struct Measured {
    pub size: Size,
    /// Distance from the top edge to the baseline of the first line.
    pub baseline: f32,
}

pub struct TextMeasurer {
    font_system: FontSystem,
    measure_cache: FxHashMap<MeasureCacheKey, Measured>,
}

impl TextMeasurer {
    pub fn new() -> Self {
        let mut font_system = FontSystem::new();
        for data in crate::get_registered_fonts() {
            font_system
                .db_mut()
                .load_font_source(cosmic_text::fontdb::Source::Binary(data));
        }
        Self {
            font_system,
            measure_cache: FxHashMap::default(),
        }
    }

    pub fn measure(&mut self, text: &str, font_size: f32, max_width: Option<f32>) -> Size {
        self.measure_styled(
            text,
            font_size,
            max_width,
            FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    pub fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
    ) -> Size {
        self.measure_full(text, font_size, max_width, font_family, font_weight, None)
            .size
    }

    /// Measure, and also report where the first line sits on its baseline.
    ///
    /// Both come out of the same shaping pass and share one cache entry — a
    /// baseline is not worth re-shaping for.
    ///
    /// A text cut by `fit` is measured as the lines it keeps, at the width the
    /// fit carries rather than `max_width`.
    pub fn measure_full(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        fit: Option<LineFit>,
    ) -> Measured {
        let max_width = fit.map_or(max_width, |f| f.width);
        let cache_key =
            MeasureCacheKey::new(text, font_size, max_width, font_family, font_weight, fit);

        // Check cache first (no allocation on this path)
        if let Some(&cached) = self.measure_cache.get(&cache_key) {
            return cached;
        }

        let measured = {
            let buffer = self.shape(text, font_size, max_width, font_family, font_weight, fit);

            let mut width = 0.0f32;
            let mut height = 0.0f32;
            // Where the first line sits: everything a baseline alignment
            // needs, since that is the line a parent lines its children up on.
            let mut baseline = None;
            for run in buffer.layout_runs() {
                width = width.max(run.line_w);
                height += run.line_height;
                baseline.get_or_insert(run.line_y);
            }

            // Ensure minimum height for empty text
            if height == 0.0 {
                height = font_size * 1.2;
            }

            Measured {
                size: Size::new(width, height),
                // Empty text still sits on a line, so a lone label in a
                // baseline row does not jump when its content clears.
                baseline: baseline.unwrap_or(font_size),
            }
        };

        // Cache the result, with wholesale eviction at the cap
        if self.measure_cache.len() >= MEASURE_CACHE_CAP {
            self.measure_cache.clear();
        }
        self.measure_cache.insert(cache_key, measured);

        measured
    }

    /// Shape text into a fresh buffer.
    ///
    /// Uses `Shaping::Advanced`, matching the renderer — measurement with
    /// `Shaping::Basic` could disagree with rendered glyphs for ligatures
    /// and complex scripts, making layout diverge from pixels.
    fn shape(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        fit: Option<LineFit>,
    ) -> Buffer {
        super::text::shape(
            &mut self.font_system,
            text,
            font_size,
            font_family,
            font_weight,
            TextAlign::Start,
            (max_width, None),
            fit,
            1.0,
        )
    }

    /// Compute the cumulative x position of every character boundary by
    /// shaping the text ONCE.
    ///
    /// Returns `char_count + 1` positions: `positions[i]` is the x offset of
    /// the boundary before character `i`, and the last entry is the total
    /// width. Characters swallowed into a ligature/cluster snap to the
    /// cluster start. Assumes single-line LTR text (the text-input model).
    ///
    /// Text inputs previously rebuilt their cursor-position table by
    /// measuring every prefix of the text — O(n) shaping passes of O(n)
    /// text per keystroke.
    pub fn char_positions_styled(
        &mut self,
        text: &str,
        font_size: f32,
        font_family: FontFamily,
        font_weight: FontWeight,
    ) -> Vec<f32> {
        let char_count = text.chars().count();
        let mut positions = vec![0.0f32; char_count + 1];
        if text.is_empty() {
            return positions;
        }

        // Map byte offsets to character indices for glyph lookup
        let mut char_index_at_byte = vec![usize::MAX; text.len() + 1];
        for (char_idx, (byte_idx, _)) in text.char_indices().enumerate() {
            char_index_at_byte[byte_idx] = char_idx;
        }
        char_index_at_byte[text.len()] = char_count;

        let buffer = self.shape(text, font_size, None, font_family, font_weight, None);

        let mut total_width = 0.0f32;
        let mut any_glyphs = false;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                if let Some(&char_idx) = char_index_at_byte.get(glyph.start)
                    && char_idx != usize::MAX
                {
                    positions[char_idx] = glyph.x;
                    any_glyphs = true;
                }
            }
            total_width = total_width.max(run.line_w);
        }
        positions[char_count] = total_width;

        // Forward-fill boundaries that got no glyph (cluster continuations):
        // they sit at the position of the cluster they belong to.
        if any_glyphs {
            for i in 1..char_count {
                if positions[i] == 0.0 && positions[i - 1] > 0.0 {
                    positions[i] = positions[i - 1];
                }
            }
        }

        positions
    }

    /// Measure text width up to a specific character index.
    /// This is useful for cursor positioning in text input widgets.
    pub fn measure_to_char(&mut self, text: &str, font_size: f32, char_index: usize) -> f32 {
        self.measure_to_char_styled(
            text,
            font_size,
            char_index,
            FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Measure text width up to a specific character index with font styling.
    pub fn measure_to_char_styled(
        &mut self,
        text: &str,
        font_size: f32,
        char_index: usize,
        font_family: FontFamily,
        font_weight: FontWeight,
    ) -> f32 {
        if char_index == 0 || text.is_empty() {
            return 0.0;
        }

        // Get the byte position for the character index
        let byte_pos = text
            .char_indices()
            .nth(char_index)
            .map(|(i, _)| i)
            .unwrap_or(text.len());

        let prefix = &text[..byte_pos];
        self.measure_styled(prefix, font_size, None, font_family, font_weight)
            .width
    }

    /// Find the character index from an x-coordinate using binary search.
    /// This is useful for click-to-position in text input widgets.
    pub fn char_from_x(&mut self, text: &str, font_size: f32, x: f32) -> usize {
        self.char_from_x_styled(
            text,
            font_size,
            x,
            FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Find the character index from an x-coordinate with font styling.
    pub fn char_from_x_styled(
        &mut self,
        text: &str,
        font_size: f32,
        x: f32,
        font_family: FontFamily,
        font_weight: FontWeight,
    ) -> usize {
        if text.is_empty() || x <= 0.0 {
            return 0;
        }

        let char_count = text.chars().count();
        let total_width = self
            .measure_styled(text, font_size, None, font_family, font_weight)
            .width;
        if x >= total_width {
            return char_count;
        }

        // Binary search for the character position
        let mut left = 0;
        let mut right = char_count;

        while left < right {
            let mid = (left + right) / 2;
            let width = self.measure_to_char_styled(text, font_size, mid, font_family, font_weight);
            if width < x {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // Check if click is closer to previous character
        if left > 0 {
            let prev_width =
                self.measure_to_char_styled(text, font_size, left - 1, font_family, font_weight);
            let curr_width =
                self.measure_to_char_styled(text, font_size, left, font_family, font_weight);
            if (x - prev_width) < (curr_width - x) {
                return left - 1;
            }
        }

        left.min(char_count)
    }
}

/// Measure through the application's one measurer, building it on first use.
///
/// Lazily, because it is a whole `FontSystem` and it reads the custom fonts
/// the application has loaded: built with the rest of [`AppState`] it would
/// predate every `load_font` call, and pay for a font system in a process
/// that never draws a character.
///
/// [`AppState`]: crate::app_state::AppState
fn with_measurer<R>(f: impl FnOnce(&mut TextMeasurer) -> R) -> R {
    with_app_state(|app| {
        let mut measurer = app.text_measurer.borrow_mut();
        f(measurer.get_or_insert_with(TextMeasurer::new))
    })
}

/// Measure text dimensions using the font system
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> Size {
    with_measurer(|m| m.measure(text, font_size, max_width))
}

/// Measure text dimensions with specified font family and weight
pub fn measure_text_styled(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: FontFamily,
    font_weight: FontWeight,
) -> Size {
    with_measurer(|m| m.measure_styled(text, font_size, max_width, font_family, font_weight))
}

/// Measure text and report where its first line sits on the baseline, as the
/// lines `fit` keeps when it is cut.
pub fn measure_text_full(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: FontFamily,
    font_weight: FontWeight,
    fit: Option<LineFit>,
) -> Measured {
    with_measurer(|m| m.measure_full(text, font_size, max_width, font_family, font_weight, fit))
}

/// Measure text width up to a specific character index (for cursor positioning)
pub fn measure_text_to_char(text: &str, font_size: f32, char_index: usize) -> f32 {
    with_measurer(|m| m.measure_to_char(text, font_size, char_index))
}

/// Measure text width up to a character index with font styling
pub fn measure_text_to_char_styled(
    text: &str,
    font_size: f32,
    char_index: usize,
    font_family: FontFamily,
    font_weight: FontWeight,
) -> f32 {
    with_measurer(|m| {
        m.measure_to_char_styled(text, font_size, char_index, font_family, font_weight)
    })
}

/// Compute cumulative x positions of every character boundary in one
/// shaping pass (for text-input cursor positioning).
pub fn measure_char_positions_styled(
    text: &str,
    font_size: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
) -> Vec<f32> {
    with_measurer(|m| m.char_positions_styled(text, font_size, font_family, font_weight))
}

/// Find the character index from an x-coordinate (for click-to-position)
pub fn char_index_from_x(text: &str, font_size: f32, x: f32) -> usize {
    with_measurer(|m| m.char_from_x(text, font_size, x))
}

/// Find character index from x-coordinate with font styling
pub fn char_index_from_x_styled(
    text: &str,
    font_size: f32,
    x: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
) -> usize {
    with_measurer(|m| m.char_from_x_styled(text, font_size, x, font_family, font_weight))
}

#[cfg(test)]
mod baseline_tests {
    use super::*;

    /// A size that cannot be shaped comes back rather than spinning.
    ///
    /// `Metrics` built from a non-positive size does not terminate inside
    /// cosmic-text: `text("...").font_size(-1.0)` hung the process with nothing
    /// drawn and nothing said, and it was the mutation job that found it — the
    /// mutant that returned `-1.0` from `TextAnims::retarget` was the one
    /// non-caught mutant of fifty-one, and it timed out rather than surviving
    /// (#349).
    ///
    /// Run on a thread with a deadline, because the failure this guards is a
    /// hang: asserted inline, a regression would take CI down with it instead
    /// of failing.
    #[test]
    fn a_size_that_cannot_be_shaped_still_answers() {
        let (done, waiting) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for font_size in [-1.0f32, 0.0, -0.0, f32::NAN, f32::NEG_INFINITY] {
                let m = measure_text_full(
                    "a line of words",
                    font_size,
                    Some(100.0),
                    FontFamily::default(),
                    FontWeight::NORMAL,
                    None,
                );
                assert!(
                    m.size.width.is_finite() && m.size.height.is_finite(),
                    "a {font_size} size measured as {:?}",
                    m.size
                );
            }
            let _ = done.send(());
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("a font size that cannot be shaped has to come back, not spin");
    }

    /// A baseline sits below the ascenders and above the descenders — never
    /// at the very bottom of the line box, which is what "align by the bottom
    /// edge" would be. If this ever holds `height`, baseline alignment has
    /// quietly degenerated into bottom alignment and every descender hangs
    /// below the line the row was supposed to share.
    #[test]
    fn a_baseline_sits_inside_the_line_box() {
        for font_size in [12.0f32, 16.0, 24.0, 48.0] {
            let m = measure_text_full(
                "Hxgjp",
                font_size,
                None,
                FontFamily::default(),
                FontWeight::NORMAL,
                None,
            );
            assert!(
                m.baseline > m.size.height * 0.5 && m.baseline < m.size.height,
                "at {font_size}px the baseline is {} of a {} line — \
                 that is not a baseline",
                m.baseline,
                m.size.height
            );
        }
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    const OVERLONG: &str = "a considerable quantity of words, far more than any two \
                            lines of a narrow box could hold, and then some more";

    fn fit(width: f32, max_lines: u32, overflow: TextOverflow, wrap: bool) -> LineFit {
        LineFit {
            width: Some(width),
            max_lines,
            overflow,
            wrap,
        }
    }

    /// The lines a text is drawn as, each as the glyph ids it shows, and the
    /// glyph id `…` is shaped as — so an assertion can say where the mark is
    /// without knowing the font.
    fn drawn(text: &str, fit: Option<LineFit>) -> (Vec<Vec<u16>>, u16) {
        let mut measurer = TextMeasurer::new();
        let family = FontFamily::default();
        let lines = measurer
            .shape(text, 20.0, None, family, FontWeight::NORMAL, fit)
            .layout_runs()
            .map(|run| run.glyphs.iter().map(|g| g.glyph_id).collect())
            .collect();
        let mark = measurer
            .shape("\u{2026}", 20.0, None, family, FontWeight::NORMAL, None)
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first().map(|g| g.glyph_id))
            .expect("the ellipsis shapes as a glyph");
        (lines, mark)
    }

    #[test]
    fn one_overlong_line_ends_in_an_ellipsis() {
        let (lines, mark) = drawn(OVERLONG, Some(fit(120.0, 1, TextOverflow::Ellipsis, true)));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].last(), Some(&mark), "{lines:?}");
    }

    #[test]
    fn the_last_of_two_wrapped_lines_ends_in_an_ellipsis() {
        let (lines, mark) = drawn(OVERLONG, Some(fit(120.0, 2, TextOverflow::Ellipsis, true)));
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].contains(&mark), "{lines:?}");
        assert_eq!(lines[1].last(), Some(&mark), "{lines:?}");
    }

    /// The marked line starts where the same line starts unmarked.
    ///
    /// cosmic-text starts a line it ellipsizes at the blank the wrap broke on,
    /// so marked through it, the last line sat a space to the right of the
    /// lines above it.
    #[test]
    fn a_marked_line_starts_where_an_unmarked_one_does() {
        // Short words, so the second line begins at a word the wrap broke
        // before rather than inside one it had to break by the glyph.
        const WORDS: &str = "one two three four five six seven eight nine ten eleven";
        let (marked, _) = drawn(WORDS, Some(fit(150.0, 2, TextOverflow::Ellipsis, true)));
        let (clipped, _) = drawn(WORDS, Some(fit(150.0, 2, TextOverflow::Clip, true)));
        assert_eq!(
            marked[1].first(),
            clipped[1].first(),
            "{marked:?} against {clipped:?}"
        );
    }

    #[test]
    fn an_unwrapped_line_in_a_narrow_box_ends_in_an_ellipsis() {
        let mut measurer = TextMeasurer::new();
        let cut = measurer.measure_full(
            OVERLONG,
            20.0,
            None,
            FontFamily::default(),
            FontWeight::NORMAL,
            Some(fit(90.0, 1, TextOverflow::Ellipsis, false)),
        );
        assert!(cut.size.width <= 90.0, "{:?}", cut.size);

        let (lines, mark) = drawn(OVERLONG, Some(fit(90.0, 1, TextOverflow::Ellipsis, false)));
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].last(), Some(&mark), "{lines:?}");
    }

    /// A cut that lands on a line break is still a cut: the paragraph before it
    /// fit, so cosmic-text alone would mark nothing.
    #[test]
    fn a_cut_at_a_line_break_ends_in_an_ellipsis() {
        let (lines, mark) = drawn(
            "one\ntwo\nthree",
            Some(fit(400.0, 2, TextOverflow::Ellipsis, true)),
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[1].last(), Some(&mark), "{lines:?}");
    }

    /// And a paragraph cut inside, after another that fit, is cut at what is
    /// left of the limit rather than at the whole of it.
    #[test]
    fn a_later_paragraph_is_cut_at_what_is_left_of_the_limit() {
        let text = format!("first\n{OVERLONG}");
        let (lines, mark) = drawn(&text, Some(fit(120.0, 2, TextOverflow::Ellipsis, true)));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[1].last(), Some(&mark), "{lines:?}");
    }

    #[test]
    fn a_clipped_cut_keeps_its_lines_and_marks_nothing() {
        let (lines, mark) = drawn(OVERLONG, Some(fit(120.0, 2, TextOverflow::Clip, true)));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines.iter().all(|l| !l.contains(&mark)), "{lines:?}");
    }

    #[test]
    fn the_mark_can_open_the_line_or_stand_in_its_middle() {
        let (lines, mark) = drawn(
            OVERLONG,
            Some(fit(120.0, 1, TextOverflow::EllipsisStart, true)),
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].first(), Some(&mark), "{lines:?}");

        let (lines, mark) = drawn(
            OVERLONG,
            Some(fit(120.0, 1, TextOverflow::EllipsisMiddle, true)),
        );
        assert_eq!(lines.len(), 1);
        let at = lines[0].iter().position(|g| *g == mark);
        assert!(
            at.is_some_and(|at| at > 0 && at < lines[0].len() - 1),
            "{lines:?}"
        );
    }

    /// Content that fits is drawn as it would be with no limit at all.
    #[test]
    fn content_that_fits_is_drawn_unchanged() {
        let (free, mark) = drawn("short", None);
        let (limited, _) = drawn("short", Some(fit(400.0, 1, TextOverflow::Ellipsis, true)));
        assert_eq!(free, limited);
        assert!(!limited[0].contains(&mark));
    }
}
