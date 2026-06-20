use std::path::PathBuf;

use trawl::parser::{goal, ParseContext};
use trawl::scanner::FileContents;
use trawl::{Config, Priority, Status};

fn fixture() -> FileContents {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/goal/example.md");
    let content = std::fs::read_to_string(&path).unwrap();
    FileContents {
        path: path.clone(),
        content,
    }
}

#[test]
fn parses_fixture_goal() {
    let ctx = ParseContext::from_config(&Config::default()).unwrap();
    let fc = fixture();
    let rel = PathBuf::from("ml/llm/cs146s/example.md");
    let goal = goal::parse(&fc.content, &rel, &ctx).expect("fixture has a goal section");

    assert_eq!(goal.title, "Complete CS146s-2025");
    assert_eq!(goal.badge, "ml/llm/");

    // The "References" section and intro text must not contribute items.
    // The two `###` subsections (Foundations, Sprint Board) are now parsed
    // as structural group nodes — each owns the items beneath it.
    assert_eq!(goal.items.len(), 2);
    assert!(goal.items[0].is_group(), "Foundations is a group node");
    assert_eq!(goal.items[0].text, "Foundations");
    assert!(goal.items[1].is_group(), "Sprint Board is a group node");
    assert_eq!(goal.items[1].text, "Sprint Board");

    // Foundations contains two checkbox milestones.
    let foundations = &goal.items[0].children;
    assert_eq!(foundations.len(), 2);
    assert!(foundations[0].is_milestone());
    assert!(foundations[1].is_milestone());

    // Sprint Board contains three table rows as flat leaves. The state
    // column drives the done heuristic.
    let sprint = &goal.items[1].children;
    assert_eq!(sprint.len(), 3);
    assert!(!sprint[0].checked().unwrap(), "TODO state is not done");
    assert!(sprint[1].checked().unwrap(), "done state is done");
    assert!(!sprint[2].checked().unwrap(), "empty state is not done");
    assert_eq!(sprint[0].metadata.priority, Some(Priority::High));
    assert_eq!(sprint[1].metadata.owner.as_deref(), Some("bob"));
}

#[test]
fn fixture_progress_is_active() {
    let ctx = ParseContext::from_config(&Config::default()).unwrap();
    let fc = fixture();
    let rel = PathBuf::from("ml/llm/cs146s/example.md");
    let goal = goal::parse(&fc.content, &rel, &ctx).unwrap();

    // Leaves: L1(done), A1(done), Reading, L3, A2, OAuth, Token, Tests = 8 total.
    // Done: L1, A1, Token = 3. So 3/8 ~ 0.375, status Active.
    assert!(
        (goal.progress() - 0.375).abs() < 1e-9,
        "progress was {}",
        goal.progress()
    );
    assert_eq!(goal.status(), Status::Active);
}
