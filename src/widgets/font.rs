//! Font family and weight types for text styling.
//!
//! These types allow configuring font family and weight on text widgets.

use std::sync::{LazyLock, RwLock};

use cosmic_text::{Family, Weight};
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

#[cfg(test)]
mod tests {
    use super::*;

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
