//! The fonts guido draws with: the system's and the application's, scanned
//! once, shared by every font system, and listed for the application.

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use cosmic_text::skrifa::{FontRef, GlyphId, MetadataProvider, Tag};
use cosmic_text::{FontSystem, fontdb};

use crate::app_state::with_app_state;
use crate::reactive::{GlobalSignal, Signal};
use crate::widgets::FontWeight;

/// What the first font system found, which every other starts from.
pub(crate) struct FontDb {
    locale: String,
    db: fontdb::Database,
    /// Whether [`font_families`] has started the thread that lists it.
    listing: bool,
}

/// A font system over the application's fonts.
///
/// The measurer and each draw path keep one. The first call scans the
/// system's fonts; every other starts from a copy of what it found — the faces
/// are paths and shared bytes, so the copy costs a fraction of the scan.
pub(crate) fn font_system() -> FontSystem {
    let (locale, db) = with_database(|fonts| (fonts.locale.clone(), fonts.db.clone()));
    FontSystem::new_with_locale_and_db(locale, db)
}

/// The application's font database, scanned on first use from the system's
/// fonts and those given to [`load_font`](crate::load_font) before it.
fn with_database<R>(f: impl FnOnce(&mut FontDb) -> R) -> R {
    with_app_state(|app| {
        let mut fonts = app.font_db.borrow_mut();
        let fonts = fonts.get_or_insert_with(|| {
            let loaded = app.custom_fonts.borrow();
            let sources = loaded
                .iter()
                .map(|data| fontdb::Source::Binary(data.clone()));
            let (locale, db) = FontSystem::new_with_fonts(sources).into_locale_and_db();
            FontDb {
                locale,
                db,
                listing: false,
            }
        });
        f(fonts)
    })
}

/// A font family guido can draw with, as [`font_families`] lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontFamilyInfo {
    name: String,
    monospaced: bool,
    weights: Vec<RangeInclusive<FontWeight>>,
    /// The characters some face of the family draws.
    coverage: Arc<[(u32, u32)]>,
}

impl FontFamilyInfo {
    /// The name [`FontFamily::name`](crate::widgets::FontFamily::name) takes.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether every face of the family is monospaced.
    pub fn monospaced(&self) -> bool {
        self.monospaced
    }

    /// The weights the family's faces are drawn at, lightest first. A static
    /// face is one weight, `w..=w`; a variable face is the span its `wght`
    /// axis covers.
    pub fn weights(&self) -> &[RangeInclusive<FontWeight>] {
        &self.weights
    }

    /// Whether a face of the family has a glyph for `ch`, as Qt's
    /// `writingSystems` counts a family's faces together. A character no face
    /// has is still drawn, in a font that falls back for it.
    pub fn covers(&self, ch: char) -> bool {
        let ch = u32::from(ch);
        let after = self.coverage.partition_point(|&(first, _)| first <= ch);
        after > 0 && ch <= self.coverage[after - 1].1
    }
}

static FAMILIES: GlobalSignal<Vec<FontFamilyInfo>> = GlobalSignal::new(Vec::new);

/// The font families guido can draw with — the system's and those given to
/// [`load_font`](crate::load_font) — sorted by name, whatever its case.
///
/// Empty until a thread has read them: knowing which characters a family
/// draws means reading every face, which is too slow for the frame that asks.
/// The first call starts that thread, and the signal changes once, when it
/// is done. Like the first text, the first call fixes the fonts there are, so
/// a font loaded after it is not listed and not drawn.
///
/// ```no_run
/// use guido::prelude::*;
///
/// let monospaced = create_derived(|| {
///     font_families().with(|families| {
///         families
///             .iter()
///             .filter(|family| family.monospaced() && family.covers('a'))
///             .map(|family| family.name().to_owned())
///             .collect::<Vec<_>>()
///     })
/// });
/// # let _ = monospaced;
/// ```
pub fn font_families() -> Signal<Vec<FontFamilyInfo>> {
    let families = FAMILIES.get();
    let start = with_database(|fonts| {
        (!std::mem::replace(&mut fonts.listing, true)).then(|| fonts.db.clone())
    });
    if let Some(db) = start {
        let write = families.writer();
        if let Err(e) = std::thread::Builder::new()
            .name("guido-font-families".into())
            .spawn(move || write.set(list(&db)))
        {
            log::warn!("failed to spawn the font listing thread: {e}");
        }
    }
    families.read_only()
}

/// Every family in `db`, each face read for its weights and its characters.
fn list(db: &fontdb::Database) -> Vec<FontFamilyInfo> {
    struct Family {
        monospaced: bool,
        weights: Vec<RangeInclusive<FontWeight>>,
        coverage: CodePoints,
    }
    let mut families: BTreeMap<&str, Family> = BTreeMap::new();
    for face in db.faces() {
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        let Some((weights, coverage)) = read_face(db, face) else {
            continue;
        };
        families
            .entry(name)
            .and_modify(|family| {
                family.monospaced &= face.monospaced;
                family.weights.push(weights.clone());
                family.coverage.extend_from_slice(&coverage);
                family.coverage = merged(std::mem::take(&mut family.coverage));
            })
            .or_insert_with(|| Family {
                monospaced: face.monospaced,
                weights: vec![weights.clone()],
                coverage: coverage.clone(),
            });
    }
    let mut listed: Vec<_> = families
        .into_iter()
        .map(|(name, mut family)| {
            family.weights.sort_by_key(|w| (*w.start(), *w.end()));
            family.weights.dedup();
            FontFamilyInfo {
                name: name.to_owned(),
                monospaced: family.monospaced,
                weights: family.weights,
                coverage: family.coverage.into(),
            }
        })
        .collect();
    listed.sort_by_cached_key(|family| family.name.to_lowercase());
    listed
}

/// Sorted, disjoint, inclusive ranges of code points.
type CodePoints = Vec<(u32, u32)>;

/// One face's weights and the characters it draws, read from its tables.
fn read_face(
    db: &fontdb::Database,
    face: &fontdb::FaceInfo,
) -> Option<(RangeInclusive<FontWeight>, CodePoints)> {
    db.with_face_data(face.id, |data, index| {
        let font = FontRef::from_index(data, index).ok()?;
        let weight = |w: f32| FontWeight(w.round() as u16);
        let weights = font
            .axes()
            .get_by_tag(Tag::new(b"wght"))
            .map(|axis| weight(axis.min_value())..=weight(axis.max_value()))
            .unwrap_or(FontWeight(face.weight.0)..=FontWeight(face.weight.0));
        let drawn = font
            .charmap()
            .mappings()
            .filter(|&(_, glyph)| glyph != GlyphId::NOTDEF)
            .map(|(point, _)| (point, point))
            .collect();
        Some((weights, merged(drawn)))
    })
    .flatten()
}

/// `ranges` sorted, with those that overlap or touch made one.
fn merged(mut ranges: Vec<(u32, u32)>) -> CodePoints {
    ranges.sort_unstable();
    let mut merged = CodePoints::with_capacity(ranges.len());
    for (first, last) in ranges {
        match merged.last_mut() {
            Some((_, end)) if first <= end.saturating_add(1) => *end = (*end).max(last),
            _ => merged.push((first, last)),
        }
    }
    merged.shrink_to_fit();
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: &[u8] = include_bytes!("../tests/assets/GuidoCaps.ttf");
    const FIGURES: &[u8] = include_bytes!("../tests/assets/GuidoFigures-Regular.ttf");

    fn has_family(fonts: &FontSystem, name: &str) -> bool {
        fonts
            .db()
            .faces()
            .any(|face| face.families.iter().any(|(family, _)| family == name))
    }

    /// The second font system is a copy of the first one's scan, not a scan
    /// of its own: a font loaded between them reaches neither.
    #[test]
    fn a_font_system_after_the_first_does_not_scan_again() {
        crate::load_font(CAPS.to_vec());
        let first = font_system();
        crate::load_font(FIGURES.to_vec());
        let second = font_system();

        assert!(has_family(&first, "Guido Caps"));
        assert!(has_family(&second, "Guido Caps"));
        assert!(
            !has_family(&second, "Guido Figures"),
            "the second font system scanned the fonts again"
        );
    }

    /// A family of two faces: the whole of DejaVu Sans Mono at 400, and a bold
    /// that draws only digits, loaded first.
    #[test]
    fn a_family_is_its_faces_together() {
        let mut db = fontdb::Database::new();
        for font in [
            &include_bytes!("../tests/assets/DejaVuSansMono-Bold-digits.ttf")[..],
            include_bytes!("../tests/assets/DejaVuSansMono.ttf"),
            include_bytes!("../tests/assets/GuidoFigures-Regular.ttf"),
            include_bytes!("../tests/assets/GuidoFigures-Italic.ttf"),
        ] {
            db.load_font_data(font.to_vec());
        }
        let families = list(&db);
        let names: Vec<_> = families.iter().map(FontFamilyInfo::name).collect();
        assert_eq!(names, ["DejaVu Sans Mono", "Guido Figures"]);

        let mono = &families[0];
        assert_eq!(
            mono.weights(),
            [
                FontWeight::NORMAL..=FontWeight::NORMAL,
                FontWeight::BOLD..=FontWeight::BOLD
            ]
        );
        assert!(mono.monospaced());
        assert!(mono.covers('a'), "the regular face draws it");
        assert!(mono.covers('7'));

        let figures = &families[1];
        assert_eq!(
            figures.weights(),
            [FontWeight::NORMAL..=FontWeight::NORMAL],
            "an upright and an italic face of one weight are one weight"
        );
        assert!(figures.covers('7'));
        assert!(!figures.covers('a'));
    }

    #[test]
    fn ranges_that_overlap_or_touch_become_one() {
        assert_eq!(
            merged(vec![(5, 5), (1, 1), (2, 3), (9, 9), (2, 2)]),
            [(1, 3), (5, 5), (9, 9)]
        );
        assert_eq!(merged(vec![(1, 10), (20, 30), (5, 25)]), [(1, 30)]);
        assert_eq!(merged(Vec::new()), []);
    }

    #[test]
    fn a_family_covers_exactly_its_ranges() {
        let family = FontFamilyInfo {
            name: "Test".into(),
            monospaced: false,
            weights: Vec::new(),
            coverage: vec![
                (u32::from('A'), u32::from('C')),
                (u32::from('x'), u32::from('x')),
            ]
            .into(),
        };
        for ch in ['A', 'B', 'C', 'x'] {
            assert!(family.covers(ch), "{ch}");
        }
        for ch in ['@', 'D', 'w', 'y', '\0'] {
            assert!(!family.covers(ch), "{ch}");
        }
    }
}
