//! Every map and set the crate keeps is keyed by `rustc_hash`, not by std's
//! default hasher.
//!
//! `HashMap` and `HashSet` spelled bare are SipHash ones. SipHash is keyed and
//! randomly seeded because the standard library has to assume the keys came
//! from somewhere hostile. Most of these did not: `WidgetId`, `SignalId`,
//! `SurfaceId`, `OutputId`, an `ObjectId`, a font hash, a finger id — small
//! dense integers minted in this process, hashed on the frame path, several of
//! them per widget per frame.
//!
//! Three are not ids, and they are the three a reader of the renderer meets
//! first: `MaskKey` in `text_mask.rs` and `TextCacheKey` in `text_quad.rs` each
//! carry the `String` that was drawn, and `text_buffer_key` in `text.rs` hashes
//! one into the `u64` its buffer cache is keyed by. Displayed text really does
//! come from outside the process — a window title, a notification body, the
//! track a player reports — so for these three the hostile-keys argument is not
//! hypothetical, and the answer is not that it cannot happen. The answer is
//! what it would buy. Each of the three caches holds one frame's worth of text
//! and is swept by the frame after it, so it is tens of entries wide and cannot
//! be grown; a collision costs a walk over a handful of keys and a `String`
//! comparison, on a cache probed once per text per frame. There is no
//! degradation to arrange, and the frame path is exactly where the cheaper hash
//! pays.
//!
//! `text_buffer_key`'s `u64` was never unpredictable anyway: it comes from
//! `DefaultHasher::new()`, which is SipHash-1-3 with a *fixed* seed rather than
//! `RandomState`. All the bare `HashMap` around it added was an unpredictable
//! bucket for a predictable key, and a second SipHash over the result of the
//! first.
//!
//! The crate moved to `FxHashMap` once already and the move was done by hand,
//! so it was done for the maps somebody happened to be looking at. The rest
//! stayed on SipHash for as long as it took for anyone to notice, which was
//! months, because nothing about a bare `HashMap` looks wrong — it is what the
//! standard library hands you and what every example writes. That is the whole
//! reason this file exists: the rule cannot be kept by remembering it.
//!
//! So the rule is read out of the source instead. A bare `HashMap` or
//! `HashSet` under `src/` is either gone or on [`NOT_FX`] with the reason it
//! stays, and a new one fails this test the moment it is written.
//!
//! It is the identifier that is looked for, not `HashMap<K, V>` with its
//! parameters: `let mut seen = HashSet::new();` declares a SipHash set without
//! writing a single type parameter, and an import left behind after the last
//! map in a file has gone is the invitation for the next one.

use std::path::{Path, PathBuf};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Bare `HashMap`s and `HashSet`s that stay, and why.
///
/// An entry is `(file, snippet, reason)`: the file it is in, enough of the line
/// to say which one it is, and what makes SipHash the right hasher there. The
/// snippet rather than a line number, because a line number is wrong by the
/// next commit and says nothing about which map it meant.
///
/// Nothing is on it today, and an entry is not a formality: it says the type is
/// somebody else's to choose, or that keys from outside the process can
/// *accumulate* here — a map nothing sweeps, holding what it is given for as
/// long as it is given it, which is the shape SipHash answers and Fx does not.
/// Text-derived keys alone are not that shape; the three caches that have them
/// are emptied every frame, and the module doc above says what a collision
/// costs there. A map that is merely not on a hot path does not need an entry
/// either: Fx costs nothing there, and "it is not hot" is an argument the next
/// reader has to re-derive.
const NOT_FX: &[(&str, &str, &str)] = &[];

/// Rust files under `src/`.
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

/// Whether the byte can be part of an identifier, which is what tells
/// `FxHashMap` from `HashMap`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Where `name` appears in `line` as a whole identifier, so that `FxHashMap`
/// and `HashMapExt` are not `HashMap`.
fn names_the_type(line: &str, name: &str) -> bool {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(offset) = line[from..].find(name) {
        let start = from + offset;
        let end = start + name.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// What the line says once the comment on it is taken off.
///
/// Comments name these types constantly — `jobs.rs` explains its dedup in terms
/// of a `HashSet` it no longer uses — and a rule that fired on prose would be
/// turned off within the week. A `//` inside a string literal is cut too, which
/// can only hide a hit, and the one string in `src/` with a `//` in it is an
/// SVG namespace.
fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// The bare declarations a source file makes, as `(line number, the line)`.
fn hits_in(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = code(line);
            names_the_type(code, "HashMap") || names_the_type(code, "HashSet")
        })
        .map(|(index, line)| (index + 1, line.trim().to_string()))
        .collect()
}

/// `(file, line number, the line)` for every bare declaration under `src/`,
/// and how many `Fx` ones were passed over on the way.
///
/// The second number is this scan's canary. A walk that read nothing, or a
/// boundary check that stopped matching, would report an empty list of hits —
/// which is also what a clean crate looks like.
fn scan() -> (Vec<(String, usize, String)>, usize) {
    let mut files = Vec::new();
    rust_files(&repo().join("src"), &mut files);
    assert!(
        files.len() > 20,
        "found only {} source files under src/: the walk is broken, not the crate",
        files.len()
    );

    let mut hits = Vec::new();
    let mut fx = 0;
    for path in files {
        let text = std::fs::read_to_string(&path).expect("source is readable");
        let file = path
            .strip_prefix(repo())
            .expect("under the repository")
            .display()
            .to_string();
        fx += text
            .lines()
            .filter(|line| {
                let code = code(line);
                names_the_type(code, "FxHashMap") || names_the_type(code, "FxHashSet")
            })
            .count();
        hits.extend(
            hits_in(&text)
                .into_iter()
                .map(|(number, line)| (file.clone(), number, line)),
        );
    }
    (hits, fx)
}

/// The rule itself.
#[test]
fn every_map_in_the_crate_is_an_fx_one_or_says_why_it_is_not() {
    let (hits, fx) = scan();
    assert!(
        fx > 20,
        "only {fx} `Fx` map(s) seen across src/: this scan is no longer reading the \
         declarations it exists to read, and would pass a crate full of SipHash"
    );

    let unexcused: Vec<String> = hits
        .iter()
        .filter(|(file, _, line)| {
            !NOT_FX
                .iter()
                .any(|(excused, snippet, _)| excused == file && line.contains(snippet))
        })
        .map(|(file, number, line)| format!("  {file}:{number}: {line}"))
        .collect();

    assert!(
        unexcused.is_empty(),
        "{} bare `HashMap`/`HashSet` declaration(s) under src/, keyed by SipHash:\n{}\n\
         Every key in this crate is one we mint ourselves, so use `rustc_hash::FxHashMap` \
         or `FxHashSet` — or add the line to NOT_FX in this file with the reason SipHash \
         is the right hasher there.",
        unexcused.len(),
        unexcused.join("\n")
    );
}

/// The opt-out list goes stale silently: a map that is converted leaves its
/// excuse behind, and the excuse then covers whatever is written on that line
/// next.
#[test]
fn the_opt_out_list_excuses_nothing_that_is_gone() {
    let (hits, _) = scan();
    for (file, snippet, _) in NOT_FX {
        assert!(
            hits.iter()
                .any(|(hit, _, line)| hit == file && line.contains(snippet)),
            "NOT_FX excuses `{snippet}` in `{file}`, where there is no bare \
             `HashMap`/`HashSet` line saying that any more"
        );
    }
}

/// The scan is where this file can fail quietly: a spelling it stops seeing is
/// a SipHash map nothing objects to, and the test above still passes. So the
/// spellings it has to see, and the ones it must not, are asked about here
/// directly.
#[test]
fn the_scan_sees_every_spelling_and_reads_no_comment() {
    let text = r#"
use std::collections::HashMap;
use rustc_hash::{FxHashMap, FxHashSet};

/// Doc comments name a `HashMap` to explain what the code does instead.
//! And so do module docs, about a HashSet.
// Ordinary ones too.
pub struct Caches {
    damage: std::collections::HashMap<WidgetId, DamageRegion>,
    masks: HashMap<MaskKey, Rc<CachedMask>>,
    widget_refs: FxHashMap<WidgetId, WidgetRef>,
    live: rustc_hash::FxHashSet<SurfaceId>, // a HashSet, once
    ext: HashMapExtension,
}

type Pending = FxHashMap<u64, Box<dyn Widget>>;

fn build() {
    let mut seen = HashSet::new();
    let mut rows = FxHashMap::default();
}
"#;
    assert_eq!(
        hits_in(text),
        [
            (2, "use std::collections::HashMap;".to_string()),
            (
                9,
                "damage: std::collections::HashMap<WidgetId, DamageRegion>,".to_string()
            ),
            (10, "masks: HashMap<MaskKey, Rc<CachedMask>>,".to_string()),
            (19, "let mut seen = HashSet::new();".to_string()),
        ]
    );
}
