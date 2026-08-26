//! The workflow describes itself, and the description has to add up.
//!
//! `AGENTS.md`, `/implement` and the pull request template describe each other:
//! they count their own steps in prose, next to lists somebody edits, and they
//! point at files by name. Inserting a step is exactly when those stop
//! matching, and a count in prose is the part a person is worst at checking.
//!
//! Nothing here reads the words. It counts headings and list markers, compares
//! them to the numbers the prose claims, checks that the files this
//! documentation names exist, and checks the one ordering the workflow depends
//! on — that the review comes after the commits, because its criteria ask about
//! them.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// Everything an agent is handed, found rather than listed: a command added
/// tomorrow is scanned tomorrow, which is the staleness this file exists to
/// catch happening to this file.
fn agent_facing_documents() -> Vec<PathBuf> {
    let mut found = vec![repo().join("AGENTS.md")];
    for dir in [".claude/commands", ".claude/agents", ".claude/skills"] {
        let mut here = Vec::new();
        collect_markdown(&repo().join(dir), &mut here);
        assert!(
            !here.is_empty(),
            "no markdown under {dir}: scanning nothing"
        );
        found.extend(here);
    }
    found
}

fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repo().join(relative))
        .unwrap_or_else(|e| panic!("cannot read {relative}: {e}"))
}

/// The backticked spans of a markdown file, with fenced blocks removed first.
///
/// Pairing backticks by parity over a whole file holds only as long as every
/// fence contributes an even number of them. One apostrophe-free shell string
/// with a stray backtick shifts the parity and every span after it is read as
/// the gap between spans — so the scan finds nothing and the test passes for
/// the worst possible reason. `skill_references.rs` already does this; this is
/// the same fix.
fn backticked(text: &str) -> Vec<String> {
    let mut prose = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        prose.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        match after.find("```") {
            Some(end) => rest = &after[end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    prose.push_str(rest);
    prose
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// "six" -> 6, for the counts that are written as words.
fn number_word(word: &str) -> Option<usize> {
    const WORDS: &[&str] = &[
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    let word = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
    let word = word.to_ascii_lowercase();
    WORDS.iter().position(|w| *w == word)
}

/// The first number word anywhere in a sentence, so that rewording the sentence
/// does not fail the build for a reason nobody meant.
fn number_in(sentence: &str) -> Option<usize> {
    sentence.split_whitespace().find_map(number_word)
}

/// The numbers at the start of `## 3. Something` headings, in order.
fn numbered_headings(text: &str) -> Vec<usize> {
    text.lines()
        .filter_map(|line| line.strip_prefix("## "))
        .filter_map(|rest| rest.split('.').next()?.parse().ok())
        .collect()
}

/// The numbers at the start of `3. **Something**` list items, in order.
fn numbered_items(text: &str) -> Vec<usize> {
    text.lines()
        .filter_map(|line| {
            let (head, rest) = line.split_once(". ")?;
            if rest.starts_with("**") {
                head.parse().ok()
            } else {
                None
            }
        })
        .collect()
}

fn assert_contiguous(numbers: &[usize], what: &str) {
    assert!(
        !numbers.is_empty(),
        "{what}: found no numbered steps at all"
    );
    let expected: Vec<usize> = (1..=numbers.len()).collect();
    assert_eq!(
        numbers,
        expected,
        "{what}: the steps are numbered {numbers:?}, which is not 1..{}",
        numbers.len()
    );
}

#[test]
fn implement_numbers_its_steps_in_order() {
    let text = read(".claude/commands/implement.md");
    assert_contiguous(&numbered_headings(&text), "/implement");
}

#[test]
fn agents_md_counts_the_steps_it_lists() {
    let text = read("AGENTS.md");
    let start = text
        .find("## Working on a change")
        .expect("AGENTS.md has no `Working on a change` section");
    // To the next heading, not to the end of the file: a numbered list further
    // down the document is not this list.
    let rest = &text[start..];
    let section = match rest[3..].find("\n## ") {
        Some(offset) => &rest[..offset + 3],
        None => rest,
    };
    let steps = numbered_items(section);
    assert_contiguous(&steps, "AGENTS.md");

    // "`/implement <issue>` does all six."
    let claim = section
        .lines()
        .find(|line| line.contains("does all "))
        .expect("AGENTS.md never says how many steps /implement does");
    let word = claim
        .split("does all ")
        .nth(1)
        .and_then(|rest| rest.split(['.', ' ']).next())
        .expect("cannot read the number out of that sentence");
    let claimed =
        number_word(word).unwrap_or_else(|| panic!("`{word}` is not a number this test knows"));

    assert_eq!(
        claimed,
        steps.len(),
        "AGENTS.md lists {} steps and then says /implement does all {word}",
        steps.len()
    );
}

#[test]
fn the_pull_request_template_asks_as_many_questions_as_it_says() {
    let text = read(".github/pull_request_template.md");
    let sections = text.lines().filter(|l| l.starts_with("## ")).count();

    let claim = text
        .lines()
        .find(|line| line.contains(" questions"))
        .expect("the template never says how many questions it asks");
    let claimed =
        number_in(claim).unwrap_or_else(|| panic!("no number this test knows in: {claim}"));

    assert_eq!(
        claimed, sections,
        "the template has {sections} sections and says: {claim}"
    );
}

/// Step 7 keeps no copy of the reviewer's criteria — it points at the file that
/// holds them. A pointer nobody checks is how that file gets moved and the step
/// quietly stops saying anything.
///
/// This checks every repository path the agent-facing documentation names, not
/// only that one: a backticked span that looks like a path in this tree has to
/// resolve to something.
#[test]
fn the_documentation_points_at_files_that_exist() {
    let mut missing = Vec::new();

    for doc in agent_facing_documents() {
        let name = doc
            .strip_prefix(repo())
            .unwrap_or(&doc)
            .display()
            .to_string();
        let text =
            std::fs::read_to_string(&doc).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        for span in backticked(&text) {
            let looks_like_a_path = span.contains('/')
                && !span.contains(' ')
                && (span.ends_with(".rs")
                    || span.ends_with(".md")
                    || span.ends_with(".toml")
                    || span.ends_with(".yml")
                    || span.ends_with('/'));
            // `target/` is build output: the documentation points at things
            // that appear there when a test fails, and they are supposed not
            // to exist the rest of the time.
            if !looks_like_a_path || span.starts_with("target/") {
                continue;
            }
            let path = repo().join(span.trim_end_matches('/'));
            if !path.exists() {
                missing.push(format!("  {name}: `{span}`"));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "documentation names {} path(s) that do not exist:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// The review runs *after* the commits, because its criteria ask about them —
/// whether they are atomic, whether their subjects say what is now true. Swap
/// the two steps and the step still reads sensibly while asking questions
/// nothing can answer, which is the state this whole change started from.
#[test]
fn the_review_comes_after_the_commits() {
    let text = read(".claude/commands/implement.md");

    let step = |needle: &str| -> usize {
        text.lines()
            .filter_map(|line| line.strip_prefix("## "))
            .find(|heading| heading.to_ascii_lowercase().contains(needle))
            .and_then(|heading| heading.split('.').next()?.parse().ok())
            .unwrap_or_else(|| panic!("/implement has no step about `{needle}`"))
    };

    // Matched on the verb, not on the whole sentence: AGENTS.md spells this
    // step "read by something that did not write it" and /implement spells it
    // "somebody", and neither wording is the thing being asserted.
    let commit = step("commit");
    let review = step("read by");
    assert!(
        review > commit,
        "the review is step {review} and the commits are step {commit}: its \
         criteria ask about commits that would not exist yet"
    );
}

/// `/implement` ends by saying how many lines to report back, and the template
/// says how many questions it asks. They are the same list, so they are the
/// same number — and a count in prose beside a list is the thing this file
/// exists to watch.
#[test]
fn the_report_back_matches_the_template() {
    let implement = read(".claude/commands/implement.md");
    let template = read(".github/pull_request_template.md");

    let claim = implement
        .lines()
        .find(|line| line.contains("report back, in "))
        .expect("/implement never says how many lines to report back");
    let claimed =
        number_in(claim).unwrap_or_else(|| panic!("no number this test knows in: {claim}"));
    let sections = template.lines().filter(|l| l.starts_with("## ")).count();

    assert_eq!(
        claimed, sections,
        "the template asks {sections} questions and /implement says: {claim}"
    );
}

/// The three mutations the counting does not see: strip step 7 down to its
/// heading, take the review section out of the template, or drop the review
/// from AGENTS.md's list. Each leaves every count consistent and removes the
/// change entirely.
#[test]
fn the_review_step_still_says_what_it_is_for() {
    let implement = read(".claude/commands/implement.md");
    let step_seven = implement
        .split("## 7.")
        .nth(1)
        .expect("/implement has no step 7")
        .split("\n## ")
        .next()
        .unwrap_or_default();

    assert!(
        step_seven.contains(".claude/agents/reviewer.md"),
        "step 7 keeps no copy of the criteria, so naming the file that holds \
         them is the whole of it — and it does not"
    );
    assert!(
        step_seven.contains("did not run"),
        "step 7 does not say what to do when the review does not run, and a \
         review that did not run looks exactly like one that found nothing"
    );

    let template = read(".github/pull_request_template.md");
    assert!(
        template
            .lines()
            .any(|l| l.starts_with("## ") && l.contains("review")),
        "the pull request template has no section asking what the review found"
    );

    let agents = read("AGENTS.md");
    assert!(
        agents.contains("`reviewer` subagent"),
        "AGENTS.md's list of what a change goes through never mentions the \
         reviewer, so the step exists only in the command"
    );
}
