//! The fonts guido draws with: the system's and the application's, scanned
//! once and shared by every font system.

use cosmic_text::{FontSystem, fontdb};

use crate::app_state::with_app_state;

/// What the first font system found, which every other starts from.
pub(crate) struct FontDb {
    locale: String,
    db: fontdb::Database,
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
fn with_database<R>(f: impl FnOnce(&FontDb) -> R) -> R {
    with_app_state(|app| {
        let mut fonts = app.font_db.borrow_mut();
        let fonts = fonts.get_or_insert_with(|| {
            let loaded = app.custom_fonts.borrow();
            let sources = loaded
                .iter()
                .map(|data| fontdb::Source::Binary(data.clone()));
            let (locale, db) = FontSystem::new_with_fonts(sources).into_locale_and_db();
            FontDb { locale, db }
        });
        f(fonts)
    })
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
}
