use crate::layout::Size;
use crate::widgets::font::{FontFamily, FontWeight};
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

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
}

impl MeasureCacheKey {
    fn new(
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: &FontFamily,
        font_weight: FontWeight,
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
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    pub fn measure_styled(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> Size {
        self.measure_full(text, font_size, max_width, font_family, font_weight)
            .size
    }

    /// Measure, and also report where the first line sits on its baseline.
    ///
    /// Both come out of the same shaping pass and share one cache entry — a
    /// baseline is not worth re-shaping for.
    pub fn measure_full(
        &mut self,
        text: &str,
        font_size: f32,
        max_width: Option<f32>,
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> Measured {
        let cache_key = MeasureCacheKey::new(text, font_size, max_width, font_family, font_weight);

        // Check cache first (no allocation on this path)
        if let Some(&cached) = self.measure_cache.get(&cache_key) {
            return cached;
        }

        let measured = {
            let buffer = self.shape(text, font_size, max_width, font_family, font_weight);

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
        font_family: &FontFamily,
        font_weight: FontWeight,
    ) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        buffer.set_size(&mut self.font_system, max_width, None);
        buffer.set_text(
            &mut self.font_system,
            text,
            &Attrs::new()
                .family(font_family.to_cosmic())
                .weight(font_weight.to_cosmic()),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, true);
        buffer
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
        font_family: &FontFamily,
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

        let buffer = self.shape(text, font_size, None, font_family, font_weight);

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
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Measure text width up to a specific character index with font styling.
    pub fn measure_to_char_styled(
        &mut self,
        text: &str,
        font_size: f32,
        char_index: usize,
        font_family: &FontFamily,
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
            &FontFamily::default(),
            FontWeight::NORMAL,
        )
    }

    /// Find the character index from an x-coordinate with font styling.
    pub fn char_from_x_styled(
        &mut self,
        text: &str,
        font_size: f32,
        x: f32,
        font_family: &FontFamily,
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

thread_local! {
    static TEXT_MEASURER: RefCell<TextMeasurer> = RefCell::new(TextMeasurer::new());
}

/// Measure text dimensions using the font system
pub fn measure_text(text: &str, font_size: f32, max_width: Option<f32>) -> Size {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure(text, font_size, max_width))
}

/// Measure text dimensions with specified font family and weight
pub fn measure_text_styled(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> Size {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.measure_styled(text, font_size, max_width, font_family, font_weight))
}

/// Measure text and report where its first line sits on the baseline.
pub fn measure_text_full(
    text: &str,
    font_size: f32,
    max_width: Option<f32>,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> Measured {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.measure_full(text, font_size, max_width, font_family, font_weight))
}

/// Measure text width up to a specific character index (for cursor positioning)
pub fn measure_text_to_char(text: &str, font_size: f32, char_index: usize) -> f32 {
    TEXT_MEASURER.with_borrow_mut(|m| m.measure_to_char(text, font_size, char_index))
}

/// Measure text width up to a character index with font styling
pub fn measure_text_to_char_styled(
    text: &str,
    font_size: f32,
    char_index: usize,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> f32 {
    TEXT_MEASURER.with_borrow_mut(|m| {
        m.measure_to_char_styled(text, font_size, char_index, font_family, font_weight)
    })
}

/// Compute cumulative x positions of every character boundary in one
/// shaping pass (for text-input cursor positioning).
pub fn measure_char_positions_styled(
    text: &str,
    font_size: f32,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> Vec<f32> {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.char_positions_styled(text, font_size, font_family, font_weight))
}

/// Find the character index from an x-coordinate (for click-to-position)
pub fn char_index_from_x(text: &str, font_size: f32, x: f32) -> usize {
    TEXT_MEASURER.with_borrow_mut(|m| m.char_from_x(text, font_size, x))
}

/// Find character index from x-coordinate with font styling
pub fn char_index_from_x_styled(
    text: &str,
    font_size: f32,
    x: f32,
    font_family: &FontFamily,
    font_weight: FontWeight,
) -> usize {
    TEXT_MEASURER
        .with_borrow_mut(|m| m.char_from_x_styled(text, font_size, x, font_family, font_weight))
}

#[cfg(test)]
mod baseline_tests {
    use super::*;

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
                &FontFamily::default(),
                FontWeight::NORMAL,
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
