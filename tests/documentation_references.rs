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
    // Linux calls and flags the password chapter tells a lock screen about.
    "mlockall",
    "MCL_FUTURE",
    "ptrace",
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

/// A line that opens or closes a fence: the character it is made of, how many
/// of them, and what follows. `None` for any other line.
///
/// A fence is three or more backticks or tildes, indented or not — a block in
/// a list item is indented with it. Outside a list, markdown reads a fence
/// indented four spaces as an indented code block instead, which nothing here
/// writes. A backtick fence's info string cannot hold a backtick, so a line
/// that starts with a span showing a fence, as a table cell writing
/// ```` ```text ```` can, is prose.
fn fence_run(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim();
    let mark = line.chars().next().filter(|&c| c == '`' || c == '~')?;
    let length = line.chars().take_while(|&c| c == mark).count();
    let info = &line[length..];
    (length >= 3 && !(mark == '`' && info.contains('`'))).then(|| (mark, length, info.trim()))
}

/// One piece of a markdown file: prose, or a fenced block with the line it
/// opens on, its info string, its lines, and whether anything closed it.
enum Part<'a> {
    Prose(&'a str),
    Fence {
        line: usize,
        info: &'a str,
        body: Vec<&'a str>,
        closed: bool,
    },
}

/// A markdown file cut into its prose and its fenced blocks, in order.
///
/// A block closes on a line holding only a run of the character that opened
/// it, at least as long — so a ```` ````md ```` block can show a three-backtick
/// one — and a block nothing closes runs to the end of the file.
fn parts(text: &str) -> Vec<Part<'_>> {
    struct Open<'a> {
        mark: char,
        length: usize,
        line: usize,
        info: &'a str,
        body: Vec<&'a str>,
    }
    let mut parts = Vec::new();
    let mut open: Option<Open> = None;
    let (mut from, mut at) = (0, 0);
    for (number, line) in text.split_inclusive('\n').enumerate() {
        let run = fence_run(line);
        let start = at;
        at += line.len();
        match open.as_mut() {
            Some(fence) => {
                let closes = run.is_some_and(|(mark, length, info)| {
                    mark == fence.mark && length >= fence.length && info.is_empty()
                });
                if closes {
                    let Open {
                        line, info, body, ..
                    } = open.take().expect("a block is open");
                    parts.push(Part::Fence {
                        line,
                        info,
                        body,
                        closed: true,
                    });
                    from = at;
                } else {
                    fence.body.push(line.trim_end_matches(['\n', '\r']));
                }
            }
            None => {
                if let Some((mark, length, info)) = run {
                    parts.push(Part::Prose(&text[from..start]));
                    open = Some(Open {
                        mark,
                        length,
                        line: number + 1,
                        info,
                        body: Vec::new(),
                    });
                }
            }
        }
    }
    match open {
        Some(Open {
            line, info, body, ..
        }) => parts.push(Part::Fence {
            line,
            info,
            body,
            closed: false,
        }),
        None => parts.push(Part::Prose(&text[from..])),
    }
    parts
}

/// The prose of a markdown file: everything outside its fenced blocks, which
/// are examples rather than claims about names.
fn prose(text: &str) -> Vec<&str> {
    parts(text)
        .into_iter()
        .filter_map(|part| match part {
            Part::Prose(prose) => Some(prose),
            Part::Fence { .. } => None,
        })
        .collect()
}

/// `text` cut at its blank lines, which no span crosses.
fn paragraphs(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let (mut from, mut at) = (0, 0);
    for line in text.split_inclusive('\n') {
        if line.trim().is_empty() {
            found.push(&text[from..at]);
            from = at + line.len();
        }
        at += line.len();
    }
    found.push(&text[from..]);
    found
}

/// Whether the character at `index` is escaped: an odd run of backslashes
/// before it, since `\\` is a backslash that escapes nothing.
fn is_escaped(bytes: &[u8], index: usize) -> bool {
    bytes[..index]
        .iter()
        .rev()
        .take_while(|&&byte| byte == b'\\')
        .count()
        % 2
        == 1
}

/// What the prose of a markdown file writes between backticks.
///
/// A span opens on a run of backticks and closes on the next run of the same
/// length in its paragraph, so ```` ```text ```` is one span and not five. A
/// run nothing closes, or a backtick escaped with `\`, is a literal backtick,
/// not the start of a span that swallows the rest of the page.
///
/// A paragraph is what blank lines bound. Markdown also ends a span at a table
/// cell, a heading or a list item; this does not, so a stray backtick in one
/// row of a table pairs with the first in the next.
fn backticked_spans(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for paragraph in prose(text).into_iter().flat_map(paragraphs) {
        let bytes = paragraph.as_bytes();
        let run = |from: usize| bytes[from..].iter().take_while(|&&b| b == b'`').count();
        let mut at = 0;
        while let Some(offset) = paragraph[at..].find('`') {
            let open = at + offset;
            if is_escaped(bytes, open) {
                at = open + 1;
                continue;
            }
            let length = run(open);
            let inside = open + length;
            let mut close = None;
            let mut next = inside;
            while let Some(offset) = paragraph[next..].find('`') {
                let start = next + offset;
                let candidate = run(start);
                if candidate == length {
                    close = Some(start);
                    break;
                }
                next = start + candidate;
            }
            let Some(close) = close else {
                at = inside;
                continue;
            };
            let span = &paragraph[inside..close];
            // One space either side lets a span start or end with a backtick;
            // it is not part of what the span says.
            let span = match span.strip_prefix(' ').and_then(|s| s.strip_suffix(' ')) {
                Some(trimmed) if !trimmed.trim().is_empty() => trimmed,
                _ => span,
            };
            found.push(span);
            at = close + length;
        }
    }
    found
}

/// The scan, on what markdown makes a span or a fence and what it does not.
#[test]
fn a_span_and_a_fence_are_found_as_markdown_finds_them() {
    let text = "A table cell shows ```` ```text ```` and `after_the_cell`.\n\
                ```` ```text ```` can start a line too, and `after_the_line` is read.\n\
                Press the ` key.\n\
                \n\
                Then `after_a_stray_backtick` is read, and \\`escaped` is not a span.\n\
                ````md\n\
                ```rust\n\
                `inside_a_long_fence`\n\
                ````\n\
                ~~~\n\
                `inside_a_tilde_fence`\n\
                ~~~\n\
                - in a list:\n  ```rust\n  `inside_an_indented_fence`\n  ```\n\
                `after_every_fence`\n\
                \n\
                C:\\\\`after_an_escaped_backslash`\n";
    assert_eq!(
        backticked_spans(text),
        [
            "```text",
            "after_the_cell",
            "```text",
            "after_the_line",
            "after_a_stray_backtick",
            "after_every_fence",
            "after_an_escaped_backslash"
        ]
    );
    let found: Vec<_> = fences(text)
        .into_iter()
        .map(|(line, info, body)| (line, info, body.len()))
        .collect();
    assert_eq!(found, [(6, "md", 2), (10, "", 1), (14, "rust", 1)]);
    assert_eq!(unclosed_fence(text), None);
    assert_eq!(unclosed_fence("Prose.\n\n```rust\nlet x = 1;\n"), Some(3));
}

/// The line of the first fence in `text` that nothing closes.
fn unclosed_fence(text: &str) -> Option<usize> {
    parts(text).into_iter().find_map(|part| match part {
        Part::Fence {
            line,
            closed: false,
            ..
        } => Some(line),
        _ => None,
    })
}

/// Every fence in the documentation closes. One that does not runs, as these
/// checks read it, to the end of the file, and everything after it is read as
/// code rather than as claims about names. Markdown would close a block in a
/// list item when the item ends; these checks would not, so it is closed here
/// in so many words.
#[test]
fn every_fence_in_the_documentation_closes() {
    let unclosed: Vec<_> = agent_documentation()
        .iter()
        .chain(&user_documentation())
        .filter_map(|doc| {
            let text = std::fs::read_to_string(doc).expect("read documentation");
            let line = unclosed_fence(&text)?;
            Some(format!(
                "  {}:{line}",
                doc.strip_prefix(repo()).unwrap_or(doc).display()
            ))
        })
        .collect();
    assert!(
        unclosed.is_empty(),
        "{} fence(s) in the documentation never close, so the rest of the file \
         is read as code:\n{}",
        unclosed.len(),
        unclosed.join("\n")
    );
}

/// The identifiers a markdown file writes in backticks.
///
/// A span between backticks counts when it reads like code and not like prose:
/// letters, digits, underscores and `::`, with an optional `()` on the end.
/// Anything with a space in it is a phrase, and anything shorter than three
/// characters is noise.
fn backticked_identifiers(text: &str) -> BTreeSet<String> {
    backticked_spans(text)
        .into_iter()
        .map(|inside| inside.strip_suffix("()").unwrap_or(inside))
        .filter(|candidate| {
            candidate.len() >= 3
                && candidate
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                && candidate.chars().any(|c| c.is_ascii_alphabetic())
        })
        .map(str::to_string)
        .collect()
}

/// Everything an agent is handed: the contract, the working knowledge, the
/// commands that drive a change, and the reviewer's criteria. All of them name
/// APIs, and all of them are followed.
fn agent_documentation() -> Vec<PathBuf> {
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
    docs
}

/// Everything a user is handed: the developer reference under `docs/`, the
/// book published to the project site, and the README.
fn user_documentation() -> Vec<PathBuf> {
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
    docs
}

/// Everything an agent is handed: the contract, the working knowledge, the
/// commands, the reviewer's criteria.
#[test]
fn every_identifier_the_agent_documentation_names_still_exists() {
    let source = crate_source();
    let docs = agent_documentation();

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
    let docs = user_documentation();

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

    let mut wrong = Vec::new();
    let mut checked = 0usize;
    for (path, source) in book_pages() {
        let name = path.strip_prefix(repo()).unwrap_or(&path).display();
        for (line, info, _) in fences(&source) {
            checked += 1;
            if ALLOWED.contains(&info) || ALSO.contains(&info) {
                continue;
            }
            wrong.push(format!("  {name}:{line}: ```{info}"));
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

/// What an `ignore`d sample's first line has to start with, in the book as in
/// `src/`. The book hides it behind mdbook's `#`, so the reason reaches the
/// next author without reaching the reader, for whom it is noise.
const REASON: &str = "// not compiled:";

/// Every chapter of the book, as (path, source).
fn book_pages() -> Vec<(PathBuf, String)> {
    let mut pages = Vec::new();
    read_dir_files(&repo().join("book/src"), "md", &mut pages);
    assert!(
        !pages.is_empty(),
        "no book chapters found: scanning nothing"
    );
    pages.sort();
    pages
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("unreadable chapter");
            (path, source)
        })
        .collect()
}

/// The fences in one chapter: line number, info string, and the body's lines.
fn fences(source: &str) -> Vec<(usize, &str, Vec<&str>)> {
    parts(source)
        .into_iter()
        .filter_map(|part| match part {
            Part::Fence {
                line, info, body, ..
            } => Some((line, info, body)),
            Part::Prose(_) => None,
        })
        .collect()
}

/// A fence rustdoc will not compile.
fn is_ignored(info: &str) -> bool {
    info.split(',').any(|token| token.trim() == "ignore")
}

/// Every `ignore`d sample in the book says why, where the next author will see it.
///
/// `mdbook test` compiles the book, which is what makes a rename in the library
/// show up as a red chapter — and an `ignore` is exempt from it. That is a real
/// need: a fragment of a trait implementation, or a line that is deliberately
/// wrong because the chapter is about why it is wrong, cannot be made to
/// compile without teaching something false. What it must not be is the default
/// for a block nobody got round to, which is what it had become: #294 made the
/// book compile and left a third of it `ignore`d, including fifteen of the
/// eighteen blocks in the chapter that teaches `#[component]` — beside each of
/// which sat a *hidden* stub the harness checked instead of the definition the
/// reader was shown.
///
/// So an `ignore` costs a sentence now. `architecture/` is exempt as a chapter:
/// its blocks describe guido's internals — `widget.paint(tree, ctx)`,
/// `sdf_rounded_rect` — and inventing a plausible `ctx` would teach something
/// false rather than prove something true.
#[test]
fn every_ignored_sample_in_the_book_says_why_not() {
    let mut unexplained = Vec::new();
    let (mut exempt, mut by_chapter) = (0usize, 0usize);

    for (path, source) in book_pages() {
        let name = path.strip_prefix(repo()).unwrap_or(&path).display();
        let internals = path.components().any(|c| c.as_os_str() == "architecture");
        for (line, info, body) in fences(&source) {
            if !is_ignored(info) {
                continue;
            }
            if internals {
                by_chapter += 1;
                continue;
            }
            let first = body.first().copied().unwrap_or_default().trim_start();
            // The reason is hidden from the rendered page with mdbook's `#`,
            // which `book.toml` sets as the hide-lines marker for Rust.
            let first = first.strip_prefix("# ").unwrap_or(first);
            if first.starts_with(REASON) {
                exempt += 1;
            } else {
                unexplained.push(format!("  {name}:{line}"));
            }
        }
    }

    assert!(
        unexplained.is_empty(),
        "{} sample(s) in the book are marked `ignore`, so `mdbook test` does not \
         compile them and nothing else checks the names inside them. Either drop \
         the `ignore` — adding whatever hidden `#` setup lines it takes, or \
         `no_run` where the sample compiles but must not open a surface — or \
         state the reason on the block's first line as `# {REASON} ...`.\n{}\n\n\
         ({exempt} exempt with a reason, {by_chapter} in architecture/)",
        unexplained.len(),
        unexplained.join("\n")
    );
}

/// The share of the book that is `ignore`d is the share the documentation quotes.
///
/// `AGENTS.md` and the `book` skill both tell their reader how much of the book
/// the compiler is watching, and that reader decides on the strength of it
/// whether to grep for a renamed spelling by hand. A number that drifts is worse
/// than no number: it is trusted. So it is recounted here rather than
/// remembered, out of the same fences the tests above walk.
#[test]
fn the_ignored_share_the_documentation_quotes_is_the_real_one() {
    let (mut rust, mut ignored) = (0usize, 0usize);
    for (_, source) in book_pages() {
        for (_, info, _) in fences(&source) {
            if !info.starts_with("rust") {
                continue;
            }
            rust += 1;
            if is_ignored(info) {
                ignored += 1;
            }
        }
    }
    assert!(rust > 0, "no Rust fences in the book: counting nothing");
    let share = (ignored * 100 + rust / 2) / rust;

    for doc in [
        repo().join("AGENTS.md"),
        repo().join(".claude/skills/book/SKILL.md"),
    ] {
        let source = std::fs::read_to_string(&doc).expect("unreadable documentation");
        let name = doc.strip_prefix(repo()).unwrap_or(&doc).display();
        let quoted = quoted_ignored_shares(&source);
        assert!(
            !quoted.is_empty(),
            "{name} no longer says what share of the book is `ignore`d. It is \
             {share}% ({ignored} of {rust} Rust blocks), and the reader of that \
             file decides whether to grep by hand on the strength of it."
        );
        for (line, percent) in quoted {
            assert_eq!(
                percent, share,
                "{name}:{line} says {percent}% of the book is `ignore`d. It is \
                 {share}% — {ignored} of {rust} Rust blocks — as of this run."
            );
        }
    }
}

/// Every percentage a document writes just before the word `ignore`.
///
/// Matched loosely on purpose: the sentence around the number is free to be
/// rewritten, and the number stays checked. Loosely, and per *paragraph* — this
/// repository hard-wraps its prose at eighty columns, so where the line break
/// falls is a fact about the wrap and not about the sentence, and a scan that
/// read one line at a time would report the number missing the first time
/// somebody reflowed the paragraph it sits in.
fn quoted_ignored_shares(source: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut line = 1usize;
    for paragraph in source.split("\n\n") {
        let first_line = line;
        // Two for the blank line this split consumed. A wider gap leaves its
        // extra newlines at the head of the *next* chunk, where they are
        // counted there, so the tally holds however widely the prose is spaced.
        line += paragraph.matches('\n').count() + 2;
        let flowed = paragraph.replace('\n', " ");
        let mut rest = flowed.as_str();
        while let Some(at) = rest.find('%') {
            let start = rest[..at]
                .rfind(|c: char| !c.is_ascii_digit())
                .map_or(0, |end| end + 1);
            let near: String = rest[at..].chars().take(40).collect();
            if let Ok(percent) = rest[start..at].parse::<usize>()
                && near.contains("`ignore`")
            {
                found.push((first_line, percent));
            }
            rest = &rest[at + 1..];
        }
    }
    found
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
            if !is_ignored(info) {
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
