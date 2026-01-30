//! Font family and weight types for text styling.
//!
//! These types allow configuring font family and weight on text widgets.

use cosmic_text::{Family, Weight};

/// Font family specification.
///
/// # Examples
///
/// ```ignore
/// text("Hello").font_family(FontFamily::Monospace)
/// text("Hello").font_family(FontFamily::Name("Inter".into()))
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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
    /// Custom font by name
    Name(String),
}

impl FontFamily {
    /// Convert to cosmic-text Family type for rendering.
    pub fn to_cosmic(&self) -> Family<'_> {
        match self {
            FontFamily::SansSerif => Family::SansSerif,
            FontFamily::Serif => Family::Serif,
            FontFamily::Monospace => Family::Monospace,
            FontFamily::Cursive => Family::Cursive,
            FontFamily::Fantasy => Family::Fantasy,
            FontFamily::Name(name) => Family::Name(name),
        }
    }
}

/// Font weight on a 100-900 scale, matching CSS font-weight values.
///
/// # Examples
///
/// ```ignore
/// text("Hello").font_weight(FontWeight::BOLD)
/// text("Hello").font_weight(FontWeight(600))
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
}
