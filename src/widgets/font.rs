//! Font family and weight types for text styling.
//!
//! These types allow configuring font family and weight on text widgets.

use std::cell::RefCell;

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

thread_local! {
    /// Every family name this thread has been asked for, and never fewer.
    ///
    /// A cell of its own rather than a field of `AppState`, which is where
    /// `docs/ARCHITECTURE.md` says new ambient state belongs. What makes
    /// `AppState` safe is that `reset` empties *all* of it between
    /// applications — and `tests/ambient_state_inventory.rs` refuses a field
    /// that opts out, because opting out is how #372 happened. A `FontFamily`
    /// is a value an application can still be holding after its `App` is
    /// gone, and it is an index into this table, so emptying the table would
    /// leave that index naming whatever the next application interned into
    /// the slot: the same family, silently meaning something else. Nothing
    /// else in `AppState` is reachable from a value that outlives it.
    ///
    /// Interning therefore lasts as long as the thread, as Blink's
    /// `AtomicString` table lasts as long as the process.
    ///
    /// The names are leaked on the way in, which is what makes reading one
    /// back a `&'static str` — a lifetime that needs nowhere to live. An
    /// application names a handful of families and never unnames one, so the
    /// table is bounded by the program text rather than by what it runs.
    static FAMILIES: RefCell<FamilyTable> = RefCell::new(FamilyTable::default());
}

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
    /// thread — including across an `App` being dropped and another built,
    /// which is why the table it interns into is the one thing `AppState`
    /// does not reset.
    ///
    /// ```no_run
    /// # use guido::prelude::*;
    /// text("Hello").font_family(FontFamily::name("Inter"));
    /// ```
    pub fn name(name: &str) -> Self {
        FontFamily::Name(FAMILIES.with_borrow_mut(|table| {
            if let Some(&id) = table.ids.get(name) {
                return id;
            }
            let leaked: &'static str = String::leak(name.to_owned());
            let id = FamilyId(table.names.len() as u32);
            table.names.push(leaked);
            table.ids.insert(leaked, id);
            id
        }))
    }

    /// The name this family was interned under, or `None` for a generic one.
    pub fn family_name(self) -> Option<&'static str> {
        match self {
            FontFamily::Name(FamilyId(index)) => {
                FAMILIES.with_borrow(|table| table.names.get(index as usize).copied())
            }
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

    /// An id outlives the `App` that minted it.
    ///
    /// The invariant the table's placement exists for — see `FAMILIES`, which
    /// argues it. Without this, a `FontFamily` held across an application
    /// boundary silently starts naming something else.
    #[test]
    fn an_id_survives_the_application_that_minted_it() {
        let before = FontFamily::name("Iosevka");
        crate::app_state::reset();
        assert_eq!(
            before,
            FontFamily::name("Iosevka"),
            "the same name still means the same family"
        );
        assert_eq!(before.family_name(), Some("Iosevka"));
    }
}
