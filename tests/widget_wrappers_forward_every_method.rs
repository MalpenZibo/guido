//! Every widget that wraps another forwards every method of `Widget`.
//!
//! Two wrappers stand between the tree and most widgets. `Box<dyn Widget>` is
//! one: an `AnyWidget` is a box, and a static child is boxed again when it is
//! added, so a list built with `into_any` reaches its rows through the box's
//! own impl. `OwnedWidget` is the other: every dynamic child is adopted as one.
//!
//! Most of `Widget`'s methods have a default. A wrapper that does not forward
//! one does not fail to compile — it quietly answers the default for the
//! widget inside it. `OwnedWidget` answered no `layout_hints`, so a dynamic
//! `fill()` child was laid out as one that does not fill; both wrappers
//! answered no `refresh_paint_bounds`, so a transformed row added through
//! either was culled where it was laid out (#558). Each was found by an
//! application, not by a test, because nothing about a missing forward looks
//! wrong.
//!
//! So the rule is read out of the source: every method the trait declares is
//! written out in both wrappers, and a method added to the trait fails this
//! test until both name it. It asks that a method is there, not what its body
//! does — `OwnedWidget::owned_scope` answers its own owner on purpose — which
//! is the failure that happened: a method forgotten, not one written wrong. `into_any` is the one exception — it takes
//! `self` by value and needs `Self: Sized`, so there is no widget to forward to.

use std::collections::BTreeSet;

/// Trait methods a wrapper does not forward, and why.
const NOT_FORWARDED: &[&str] = &[
    // A consuming conversion into a box, bounded by `Self: Sized`.
    "into_any",
];

/// The `fn` names declared directly inside the item whose header is `header`.
fn methods(file: &str, header: &str) -> BTreeSet<String> {
    let path = format!("{}/{file}", env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("`{header}` is not in {file}"));
    let open = start + source[start..].find('{').expect("the item has a body");

    let mut depth = 1usize;
    let mut names = BTreeSet::new();
    for line in source[open + 1..].lines() {
        // Only the item's own level: a method's body holds no `fn` it declares.
        if depth == 1
            && let Some(rest) = line.trim_start().strip_prefix("fn ")
        {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            names.insert(name);
        }
        for c in line.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if depth == 0 {
            break;
        }
    }
    names
}

fn assert_forwards(wrapper: &str, file: &str) {
    let declared = methods("src/widgets/widget.rs", "pub trait Widget");
    assert!(
        declared.contains("layout") && declared.contains("layout_hints"),
        "the trait's methods were not read: {declared:?}"
    );
    let forwarded = methods(file, &format!("impl Widget for {wrapper}"));
    let missing: Vec<_> = declared
        .iter()
        .filter(|name| !NOT_FORWARDED.contains(&name.as_str()))
        .filter(|name| !forwarded.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "{wrapper} does not forward {missing:?}: the tree reaches the widget inside \
         it through the wrapper, so it would be answered with the trait's default"
    );
}

#[test]
fn a_boxed_widget_forwards_every_method() {
    assert_forwards("Box<dyn Widget>", "src/widgets/widget.rs");
}

#[test]
fn an_owned_widget_forwards_every_method() {
    assert_forwards("OwnedWidget", "src/widgets/children.rs");
}
