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

/// Below this, a name is more likely to be a word than an identifier, and the
/// two tests have to agree on it — otherwise raising one is a way to make a
/// failure go away.
const SHORTEST_NAME: usize = 3;

/// Backticked words that are not, and never were, symbols in this crate:
/// external tools, protocols, environment variables belonging to something
/// else, and file names.
const NOT_CRATE_SYMBOLS: &[&str] = &[
    "grim",
    "reviewer",
    // Names the crate does not contain because something else produces them:
    // a macro builds them from the caller's own type or function, an example
    // file is named after them, or they belong to the reader's code rather
    // than the library's. Compiling the examples is what would check these;
    // the book's own samples are compiled by `mdbook test` since #294.
    "Button",
    "rotation_signal",
    "mdbook",
    "lavapipe",
    "llvmpipe",
    "VK_ICD_FILENAMES",
    "WAYLAND_DISPLAY",
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
    let mut text = files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n");

    // The documentation names examples by their file name, and an example is
    // as real as a function: `status_bar` should resolve because
    // `examples/status_bar.rs` is there, and stop resolving when it is not.
    if let Ok(entries) = std::fs::read_dir(repo().join("examples")) {
        for entry in entries.flatten() {
            if let Some(stem) = entry.path().file_stem() {
                text.push('\n');
                text.push_str(&stem.to_string_lossy());
            }
        }
    }

    text
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
            if leaf.len() < SHORTEST_NAME {
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
            if leaf.len() < SHORTEST_NAME {
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

/// Every fence in the book says what it holds, in a spelling rustdoc knows.
///
/// A fence rustdoc does not recognise is not an error — it is a block rustdoc
/// declines to test, silently. While making the book compile, a mechanical pass
/// appended a `;` to 38 fences; ```` ```rust; ```` tested nothing, `mdbook test`
/// stayed green, and 8% of the book was quietly exempt. One of the blocks
/// behind those fences taught two methods the crate does not have.
///
/// An unlabelled fence is the same trap from the other side: mdbook hands it to
/// rustdoc as Rust, so a diagram of box-drawing characters becomes a failing
/// test for reasons that read like a compiler bug.
#[test]
fn every_fence_in_the_book_says_what_it_holds() {
    const ALLOWED: [&str; 6] = ["rust", "rust,ignore", "rust,no_run", "text", "bash", "toml"];
    // wgsl is a shader, and there is exactly one.
    const ALSO: [&str; 1] = ["wgsl"];

    let mut pages = Vec::new();
    read_dir_files(&repo().join("book/src"), "md", &mut pages);
    assert!(
        !pages.is_empty(),
        "no book chapters found: scanning nothing"
    );

    let mut wrong = Vec::new();
    let mut checked = 0usize;
    for doc in pages {
        let source = std::fs::read_to_string(&doc).expect("unreadable chapter");
        let mut inside = false;
        for (number, line) in source.lines().enumerate() {
            if !line.starts_with("```") {
                continue;
            }
            if inside {
                inside = false;
                continue;
            }
            inside = true;
            checked += 1;
            let info = line[3..].trim();
            if ALLOWED.contains(&info) || ALSO.contains(&info) {
                continue;
            }
            let name = doc.strip_prefix(repo()).unwrap_or(&doc).display();
            wrong.push(format!("  {name}:{}: ```{info}", number + 1));
        }
    }

    assert!(
        wrong.is_empty(),
        "{} fence(s) in the book carry an info string rustdoc does not know, so \
         `mdbook test` skips them without saying so. An empty one counts: mdbook \
         gives it to rustdoc as Rust.\n{}\n\n({checked} fences checked)",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Every rustdoc sample in `src/` is compiled, or says why it is not.
///
/// An `ignore`d block is the one place in this repository where an API name can
/// be written and nothing at all checks it. Prose in backticks is checked above;
/// the book's fences are checked below and by `mdbook test`; every other rustdoc
/// sample is compiled by rustdoc in CI. An `ignore` is exempt from all four, and
/// it does not announce itself in the rendered page.
///
/// Three samples were found teaching methods the crate does not have, two of them
/// `container().font_size(..)` and one `container().transform(..)`. The first
/// would have been copied by a caller: it sat on `Text`'s own documentation, next
/// to a line saying a container declares nothing about text.
///
/// Extending the name check above cannot catch these. `font_size` exists three
/// times over — on `Text`, on `TextInput`, on `TextStyle` — and what made the
/// sample false was the receiver. Only a compiler knows receivers, so the sample
/// has to reach one.
///
/// The escape hatch is the block that says why, as its first line: a sample built
/// around an `async` block, or one that would open a surface on somebody's screen,
/// has a reason and should carry it where the next reader meets it.
#[test]
fn every_rustdoc_sample_is_compiled_or_says_why_not() {
    /// What an exempt block's first line has to start with.
    const REASON: &str = "// not compiled:";

    // Both crates, as the prose test above reads both: `guido-macros` carries the
    // `#[component]` sample, which is the first one a new caller copies.
    let mut sources = Vec::new();
    read_dir_files(&repo().join("src"), "rs", &mut sources);
    read_dir_files(&repo().join("guido-macros/src"), "rs", &mut sources);
    assert!(!sources.is_empty(), "no sources found: scanning nothing");

    let mut unexplained = Vec::new();
    let mut exempt = 0usize;
    for file in sources {
        let source = std::fs::read_to_string(&file).expect("unreadable source");
        let lines: Vec<&str> = source.lines().collect();
        for (number, line) in lines.iter().enumerate() {
            let Some(fence) = doc_comment_body(line) else {
                continue;
            };
            // Tokenised rather than compared: ```` ```rust,ignore ```` and
            // ```` ```ignore,no_run ```` are skipped by rustdoc exactly the same,
            // and the first is in the book's own allowlist twenty lines up — so it
            // is the spelling an author coming from the book reaches for. Matching
            // one spelling is how the fence-label scar above happened.
            let Some(info) = fence.trim().strip_prefix("```") else {
                continue;
            };
            if !info.split(',').any(|token| token.trim() == "ignore") {
                continue;
            }
            // The first line of the block, which is where the reason goes.
            let first = lines
                .get(number + 1)
                .and_then(|next| doc_comment_body(next))
                .unwrap_or_default();
            if first.trim_start().starts_with(REASON) {
                exempt += 1;
                continue;
            }
            let name = file.strip_prefix(repo()).unwrap_or(&file).display();
            unexplained.push(format!("  {name}:{}", number + 1));
        }
    }

    assert!(
        unexplained.is_empty(),
        "{} rustdoc sample(s) are marked `ignore`, so nothing compiles them and \
         nothing else checks the names inside them. Either drop the `ignore` — \
         adding whatever hidden `#` setup lines it takes, or `no_run` where the \
         sample compiles but must not run — or state the reason on the block's \
         first line as `{REASON} ...`.\n{}\n\n({exempt} exempt with a reason)",
        unexplained.len(),
        unexplained.join("\n")
    );
}

/// What a line says after its doc-comment marker, if it is one.
///
/// Both markers: an inner `//!` block is documentation the same as an outer
/// `///` one, and `src/lib.rs` carries most of its samples in the inner kind.
fn doc_comment_body(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let body = trimmed
        .strip_prefix("///")
        .or_else(|| trimmed.strip_prefix("//!"))?;
    Some(body.strip_prefix(' ').unwrap_or(body))
}
