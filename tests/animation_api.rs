//! A timing is never a declaration of its own.
//!
//! The seventeen `animate_*` and `keyframes_*` builders that `Container` used
//! to carry each named a property and said how it moved, two lines away from
//! where the property's value was set. Nothing checked that the two agreed,
//! and nothing stopped a timing being declared for a property that was never
//! set at all. They are gone: a motion now rides with the value, so there is
//! no way to spell one without naming what it moves.
//!
//! This reads the source the way `documentation_references.rs` does, because
//! the property being asserted is the *absence* of an API — nothing can call
//! what is not there, so no ordinary test can watch for its return.

use std::path::{Path, PathBuf};

/// `AnimationState::animate_to` retargets a running animation from inside the
/// advance loop. It is not a builder and names no property: the value it is
/// given is the one it moves to.
const NOT_A_DECLARATION: &[&str] = &["animate_to"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The name a `pub fn` line declares, if the line declares one.
fn public_fn_name(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("pub fn ")?;
    let end = rest.find(|c: char| !c.is_alphanumeric() && c != '_')?;
    Some(&rest[..end])
}

#[test]
fn no_public_builder_declares_a_timing_beside_its_property() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let mut found: Vec<String> = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("a source file this crate owns");
        for line in text.lines() {
            let Some(name) = public_fn_name(line) else {
                continue;
            };
            if (name.starts_with("animate_") || name.starts_with("keyframes_"))
                && !NOT_A_DECLARATION.contains(&name)
            {
                let shown = file.strip_prefix(&src).unwrap_or(file).display();
                found.push(format!("{shown}: {name}"));
            }
        }
    }
    found.sort();

    assert!(
        found.is_empty(),
        "a motion rides with the value it moves, so no builder names a \
         property and a timing apart. Found:\n  {}",
        found.join("\n  ")
    );
}
