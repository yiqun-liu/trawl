use std::path::{Path, PathBuf};

use trawl::parser::{goal, ParseContext};
use trawl::scanner::FileContents;
use trawl::{Config, NodeState, Status};

fn load(name: &str) -> FileContents {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/goal/")
        .join(name);
    let content = std::fs::read_to_string(&path).unwrap();
    FileContents { path, content }
}

fn ctx() -> ParseContext {
    ParseContext::from_config(&Config::default()).unwrap()
}

#[test]
fn subsections_become_group_nodes() {
    let fc = load("subsections.md");
    let rel = PathBuf::from("subsections.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).expect("has section");

    // Top-level: Pre-section task, Foundations, Advanced, Empty Placeholder, Final.
    // (Sub-topic is nested under Foundations, not top-level.)
    assert_eq!(goal.items.len(), 5);

    // Pre-section checkbox task at the root.
    assert!(goal.items[0].is_checkbox());
    assert!(!goal.items[0].is_milestone());

    // The four `###` subsections become Group nodes.
    assert!(goal.items[1].is_group());
    assert_eq!(goal.items[1].text, "Foundations");
    assert!(goal.items[2].is_group());
    assert_eq!(goal.items[2].text, "Advanced");
    assert!(goal.items[3].is_group());
    assert_eq!(goal.items[3].text, "Empty Placeholder");
    assert!(goal.items[4].is_group());
    assert_eq!(goal.items[4].text, "Final");
}

#[test]
fn nested_heading_is_child_of_parent_heading() {
    let fc = load("subsections.md");
    let rel = PathBuf::from("subsections.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // Foundations contains: Chapter 1, Chapter 2, Sub-topic (nested #### group).
    let foundations = &goal.items[1].children;
    assert_eq!(foundations.len(), 3);
    assert!(foundations[2].is_group());
    assert_eq!(foundations[2].text, "Sub-topic");
    // Sub-topic owns the deep task.
    assert_eq!(foundations[2].children.len(), 1);
    assert!(foundations[2].children[0].is_checkbox());
}

#[test]
fn heading_resets_indent_context() {
    let fc = load("subsections.md");
    let rel = PathBuf::from("subsections.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // After Foundations' nested Sub-topic -> Deep task, the next `### Advanced`
    // heading pops back to top-level. Chapter 3 is a child of Advanced, not
    // of any prior checkbox or Sub-topic.
    let advanced = &goal.items[2].children;
    assert_eq!(advanced.len(), 1);
    assert_eq!(advanced[0].text, "Chapter 3");
}

#[test]
fn empty_subsection_is_group_leaf() {
    let fc = load("subsections.md");
    let rel = PathBuf::from("subsections.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // "Empty Placeholder" heading is immediately followed by another heading.
    // It becomes a Group leaf (no children) — a planned placeholder.
    let empty = &goal.items[3];
    assert!(empty.is_group());
    assert!(empty.children.is_empty());
}

#[test]
fn empty_subsection_does_not_affect_progress() {
    let fc = load("subsections.md");
    let rel = PathBuf::from("subsections.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // Checkbox leaves: Pre-section(not), Ch1(done), Ch2(not), Deep(not),
    // Ch3(not), Done(done) = 2/6 done.
    // Empty Placeholder is a group leaf and does NOT count toward total.
    assert!(
        (goal.progress() - (2.0 / 6.0)).abs() < 1e-9,
        "progress was {}",
        goal.progress()
    );
    assert_eq!(goal.status(), Status::Active);
}

#[test]
fn plain_bullets_without_children_are_ignored() {
    let fc = load("plain-bullets.md");
    let rel = PathBuf::from("plain-bullets.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // "A standalone note", "Another context comment", and "Just a comment
    // again" have no children — they must be dropped. Only the two
    // child-bearing groups survive.
    assert_eq!(goal.items.len(), 2);
    assert!(goal.items[0].is_group());
    assert_eq!(goal.items[0].text, "Group with children");
    assert!(goal.items[1].is_group());
    assert_eq!(goal.items[1].text, "Sibling group");
}

#[test]
fn plain_bullet_groups_have_checkbox_children() {
    let fc = load("plain-bullets.md");
    let rel = PathBuf::from("plain-bullets.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    let group = &goal.items[0];
    assert_eq!(group.children.len(), 2);
    assert!(matches!(
        group.children[0].state,
        NodeState::Checkbox { checked: true }
    ));
    assert!(matches!(
        group.children[1].state,
        NodeState::Checkbox { checked: false }
    ));
}

#[test]
fn plain_bullet_groups_progress_counts_only_checkbox_leaves() {
    let fc = load("plain-bullets.md");
    let rel = PathBuf::from("plain-bullets.md");
    let goal = goal::parse(&fc.content, &rel, &ctx()).unwrap();

    // Checkbox leaves: Task under group(done), Another(not), Done(done) = 2/3.
    assert!(
        (goal.progress() - (2.0 / 3.0)).abs() < 1e-9,
        "progress was {}",
        goal.progress()
    );
}

#[test]
fn checkbox_only_tracker_parses_unchanged() {
    // Regression: a tracker with no headings and no plain bullets must
    // produce the same structure as before this feature.
    let md = "# T\n\n## GOAL TRACKER\n\n- [x] Week 1\n  - [x] Lecture 1\n  - [ ] Lecture 2\n- [ ] Week 2\n  - [ ] Lecture 3\n";
    let goal = goal::parse(md, Path::new("course/README.md"), &ctx()).unwrap();
    assert_eq!(goal.items.len(), 2);
    assert!(goal.items[0].is_checkbox());
    assert!(goal.items[0].is_milestone());
    assert_eq!(goal.items[0].children.len(), 2);
    // Leaf-ratio progress unchanged: 1/3 done.
    assert!((goal.progress() - (1.0 / 3.0)).abs() < 1e-9);
}
