//! The rule that decides whether a run may rewrite a reference.
//!
//! `CLAUDE.md` calls it non-negotiable, and it was prose: both harnesses wrote
//! on `UPDATE_*` whatever was already on disk, and neither read `REBLESS_*` at
//! all. The rule is a pure function of three booleans now, so the table it is
//! supposed to satisfy can be written down.

mod common;

use common::{Blessing, blessing};

/// Making a reference that does not exist yet is ordinary work — a new
/// scenario needs a first picture — and `UPDATE_*` is what makes it.
#[test]
fn update_creates_a_reference_that_is_not_there() {
    assert_eq!(blessing(true, false, false), Blessing::Write);
}

/// The line the harnesses were missing. Pointed at a reference that already
/// exists, `UPDATE_*` declines and lets the comparison run, because rewriting
/// one turns a failing test green without changing anything back.
#[test]
fn update_declines_a_reference_that_already_exists() {
    assert_eq!(blessing(true, false, true), Blessing::Compare);
}

/// Rewriting is what `REBLESS_*` is for, and the only thing it is for.
#[test]
fn rebless_rewrites_whatever_is_there() {
    assert_eq!(blessing(false, true, true), Blessing::Write);
    assert_eq!(blessing(false, true, false), Blessing::Write);
}

/// Asking for both is asking for the stronger of the two. Nobody should write
/// it, and it should not mean "decline".
#[test]
fn rebless_wins_over_update() {
    assert_eq!(blessing(true, true, true), Blessing::Write);
    assert_eq!(blessing(true, true, false), Blessing::Write);
}

/// An ordinary run compares, and never writes, whatever is on disk.
#[test]
fn an_ordinary_run_only_compares() {
    assert_eq!(blessing(false, false, true), Blessing::Compare);
    assert_eq!(blessing(false, false, false), Blessing::Compare);
}

/// The rule decides and `write_if_blessed` acts, and the two have to agree
/// about a real file. Deciding correctly and writing anyway is the defect this
/// whole change is about, and it would read as compliant at both call sites.
#[test]
fn a_declined_reference_is_left_exactly_as_it_was() {
    let path = scratch("declined.snap");
    std::fs::write(&path, "the reference that was already here").unwrap();

    let wrote = common::write_if_blessed(&path, Blessing::Compare, || {
        std::fs::write(&path, "the rewrite").unwrap();
    });

    assert!(!wrote, "write_if_blessed claimed it wrote under Compare");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "the reference that was already here",
        "a declined bless rewrote the reference anyway"
    );
    std::fs::remove_file(&path).ok();
}

/// And the other half: a reference the rule allows really is written, so
/// `Compare` cannot be made unconditionally safe by never writing at all.
#[test]
fn a_blessed_reference_is_written() {
    let path = scratch("blessed.snap");
    std::fs::remove_file(&path).ok();

    let wrote = common::write_if_blessed(&path, Blessing::Write, || {
        std::fs::write(&path, "the first picture").unwrap();
    });

    assert!(
        wrote,
        "write_if_blessed claimed it did not write under Write"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "the first picture");
    std::fs::remove_file(&path).ok();
}

/// Under `target/`, not the system temporary directory: these are a few bytes
/// and they belong with the build output rather than in whatever `/tmp` is
/// mounted on.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("blessing-rules");
    std::fs::create_dir_all(&dir).expect("scratch directory");
    dir.join(name)
}

// ---------------------------------------------------------------------------
// That the rule is actually wired, in the manner of `tests/agent_workflow.rs`:
// a rule stated in one file and applied in another is a rule until somebody
// edits one of them.
// ---------------------------------------------------------------------------

fn read(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {relative}: {e}"))
}

const HARNESSES: [&str; 2] = ["tests/render_snapshots.rs", "tests/golden_images.rs"];

/// Both harnesses ask the shared rule rather than deciding for themselves.
#[test]
fn both_harnesses_consult_the_shared_rule() {
    for harness in HARNESSES {
        assert!(
            read(harness).contains("blessing_from_env"),
            "{harness} decides on its own whether it may write a reference. \
             The rule is `common::blessing`, and a harness that does not ask it \
             is where the last two copies of the rule drifted apart from it."
        );
    }
}

/// And neither keeps a copy of it. Both used to read `UPDATE_*` directly and
/// write whatever was already on disk, while their own module headers said
/// they declined — two copies of one rule, of which only the prose was right.
#[test]
fn no_harness_reads_the_blessing_variables_itself() {
    for harness in HARNESSES {
        let source = read(harness);
        for spelling in [
            "var_os(\"UPDATE",
            "var(\"UPDATE",
            "var_os(\"REBLESS",
            "var(\"REBLESS",
        ] {
            assert!(
                !source.contains(spelling),
                "{harness} reads a blessing variable itself ({spelling}…). \
                 One reader, `common::blessing_from_env`, or the rule has two \
                 homes again."
            );
        }
    }
}

/// The hook guards the half that rewrites. It is the last thing standing
/// between an agent and a reference nobody decided to change, so the pair of
/// names it denies has to stay the pair that writes.
#[test]
fn the_hook_denies_the_variables_that_rewrite() {
    let hook = read(".claude/hooks/guard-bash.sh");
    for variable in ["REBLESS_GOLDEN", "REBLESS_SNAPSHOTS"] {
        assert!(
            hook.contains(variable),
            ".claude/hooks/guard-bash.sh stopped denying {variable}, which is \
             now the only spelling that can rewrite a blessed reference."
        );
    }
}
