//! Font family, weight, style and feature types for text styling.
//!
//! These types allow configuring how text widgets pick and shape their font.

use std::sync::{LazyLock, RwLock};

use cosmic_text::{Family, Style, Weight};
use rustc_hash::FxHashMap;

/// Font family specification.
///
/// `Copy`, and eight bytes: a named family is an [`FamilyId`] into a table of
/// names rather than a string of its own. That is what lets a text property
/// hold one directly — a property stores its value inline since #450, so
/// `Prop<T>` is `Copy` only where `T` is, and declaring a family no longer
/// claims a signal slot to hold a value that cannot change.
///
/// # Examples
///
/// ```no_run
/// # use guido::prelude::*;
/// text("Hello").font_family(FontFamily::Monospace);
/// text("Hello").font_family(FontFamily::name("Inter"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontFamily {
    /// Sans-serif font (default system sans-serif)
    #[default]
    SansSerif,
    /// Serif font (default system serif)
    Serif,
    /// Monospace font (default system monospace)
    Monospace,
    /// Cursive font
    Cursive,
    /// Fantasy font
    Fantasy,
    /// Custom font by name, interned. Build one with [`FontFamily::name`].
    Name(FamilyId),
}

/// Every family name this process has been asked for, and never fewer.
///
/// **Not** a `thread_local!`, and not a field of `AppState` either, although
/// `docs/ARCHITECTURE.md` says new ambient state belongs in the latter. Both
/// have the same fault from opposite directions, and a `FontFamily` outlives
/// them both:
///
/// - `AppState` is safe because `reset` empties *all* of it between
///   applications, and `tests/ambient_state_inventory.rs` refuses a field that
///   opts out — opting out is how #372 happened. But a `FontFamily` an
///   application still holds is an index into this table, so a recycled slot
///   would silently rename a family.
/// - A thread's table would be worse. `FontFamily` is `Copy` and therefore
///   `Send`, `create_signal` requires `Send`, and `WriteSignal` is `Send` on
///   purpose so a background task can write — so
///   `writer.set(FontFamily::name("Inter"))` from a service would mint an
///   index in the worker's table and apply it on the main thread, where it
///   names a different family or none at all. `to_cosmic` turns "none" into
///   sans-serif, so the failure would be a silently wrong font.
///
/// Per-process is what Blink's `AtomicString` table is, for this reason.
///
/// The names are leaked on the way in, which is what makes reading one back a
/// `&'static str` — a lifetime that needs nowhere to live, on any thread. An
/// application names a handful of families and never unnames one, so the table
/// is bounded by the program text rather than by what it runs.
///
/// The lock is not on a hot path: interning happens where a widget is built,
/// and reading happens on the shaping paths, each of which is a cache miss
/// about to run cosmic-text.
static FAMILIES: LazyLock<RwLock<FamilyTable>> =
    LazyLock::new(|| RwLock::new(FamilyTable::default()));

#[derive(Default)]
struct FamilyTable {
    names: Vec<&'static str>,
    ids: FxHashMap<&'static str, FamilyId>,
}

/// A name in the family table, which is what a named [`FontFamily`] holds.
///
/// Opaque and `Copy`. Equality is the whole point: two declarations of the
/// same name compare as one integer rather than as bytes, and that comparison
/// is on the text cache's key path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FamilyId(u32);

impl FontFamily {
    /// A family by name, interned.
    ///
    /// The same name always gives the same family, for the life of the
    /// process — on any thread, and across an `App` being dropped and another
    /// built. See the `FAMILIES` table for why it has to be both.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// text("Hello").font_family(FontFamily::name("Inter"));
    /// ```
    pub fn name(name: &str) -> Self {
        if let Some(&id) = FAMILIES.read().expect("family table").ids.get(name) {
            return FontFamily::Name(id);
        }
        let mut table = FAMILIES.write().expect("family table");
        // Read again: another thread may have interned it between the two.
        if let Some(&id) = table.ids.get(name) {
            return FontFamily::Name(id);
        }
        let leaked: &'static str = String::leak(name.to_owned());
        let id = FamilyId(table.names.len() as u32);
        table.names.push(leaked);
        table.ids.insert(leaked, id);
        FontFamily::Name(id)
    }

    /// The name this family was interned under, or `None` for a generic one.
    pub fn family_name(self) -> Option<&'static str> {
        match self {
            FontFamily::Name(FamilyId(index)) => FAMILIES
                .read()
                .expect("family table")
                .names
                .get(index as usize)
                .copied(),
            _ => None,
        }
    }

    /// Convert to cosmic-text Family type for rendering.
    pub fn to_cosmic(self) -> Family<'static> {
        match self {
            FontFamily::SansSerif => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Monospace => Family::Monospace,
            FontFamily::Cursive => Family::Cursive,
            FontFamily::Fantasy => Family::Fantasy,
            FontFamily::Name(_) => self.family_name().map_or(Family::SansSerif, Family::Name),
        }
    }
}

/// Font weight on a 100-900 scale, matching CSS font-weight values.
///
/// # Examples
///
/// ```no_run
/// # use guido::prelude::*;
/// text("Hello").font_weight(FontWeight::BOLD);
/// text("Hello").font_weight(FontWeight(600));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontWeight(pub u16);

impl FontWeight {
    /// Thin weight (100)
    pub const THIN: Self = Self(100);
    /// Extra-light weight (200)
    pub const EXTRA_LIGHT: Self = Self(200);
    /// Light weight (300)
    pub const LIGHT: Self = Self(300);
    /// Normal/regular weight (400) - default
    pub const NORMAL: Self = Self(400);
    /// Medium weight (500)
    pub const MEDIUM: Self = Self(500);
    /// Semi-bold weight (600)
    pub const SEMI_BOLD: Self = Self(600);
    /// Bold weight (700)
    pub const BOLD: Self = Self(700);
    /// Extra-bold weight (800)
    pub const EXTRA_BOLD: Self = Self(800);
    /// Black/heavy weight (900)
    pub const BLACK: Self = Self(900);

    /// Convert to cosmic-text Weight type for rendering.
    pub fn to_cosmic(self) -> Weight {
        Weight(self.0)
    }
}

/// Whether the glyphs are upright or slanted, as CSS `font-style`.
///
/// The family's own italic or oblique face draws a slanted style where it has
/// one, and its line height is that face's. A family with none is drawn in its
/// upright face, sheared by the shaper — cosmic-text's synthetic italic, as
/// CSS `font-synthesis` allows.
///
/// ```no_run
/// # use guido::prelude::*;
/// text("Quoted").font_style(FontStyle::Italic);
/// text("Quoted").italic();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    /// Upright.
    #[default]
    Normal,
    /// The family's italic face, a separately drawn slanted design.
    Italic,
    /// The family's oblique face, the upright design slanted.
    Oblique,
}

impl FontStyle {
    /// Convert to the cosmic-text style the shaper and the font database take.
    pub fn to_cosmic(self) -> Style {
        match self {
            FontStyle::Normal => Style::Normal,
            FontStyle::Italic => Style::Italic,
            FontStyle::Oblique => Style::Oblique,
        }
    }
}

/// OpenType features a text is shaped with, as CSS `font-feature-settings`:
/// four-letter tags, each with a value — `1` turns a feature on, `0` off, and
/// a larger number picks an alternate.
///
/// The commonest is `tnum`, tabular figures: most interface fonts draw their
/// digits at different widths, so a clock or a percentage that changes in
/// place shifts sideways as it does, and `tnum` gives every digit one width.
///
/// ```no_run
/// # use guido::prelude::*;
/// let tabular = FontFeatures::new().enable(b"tnum");
/// text("12:41").font_features(tabular);
/// text("ffi").font_features(FontFeatures::new().disable(b"liga"));
/// ```
///
/// `Copy`, and four bytes: a set is an id into a table of every distinct set
/// this process has asked for, as a named [`FontFamily`] is an id into a table
/// of names. Two sets of the same features are the same id however they were
/// written, so a cache keys on the id, and the tags themselves are read only
/// where a text is shaped. A feature the font does not have does nothing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontFeatures(u32);

/// A feature set as the table keeps it: sorted by tag, each tag once.
type FeatureList = [([u8; 4], u32)];

/// Every distinct feature set this process has been asked for, and never
/// fewer.
///
/// Where it lives, and why, is `FAMILIES`'s argument over again, because a
/// `FontFeatures` is the same kind of value: `Copy` and so `Send`, held by
/// properties an application keeps across an `App` being dropped and another
/// built, and written from a background task through a `WriteSignal`. A table
/// in `AppState` would be emptied by `reset` under ids still held; a table per
/// thread would read an id minted on a worker as another set or none.
///
/// The sets are leaked on the way in, which is what makes reading one back a
/// `&'static` slice. The table holds every distinct set the program asks
/// for, and never gives one back: a builder chain interns each step, so a
/// chain of three tags is three entries, and a set built from a signal's value
/// leaks one entry per distinct value it ever takes. An application asks for
/// a handful; one that spells a set from unbounded data grows the table with
/// it.
///
/// The lock is not on a hot path: interning happens where a widget is built or
/// a property's closure runs, and reading happens where a text is shaped,
/// which is a cache miss about to run cosmic-text.
static FEATURE_SETS: LazyLock<RwLock<FeatureTable>> =
    LazyLock::new(|| RwLock::new(FeatureTable::default()));

#[derive(Default)]
struct FeatureTable {
    /// The set of id `n + 1`; id 0 is the empty set, which is never stored.
    sets: Vec<&'static FeatureList>,
    ids: FxHashMap<&'static FeatureList, u32>,
}

impl FontFeatures {
    /// No features: the font's defaults.
    pub const fn new() -> Self {
        Self(0)
    }

    /// Turn `tag` on, as `set(tag, 1)`.
    pub fn enable(self, tag: &[u8; 4]) -> Self {
        self.set(tag, 1)
    }

    /// Turn `tag` off, as `set(tag, 0)` — `liga`, say, which a font turns on
    /// by itself.
    pub fn disable(self, tag: &[u8; 4]) -> Self {
        self.set(tag, 0)
    }

    /// Give `tag` a value, replacing any it had, as CSS keeps the last of a
    /// repeated tag.
    pub fn set(self, tag: &[u8; 4], value: u32) -> Self {
        let mut list = self.features().to_vec();
        match list.binary_search_by_key(tag, |(t, _)| *t) {
            Ok(at) => list[at].1 = value,
            Err(at) => list.insert(at, (*tag, value)),
        }
        Self::intern(&list)
    }

    /// Whether the set holds no feature at all.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Each tag and its value, in the order of their tags.
    pub fn features(self) -> &'static FeatureList {
        match self.0 {
            0 => &[],
            id => FEATURE_SETS.read().expect("feature table").sets[id as usize - 1],
        }
    }

    /// The id of `list`, which is sorted and holds each tag once.
    fn intern(list: &FeatureList) -> Self {
        if list.is_empty() {
            return Self::new();
        }
        if let Some(&id) = FEATURE_SETS.read().expect("feature table").ids.get(list) {
            return Self(id);
        }
        let mut table = FEATURE_SETS.write().expect("feature table");
        // Read again: another thread may have interned it between the two.
        if let Some(&id) = table.ids.get(list) {
            return Self(id);
        }
        let leaked: &'static FeatureList = Box::leak(list.into());
        table.sets.push(leaked);
        let id = table.sets.len() as u32;
        table.ids.insert(leaked, id);
        Self(id)
    }
}

/// Written as CSS writes them: `"tnum" 1, "liga" 0`.
impl std::fmt::Debug for FontFeatures {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for (tag, value) in self.features() {
            list.entry(&format_args!(
                "\"{}\" {value}",
                String::from_utf8_lossy(tag)
            ));
        }
        list.finish()
    }
}

/// How tall each line of a text is.
///
/// A bare number is a factor of the font size, as CSS's `line-height`, iced
/// and Flutter all read it: `line_height(1.5)` is `Relative(1.5)`. A height in
/// pixels is spelled `Absolute`.
///
/// # Examples
///
/// ```no_run
/// # use guido::prelude::*;
/// text("Notes").line_height(1.5);
/// text("Sign in").font_size(14.0).line_height(LineHeight::Absolute(20.0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum LineHeight {
    /// The font's own line height: its ascent, descent and line gap, scaled to
    /// the size. Read from the face that draws the family at this weight — a
    /// fallback face where the family's own font is not installed — and a
    /// fallback glyph later in the line does not change it.
    #[default]
    Normal,
    /// A multiple of the font size. A bare number means this.
    Relative(f32),
    /// Logical pixels.
    Absolute(f32),
}

impl From<f32> for LineHeight {
    fn from(factor: f32) -> Self {
        LineHeight::Relative(factor)
    }
}

/// So a bare literal, which Rust types as `f64` when nothing narrows it,
/// is a factor too.
impl From<f64> for LineHeight {
    fn from(factor: f64) -> Self {
        LineHeight::Relative(factor as f32)
    }
}

crate::reactive::converts!(
    f32 => LineHeight,
    f64 => LineHeight,
);

impl LineHeight {
    /// The line height as something a cache can key by: the number by its
    /// bits, as every float in the text caches' keys is held.
    pub(crate) fn key(self) -> (u8, u32) {
        match self {
            LineHeight::Normal => (0, 0),
            LineHeight::Relative(factor) => (1, factor.to_bits()),
            LineHeight::Absolute(height) => (2, height.to_bits()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare number is a factor of the font size, whichever width of float
    /// the literal came out as.
    #[test]
    fn a_bare_number_is_a_factor() {
        assert_eq!(LineHeight::from(1.5_f32), LineHeight::Relative(1.5));
        assert_eq!(LineHeight::from(1.5_f64), LineHeight::Relative(1.5));
    }

    /// Two sets holding the same features are one set, whatever order they
    /// were written in — they key the text caches, so an order that made a
    /// second id would shape the same text twice.
    #[test]
    fn a_feature_set_is_its_features_and_not_their_order() {
        let one = FontFeatures::new().enable(b"tnum").disable(b"liga");
        let other = FontFeatures::new().disable(b"liga").enable(b"tnum");
        assert_eq!(one, other);
        assert_eq!(
            one.features(),
            [(*b"liga", 0), (*b"tnum", 1)],
            "in the order of their tags"
        );
        assert_eq!(format!("{one:?}"), r#"["liga" 0, "tnum" 1]"#);
    }

    /// A set is an id, so a property holds one in four bytes and a text draw
    /// command carries no list.
    #[test]
    fn a_feature_set_is_four_bytes() {
        assert_eq!(std::mem::size_of::<FontFeatures>(), 4);
    }

    /// A tag written twice keeps the last value, as CSS takes the last of a
    /// repeated `font-feature-settings` tag, and is held once.
    #[test]
    fn a_tag_set_again_is_given_its_new_value() {
        let features = FontFeatures::new().enable(b"tnum").disable(b"tnum");
        assert_eq!(features, FontFeatures::new().disable(b"tnum"));
        assert_eq!(features.features().len(), 1);
        assert!(FontFeatures::new().is_empty());
        assert!(!features.is_empty());
    }

    /// There is no limit to how many tags a set holds.
    #[test]
    fn a_set_holds_as_many_tags_as_it_is_given() {
        let tags = [
            b"ss01", b"ss02", b"ss03", b"ss04", b"ss05", b"ss06", b"ss07", b"ss08", b"ss09",
            b"tnum",
        ];
        let all = tags
            .iter()
            .fold(FontFeatures::new(), |set, tag| set.enable(tag));
        assert_eq!(all.features().len(), tags.len());
    }

    /// A set is the same set on every thread and across an `App` being
    /// dropped — the two invariants the table's placement exists for, which
    /// `an_id_survives_the_application_and_the_thread_that_minted_it` pins
    /// for families.
    #[test]
    fn a_set_survives_the_application_and_the_thread_that_minted_it() {
        // A decoy first, so a worker with a table of its own would number the
        // set differently and disagree.
        let _decoy = FontFeatures::new().enable(b"c2sc").enable(b"smcp");
        let before = FontFeatures::new().enable(b"zero");
        crate::app_state::reset();
        assert_eq!(before, FontFeatures::new().enable(b"zero"));
        let elsewhere = std::thread::spawn(|| FontFeatures::new().enable(b"zero"))
            .join()
            .expect("the worker interned a set");
        assert_eq!(elsewhere, before);
        assert_eq!(elsewhere.features(), [(*b"zero", 1)]);
    }

    #[test]
    fn font_family_default() {
        assert_eq!(FontFamily::default(), FontFamily::SansSerif);
    }

    #[test]
    fn font_weight_default() {
        assert_eq!(FontWeight::default(), FontWeight(0));
    }

    #[test]
    fn font_weight_constants() {
        assert_eq!(FontWeight::NORMAL.0, 400);
        assert_eq!(FontWeight::BOLD.0, 700);
    }

    /// A family is a number, so a property can hold one without a heap.
    ///
    /// Eight bytes is what makes `Prop<FontFamily>` weigh exactly what a
    /// `Signal` weighs, so naming a family reactively wastes nothing; `Copy`
    /// is what lets `TextStyle` hold a `Prop` at all. The type's own doc says
    /// why both matter.
    #[test]
    fn a_family_is_eight_bytes_and_a_property_holding_one_is_twelve() {
        use std::mem::size_of;
        assert_eq!(size_of::<FontFamily>(), 8, "a discriminant and an id");
        assert_eq!(
            size_of::<crate::reactive::Prop<FontFamily>>(),
            size_of::<crate::reactive::Signal<FontFamily>>(),
            "so a reactive family costs the signal and not a byte more"
        );

        // `Copy`, said in a way that fails to compile if it is ever lost.
        fn only_for_copy<T: Copy>(_: T) {}
        let family = FontFamily::name("Inter");
        only_for_copy(family);
        only_for_copy(family);
    }

    /// The same name is the same family, and a different name is not.
    ///
    /// What interning has to get right: the id is the identity, so two
    /// declarations of "Inter" must compare equal without comparing bytes —
    /// that equality is on the text cache key path.
    #[test]
    fn one_name_is_one_id_however_often_it_is_named() {
        assert_eq!(FontFamily::name("Inter"), FontFamily::name("Inter"));
        assert_ne!(FontFamily::name("Inter"), FontFamily::name("Iosevka"));
        assert_ne!(FontFamily::name("Inter"), FontFamily::Monospace);

        // And the name comes back, because the renderer needs it to shape.
        assert_eq!(FontFamily::name("Inter").family_name(), Some("Inter"));
        assert_eq!(FontFamily::Monospace.family_name(), None);
    }

    /// An id outlives the `App` that minted it, and the thread that minted it.
    ///
    /// The two invariants the table's placement exists for — see `FAMILIES`,
    /// which argues both. Without the first, a `FontFamily` held across an
    /// application boundary silently starts naming something else; without the
    /// second, one written from a background task through a `WriteSignal` —
    /// which is `Send` on purpose — names something else on the thread that
    /// reads it.
    #[test]
    fn an_id_survives_the_application_and_the_thread_that_minted_it() {
        // A decoy first, so a worker with a table of its own would number
        // "Iosevka" from zero and disagree — without it the two could agree by
        // accident, and the assertion below would pin nothing.
        let _decoy = FontFamily::name("a name nothing else in the suite uses");
        let before = FontFamily::name("Iosevka");
        crate::app_state::reset();
        assert_eq!(
            before,
            FontFamily::name("Iosevka"),
            "the same name still means the same family"
        );
        assert_eq!(before.family_name(), Some("Iosevka"));

        let elsewhere = std::thread::spawn(|| FontFamily::name("Iosevka"))
            .join()
            .expect("the worker interned a name");
        assert_eq!(
            elsewhere, before,
            "and a family named on another thread is the same family here"
        );
        assert_eq!(elsewhere.family_name(), Some("Iosevka"));
    }
}
