//! The workflow describes itself, and the description has to add up.
//!
//! `AGENTS.md`, `/implement` and the pull request template all count their own
//! steps, and the counts are written out in prose next to lists that somebody
//! edits. Inserting a step is exactly when they stop matching: the change that
//! added a review step left the template saying "Four questions" above five of
//! them, and the only reason anybody noticed is that a reviewer read it.
//!
//! Nothing here reads the words. It counts headings and list markers, and
//! compares them to the numbers the prose claims — which is the whole of what
//! went wrong, and the part a person is worst at checking.

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repo().join(relative))
        .unwrap_or_else(|e| panic!("cannot read {relative}: {e}"))
}

/// "six" -> 6, for the counts that are written as words.
fn number_word(word: &str) -> Option<usize> {
    const WORDS: &[&str] = &[
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    WORDS.iter().position(|w| *w == word)
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
    let section = &text[start..];
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
    let word = claim.split_whitespace().next().unwrap_or_default();
    let claimed = number_word(&word.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("`{word}` is not a number this test knows"));

    assert_eq!(
        claimed, sections,
        "the template has {sections} sections and says it asks {word} questions"
    );
}
