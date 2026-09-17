//! Every piece of per-thread state in the crate is listed under **Ambient
//! state** in `docs/ARCHITECTURE.md`, with the reason nothing explicit carries
//! it — see that section for why the list exists.
//!
//! Two declarations count: a `static` inside `thread_local!`, and a
//! `static _: GlobalSignal<_>`, which is a thread's signal reached through a
//! `static` and outlives the `App` the same way. Counting only the first would
//! make the second the way to add state without a row.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

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

/// The name right after `static `, or `None` when the line declares no static.
fn static_name(line: &str) -> Option<String> {
    let after = line.split("static ").nth(1)?;
    let name: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    assert!(
        !name.is_empty(),
        "a static the scan cannot name, from a macro or an unusual spelling: `{}`",
        line.trim()
    );
    Some(name)
}

/// The cells `text` declares, outside the body of any inline `#[cfg(test)]`
/// module: a `static` inside `thread_local!`, in either spelling of the macro,
/// and a `static` whose type is a `GlobalSignal`, by whatever path.
///
/// Lines rather than a parser, relying on rustfmt: a block closes with a `}` at
/// the indentation its opening line had.
fn cells_in(text: &str) -> Vec<String> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut cells = Vec::new();
    // The indentation a `thread_local!` block or a test module opened at.
    let mut in_cells: Option<usize> = None;
    let mut in_test: Option<usize> = None;
    let mut after_cfg_test = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(open) = in_test {
            if trimmed == "}" && indent(line) == open {
                in_test = None;
            }
            continue;
        }
        if after_cfg_test && trimmed.starts_with("mod ") && trimmed.ends_with('{') {
            in_test = Some(indent(line));
            after_cfg_test = false;
            continue;
        }
        after_cfg_test = trimmed == "#[cfg(test)]";

        if in_cells.is_some_and(|open| trimmed.starts_with('}') && indent(line) == open) {
            in_cells = None;
            continue;
        }
        if trimmed.contains("thread_local!") && trimmed.ends_with('{') {
            in_cells = Some(indent(line));
        }
        let is_cell = in_cells.is_some()
            || trimmed.contains("thread_local!")
            || trimmed.split_once(':').is_some_and(|(_, ty)| {
                ty.trim_start()
                    .split('<')
                    .next()
                    .is_some_and(|path| path.ends_with("GlobalSignal"))
            });
        if is_cell && let Some(name) = static_name(line) {
            cells.push(name);
        }
    }
    cells
}

/// `(file, name)` of every cell declared under `src/`.
///
/// Keyed by file as well as name: `PENDING`, `REQUEST` and `STATE` say little
/// on their own, and two modules may use the same one.
fn declared_cells() -> BTreeSet<(String, String)> {
    let mut files = Vec::new();
    rust_files(&repo().join("src"), &mut files);

    let mut cells = BTreeSet::new();
    for path in files {
        let text = std::fs::read_to_string(&path).expect("source is readable");
        let file = path
            .strip_prefix(repo())
            .expect("under the repository")
            .display()
            .to_string();
        cells.extend(cells_in(&text).into_iter().map(|name| (file.clone(), name)));
    }
    cells
}

/// `(file, name) -> reason` for every row of the table under **Ambient state**.
fn listed_cells() -> BTreeMap<(String, String), String> {
    let doc = std::fs::read_to_string(repo().join("docs/ARCHITECTURE.md"))
        .expect("docs/ARCHITECTURE.md is readable");
    let section = doc
        .split_once("\n## Ambient state\n")
        .map(|(_, after)| after)
        .expect("docs/ARCHITECTURE.md has an `## Ambient state` section");
    let section = section.split("\n## ").next().unwrap_or(section);

    section
        .lines()
        .filter_map(|line| {
            let columns: Vec<&str> = line.split('|').map(str::trim).collect();
            // `| `NAME` | `src/file.rs` | reason |` splits into five, the outer two empty.
            (columns.len() == 5 && columns[1].starts_with('`')).then(|| {
                let key = (
                    columns[2].trim_matches('`').to_string(),
                    columns[1].trim_matches('`').to_string(),
                );
                (key, columns[3].to_string())
            })
        })
        .collect()
}

#[test]
fn the_ambient_state_table_lists_every_cell_and_only_those() {
    let declared = declared_cells();
    assert!(
        declared.len() > 10,
        "found only {} cells: the scan is broken, not the crate",
        declared.len()
    );
    let listed = listed_cells();

    let unlisted: Vec<_> = declared
        .iter()
        .filter(|key| listed.get(*key).is_none_or(String::is_empty))
        .map(|(file, name)| {
            format!("  | `{name}` | `{file}` | <why nothing explicit carries it> |")
        })
        .collect();
    let stale: Vec<_> = listed
        .keys()
        .filter(|key| !declared.contains(*key))
        .map(|(file, name)| format!("  `{name}` in `{file}`"))
        .collect();

    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "docs/ARCHITECTURE.md, `## Ambient state`:\n\
         {} cell(s) without a row and a reason — say why neither the `Tree`, a pass \
         nor an existing struct could carry each:\n{}\n\
         {} row(s) naming a cell that is no longer declared there:\n{}",
        unlisted.len(),
        unlisted.join("\n"),
        stale.len(),
        stale.join("\n")
    );
}

/// The scan itself, on the spellings it has to see and the one it must not.
///
/// A scan that misses a cell passes silently, which is the one failure the test
/// above cannot report — so the misses are asked about here directly.
#[test]
fn the_scan_sees_every_spelling_and_skips_only_test_bodies() {
    let text = r#"
#[cfg(test)]
mod characterization;

thread_local! {
    static AFTER_A_TEST_MOD_DECLARATION: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
mod tests {
    thread_local! {
        static INSIDE_A_TEST: Cell<bool> = const { Cell::new(false) };
    }
    static TEST_GLOBAL: GlobalSignal<u32> = GlobalSignal::new(|| 0);
}

std::thread_local!(static ONE_LINE: Cell<u8> = const { Cell::new(0) });

mod imp {
    thread_local! {
        /// Documented.
        pub(super) static NESTED: Cell<u32> = const { Cell::new(0) };
    }
    static NOT_A_CELL: &str = "after the block closed";
}

static GLOBAL: GlobalSignal<u32> = GlobalSignal::new(|| 0);
static BY_PATH: crate::reactive::GlobalSignal<u32> = crate::reactive::GlobalSignal::new(|| 0);
static PLAIN: u32 = 0;
"#;
    assert_eq!(
        cells_in(text),
        [
            "AFTER_A_TEST_MOD_DECLARATION",
            "ONE_LINE",
            "NESTED",
            "GLOBAL",
            "BY_PATH"
        ]
    );
}
