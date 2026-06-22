use std::collections::HashSet;
use std::path::{Path, PathBuf};

use trawl::parser::{goal, resolve, ParseContext};
use trawl::{BrokenReason, Config, Reference};

fn load(name: &str) -> (PathBuf, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/goal/")
        .join(name);
    let content = std::fs::read_to_string(&path).unwrap();
    (path, content)
}

fn ctx() -> ParseContext {
    ParseContext::from_config(&Config::default()).unwrap()
}

fn parse_rel(name: &str, rel: &str) -> trawl::model::Goal {
    let (_abs, content) = load(name);
    let rel = PathBuf::from(rel);
    goal::parse(&content, &rel, &ctx()).expect("fixture has a goal section")
}

/// Build a Vec<FileContents> from a list of (filename, rel) pairs and parse
/// all goals, mimicking lib::scan's pipeline without invoking the scanner.
fn parse_all(specs: &[(&str, &str)]) -> Vec<trawl::model::Goal> {
    let mut goals = Vec::new();
    for (file, rel) in specs {
        let (abs, content) = load(file);
        let _ = abs; // unused; rel is what we feed to the parser
        let rel_path = PathBuf::from(rel);
        if let Some(g) = goal::parse(&content, &rel_path, &ctx()) {
            goals.push(g);
        }
    }
    goals
}

fn empty_scanned() -> HashSet<PathBuf> {
    HashSet::new()
}

#[test]
fn wikilink_reference_resolves_with_target_title() {
    let mut goals = parse_all(&[
        ("ref-wikilink.md", "ref-wikilink.md"),
        ("ref-target.md", "ref-target.md"),
    ]);
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = goals.iter().find(|g| g.title == "Wikilink Source").unwrap();
    assert_eq!(source.items.len(), 2);

    // First item is the wikilink reference; its text is filled from the
    // target's H1 title.
    let ref_item = &source.items[0];
    assert!(matches!(
        &ref_item.reference,
        Some(Reference::Resolved { target_path, .. })
            if target_path == &PathBuf::from("ref-target.md")
    ));
    assert_eq!(ref_item.text, "Resolved Target");
    // The target's two items are attached as children.
    assert_eq!(ref_item.children.len(), 2);
}

#[test]
fn markdown_link_reference_keeps_display_text() {
    let mut goals = parse_all(&[
        ("ref-mdlink.md", "ref-mdlink.md"),
        ("ref-target.md", "ref-target.md"),
    ]);
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = goals
        .iter()
        .find(|g| g.title == "Markdown Link Source")
        .unwrap();
    let ref_item = &source.items[0];
    assert!(matches!(
        &ref_item.reference,
        Some(Reference::Resolved { .. })
    ));
    assert_eq!(ref_item.text, "Custom Display Text");
}

#[test]
fn cycle_detected_between_two_docs() {
    let mut goals = parse_all(&[
        ("ref-cycle-a.md", "ref-cycle-a.md"),
        ("ref-cycle-b.md", "ref-cycle-b.md"),
    ]);
    resolve::resolve_references(&mut goals, &empty_scanned());

    // a -> b resolves; the nested b -> a becomes Cycle because a is on the chain.
    let a = goals.iter().find(|g| g.title == "Cycle A").unwrap();
    let a_to_b = &a.items[0];
    assert!(matches!(
        &a_to_b.reference,
        Some(Reference::Resolved { .. })
    ));
    // The cloned child (originally from b.md) carries a now-Cycle reference.
    let nested = &a_to_b.children[0];
    match &nested.reference {
        Some(Reference::Cycle { chain }) => {
            // Chain must contain both cycle participants.
            assert!(chain.contains(&PathBuf::from("ref-cycle-a.md")));
            assert!(chain.contains(&PathBuf::from("ref-cycle-b.md")));
        }
        other => panic!("expected Cycle, got {other:?}"),
    }
}

#[test]
fn broken_reference_to_nonexistent_file() {
    let mut goals = parse_all(&[("ref-broken.md", "ref-broken.md")]);
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = &goals[0];
    let ref_item = &source.items[0];
    match &ref_item.reference {
        Some(Reference::Broken { reason, .. }) => {
            assert_eq!(*reason, BrokenReason::NotFound);
        }
        other => panic!("expected Broken(NotFound), got {other:?}"),
    }
    assert!(ref_item.children.is_empty());
}

#[test]
fn relative_path_reference_resolves() {
    // Parse ref-target.md under a nested path; reference it from a doc in
    // a sibling directory using a `..` relative path.
    let target_content = load("ref-target.md").1;
    let source_content =
        "# Subdir Source\n\n## GOAL TRACKER\n\n- [ ] [[../sub/ref-target]]\n".to_string();
    let target_goal =
        goal::parse(&target_content, Path::new("a/sub/ref-target.md"), &ctx()).unwrap();
    let source_goal = goal::parse(&source_content, Path::new("a/other/source.md"), &ctx()).unwrap();
    let mut goals = vec![source_goal, target_goal];
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = goals.iter().find(|g| g.title == "Subdir Source").unwrap();
    assert!(matches!(
        &source.items[0].reference,
        Some(Reference::Resolved { target_path, .. })
            if target_path == &PathBuf::from("a/sub/ref-target.md")
    ));
}

#[test]
fn resolved_subtree_progress_reflects_cloned_leaves() {
    let mut goals = parse_all(&[
        ("ref-wikilink.md", "ref-wikilink.md"),
        ("ref-target.md", "ref-target.md"),
    ]);
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = goals.iter().find(|g| g.title == "Wikilink Source").unwrap();
    // Leaves: Standalone(done), Target task one(done), Target task two(not) = 2/3.
    assert!(
        (source.progress() - (2.0 / 3.0)).abs() < 1e-9,
        "progress was {}",
        source.progress()
    );
}

#[test]
fn standalone_reference_becomes_group_node() {
    // A standalone [[ref]] line (no `- ` bullet) becomes a Group node with
    // the reference attached.
    let md = "# S\n\n## GOAL TRACKER\n\n[[ref-target]]\n";
    let goal = goal::parse(md, Path::new("s.md"), &ctx()).unwrap();
    assert_eq!(goal.items.len(), 1);
    assert!(goal.items[0].is_group());
    assert!(matches!(
        &goal.items[0].reference,
        Some(Reference::Pending { raw_target, .. }) if raw_target == "ref-target"
    ));
}

#[test]
fn embedded_reference_is_not_treated_as_structural() {
    // "- [ ] see [[x]] for details" — embedded reference stays literal text;
    // the line is not a structural reference.
    let md = "# S\n\n## GOAL TRACKER\n\n- [ ] see [[x]] for details\n";
    let goal = goal::parse(md, Path::new("s.md"), &ctx()).unwrap();
    assert_eq!(goal.items.len(), 1);
    assert!(
        goal.items[0].reference.is_none(),
        "embedded ref stays literal"
    );
    assert_eq!(goal.items[0].text, "see [[x]] for details");
}

#[test]
fn regression_existing_trackers_still_parse() {
    // Sanity: a tracker with no references and no subsections still produces
    // the same flat checkbox structure.
    let _ = parse_rel("ref-target.md", "ref-target.md");
}

#[test]
fn checkbox_form_broken_reference_does_not_count_toward_progress() {
    // A done task alongside a checkbox-form broken reference. The broken
    // reference must NOT count as a not-done leaf — only the real task
    // participates, so progress is 100% (without the fix it would be 50%).
    let md = "# Dead Ref Source\n\n## GOAL TRACKER\n\n- [x] real task\n- [ ] [[does-not-exist]]\n";
    let mut goals = vec![goal::parse(md, Path::new("dead.md"), &ctx()).unwrap()];
    resolve::resolve_references(&mut goals, &empty_scanned());

    let source = &goals[0];
    let broken = source
        .items
        .iter()
        .find(|i| i.is_dead_reference())
        .expect("a dead reference is present");
    assert!(matches!(
        &broken.reference,
        Some(Reference::Broken { reason, .. }) if *reason == BrokenReason::NotFound
    ));
    assert!(
        broken.is_checkbox(),
        "broken ref retains its checkbox state but must not count"
    );
    assert!(
        (source.progress() - 1.0).abs() < 1e-9,
        "progress was {}",
        source.progress()
    );
}

#[test]
fn checkbox_form_cycle_reference_does_not_count_toward_progress() {
    // a -> b; b contains a done task and a back-reference to a. The cloned
    // back-reference becomes Cycle and must NOT count as a not-done leaf —
    // only the done task participates, so a's progress is 100% (without the
    // fix it would be 50%).
    let a = "# Cycle A\n\n## GOAL TRACKER\n\n- [ ] [[cb]]\n";
    let b = "# Cycle B\n\n## GOAL TRACKER\n\n- [x] real task\n- [ ] [[ca]]\n";
    let mut goals = vec![
        goal::parse(a, Path::new("ca.md"), &ctx()).unwrap(),
        goal::parse(b, Path::new("cb.md"), &ctx()).unwrap(),
    ];
    resolve::resolve_references(&mut goals, &empty_scanned());

    let a = goals.iter().find(|g| g.title == "Cycle A").unwrap();
    assert!(
        a.items
            .iter()
            .any(|i| i.children.iter().any(|c| c.is_dead_reference())),
        "expected a cycle marker in a's expanded subtree"
    );
    assert!(
        (a.progress() - 1.0).abs() < 1e-9,
        "progress was {}",
        a.progress()
    );
}
