use crate::app_state::with_app_state;
use crate::layout::Size;
use crate::widgets::font::{FontFamily, FontWeight, LineHeight};
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

/// A size the shaper can actually work with.
///
/// Asked by [`shape`](super::text::shape), the one place `Metrics` is built
/// for the measurer and the three draw paths alike, because a non-finite or
/// non-positive size is the single input that does not come back. The line
/// height that goes with it is resolved beside it there.
///
/// A clamped size draws a glyph far too small to see, which is what a caller
/// who asked for nothing drawable should get.
pub(crate) fn shapeable_size(font_size: f32) -> f32 {
    if font_size.is_finite() && font_size >= SMALLEST_SHAPEABLE {
        font_size
    } else {
        SMALLEST_SHAPEABLE
    }
}

use std::hash::{Hash, Hasher};

/// How a text was laid out: the width its lines were broken in, and the cut,
/// when there is one. Decided by layout and carried to every path that shapes
/// the text.
///
/// Carried rather than re-derived, because the measurer and the three draw
/// paths each shape the text for themselves, and a text that one of them lays
/// out at another width is measured on one set of lines and drawn on another —
/// four lines high and drawn on two (#622), or cut one line high and drawn two.
/// The width is the one it was laid out in, not the box it came back as: a
/// wrapped text is measured at the width it was offered, and shaped anywhere
/// narrower it breaks its lines somewhere else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineFit {
    /// The width the lines are laid out in, in logical pixels, or `None` for
    /// as wide as they run.
    pub width: Option<f32>,
    /// The most lines drawn, at least one; `None` for every line the text has.
    pub max_lines: Option<u32>,
    /// What marks the cut.
    pub overflow: TextOverflow,
    /// Whether the text wraps at `width` or runs on as one line per paragraph.
    pub wrap: bool,
}

/// A [`LineFit`] as something a cache can key by: the width by its bits.
pub(crate) type LineFitKey = (Option<u32>, Option<u32>, TextOverflow, bool);

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
    line_height: (u8, u32),
    letter_spacing_bits: u32,
    max_width_bits: Option<u32>,
    /// The cut, when there is one.
    fit: Option<LineFitKey>,
}

impl MeasureCacheKey {
    #[allow(clippy::too_many_arguments)]
    fn new(
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        line_height: LineHeight,
        letter_spacing: f32,
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
            line_height: line_height.key(),
            letter_spacing_bits: letter_spacing.to_bits(),
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
            LineHeight::Normal,
            0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        line_height: LineHeight,
        letter_spacing: f32,
    ) -> Size {
        self.measure_full(
            text,
            font_size,
            max_width,
            font_family,
            font_weight,
            line_height,
            letter_spacing,
            None,
        )
        .size
    }

    /// Measure, and also report where the first line sits on its baseline.
    ///
    /// Both come out of the same shaping pass and share one cache entry — a
    /// baseline is not worth re-shaping for.
    ///
    /// A text laid out by `fit` is measured at the width the fit carries
    /// rather than `max_width`, and as the lines it keeps when it is cut.
    #[allow(clippy::too_many_arguments)]
    pub fn measure_full(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        line_height: LineHeight,
        letter_spacing: f32,
        fit: Option<LineFit>,
    ) -> Measured {
        let max_width = fit.map_or(max_width, |f| f.width);
        let cache_key = MeasureCacheKey::new(
            text,
            font_size,
            max_width,
            font_family,
            font_weight,
            line_height,
            letter_spacing,
            fit,
        );

        // Check cache first (no allocation on this path)
        if let Some(&cached) = self.measure_cache.get(&cache_key) {
            return cached;
        }

        let measured = {
            let buffer = self.shape(
                text,
                font_size,
                max_width,
                font_family,
                font_weight,
                line_height,
                letter_spacing,
                fit,
            );

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

            // Empty text is still one line tall, of the height it would have.
            if height == 0.0 {
                height = buffer.metrics().line_height;
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
    #[allow(clippy::too_many_arguments)]
    fn shape(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: FontFamily,
        font_weight: FontWeight,
        line_height: LineHeight,
        letter_spacing: f32,
        fit: Option<LineFit>,
    ) -> Buffer {
        super::text::shape(
            &mut self.font_system,
            text,
            font_size,
            font_family,
            font_weight,
            line_height,
            letter_spacing,
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
    /// width. A cluster's boundary is the x of its first glyph, and
    /// characters swallowed into a ligature/cluster snap to it. Assumes
    /// single-line LTR text (the text-input model).
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
        letter_spacing: f32,
    ) -> Vec<f32> {
        if text.is_empty() {
            return vec![0.0];
        }

        // Widths only, which no line height changes.
        let buffer = self.shape(
            text,
            font_size,
            None,
            font_family,
            font_weight,
            LineHeight::Normal,
            letter_spacing,
            None,
        );

        // The x each cluster starts at, by byte. A letter and a mark the font
        // cannot compose are two glyphs with one start, and the mark's pen is
        // past the letter: the first one counts.
        let mut starts = vec![None; text.len()];
        let mut total_width = 0.0f32;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                if let Some(start) = starts.get_mut(glyph.start) {
                    start.get_or_insert(glyph.x);
                }
            }
            total_width = total_width.max(run.line_w);
        }

        // A character no glyph starts at is inside a cluster, and its
        // boundary sits where that cluster starts.
        let mut x = 0.0;
        let mut positions: Vec<f32> = text
            .char_indices()
            .map(|(byte, _)| {
                x = starts[byte].unwrap_or(x);
                x
            })
            .collect();
        positions.push(total_width);

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
            0.0,
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
        letter_spacing: f32,
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
        self.measure_styled(
            prefix,
            font_size,
            None,
            font_family,
            font_weight,
            LineHeight::Normal,
            letter_spacing,
        )
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
            0.0,
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
        letter_spacing: f32,
    ) -> usize {
        if text.is_empty() || x <= 0.0 {
            return 0;
        }

        let char_count = text.chars().count();
        let total_width = self
            .measure_styled(
                text,
                font_size,
                None,
                font_family,
                font_weight,
                LineHeight::Normal,
                letter_spacing,
            )
            .width;
        if x >= total_width {
            return char_count;
        }

        // Binary search for the character position
        let mut left = 0;
        let mut right = char_count;

        while left < right {
            let mid = (left + right) / 2;
            let width = self.measure_to_char_styled(
                text,
                font_size,
                mid,
                font_family,
                font_weight,
                letter_spacing,
            );
            if width < x {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        // Check if click is closer to previous character
        if left > 0 {
            let prev_width = self.measure_to_char_styled(
                text,
                font_size,
                left - 1,
                font_family,
                font_weight,
                letter_spacing,
            );
            let curr_width = self.measure_to_char_styled(
                text,
                font_size,
                left,
                font_family,
                font_weight,
                letter_spacing,
            );
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

/// Measure text dimensions with specified font family, weight, line height
/// and letter spacing
pub fn measure_text_styled(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: FontFamily,
    font_weight: FontWeight,
    line_height: LineHeight,
    letter_spacing: f32,
) -> Size {
    with_measurer(|m| {
        m.measure_styled(
            text,
            font_size,
            max_width,
            font_family,
            font_weight,
            line_height,
            letter_spacing,
        )
    })
}

/// Measure text and report where its first line sits on the baseline, as the
/// lines `fit` keeps when it is cut.
#[allow(clippy::too_many_arguments)]
pub fn measure_text_full(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: FontFamily,
    font_weight: FontWeight,
    line_height: LineHeight,
    letter_spacing: f32,
    fit: Option<LineFit>,
) -> Measured {
    with_measurer(|m| {
        m.measure_full(
            text,
            font_size,
            max_width,
            font_family,
            font_weight,
            line_height,
            letter_spacing,
            fit,
        )
    })
}

/// How tall one line of this style is, in logical pixels: the height an
/// empty text measures, and so the line a text beside it is measured on.
///
/// No letter spacing: it widens a line and never makes one taller.
pub(crate) fn measure_line_height(
    font_size: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
    line_height: LineHeight,
) -> f32 {
    with_measurer(|m| {
        m.measure_full(
            "",
            font_size,
            None,
            font_family,
            font_weight,
            line_height,
            0.0,
            None,
        )
        .size
        .height
    })
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
    letter_spacing: f32,
) -> f32 {
    with_measurer(|m| {
        m.measure_to_char_styled(
            text,
            font_size,
            char_index,
            font_family,
            font_weight,
            letter_spacing,
        )
    })
}

/// Compute cumulative x positions of every character boundary in one
/// shaping pass (for text-input cursor positioning).
pub fn measure_char_positions_styled(
    text: &str,
    font_size: f32,
    font_family: FontFamily,
    font_weight: FontWeight,
    letter_spacing: f32,
) -> Vec<f32> {
    with_measurer(|m| {
        m.char_positions_styled(text, font_size, font_family, font_weight, letter_spacing)
    })
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
    letter_spacing: f32,
) -> usize {
    with_measurer(|m| {
        m.char_from_x_styled(text, font_size, x, font_family, font_weight, letter_spacing)
    })
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
                    LineHeight::Normal,
                    0.0,
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
                LineHeight::Normal,
                0.0,
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
            max_lines: Some(max_lines),
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
            .shape(
                text,
                20.0,
                None,
                family,
                FontWeight::NORMAL,
                LineHeight::Normal,
                0.0,
                fit,
            )
            .layout_runs()
            .map(|run| run.glyphs.iter().map(|g| g.glyph_id).collect())
            .collect();
        let mark = measurer
            .shape(
                "\u{2026}",
                20.0,
                None,
                family,
                FontWeight::NORMAL,
                LineHeight::Normal,
                0.0,
                None,
            )
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
            LineHeight::Normal,
            0.0,
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

/// What a line is as tall as, measured with the vendored font alone, so the
/// font's own spacing is DejaVu Sans Mono's and not the machine's.
#[cfg(test)]
mod line_height_tests {
    use super::*;

    const FONT: &[u8] = include_bytes!("../../tests/assets/DejaVuSansMono.ttf");

    /// DejaVu Sans Mono's own line height over its size: hhea's ascent (1901)
    /// less its descent (-483) plus its line gap (0), over 2048 units per em.
    pub(super) const DEJAVU_RATIO: f32 = (1901.0 + 483.0) / 2048.0;

    pub(super) fn measurer() -> TextMeasurer {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(FONT.to_vec());
        TextMeasurer {
            font_system: FontSystem::new_with_locale_and_db("en-US".into(), db),
            measure_cache: FxHashMap::default(),
        }
    }

    fn height(measurer: &mut TextMeasurer, text: &str, line_height: LineHeight) -> f32 {
        measurer
            .measure_full(
                text,
                14.0,
                None,
                FontFamily::name("DejaVu Sans Mono"),
                FontWeight::NORMAL,
                line_height,
                0.0,
                None,
            )
            .size
            .height
    }

    pub(super) fn assert_near(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 0.01,
            "{what}: measured {actual}, expected {expected}"
        );
    }

    #[test]
    fn two_lines_are_two_of_the_line_height_asked_for() {
        let mut m = measurer();
        assert_near(
            height(&mut m, "a\nb", LineHeight::Normal),
            2.0 * DEJAVU_RATIO * 14.0,
            "by default a line is as tall as the font says",
        );
        assert_near(
            height(&mut m, "a\nb", LineHeight::Relative(1.5)),
            2.0 * 21.0,
            "a factor is a multiple of the font size",
        );
        assert_near(
            height(&mut m, "a\nb", LineHeight::Absolute(20.0)),
            40.0,
            "an absolute height is logical pixels",
        );
    }

    /// Two texts that differ only in line height are two measurements: a cache
    /// keyed without it hands the second the first one's lines.
    #[test]
    fn a_line_height_is_part_of_the_measurement_s_key() {
        let mut m = measurer();
        let normal = height(&mut m, "a\nb", LineHeight::Normal);
        let tall = height(&mut m, "a\nb", LineHeight::Absolute(30.0));
        assert_near(
            tall,
            60.0,
            "the second text was handed the first one's lines",
        );
        assert!(normal < tall);
    }

    #[test]
    fn empty_text_is_one_line_of_the_resolved_height() {
        let mut m = measurer();
        assert_near(
            height(&mut m, "", LineHeight::Normal),
            DEJAVU_RATIO * 14.0,
            "an empty text by default",
        );
        assert_near(
            height(&mut m, "", LineHeight::Absolute(20.0)),
            20.0,
            "an empty text at an absolute height",
        );
    }

    /// A number nobody can lay out falls back to the font's own line, never to
    /// `Metrics`, which a non-positive line height would hand to cosmic-text.
    #[test]
    fn a_line_height_that_is_not_a_height_is_the_font_s() {
        let mut m = measurer();
        for bad in [
            LineHeight::Relative(0.0),
            LineHeight::Relative(-1.0),
            LineHeight::Absolute(f32::NAN),
            LineHeight::Absolute(f32::INFINITY),
        ] {
            assert_near(
                height(&mut m, "a", bad),
                DEJAVU_RATIO * 14.0,
                &format!("{bad:?}"),
            );
        }
    }
}

/// Letter spacing, measured with the vendored font alone: DejaVu Sans Mono
/// advances every glyph it has by 1233 of its 2048 units, so what a line of it
/// measures can be worked out by hand rather than asked of the shaper twice.
#[cfg(test)]
mod letter_spacing_tests {
    use super::line_height_tests::{DEJAVU_RATIO as LINE, assert_near, measurer};
    use super::*;

    /// One glyph's advance over its size: hmtx's 1233 over 2048 units per em.
    const ADVANCE: f32 = 1233.0 / 2048.0;

    fn measure(
        m: &mut TextMeasurer,
        text: &str,
        size: f32,
        max_width: Option<f32>,
        spacing: f32,
    ) -> Size {
        m.measure_styled(
            text,
            size,
            max_width,
            FontFamily::name("DejaVu Sans Mono"),
            FontWeight::NORMAL,
            LineHeight::Normal,
            spacing,
        )
    }

    /// Every glyph takes the spacing, the last one included — four letters
    /// are four spacings wider, not three — and it is logical pixels, the
    /// same width at every size though cosmic-text is handed it in em.
    #[test]
    fn a_spacing_is_added_after_every_glyph_at_every_size() {
        let mut m = measurer();
        for (size, spacing) in [
            (20.0, 0.0),
            (20.0, 3.0),
            (20.0, -2.0),
            (10.0, 1.5),
            (14.0, 1.5),
            (40.0, 1.5),
        ] {
            assert_near(
                measure(&mut m, "abcd", size, None, spacing).width,
                4.0 * (ADVANCE * size + spacing),
                &format!("{spacing} px at {size} px"),
            );
        }
    }

    /// Nine glyphs are 108.4 px wide at 20 px, and 126.4 with two pixels
    /// between them: a width in between holds the first on one line and
    /// breaks the second onto two.
    #[test]
    fn a_spacing_moves_where_a_line_wraps() {
        let mut m = measurer();
        let width = 117.0;
        assert!(9.0 * ADVANCE * 20.0 < width && width < 9.0 * (ADVANCE * 20.0 + 2.0));

        let unspaced = measure(&mut m, "aaaa bbbb", 20.0, Some(width), 0.0);
        assert_near(unspaced.height, LINE * 20.0, "unspaced, one line");
        let spaced = measure(&mut m, "aaaa bbbb", 20.0, Some(width), 2.0);
        assert_near(spaced.height, 2.0 * LINE * 20.0, "spaced, two lines");
    }

    /// Two measurements that differ only in spacing are two cache entries: a
    /// key without it hands the second the first one's width.
    #[test]
    fn a_spacing_is_part_of_the_measurement_s_key() {
        let mut m = measurer();
        let plain = measure(&mut m, "abcd", 20.0, None, 0.0).width;
        let spaced = measure(&mut m, "abcd", 20.0, None, 5.0).width;
        assert_near(
            spaced - plain,
            20.0,
            "the second measurement was the first one's",
        );
    }

    /// A number that is not one is no spacing at all, rather than a width
    /// nothing can lay out.
    #[test]
    fn a_spacing_that_is_not_a_number_is_none() {
        let mut m = measurer();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_near(
                measure(&mut m, "abcd", 20.0, None, bad).width,
                4.0 * ADVANCE * 20.0,
                &format!("{bad}"),
            );
        }
    }

    /// The caret's table and a prefix's width agree, and both carry the
    /// spacing of every glyph before the boundary.
    #[test]
    fn a_caret_stands_after_the_spacing_of_every_glyph_before_it() {
        let mut m = measurer();
        let family = FontFamily::name("DejaVu Sans Mono");
        let cell = ADVANCE * 20.0 + 4.0;
        let positions = m.char_positions_styled("abcde", 20.0, family, FontWeight::NORMAL, 4.0);
        assert_eq!(positions.len(), 6);
        for (i, x) in positions.iter().enumerate() {
            assert_near(*x, i as f32 * cell, &format!("boundary {i}"));
            let prefix =
                m.measure_to_char_styled("abcde", 20.0, i, family, FontWeight::NORMAL, 4.0);
            assert_near(prefix, i as f32 * cell, &format!("prefix of {i}"));
        }
    }

    /// A click lands on the boundary nearest it in the spaced line: three
    /// spaced cells in is boundary 3, where the unspaced line would put it
    /// past the fifth glyph.
    #[test]
    fn a_click_finds_the_boundary_of_the_spaced_line() {
        let mut m = measurer();
        let family = FontFamily::name("DejaVu Sans Mono");
        let x = 3.0 * (ADVANCE * 20.0 + 10.0);
        assert_eq!(
            m.char_from_x_styled("abcdefgh", 20.0, x, family, FontWeight::NORMAL, 10.0),
            3
        );
        assert_eq!(
            m.char_from_x_styled("abcdefgh", 20.0, x, family, FontWeight::NORMAL, 0.0),
            5
        );
    }

    /// What cosmic-text adds the spacing to, pinned rather than chosen: the
    /// advance of every glyph it shapes. These are four clusters at 20 px
    /// with 4 px of spacing, each as many spacings wider as it has glyphs —
    /// which is not always as many as it has characters.
    #[test]
    fn a_spacing_is_added_per_glyph_and_not_per_character() {
        let mut m = measurer();
        let cell = ADVANCE * 20.0;
        // (text, advances unspaced, glyphs, what it is)
        for (text, advances, glyphs, what) in [
            // x has no precomposed accented form, so the mark is a glyph of
            // its own, zero wide, and takes a spacing of its own.
            (
                "x\u{301}",
                1.0,
                2.0,
                "a mark after a base it cannot compose with",
            ),
            // e and the mark are composed into é before shaping: one glyph.
            ("e\u{301}", 1.0, 1.0, "a mark the shaper composes"),
            // Lam and alef are one ligature glyph: two letters, one spacing.
            ("\u{644}\u{627}", 1.0, 1.0, "the lam-alef ligature"),
            // Three joined Arabic letters, three glyphs, pulled apart between
            // them: no cursive connection is kept across the gap.
            (
                "\u{633}\u{644}\u{645}",
                3.0,
                3.0,
                "three joined Arabic letters",
            ),
        ] {
            assert_near(
                measure(&mut m, text, 20.0, None, 0.0).width,
                advances * cell,
                &format!("{what}, unspaced"),
            );
            assert_near(
                measure(&mut m, text, 20.0, None, 4.0).width,
                advances * cell + glyphs * 4.0,
                what,
            );
        }
    }

    /// And a mark is placed from the pen the spacing moved, so it lands that
    /// far right of the base it is attached to.
    #[test]
    fn a_combining_mark_lands_one_spacing_past_its_base() {
        let mut m = measurer();
        let mark_from_base = |m: &mut TextMeasurer, spacing| {
            let buffer = m.shape(
                "x\u{301}",
                20.0,
                None,
                FontFamily::name("DejaVu Sans Mono"),
                FontWeight::NORMAL,
                LineHeight::Normal,
                spacing,
                None,
            );
            let run = buffer.layout_runs().next().expect("one line");
            assert_eq!(run.glyphs.len(), 2, "a base and its mark");
            let at = |g: &cosmic_text::LayoutGlyph| g.x + g.x_offset;
            at(&run.glyphs[1]) - at(&run.glyphs[0])
        };
        let attached = mark_from_base(&mut m, 0.0);
        assert_near(
            mark_from_base(&mut m, 4.0),
            attached + 4.0,
            "the mark's place",
        );
    }

    /// x and U+0301 are two glyphs with one cluster start, the mark's pen
    /// past the base's: the boundary before the cluster is the base's x, and
    /// the one inside it snaps to the same place. Spaced, the mark takes a
    /// spacing of its own and the boundaries still start at 0.
    #[test]
    fn a_boundary_takes_the_first_glyph_of_its_cluster() {
        let mut m = measurer();
        let family = FontFamily::name("DejaVu Sans Mono");
        let cell = ADVANCE * 20.0;
        for (text, spacing, expected) in [
            ("x\u{301}", 0.0, vec![0.0, 0.0, cell]),
            ("x\u{301}", 4.0, vec![0.0, 0.0, cell + 8.0]),
            (
                "ax\u{301}b",
                0.0,
                vec![0.0, cell, cell, 2.0 * cell, 3.0 * cell],
            ),
        ] {
            let got = m.char_positions_styled(text, 20.0, family, FontWeight::NORMAL, spacing);
            assert_eq!(got.len(), expected.len(), "{text:?}: {got:?}");
            for (i, (x, want)) in got.iter().zip(&expected).enumerate() {
                assert_near(
                    *x,
                    *want,
                    &format!("{text:?} at {spacing} px, boundary {i}"),
                );
            }
        }
    }
}
