#![cfg(feature = "testing")]
//! The fonts guido draws with, listed for the application.

use std::time::{Duration, Instant};

use guido::prelude::*;
use guido::testing::Headless;

const CAPS: &[u8] = include_bytes!("assets/GuidoCaps.ttf");
const MONO: &[u8] = include_bytes!("assets/DejaVuSansMono.ttf");
const VARIABLE: &[u8] = include_bytes!("assets/GuidoVariable.ttf");

/// The families, once the thread that reads them has written them.
fn listed(app: &mut Headless) -> Vec<FontFamilyInfo> {
    let families = font_families();
    assert!(
        families.get_untracked().is_empty(),
        "read on a thread, so nothing is there before the loop takes its write"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while families.get_untracked().is_empty() {
        assert!(Instant::now() < deadline, "the family list never arrived");
        std::thread::sleep(Duration::from_millis(5));
        app.step();
    }
    families.get_untracked()
}

fn family<'a>(families: &'a [FontFamilyInfo], name: &str) -> &'a FontFamilyInfo {
    families
        .iter()
        .find(|f| f.name() == name)
        .unwrap_or_else(|| panic!("{name} is not listed"))
}

#[test]
fn the_list_holds_the_fonts_the_application_loaded() {
    guido::load_font(CAPS.to_vec());
    guido::load_font(MONO.to_vec());
    guido::load_font(VARIABLE.to_vec());
    let Some(mut app) = Headless::new_or_skip() else {
        return;
    };
    let families = listed(&mut app);

    let caps = family(&families, "Guido Caps");
    assert!(!caps.monospaced());
    assert_eq!(caps.weights(), [FontWeight::NORMAL..=FontWeight::NORMAL]);
    // Guido Caps draws a space, `H`, `p` and `x`, and nothing else.
    assert!(caps.covers('H'));
    assert!(caps.covers('x'));
    assert!(!caps.covers('A'));
    assert!(!caps.covers('a'));

    assert!(family(&families, "DejaVu Sans Mono").monospaced());

    let variable = family(&families, "Guido Variable");
    assert!(
        variable
            .weights()
            .iter()
            .any(|w| w.start() < w.end() && w.contains(&FontWeight::BOLD)),
        "a variable face lists its whole span: {:?}",
        variable.weights()
    );
}
