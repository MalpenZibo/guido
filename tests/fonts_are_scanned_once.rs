//! Every `FontSystem` guido keeps starts from one scan of the system's fonts.
//!
//! A `FontSystem` built with `new` or `new_with_fonts` scans every font on the
//! machine for itself. The measurer, the upright text and the turned text each
//! built theirs that way, so the fonts were scanned three times, and the three
//! could disagree about a font loaded between them. Now `src/fonts.rs` scans
//! once and every other site builds from a copy of that database. That is a
//! rule about where a constructor is called, so it is read from the source.

use std::path::{Path, PathBuf};

/// The constructors that scan. `new_with_locale_and_db`, which takes a
/// database already filled, is the one the rest of the crate calls.
const SCANNING: [&str; 2] = ["FontSystem::new()", "FontSystem::new_with_fonts("];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable source tree") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_the_font_database_scans_the_system_fonts() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 50, "found the source: {}", files.len());

    let mut scanning = Vec::new();
    for file in files {
        if file.ends_with("src/fonts.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&file).expect("readable file");
        for (n, line) in source.lines().enumerate() {
            if SCANNING.iter().any(|call| line.contains(call)) {
                scanning.push(format!("{}:{}", file.display(), n + 1));
            }
        }
    }
    assert!(
        scanning.is_empty(),
        "a font system that scans for itself — build it with crate::fonts::font_system():\n{}",
        scanning.join("\n")
    );
}
