//! Every piece of ambient state in the crate is listed under **Ambient state**
//! in `docs/ARCHITECTURE.md`, with the reason nothing explicit carries it — see
//! that section for why the list exists.
//!
//! Three declarations count, and each is a way to add state without a row if
//! only the others are counted:
//!
//! - a `static` inside `thread_local!`, which is the thread's own cell;
//! - a `static _: GlobalSignal<_>`, which is a thread's signal reached through
//!   a `static` and outlives the `App` the same way;
//! - a module-scope `static` behind a lock or an atomic — `Mutex`, `RwLock`,
//!   `OnceLock`, `LazyLock`, `Atomic*` — which is the crate's answer to the
//!   half of the problem a thread's cell cannot hold. `WriteSignal` is `Send`,
//!   so a background task has to put its writes somewhere, and what it puts
//!   them in is the state with the *longest* life here, not the shortest.
//!
//! What that third kind leaves out, and why, is in that section. The
//! asymmetry is here: the thread's two forms are counted wherever they are
//! written, function bodies included, because for them a row too many costs a
//! sentence and a scan taught to look in fewer places is a place to hide one.
//! Nobody has written either inside a function, so the two rules agree on this
//! crate and only one of them ever had to be decided.

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

/// Whether the line opens a `static` item rather than merely spelling the
/// keyword. `&'static str` in a signature is what would otherwise be read as a
/// declaration, and a signature mentioning a `Mutex` return type would then be
/// a cell named `str`.
fn declares_static(trimmed: &str) -> bool {
    let after_visibility = trimmed.strip_prefix("pub").map_or(trimmed, |rest| {
        // `pub(crate)`, `pub(super)`, `pub(in path)`.
        rest.strip_prefix('(')
            .and_then(|restricted| restricted.split_once(')'))
            .map_or(rest, |(_, after)| after)
    });
    after_visibility.trim_start().starts_with("static ")
}

/// The type a `static` is declared as, without its path or its generic
/// arguments: `Mutex` for `static Q: std::sync::Mutex<Vec<Write>> = ..`.
///
/// The type ends at the `=` that opens the initializer, which no type can
/// contain — a declaration split across lines by rustfmt keeps that `=` on the
/// first one.
fn static_type(trimmed: &str) -> Option<&str> {
    let declared = trimmed.split_once(':')?.1.split('=').next()?;
    let head = declared.split('<').next()?.trim();
    head.rsplit("::").next()
}

/// Whether the type carries interior mutability, which is what makes a
/// `static` state rather than a constant written the long way.
fn is_shared_cell(ty: &str) -> bool {
    ty.starts_with("Atomic") || matches!(ty, "Mutex" | "RwLock" | "OnceLock" | "LazyLock")
}

/// The cells `text` declares, outside the body of any inline `#[cfg(test)]`
/// module: a `static` inside `thread_local!`, in either spelling of the macro;
/// a `static` whose type is a `GlobalSignal`, by whatever path; and a `static`
/// behind a lock or an atomic at module scope.
///
/// Lines rather than a parser, relying on rustfmt: a block closes with a `}` at
/// the indentation its opening line had.
fn cells_in(text: &str) -> Vec<String> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    let mut cells = Vec::new();
    // The indentation a `thread_local!` block, a test module or a function
    // body opened at.
    let mut in_cells: Option<usize> = None;
    let mut in_test: Option<usize> = None;
    let mut in_fn: Option<usize> = None;
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

        // Function bodies are tracked rather than skipped, because only the
        // process-wide form is bounded to module scope. Opening on the brace
        // rather than on the `fn` keeps a signature rustfmt broke over several
        // lines, and a bodiless one in a trait, from swallowing the rest of
        // the file — the cost is a function-local `static` reported as
        // ambient, which is loud, where the reverse is silent.
        if in_fn.is_some_and(|open| trimmed.starts_with('}') && indent(line) == open) {
            in_fn = None;
        } else if in_fn.is_none()
            && trimmed.ends_with('{')
            && trimmed.split_whitespace().any(|word| word == "fn")
        {
            in_fn = Some(indent(line));
        }

        if in_cells.is_some_and(|open| trimmed.starts_with('}') && indent(line) == open) {
            in_cells = None;
            continue;
        }
        if trimmed.contains("thread_local!") && trimmed.ends_with('{') {
            in_cells = Some(indent(line));
        }
        let declared_type = declares_static(trimmed)
            .then(|| static_type(trimmed))
            .flatten();
        let is_cell = in_cells.is_some()
            || trimmed.contains("thread_local!")
            || declared_type == Some("GlobalSignal");
        let is_process_wide =
            in_cells.is_none() && in_fn.is_none() && declared_type.is_some_and(is_shared_cell);
        if (is_cell || is_process_wide)
            && let Some(name) = static_name(line)
        {
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
fn the_scan_sees_every_spelling_and_the_scope_each_one_needs() {
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
    pub(super) static NESTED_QUEUE: Mutex<Vec<u8>> = Mutex::new(Vec::new());
}

static GLOBAL: GlobalSignal<u32> = GlobalSignal::new(|| 0);
static BY_PATH: crate::reactive::GlobalSignal<u32> = crate::reactive::GlobalSignal::new(|| 0);
static PLAIN: u32 = 0;

static EPOCH: AtomicU64 = AtomicU64::new(0);
static QUEUE: Mutex<Vec<Write>> = Mutex::new(Vec::new());
static BY_FULL_PATH: std::sync::RwLock<Table> = std::sync::RwLock::new(Table::new());
static SPLIT_OVER_LINES: LazyLock<RwLock<FamilyTable>> =
    LazyLock::new(|| RwLock::new(FamilyTable::default()));
static ONCE: OnceLock<Handle> = OnceLock::new();
const NOT_A_STATIC: AtomicBool = AtomicBool::new(false);

impl SurfaceId {
    pub fn next(&self, name: &'static str) -> Mutex<u8> {
        static FUNCTION_LOCAL: AtomicU64 = AtomicU64::new(0);
        Mutex::new(0)
    }
}

static AFTER_A_FUNCTION_BODY: AtomicBool = AtomicBool::new(false);
"#;
    assert_eq!(
        cells_in(text),
        [
            "AFTER_A_TEST_MOD_DECLARATION",
            "ONE_LINE",
            "NESTED",
            "NESTED_QUEUE",
            "GLOBAL",
            "BY_PATH",
            "EPOCH",
            "QUEUE",
            "BY_FULL_PATH",
            "SPLIT_OVER_LINES",
            "ONCE",
            "AFTER_A_FUNCTION_BODY",
        ]
    );
}

/// The `APP` and `REACTIVE` rows stand for thirty-four values between them, so
/// what keeps those two structs honest is not the row — it is that each
/// `reset` names every field. Names: that a bound field is also *given back*
/// is the compiler's, through the unused-binding warning `-D warnings` makes
/// an error. Two claims, and this is what holds them: that
/// each field says why it is ambient, which the table used to ask of each cell
/// separately, and that the reset's pattern is exhaustive.
///
/// The compiler already refuses a field the pattern does not mention, and
/// `-D warnings` refuses one it mentions and does not use. What it offers
/// instead, in the same error, is `..` — and taking that suggestion reopens
/// #372 silently, with every test still green. So the pattern is read here as
/// text.
mod the_two_structs {
    use std::path::Path;

    /// Each struct a row of the table stands for: its file, its name, and how
    /// many fields it has at least. The count is the scan's own canary — a
    /// parser that stopped finding fields would otherwise pass this file by
    /// comparing two truncated lists.
    const STRUCTS: [(&str, &str, usize); 2] = [
        ("src/app_state.rs", "AppState", 22),
        ("src/reactive/state.rs", "ReactiveState", 12),
    ];

    fn source(file: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
            .unwrap_or_else(|_| panic!("{file} is readable"))
    }

    /// Everything between `struct <name> {` and its closing brace.
    fn body<'a>(source: &'a str, name: &str) -> &'a str {
        source
            .split_once(&format!("pub(crate) struct {name} {{"))
            .unwrap_or_else(|| panic!("the struct `{name}` is declared there"))
            .1
            .split("\n}")
            .next()
            .expect("the struct closes")
    }

    /// The names it declares.
    fn fields(body: &str) -> Vec<String> {
        body.lines()
            .filter_map(|line| {
                let field = line.trim().strip_prefix("pub(crate) ")?;
                Some(field.split(':').next()?.to_string())
            })
            .collect()
    }

    /// The names bound by that module's `let <name> { .. } = ..;`.
    fn bound_by_reset(source: &str, name: &str) -> Vec<String> {
        let pattern = source
            .split_once(&format!("let {name} {{"))
            .unwrap_or_else(|| panic!("`reset` destructures `{name}`"))
            .1
            .split_once('}')
            .expect("and the pattern closes")
            .0;
        assert!(
            !pattern.contains("..") && !pattern.contains(": _"),
            "`{name}`'s reset binds its fields with `..` or `: _`:\n{pattern}\n\
             Both are what the compiler suggests when a field is missing, and both \
             let the next field be forgotten — which is #372, again. Name every one."
        );
        pattern
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(|line| line.trim_end_matches(',').to_string())
            .collect()
    }

    #[test]
    fn every_field_is_named_by_the_reset() {
        for (file, name, at_least) in STRUCTS {
            let source = source(file);
            let fields = fields(body(&source, name));
            assert!(
                fields.len() >= at_least,
                "found only {} of `{name}`'s {at_least} fields: the scan is broken, \
                 not the struct",
                fields.len()
            );
            assert_eq!(
                bound_by_reset(&source, name),
                fields,
                "`{file}`'s reset does not name the fields of `{name}`, in order"
            );
        }
    }

    #[test]
    fn every_field_says_why_nothing_explicit_carries_it() {
        for (file, name, _) in STRUCTS {
            let source = source(file);
            let mut undocumented = Vec::new();
            let mut documented = false;
            for line in body(&source, name).lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("//") && !line.starts_with("///") {
                    continue;
                }
                if line.starts_with("///") {
                    documented = true;
                    continue;
                }
                if let Some(field) = line.strip_prefix("pub(crate) ")
                    && !documented
                {
                    undocumented.push(field.split(':').next().unwrap_or(field).to_string());
                }
                documented = false;
            }

            assert!(
                undocumented.is_empty(),
                "fields of `{name}` with no line saying why nothing explicit carries \
                 them: {undocumented:?}. The **Ambient state** table asks that of every \
                 cell, and one row stands for all of these"
            );
        }
    }
}
