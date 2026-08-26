//! The documentation is checked against the code it describes.
//!
//! `AGENTS.md`, the skills, the commands and the reviewer's criteria are read
//! by whoever — or whatever — is about to change this library. `docs/`, the
//! book and the README are read by whoever is about to use it. All of them name
//! APIs. A
//! renamed function leaves them quietly wrong, and quietly wrong instructions
//! are worse than none: they are followed.
//!
//! This is not a spell-checker for prose. It takes every identifier they
//! write in backticks, and asserts the crate still has something by that name.
//! It cannot tell whether the sentence around it is true — only that the thing
//! it points at exists, which is the failure mode that actually happens.
//!
//! When it fails, either the documentation is stale or the identifier is new
//! and belongs in `NOT_CRATE_SYMBOLS` below, which is the list of words this
//! documentation deliberately writes in backticks without the crate owning
//! them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Backticked words that are not, and never were, symbols in this crate:
/// external tools, protocols, environment variables belonging to something
/// else, and file names.
const NOT_CRATE_SYMBOLS: &[&str] = &[
    "grim",
    "reviewer",
    // Names the crate does not contain because something else produces them:
    // a macro builds them from the caller's own type or function, an example
    // file is named after them, or they belong to the reader's code rather
    // than the library's. Compiling the examples is what would check these,
    // and until the book compiles nothing here can.
    "AppStateWriters",
    "status_bar",
    "Button",
    "rotation_signal",
    "mdbook",
    "lavapipe",
    "llvmpipe",
    "VK_ICD_FILENAMES",
    "vulkan-swrast",
    "mesa-vulkan-drivers",
];

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read_dir_files(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            read_dir_files(&path, extension, out);
        } else if path.extension().is_some_and(|e| e == extension) {
            out.push(path);
        }
    }
}

/// Everything the crate is written in, as one haystack.
fn crate_source() -> String {
    let mut files = Vec::new();
    read_dir_files(&repo().join("src"), "rs", &mut files);
    read_dir_files(&repo().join("guido-macros/src"), "rs", &mut files);
    read_dir_files(&repo().join("tests"), "rs", &mut files);
    files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The identifiers a markdown file writes in backticks.
///
/// A span between backticks counts when it reads like code and not like prose:
/// letters, digits, underscores and `::`, with an optional `()` on the end.
/// Anything with a space in it is a phrase, and anything shorter than three
/// characters is noise.
fn backticked_identifiers(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut rest = text;

    // Fenced code blocks are examples, not claims about names: skip them.
    while let Some(start) = rest.find("```") {
        scan_spans(&rest[..start], &mut found);
        let after = &rest[start + 3..];
        match after.find("```") {
            Some(end) => rest = &after[end + 3..],
            None => return found,
        }
    }
    scan_spans(rest, &mut found);
    found
}

fn scan_spans(text: &str, found: &mut BTreeSet<String>) {
    let mut parts = text.split('`');
    // Outside a span, then inside, alternating.
    parts.next();
    while let Some(inside) = parts.next() {
        let candidate = inside.strip_suffix("()").unwrap_or(inside);
        let looks_like_code = candidate.len() >= 3
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
            && candidate.chars().any(|c| c.is_ascii_alphabetic());
        if looks_like_code {
            found.insert(candidate.to_string());
        }
        parts.next();
    }
}

/// Everything an agent is handed: the contract, the working knowledge, the
/// commands, the reviewer's criteria.
#[test]
fn every_identifier_the_agent_documentation_names_still_exists() {
    let source = crate_source();

    // Everything an agent is handed: the contract, the working knowledge, the
    // commands that drive a change, and the reviewer's criteria. All of them
    // name APIs, and all of them are followed.
    let mut docs = vec![repo().join("AGENTS.md")];
    for dir in [".claude/skills", ".claude/commands", ".claude/agents"] {
        let mut found = Vec::new();
        read_dir_files(&repo().join(dir), "md", &mut found);
        assert!(
            !found.is_empty(),
            "no documentation found under {dir} — this test is checking nothing"
        );
        docs.extend(found);
    }

    let mut stale = Vec::new();
    let mut checked = 0usize;

    for doc in &docs {
        let text = std::fs::read_to_string(doc).expect("read documentation");
        for identifier in backticked_identifiers(&text) {
            if NOT_CRATE_SYMBOLS.contains(&identifier.as_str()) {
                continue;
            }
            // A path is only as real as its last segment: `Signal::select` is
            // wrong when `select` does not exist, whatever `Signal` is.
            let leaf = identifier.rsplit("::").next().unwrap_or(&identifier);
            if leaf.len() < 3 {
                continue;
            }
            checked += 1;
            if !contains_word(&source, leaf) {
                let name = doc
                    .strip_prefix(repo())
                    .unwrap_or(doc)
                    .display()
                    .to_string();
                stale.push(format!("  {name}: `{identifier}`"));
            }
        }
    }

    assert!(
        stale.is_empty(),
        "documentation names {} identifier(s) the crate does not have. Either \
         the documentation went stale when something was renamed, or the name \
         belongs in NOT_CRATE_SYMBOLS in this file:\n{}\n\n({checked} \
         identifiers checked)",
        stale.len(),
        stale.join("\n")
    );
}

/// Whole-word search, so `select` does not match `selection`.
fn contains_word(haystack: &str, word: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(word) {
        let start = from + offset;
        let end = start + word.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// And everything a user is handed: the developer reference under `docs/`, the
/// book published to the project site, and the README.
///
/// This half went unwatched while the other half was being fixed. `mdbook build`
/// asks whether the book renders, not whether it is true, and nothing asked
/// anything of `docs/` at all — which is how six false claims accumulated in
/// the agent-facing files before anybody looked, and there is no reason the
/// user-facing ones would decay more slowly.
#[test]
fn every_identifier_the_user_documentation_names_still_exists() {
    let source = crate_source();

    let mut docs = vec![repo().join("README.md")];
    for dir in ["docs", "book/src"] {
        let mut found = Vec::new();
        read_dir_files(&repo().join(dir), "md", &mut found);
        assert!(
            !found.is_empty(),
            "no markdown under {dir}: checking nothing"
        );
        docs.extend(found);
    }

    let mut stale = Vec::new();
    let mut checked = 0usize;

    for doc in &docs {
        let text = std::fs::read_to_string(doc).expect("read documentation");
        for identifier in backticked_identifiers(&text) {
            if NOT_CRATE_SYMBOLS.contains(&identifier.as_str()) {
                continue;
            }
            let leaf = identifier.rsplit("::").next().unwrap_or(&identifier);
            if leaf.len() < 4 {
                continue;
            }
            checked += 1;
            if !contains_word(&source, leaf) {
                let name = doc.strip_prefix(repo()).unwrap_or(doc).display();
                stale.push(format!("  {name}: `{identifier}`"));
            }
        }
    }

    assert!(
        stale.is_empty(),
        "the user documentation names {} identifier(s) the crate does not \
         have. Either it went stale when something was renamed, or the name \
         belongs in NOT_CRATE_SYMBOLS in this file:\n{}\n\n({checked} \
         identifiers checked)",
        stale.len(),
        stale.join("\n")
    );
}
