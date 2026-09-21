//! One job decides whether a pull request may merge, and it has to be waiting
//! on every job that matters.
//!
//! The `main` ruleset named five jobs by their display strings, in a repository
//! setting nothing here can see. Renaming one of them left #288 green and
//! unmergeable: the required check could no longer report, so it stayed pending
//! for ever, and `repos/.../branches/main/protection` answers `404` and reads
//! as *nothing is required*. `ci.yml` ends in one job for the ruleset to
//! require instead — `CI` — and the list of what gates a merge is that job's
//! `needs:`, where it is versioned, diffed and reviewed.
//!
//! That trade is only an improvement if forgetting the list is loud, and by
//! itself it is the quietest thing in the repository: a job left out of
//! `needs:` gates nothing, every check is green, the merge is allowed, and the
//! first sign of it is something broken on `main`. This file is what makes it
//! loud again. Every job in `ci.yml` is in the gate's `needs:` or on the
//! opt-out list below, and that list is read here rather than remembered.
//!
//! It watches the other half too: that the gate can fail at all. The `ci` job's
//! own comment has why — a skipped required check counts as a passing one — and
//! the two tests below hold it to `if: always()` and to reading what its needs
//! reported.
//!
//! What it cannot watch is the fence itself. Nothing here can see the ruleset,
//! so the gate's display name is checked against it by a step that asks GitHub
//! — `.github/required-context.sh`, run by a gating job — and what is left for
//! a test is that the step is still there.

use std::path::Path;

/// The key of the job the ruleset waits for.
const GATE: &str = "ci";

/// The script that asks GitHub what the ruleset requires and compares the
/// answer to the gate's `name:`.
///
/// The display name itself is written once, in `ci.yml`, and this file no
/// longer holds a copy: a constant here could only say that the workflow agrees
/// with the constant, and a rename that edits both lines in one commit passed
/// it. What the ruleset actually requires is knowable only from GitHub, so it
/// is read from GitHub — see #434.
const RULESET_READER: &str = ".github/required-context.sh";

fn repository(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

/// The jobs that run in `ci.yml` and deliberately do not gate a merge.
///
/// `mutants` is the whole of it, and AGENTS.md is where that was decided: *"That
/// job reports; it does not block, until somebody decides the ratchet is worth
/// the friction."* It mutates only the lines a pull request touched and answers
/// a question — if this were wrong, would anything have noticed — rather than
/// returning a verdict.
///
/// Anything added here is a job somebody has decided may fail without stopping
/// a merge. That is the decision this list exists to make visible; it is not a
/// place to put a job that is merely inconvenient.
const DOES_NOT_GATE: &[&str] = &["mutants"];

/// Every job in `ci.yml`, as a key and the lines underneath it, in file order.
///
/// A job header is a key at exactly two spaces of indentation under the
/// top-level `jobs:`, and nothing deeper is one — not a step, not a `with:`, not
/// an `env:`. `on:`, `concurrency:` and `env:` have children at that depth too,
/// so the scan starts at `jobs:` and stops at the next key in column zero.
fn jobs() -> Vec<(String, String)> {
    let path = repository(".github/workflows/ci.yml");
    let yaml =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"));
    let mut found: Vec<(String, String)> = Vec::new();
    let mut inside = false;
    for line in yaml.lines() {
        if !line.is_empty() && !line.starts_with([' ', '#']) {
            if inside {
                break;
            }
            inside = line.starts_with("jobs:");
            continue;
        }
        if !inside {
            continue;
        }
        match job_key(line) {
            Some(key) => found.push((key, String::new())),
            None => {
                if let Some((_, body)) = found.last_mut() {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }
    assert!(
        !found.is_empty(),
        "no jobs found in ci.yml — this scan is reading a shape the file no longer has"
    );
    found
}

/// The key of a job header, or `None` for a line that is not one.
///
/// A header can carry a trailing comment or a trailing space, so the line is
/// not taken whole: a scan that misses one folds the job into its
/// predecessor's body, and a job the scan never saw is in neither list and
/// reported by nothing. Anything else at a job's indentation is not something
/// this can guess at, so it says so rather than skipping it.
fn job_key(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  ")?;
    if rest.is_empty() || rest.starts_with([' ', '#']) {
        return None;
    }
    let declaration = rest.split('#').next().unwrap_or(rest).trim_end();
    let key = declaration.strip_suffix(':').unwrap_or_else(|| {
        panic!("`{line}` is indented like a job of ci.yml and is not a job header")
    });
    assert!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "`{line}` is indented like a job of ci.yml and `{key}` is not a job key"
    );
    Some(key.to_string())
}

fn body<'a>(jobs: &'a [(String, String)], key: &str) -> &'a str {
    jobs.iter()
        .find(|(name, _)| name == key)
        .map(|(_, body)| body.as_str())
        .unwrap_or_else(|| panic!("ci.yml has no job named `{key}`"))
}

/// A key written at a job's own level — `name:`, `if:`, `needs:` — and not one
/// belonging to a step or a `with:` block four spaces further in.
fn job_level<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("    {key}:");
    body.lines()
        .find(|line| line.starts_with(&prefix))
        .map(|line| line[prefix.len()..].trim())
}

/// A `needs:` list, which `ci.yml` writes inline.
fn needs(job: &str) -> Vec<String> {
    let Some(list) = job_level(job, "needs") else {
        return Vec::new();
    };
    let inner = list
        .strip_prefix('[')
        .and_then(|l| l.strip_suffix(']'))
        .unwrap_or_else(|| panic!("`needs: {list}` is not the inline list this test reads"));
    inner
        .split(',')
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// The jobs a merge waits on, which is the gate's `needs:` and nothing else.
fn gating(jobs: &[(String, String)]) -> Vec<String> {
    let list = needs(body(jobs, GATE));
    assert!(
        !list.is_empty(),
        "the `{GATE}` job has no `needs:` — it is waiting on nothing, and it is the only check the \
         ruleset waits for"
    );
    list
}

/// The scan is where this file can fail silently, and a job header is the one
/// line it has to recognise. A trailing comment or a trailing space on one used
/// to leave the job folded into its predecessor's body: absent from `needs:`,
/// absent from the opt-out list, and reported by nothing — which is precisely
/// the silence the rest of this file exists to end.
#[test]
fn a_job_header_survives_a_trailing_comment_or_space() {
    assert_eq!(job_key("  golden-guard:").as_deref(), Some("golden-guard"));
    assert_eq!(
        job_key("  golden-guard:   ").as_deref(),
        Some("golden-guard")
    );
    assert_eq!(
        job_key("  golden-guard: # the one that guards").as_deref(),
        Some("golden-guard")
    );
    assert_eq!(job_key("    name: CI"), None);
    assert_eq!(job_key("  # a comment between two jobs"), None);
    assert_eq!(job_key(""), None);
}

/// The property the whole arrangement rests on: a job in neither list has been
/// added to CI and gates nothing, and nothing else about the run looks wrong
/// when that happens.
#[test]
fn every_job_either_gates_or_is_named_as_not_gating() {
    let jobs = jobs();
    let gating = gating(&jobs);

    let ungoverned: Vec<&str> = jobs
        .iter()
        .map(|(key, _)| key.as_str())
        .filter(|key| {
            *key != GATE && !gating.iter().any(|need| need == key) && !DOES_NOT_GATE.contains(key)
        })
        .collect();

    assert!(
        ungoverned.is_empty(),
        "these jobs of ci.yml neither gate a merge nor say they do not: {ungoverned:?}\n\
         Add each to the `{GATE}` job's `needs:`, or to DOES_NOT_GATE in this file \
         with the reason it may fail without stopping a merge."
    );
}

/// The opt-out list goes stale the other way round, and silently. A `needs:`
/// naming a job that has gone fails the workflow outright and every test below
/// with it; an exemption naming one fails nothing. It is what folding `mutants`
/// into another job would leave behind — an exemption that outlives the thing
/// it exempts and quietly covers whatever is given that name later.
#[test]
fn the_opt_out_list_names_no_job_that_is_gone() {
    let jobs = jobs();
    for exempt in DOES_NOT_GATE {
        assert!(
            jobs.iter().any(|(key, _)| key == exempt),
            "DOES_NOT_GATE names `{exempt}`, which ci.yml no longer defines"
        );
    }
}

/// `if: always()` is what makes it a gate rather than a decoration. Without it
/// the job skips the moment a need fails — and GitHub reads a skipped required
/// check as a passing one, so the one check the ruleset waits for would report
/// green exactly when CI is red.
#[test]
fn the_gate_runs_even_when_a_need_has_failed() {
    let jobs = jobs();
    let condition = job_level(body(&jobs, GATE), "if").unwrap_or_else(|| {
        panic!(
            "the `{GATE}` job has no `if:`. It needs `always()`: without it the job skips when a \
             need fails, and a skipped required check counts as a passing one."
        )
    });
    assert_eq!(
        condition, "always()",
        "the `{GATE}` job runs on `{condition}`. Anything that can be false is a way for the one \
         required check to skip, and a skipped required check counts as a passing one."
    );
}

/// Running is not gating. `always()` by itself gives a job that runs after a
/// failed need and succeeds anyway, which is the fail-open shape this design is
/// accused of; the assertion is the half that refuses.
#[test]
fn the_gate_reads_what_its_needs_reported() {
    let jobs = jobs();
    let gate = body(&jobs, GATE);
    for fragment in ["toJson(needs)", "!= \"success\""] {
        assert!(
            gate.contains(fragment),
            "the `{GATE}` job does not contain `{fragment}`. It runs on `always()`, so it runs \
             after a need has failed — and unless it reads every result and demands success, it \
             reports green over a red run."
        );
    }
}

/// The gate demands `success` from every need and knows nothing about any of
/// them, which holds only while no gating job can skip for a reason of its own.
/// A job-level `if:` is one such reason — `golden-guard` carried
/// `if: github.event_name == 'pull_request'`, and nothing can tell that skip
/// from the skip of a job whose condition somebody broke — and waiting on a job
/// that is allowed to skip is the other, because a skip is inherited. So a
/// gating job runs on every event, its condition goes on its steps, and what it
/// waits on gates too.
///
/// `continue-on-error:` is the twin of both: a job that carries it keeps a
/// workflow run from failing when it fails, so it has no business being one the
/// merge waits on.
#[test]
fn a_gating_job_cannot_skip_or_excuse_its_failure() {
    let jobs = jobs();
    let gating = gating(&jobs);
    for need in &gating {
        let job = body(&jobs, need);
        assert!(
            job_level(job, "continue-on-error").is_none(),
            "the gating job `{need}` carries `continue-on-error:`, which is what a job says when \
             its failure is not supposed to stop anything. Take it off, or take the job out of the \
             gate's `needs:` and on to DOES_NOT_GATE."
        );
        assert!(
            job_level(job, "if").is_none(),
            "the gating job `{need}` has an `if:` of its own, so it can skip — and the gate cannot \
             tell a skip that was meant from one caused by a broken condition. Put the condition \
             on the job's steps, where a skipped step still leaves the job green, or take the job \
             out of the gate's `needs:` and on to DOES_NOT_GATE."
        );
        for upstream in needs(job) {
            assert!(
                gating.contains(&upstream),
                "the gating job `{need}` waits on `{upstream}`, which does not gate: a job skips \
                 when what it needs skips, so `{need}` would take the exemption with it."
            );
        }
    }
}

/// The gate reports under its display name, so it has to have one. A job with
/// no `name:` reports under its key, and the ruleset would be waiting for a
/// context nothing produces.
///
/// `required-context.sh` refuses a nameless gate too. This is the copy that
/// needs no network and no shell, so it is the one that fails on a laptop.
#[test]
fn the_gate_carries_a_name_for_the_ruleset_to_require() {
    let jobs = jobs();
    let name = job_level(body(&jobs, GATE), "name")
        .unwrap_or_else(|| panic!("the `{GATE}` job has no `name:`"));
    assert!(
        !name.is_empty(),
        "the `{GATE}` job's `name:` is empty. It is the context the ruleset requires, and a \
         required check that never reports stays pending for ever — which is #288."
    );
}

/// The half neither this file nor `ci.yml` can decide by reading itself: that
/// the name the gate reports under is the one the `main` ruleset waits for.
///
/// This used to be a constant here, and all it proved was that the workflow
/// agreed with it — a rename edited both lines in one commit and passed. The
/// ruleset is only knowable from GitHub, so a gating job asks GitHub. What is
/// left for a test is that the asking still happens: delete the step and the
/// coupling is unwatched again, with nothing red to say so.
#[test]
fn a_gating_job_reads_the_required_contexts_back_from_github() {
    let script = repository(RULESET_READER);
    let source = std::fs::read_to_string(&script).unwrap_or_else(|e| {
        panic!(
            "cannot read `{RULESET_READER}` ({e}). It is what compares the `{GATE}` job's `name:` \
             to the contexts the `main` ruleset requires, and without it that name is written on \
             one side of a fence nothing looks over."
        )
    });

    // The one string the two of them still share. It is the job's key rather
    // than its name, so a mismatch breaks `needs:` loudly as well — but the
    // script would report it as a gate with no `name:`, which names the wrong
    // fault, and this says so first.
    assert!(
        source.contains(&format!("\ngate={GATE}\n")),
        "`{RULESET_READER}` does not read the `{GATE}` job. It is looking at some other job's \
         `name:`, and comparing that to what the ruleset requires."
    );

    let jobs = jobs();
    let gating = gating(&jobs);
    assert!(
        gating
            .iter()
            .any(|need| body(&jobs, need).contains(RULESET_READER)),
        "no job in the `{GATE}` job's `needs:` runs `{RULESET_READER}`, so nothing fails when the \
         ruleset and the gate's `name:` disagree. It belongs in a job the gate waits on: the gate \
         itself cannot report a name it no longer has."
    );
}

/// `run: .github/required-context.sh` is an execution, not an interpretation: a
/// script committed without its executable bit fails the job with `Permission
/// denied` and a reader who has to work out why.
///
/// The list is read out of `ci.yml` rather than written here, so a script added
/// to a `run:` is covered by having been added.
#[test]
fn the_scripts_the_workflow_executes_are_executable() {
    use std::os::unix::fs::PermissionsExt;

    let mut found = 0;
    for (_, body) in jobs() {
        for line in body.lines() {
            let Some(script) = line.trim().strip_prefix("run: ") else {
                continue;
            };
            let script = script.trim();
            // A bare path and nothing else. A `run:` with arguments, a pipe or
            // an interpreter in front of it is somebody else's shell to read.
            if !script.starts_with('.') || script.contains(char::is_whitespace) {
                continue;
            }
            found += 1;
            let mode = std::fs::metadata(repository(script))
                .unwrap_or_else(|e| panic!("`ci.yml` runs `{script}`, which cannot be read: {e}"))
                .permissions()
                .mode();
            assert!(
                mode & 0o111 != 0,
                "`{script}` is not executable ({mode:o}), and `ci.yml` runs it by path."
            );
        }
    }
    assert!(
        found >= 3,
        "only {found} of `ci.yml`'s `run:` lines were read as a script path. This scan has stopped \
         seeing them, and an unexecutable script would now go unnoticed."
    );
}
